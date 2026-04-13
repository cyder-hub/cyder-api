use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl, SqliteConnection,
    connection::SimpleConnection,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    sql_types::Text,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use std::fs::File;
use std::path::Path;
use std::sync::LazyLock;

use crate::{config::CONFIG, controller::BaseError};
use serde::Serialize;

pub mod access_control;
pub mod cost;
pub mod custom_field;
pub mod model;
pub mod model_alias;
pub mod provider;
pub mod request_log;
pub mod stat;
pub mod system_api_key;
//pub mod record; // Assuming this will be replaced or removed if request_log supersedes it
//pub mod model_transform;

pub enum DbType {
    Postgres,
    Sqlite,
}

pub enum DbPool {
    Postgres(Pool<ConnectionManager<PgConnection>>),
    Sqlite(Pool<ConnectionManager<SqliteConnection>>),
}

pub enum DbConnection {
    Postgres(PooledConnection<ConnectionManager<PgConnection>>),
    Sqlite(PooledConnection<ConnectionManager<SqliteConnection>>),
}

pub fn get_connection() -> DbResult<DbConnection> {
    match &*DB_POOL {
        DbPool::Postgres(pool) => {
            let conn = pool.get().map_err(|e| {
                BaseError::DatabaseFatal(Some(format!("Postgres pool error: {}", e)))
            })?;
            Ok(DbConnection::Postgres(conn))
        }
        DbPool::Sqlite(pool) => {
            let conn = pool
                .get()
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Sqlite pool error: {}", e))))?;
            Ok(DbConnection::Sqlite(conn))
        }
    }
}

fn parse_db_type(db_url: &str) -> DbType {
    if db_url.starts_with("postgres") {
        DbType::Postgres
    } else {
        DbType::Sqlite
    }
}

impl DbPool {
    pub fn establish() -> Self {
        let db_url = &CONFIG.db_url;
        let db_type = parse_db_type(db_url);
        match db_type {
            DbType::Postgres => {
                let pool = init_pg_pool(db_url);
                DbPool::Postgres(pool)
            }
            DbType::Sqlite => {
                let pool = init_sqlite_pool(db_url);
                DbPool::Sqlite(pool)
            }
        }
    }
}

#[path = "../schema/sqlite.rs"]
pub mod _sqlite_schema;

#[path = "../schema/postgres.rs"]
pub mod _postgres_schema;

