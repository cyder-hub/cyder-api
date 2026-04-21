use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl, SqliteConnection,
    connection::SimpleConnection,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    sql_types::Text,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::path::Path;
use std::sync::LazyLock;

use crate::{config::CONFIG, controller::BaseError};
use serde::Serialize;

pub mod access_control;
pub mod api_key;
pub mod api_key_acl_rule;
pub mod api_key_rollup;
pub mod cost;
pub mod model;
// Legacy data-access helper for the historical `model_alias` table.
// Keep this private so new code uses `model_route` and `api_key_model_override`.
mod model_alias;
pub mod model_route;
pub mod provider;
pub mod provider_runtime;
pub mod request_log;
pub mod request_patch;
pub mod stat;
pub mod system_api_key;
//pub mod record; // Assuming this will be replaced or removed if request_log supersedes it

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

#[derive(QueryableByName)]
struct ApiKeyBackfillRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    id: i64,
    #[diesel(sql_type = Text)]
    api_key: String,
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

fn compute_api_key_hash(api_key: &str) -> String {
    format!("{:x}", Sha256::digest(api_key.as_bytes()))
}

fn compute_key_prefix(api_key: &str) -> String {
    api_key.chars().take(12).collect()
}

fn compute_key_last4(api_key: &str) -> String {
    let last4: String = api_key.chars().rev().take(4).collect();
    last4.chars().rev().collect()
}

fn backfill_api_key_shadow_sqlite(
    connection: &mut SqliteConnection,
) -> Result<(), diesel::result::Error> {
    let rows = diesel::sql_query(
        "SELECT id, api_key
         FROM api_key
         WHERE api_key_hash IS NULL
            OR api_key_hash = ''
            OR key_prefix = ''
            OR key_last4 = ''",
    )
    .load::<ApiKeyBackfillRow>(connection)?;

    for row in rows {
        diesel::sql_query(
            "UPDATE api_key
             SET api_key_hash = ?,
                 key_prefix = ?,
                 key_last4 = ?
             WHERE id = ?",
        )
        .bind::<diesel::sql_types::Text, _>(compute_api_key_hash(&row.api_key))
        .bind::<diesel::sql_types::Text, _>(compute_key_prefix(&row.api_key))
        .bind::<diesel::sql_types::Text, _>(compute_key_last4(&row.api_key))
        .bind::<diesel::sql_types::BigInt, _>(row.id)
        .execute(connection)?;
    }

    Ok(())
}

fn backfill_api_key_shadow_postgres(
    connection: &mut PgConnection,
) -> Result<(), diesel::result::Error> {
    let rows = diesel::sql_query(
        "SELECT id, api_key
         FROM api_key
         WHERE api_key_hash IS NULL
            OR api_key_hash = ''
            OR key_prefix = ''
            OR key_last4 = ''",
    )
    .load::<ApiKeyBackfillRow>(connection)?;

    for row in rows {
        diesel::sql_query(
            "UPDATE api_key
             SET api_key_hash = $1,
                 key_prefix = $2,
                 key_last4 = $3
             WHERE id = $4",
        )
        .bind::<diesel::sql_types::Text, _>(compute_api_key_hash(&row.api_key))
        .bind::<diesel::sql_types::Text, _>(compute_key_prefix(&row.api_key))
        .bind::<diesel::sql_types::Text, _>(compute_key_last4(&row.api_key))
        .bind::<diesel::sql_types::BigInt, _>(row.id)
        .execute(connection)?;
    }

    Ok(())
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
    backfill_api_key_shadow_sqlite(&mut connection)
        .expect("failed to backfill api_key shadow table");

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
    backfill_api_key_shadow_postgres(&mut connection)
        .expect("failed to backfill api_key shadow table");

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
        if let Err(err) = connection.batch_execute(sql_text) {
            panic!("sql should execute successfully: {err}\n{sql_text}");
        }
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

    #[derive(QueryableByName)]
    struct ApiKeyMigrationRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        id: i64,
        #[diesel(sql_type = diesel::sql_types::Text)]
        api_key_hash: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        key_prefix: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        key_last4: String,
        #[diesel(sql_type = diesel::sql_types::Text)]
        default_action: String,
    }

    #[derive(QueryableByName)]
    struct CostCatalogVersionFreezeRow {
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::BigInt>)]
        first_used_at: Option<i64>,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        is_archived: bool,
    }

    #[test]
    fn sqlite_api_key_governance_migration_preserves_ids_and_request_log_links() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("api-key-governance.sqlite");
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
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-04-10-120000_cost_schema_foundation/up.sql"),
        );
        apply_sql(
            &mut connection,
            include_str!(
                "../../migrations/sqlite/2026-04-14-090000_cost_catalog_version_freeze_flags/up.sql"
            ),
        );

        for version in [
            "20250320062357",
            "20250702140210",
            "20260128233111",
            "20260203230221",
            "20260408090000",
            "20260410120000",
            "20260414090000",
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
                2, 1, 'provider-secret', NULL, NULL, 1, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name, is_enabled,
                deleted_at, created_at, updated_at
            ) VALUES (
                10, 1, NULL, 'demo-model', 'demo-model', 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO access_control_policy (
                id, name, description, default_action, created_at, updated_at, deleted_at
            ) VALUES (
                30, 'policy', NULL, 'ALLOW', 1, 1, NULL
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO access_control_rule (
                id, policy_id, rule_type, priority, scope, provider_id, model_id, is_enabled,
                description, created_at, updated_at, deleted_at
            ) VALUES (
                31, 30, 'DENY', 5, 'MODEL', 1, 10, 1, 'deny demo model', 1, 1, NULL
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO system_api_key (
                id, api_key, name, description, access_control_policy_id, is_enabled, deleted_at,
                created_at, updated_at
            ) VALUES (
                3, 'cyder-abcdefghijklmnopqrstuvwxyz', 'demo', NULL, 30, 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO request_log (
                id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
                real_model_name, request_received_at, llm_request_sent_at,
                llm_response_first_chunk_at, llm_response_completed_at, client_ip,
                llm_request_uri, llm_response_status, status, is_stream, estimated_cost_nanos,
                estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
                cost_snapshot_json, created_at, updated_at, total_input_tokens,
                total_output_tokens, input_text_tokens, output_text_tokens, input_image_tokens,
                output_image_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                total_tokens, storage_type, user_request_body, llm_request_body,
                llm_response_body, user_response_body, user_api_type, llm_api_type
            ) VALUES (
                20, 3, 1, 10, 2, 'demo-model', 'demo-model', 123456, 123456, NULL, NULL, NULL,
                NULL, 200, 'SUCCESS', 0, 10, 'USD', NULL, NULL, NULL, 123456, 123456, 1, 1,
                NULL, NULL, 0, 0, 0, 0, 0, 2, NULL, NULL, NULL, NULL, NULL, 'OPENAI', 'OPENAI'
            );",
        );

        connection
            .run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("remaining migrations should succeed");
        backfill_api_key_shadow_sqlite(&mut connection)
            .expect("api_key shadow backfill should succeed");

        let migrated_key = diesel::sql_query(
            "SELECT id, api_key_hash, key_prefix, key_last4, default_action
             FROM api_key
             WHERE id = 3",
        )
        .get_result::<ApiKeyMigrationRow>(&mut connection)
        .expect("migrated api_key row should be readable");

        assert_eq!(migrated_key.id, 3);
        assert_eq!(
            migrated_key.api_key_hash,
            compute_api_key_hash("cyder-abcdefghijklmnopqrstuvwxyz")
        );
        assert_eq!(migrated_key.key_prefix, "cyder-abcdef");
        assert_eq!(migrated_key.key_last4, "wxyz");
        assert_eq!(migrated_key.default_action, "ALLOW");

        let acl_rule_count = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM api_key_acl_rule
             WHERE api_key_id = 3
               AND scope = 'MODEL'
               AND model_id = 10",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("api_key_acl_rule count should be readable")
        .count;
        assert_eq!(acl_rule_count, 1);

        let join_count = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM request_log AS rl
             JOIN api_key AS ak
               ON rl.system_api_key_id = ak.id
             WHERE rl.id = 20
               AND ak.id = 3",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("request_log/api_key join count should be readable")
        .count;
        assert_eq!(join_count, 1);
    }

    #[test]
    fn sqlite_cost_catalog_version_freeze_migration_backfills_first_used_at() {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("cost-version-freeze.sqlite");
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
        apply_sql(
            &mut connection,
            include_str!("../../migrations/sqlite/2026-04-10-120000_cost_schema_foundation/up.sql"),
        );

        for version in [
            "20250320062357",
            "20250702140210",
            "20260128233111",
            "20260203230221",
            "20260408090000",
            "20260410120000",
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
                id, api_key, name, description, access_control_policy_id, is_enabled, deleted_at,
                created_at, updated_at
            ) VALUES (
                3, 'system-secret', 'demo', NULL, NULL, 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO cost_catalogs (
                id, name, description, created_at, updated_at, deleted_at
            ) VALUES (
                100, 'demo-catalog', NULL, 1, 1, NULL
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name, is_enabled,
                deleted_at, created_at, updated_at
            ) VALUES (
                10, 1, 100, 'demo-model', NULL, 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO cost_catalog_versions (
                id, catalog_id, version, currency, source, effective_from, effective_until,
                is_enabled, created_at, updated_at
            ) VALUES (
                101, 100, 'v1', 'USD', NULL, 1, NULL, 1, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO request_log (
                id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
                real_model_name, request_received_at, llm_request_sent_at,
                llm_response_first_chunk_at, llm_response_completed_at, client_ip,
                llm_request_uri, llm_response_status, status, is_stream, estimated_cost_nanos,
                estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
                cost_snapshot_json, created_at, updated_at, total_input_tokens,
                total_output_tokens, input_text_tokens, output_text_tokens, input_image_tokens,
                output_image_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
                total_tokens, storage_type, user_request_body, llm_request_body,
                llm_response_body, user_response_body, user_api_type, llm_api_type
            ) VALUES (
                200, 3, 1, 10, 2, 'demo-model', 'demo-model', 123456, 123456, NULL, NULL, NULL,
                NULL, 200, 'SUCCESS', 0, 10, 'USD', 100, 101, NULL, 123456, 123456, 1, 1, NULL,
                NULL, NULL, NULL, 0, NULL, 0, 2, NULL, NULL, NULL, NULL, NULL, 'OPENAI', 'OPENAI'
            );",
        );

        connection
            .run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("remaining migrations should succeed");

        let row = diesel::sql_query(
            "SELECT first_used_at, is_archived FROM cost_catalog_versions WHERE id = 101",
        )
        .get_result::<CostCatalogVersionFreezeRow>(&mut connection)
        .expect("cost catalog version should be readable after migration");

        assert_eq!(row.first_used_at, Some(123456));
        assert!(!row.is_archived);
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
                id, api_key, name, description, access_control_policy_id, is_enabled, deleted_at,
                created_at, updated_at
            ) VALUES (
                3, 'system-secret', 'demo', NULL, NULL, 1, NULL, 1, 1
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

    #[test]
    fn sqlite_request_patch_rule_migration_replaces_legacy_tables_and_adds_request_log_trace_columns()
     {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("request-patch-rule.sqlite");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut connection =
            SqliteConnection::establish(&db_url).expect("sqlite connection should be established");

        connection
            .run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("migrations should run");

        let request_patch_table_count = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM sqlite_master
             WHERE type = 'table'
               AND name = 'request_patch_rule'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("request_patch_rule table count should be readable")
        .count;
        assert_eq!(request_patch_table_count, 1);

        for legacy_table in [
            "custom_field_definition",
            "provider_custom_field_assignment",
            "model_custom_field_assignment",
        ] {
            let legacy_table_count = diesel::sql_query(format!(
                "SELECT COUNT(*) AS count
                 FROM sqlite_master
                 WHERE type = 'table'
                   AND name = '{legacy_table}'"
            ))
            .get_result::<CountRow>(&mut connection)
            .expect("legacy table count should be readable")
            .count;
            assert_eq!(legacy_table_count, 0, "{legacy_table} should be removed");
        }

        for column in [
            "applied_request_patch_ids_json",
            "request_patch_summary_json",
        ] {
            let column_count = diesel::sql_query(format!(
                "SELECT COUNT(*) AS count
                 FROM pragma_table_info('request_log')
                 WHERE name = '{column}'"
            ))
            .get_result::<CountRow>(&mut connection)
            .expect("request_log column count should be readable")
            .count;
            assert_eq!(column_count, 1, "{column} should exist on request_log");
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
                id, provider_id, cost_catalog_id, model_name, real_model_name, is_enabled,
                deleted_at, created_at, updated_at
            ) VALUES (
                10, 1, NULL, 'demo-model', NULL, 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO request_patch_rule (
                id, provider_id, model_id, placement, target, operation, value_json, description,
                is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                100, 1, NULL, 'HEADER', 'x-demo', 'SET', '\"demo\"', NULL, 1, NULL, 1, 1
            );",
        );

        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_patch_rule (
                        id, provider_id, model_id, placement, target, operation, value_json,
                        description, is_enabled, deleted_at, created_at, updated_at
                    ) VALUES (
                        101, 1, NULL, 'HEADER', 'x-demo', 'SET', '\"another\"', NULL, 1, NULL, 1, 1
                    );"
                )
                .is_err(),
            "duplicate active provider identity should be rejected"
        );

        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_patch_rule (
                        id, provider_id, model_id, placement, target, operation, value_json,
                        description, is_enabled, deleted_at, created_at, updated_at
                    ) VALUES (
                        102, NULL, 10, 'QUERY', 'debug', 'REMOVE', 'true', NULL, 1, NULL, 1, 1
                    );"
                )
                .is_err(),
            "REMOVE with value_json should be rejected"
        );

        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_patch_rule (
                        id, provider_id, model_id, placement, target, operation, value_json,
                        description, is_enabled, deleted_at, created_at, updated_at
                    ) VALUES (
                        103, 1, 10, 'BODY', '/temperature', 'SET', '0.1', NULL, 1, NULL, 1, 1
                    );"
                )
                .is_err(),
            "provider/model xor constraint should be enforced"
        );

        assert!(
            connection
                .batch_execute(
                    "INSERT INTO request_patch_rule (
                        id, provider_id, model_id, placement, target, operation, value_json,
                        description, is_enabled, deleted_at, created_at, updated_at
                    ) VALUES (
                        104, NULL, 10, 'BODY', '/temperature', 'SET', 'not-json', NULL, 1, NULL, 1, 1
                    );"
                )
                .is_err(),
            "invalid json payload should be rejected"
        );
    }
}