#[macro_export]
macro_rules! db_object {
    (
        $(
            $( #[$attr:meta] )*
            pub struct $name:ident {
                $( $( #[$field_attr:meta] )* $vis:vis $field:ident : $typ:ty ),+
                $(,)?
            }
        )+
    ) => {
        $(
            #[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
            pub struct $name { $( $vis $field : $typ, )+ }
        )+

        pub mod _postgres_model {
            $( $crate::db_object! { @expand postgres |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
        pub mod _sqlite_model {
            $( $crate::db_object! { @expand sqlite |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
    };
    ( @expand $db_type:ident | $( #[$attr:meta] )* | $name:ident | $( $( #[$field_attr:meta] )* $vis:vis $field:ident : $typ:ty),+) => {
        paste::paste! {
            #[allow(unused_imports)] use super::*;
            #[allow(unused_imports)] use crate::database::[<_ $db_type _schema>]::*;
            #[allow(unused_imports)] use diesel::prelude::*;

            $( #[$attr] )*
            pub struct [<$name Db>] { $(
                $( #[$field_attr] )* $vis $field : $typ,
            )+ }

            impl [<$name Db>] {
                #[inline(always)]
                pub fn from_db(self) -> super::$name {
                    super::$name { $( $field: self.$field, )+ }
                }

                #[inline(always)]
                pub fn to_db(x: &super::$name) -> Self {
                    Self {
                        $( $field: x.$field.clone(), )+
                    }
                }
            }
        }
    }
}

#[macro_export]
macro_rules! db_execute {
    ($conn:ident, $block:block) => {
        match $conn {
            crate::database::DbConnection::Postgres($conn) => {
                use crate::database::_postgres_schema::*;
                #[allow(unused_imports)]
                use _postgres_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
            crate::database::DbConnection::Sqlite($conn) => {
                use crate::database::_sqlite_schema::*;
                #[allow(unused_imports)]
                use _sqlite_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
        }
    };
}

static DB_POOL: LazyLock<DbPool> = LazyLock::new(DbPool::establish);
const SQLITE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");
const POSTGRES_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/postgres");

#[derive(QueryableByName)]
struct SqliteTableInfoRow {
    #[diesel(sql_type = Text)]
    name: String,
}

fn sqlite_table_has_column(
    connection: &mut SqliteConnection,
    table_name: &str,
    column_name: &str,
) -> Result<Option<bool>, diesel::result::Error> {
    let pragma = format!("SELECT name FROM pragma_table_info('{table_name}')");
    let rows = diesel::sql_query(pragma).load::<SqliteTableInfoRow>(connection)?;
    if rows.is_empty() {
        return Ok(None);
    }

    Ok(Some(rows.iter().any(|row| row.name == column_name)))
}

fn repair_legacy_sqlite_schema(
    connection: &mut SqliteConnection,
) -> Result<(), diesel::result::Error> {
    match sqlite_table_has_column(connection, "model", "cost_catalog_id")? {
        Some(true) | None => Ok(()),
        Some(false) => {
            connection.batch_execute("ALTER TABLE model ADD COLUMN cost_catalog_id BIGINT;")
        }
    }
}

fn init_sqlite_pool(db_url: &str) -> Pool<ConnectionManager<SqliteConnection>> {
    let db_path = Path::new(db_url);
    if !db_path.exists() {
        if let Some(parent_dir) = db_path.parent() {
            if !parent_dir.exists() {
                std::fs::create_dir_all(parent_dir).expect("failed to create database directory");
            }
        }
        File::create(db_path).expect("failed to create database file");
    }

    let mut connection =
        SqliteConnection::establish(db_url).expect("failed to establish migration connection");

    {
        use diesel::prelude::*;
        use diesel::sql_types::Text;
        let version: Result<String, _> =
            diesel::select(diesel::dsl::sql::<Text>("sqlite_version()"))
                .get_result(&mut connection);

        match version {
            Ok(v) => println!("database sqlite version: {}", v),
            Err(e) => println!("failed to get sqlite version: {}", e),
        }
    }

    repair_legacy_sqlite_schema(&mut connection)
        .expect("failed to repair legacy sqlite schema before migrations");

    connection
        .run_pending_migrations(SQLITE_MIGRATIONS)
        .expect("failed to run migrations");

    let manager = ConnectionManager::<SqliteConnection>::new(db_url);
    Pool::builder()
        .test_on_check_out(true)
        .max_size(CONFIG.db_pool_size)
        .build(manager)
        .expect("Failed to create pool.")
}

fn init_pg_pool(db_url: &str) -> Pool<ConnectionManager<PgConnection>> {
    let mut connection =
        PgConnection::establish(db_url).expect("failed to establish migration connection");

    connection
        .run_pending_migrations(POSTGRES_MIGRATIONS)
        .expect("failed to run migrations");

    let manager = ConnectionManager::<PgConnection>::new(db_url);
    Pool::builder()
        .max_size(CONFIG.db_pool_size)
        .build(manager)
        .expect("Failed to create pool.")
}

pub type DbResult<T> = Result<T, BaseError>;

#[derive(Serialize)]
pub struct ListResult<T> {
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub list: Vec<T>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn apply_sql(connection: &mut SqliteConnection, sql_text: &str) {
        connection
            .batch_execute(sql_text)
            .expect("sql should execute successfully");
    }

    fn mark_sqlite_migration_applied(connection: &mut SqliteConnection, version: &str) {
        connection
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (
                    version VARCHAR(50) PRIMARY KEY NOT NULL,
                    run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
                );",
            )
            .expect("migration metadata table should be created");

        diesel::sql_query(format!(
            "INSERT INTO __diesel_schema_migrations (version) VALUES ('{version}')"
        ))
        .execute(connection)
        .expect("migration version should be recorded");
    }

    #[derive(QueryableByName)]
    struct NullableBigIntRow {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::BigInt>)]
        cost_catalog_id: Option<i64>,
    }

    #[test]
    fn sqlite_cost_foundation_migration_tolerates_legacy_model_billing_plan_id() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("legacy.sqlite");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut connection =
            SqliteConnection::establish(&db_url).expect("sqlite connection should be established");

        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2025-03-20-062357_initial_setup/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2025-07-02-140210_api_key_jwt/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-01-28-233111_request_log_optimize/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-02-03-230221_request_log_field_opt/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!(
                "../../migrations/sqlite/2026-04-08-090000_expand_llm_api_type_for_request_log/up.sql"
            ),
        );

        for version in [
            "20250320062357",
            "20250702140210",
            "20260128233111",
            "20260203230221",
            "20260408090000",
        ] {
            mark_sqlite_migration_applied(&mut connection, version);
        }

        apply_sql(
            &mut connection,
            "INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at, created_at,
                updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                1, 'p', 'Provider', 'https://example.com', 0, 1, NULL, 1, 1, 'OPENAI', 'QUEUE'
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO billing_plans (
                id, name, description, is_default, currency, created_at, updated_at, deleted_at
            ) VALUES (
                9999, 'legacy-plan', NULL, 0, 'USD', 1, 1, NULL
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO model (
                id, provider_id, billing_plan_id, model_name, real_model_name, is_enabled,
                deleted_at, created_at, updated_at
            ) VALUES (
                10, 1, 9999, 'demo-model', NULL, 1, NULL, 1, 1
            );",
        );

        connection
            .run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("remaining migrations should succeed");

        let migrated_cost_catalog_id =
            diesel::sql_query("SELECT cost_catalog_id FROM model WHERE id = 10")
                .get_result::<NullableBigIntRow>(&mut connection)
                .expect("migrated model row should be readable")
                .cost_catalog_id;

        assert_eq!(migrated_cost_catalog_id, None);
    }

    #[derive(QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }

    #[test]
    fn sqlite_cost_foundation_migration_filters_request_logs_with_orphan_foreign_keys() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("legacy-request-log.sqlite");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut connection =
            SqliteConnection::establish(&db_url).expect("sqlite connection should be established");

        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2025-03-20-062357_initial_setup/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2025-07-02-140210_api_key_jwt/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-01-28-233111_request_log_optimize/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-02-03-230221_request_log_field_opt/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!(
                "../../migrations/sqlite/2026-04-08-090000_expand_llm_api_type_for_request_log/up.sql"
            ),
        );

        for version in [
            "20250320062357",
            "20250702140210",
            "20260128233111",
            "20260203230221",
            "20260408090000",
        ] {
            mark_sqlite_migration_applied(&mut connection, version);
        }

        apply_sql(
            &mut connection,
            "INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at, created_at,
                updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                1, 'p', 'Provider', 'https://example.com', 0, 1, NULL, 1, 1, 'OPENAI', 'QUEUE'
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO provider_api_key (
                id, provider_id, api_key, description, deleted_at, is_enabled, created_at, updated_at
            ) VALUES (
                2, 1, 'secret', NULL, NULL, 1, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO system_api_key (
                id, api_key, name, description, access_control_policy_id, usage_limit_policy_id,
                is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                3, 'system-secret', 'demo', NULL, NULL, NULL, 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO model (
                id, provider_id, billing_plan_id, model_name, real_model_name, is_enabled,
                deleted_at, created_at, updated_at
            ) VALUES (
                10, 1, NULL, 'demo-model', NULL, 1, NULL, 1, 1
            );",
        );
        apply_sql(&mut connection, "PRAGMA foreign_keys=off;");
        apply_sql(
            &mut connection,
            "INSERT INTO request_log (
                id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
                real_model_name, request_received_at, llm_request_sent_at,
                llm_response_first_chunk_at, llm_response_completed_at, client_ip,
                llm_request_uri, llm_response_status, status, is_stream, calculated_cost,
                cost_currency, created_at, updated_at, input_tokens, output_tokens,
                reasoning_tokens, total_tokens, storage_type, user_request_body,
                llm_request_body, llm_response_body, user_response_body, cached_tokens,
                input_image_tokens, output_image_tokens, user_api_type, llm_api_type
            ) VALUES (
                20, 3, 1, 999, 2, 'demo-model', 'demo-model', 1, 1, NULL, NULL, NULL,
                NULL, 200, 'SUCCESS', 0, 10, 'USD', 1, 1, 1, 1, 0, 2, NULL, NULL, NULL,
                NULL, NULL, 0, 0, 0, 'OPENAI', 'OPENAI'
            );",
        );
        apply_sql(&mut connection, "PRAGMA foreign_keys=on;");

        connection
            .run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("remaining migrations should succeed");

        let request_log_count = diesel::sql_query("SELECT COUNT(*) AS count FROM request_log")
            .get_result::<CountRow>(&mut connection)
            .expect("request_log count should be readable")
            .count;

        assert_eq!(request_log_count, 0);
    }

    #[test]
    fn sqlite_cost_foundation_migration_preserves_model_alias_references() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("legacy-model-alias.sqlite");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut connection =
            SqliteConnection::establish(&db_url).expect("sqlite connection should be established");

        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2025-03-20-062357_initial_setup/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2025-07-02-140210_api_key_jwt/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-01-28-233111_request_log_optimize/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-02-03-230221_request_log_field_opt/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!(
                "../../migrations/sqlite/2026-04-08-090000_expand_llm_api_type_for_request_log/up.sql"
            ),
        );

        for version in [
            "20250320062357",
            "20250702140210",
            "20260128233111",
            "20260203230221",
            "20260408090000",
        ] {
            mark_sqlite_migration_applied(&mut connection, version);
        }

        apply_sql(
            &mut connection,
            "INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at, created_at,
                updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                1, 'p', 'Provider', 'https://example.com', 0, 1, NULL, 1, 1, 'OPENAI', 'QUEUE'
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO model (
                id, provider_id, billing_plan_id, model_name, real_model_name, is_enabled,
                deleted_at, created_at, updated_at
            ) VALUES (
                10, 1, NULL, 'demo-model', NULL, 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO model_alias (
                id, alias_name, target_model_id, description, priority, is_enabled, deleted_at,
                created_at, updated_at
            ) VALUES (
                30, 'demo-alias', 10, NULL, 0, 1, NULL, 1, 1
            );",
        );

        connection
            .run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("remaining migrations should succeed");

        let alias_target_count = diesel::sql_query(
            "SELECT COUNT(*) AS count FROM model_alias WHERE id = 30 AND target_model_id = 10",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("model_alias row should be readable")
        .count;

        assert_eq!(alias_target_count, 1);
    }
}
