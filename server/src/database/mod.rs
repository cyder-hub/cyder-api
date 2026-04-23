use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl, SqliteConnection,
    connection::SimpleConnection,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    sql_types::Text,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use sha2::{Digest, Sha256};
use std::error::Error as StdError;
use std::fs::File;
use std::path::Path;
use std::sync::LazyLock;

use crate::{config::CONFIG, controller::BaseError};
use serde::Serialize;

#[cfg(test)]
use std::{
    cell::RefCell,
    future::Future,
    panic::{AssertUnwindSafe, resume_unwind},
    sync::Arc,
};

#[cfg(test)]
use tempfile::TempDir;

pub mod api_key;
pub mod api_key_acl_rule;
pub mod api_key_rollup;
pub mod cost;
pub mod model;
pub mod model_route;
pub mod provider;
pub mod provider_runtime;
pub mod request_attempt;
pub mod request_log;
pub mod request_patch;
pub mod request_replay_run;
pub mod stat;
//pub mod record; // Assuming this will be replaced or removed if request_log supersedes it

pub enum DbType {
    Postgres,
    Sqlite,
}

pub enum DbPool {
    Postgres(Pool<ConnectionManager<PgConnection>>),
    Sqlite(Pool<ConnectionManager<SqliteConnection>>),
}

impl Clone for DbPool {
    fn clone(&self) -> Self {
        match self {
            DbPool::Postgres(pool) => DbPool::Postgres(pool.clone()),
            DbPool::Sqlite(pool) => DbPool::Sqlite(pool.clone()),
        }
    }
}

pub enum DbConnection {
    Postgres(PooledConnection<ConnectionManager<PgConnection>>),
    Sqlite(PooledConnection<ConnectionManager<SqliteConnection>>),
}

pub fn get_connection() -> DbResult<DbConnection> {
    #[cfg(test)]
    {
        return get_connection_from_pool(&current_test_db_pool());
    }

    #[cfg(not(test))]
    {
        get_connection_from_pool(&DB_POOL)
    }
}

fn get_connection_from_pool(pool: &DbPool) -> DbResult<DbConnection> {
    match pool {
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
            #[cfg(test)]
            let mut conn = conn;
            #[cfg(test)]
            apply_test_sqlite_pragmas(&mut conn).map_err(|e| {
                BaseError::DatabaseFatal(Some(format!("Sqlite pragma error: {}", e)))
            })?;
            Ok(DbConnection::Sqlite(conn))
        }
    }
}

#[cfg(test)]
tokio::task_local! {
    static ACTIVE_TEST_DB_POOL: DbPool;
}

#[cfg(test)]
thread_local! {
    static TEST_DB_SCOPE_STACK: RefCell<Vec<DbPool>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
static DEFAULT_TEST_DB_DIR: LazyLock<TempDir> =
    LazyLock::new(|| tempfile::tempdir().expect("default test sqlite dir should be created"));

#[cfg(test)]
static DEFAULT_TEST_DB_POOL: LazyLock<DbPool> = LazyLock::new(|| {
    let db_url = DEFAULT_TEST_DB_DIR
        .path()
        .join("server-unit-tests.sqlite")
        .to_string_lossy()
        .into_owned();
    DbPool::establish_for_url(&db_url)
});

#[cfg(test)]
fn current_test_db_pool() -> DbPool {
    ACTIVE_TEST_DB_POOL
        .try_with(Clone::clone)
        .ok()
        .or_else(|| TEST_DB_SCOPE_STACK.with(|stack| stack.borrow().last().cloned()))
        .unwrap_or_else(|| DEFAULT_TEST_DB_POOL.clone())
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestDbContext {
    inner: Arc<TestDbContextInner>,
}

#[cfg(test)]
struct TestDbContextInner {
    _temp_dir: TempDir,
    pool: DbPool,
}

#[cfg(test)]
struct TestDbScopeGuard;

#[cfg(test)]
impl Drop for TestDbScopeGuard {
    fn drop(&mut self) {
        TEST_DB_SCOPE_STACK.with(|stack| {
            let popped = stack.borrow_mut().pop();
            debug_assert!(popped.is_some(), "test db scope stack should not underflow");
        });
    }
}

#[cfg(test)]
impl TestDbContext {
    pub(crate) fn new_sqlite(file_name: &str) -> Self {
        let temp_dir = tempfile::tempdir().expect("test sqlite temp dir should be created");
        let db_url = temp_dir
            .path()
            .join(file_name)
            .to_string_lossy()
            .into_owned();
        let pool = DbPool::establish_for_url(&db_url);

        Self {
            inner: Arc::new(TestDbContextInner {
                _temp_dir: temp_dir,
                pool,
            }),
        }
    }

    pub(crate) fn run_sync<R>(&self, operation: impl FnOnce() -> R) -> R {
        let _guard = self.enter_scope();
        match std::panic::catch_unwind(AssertUnwindSafe(operation)) {
            Ok(result) => result,
            Err(panic_payload) => resume_unwind(panic_payload),
        }
    }

    pub(crate) async fn run_async<F>(&self, future: F) -> F::Output
    where
        F: Future,
    {
        ACTIVE_TEST_DB_POOL
            .scope(self.inner.pool.clone(), future)
            .await
    }

    pub(crate) fn spawn<F>(&self, future: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(ACTIVE_TEST_DB_POOL.scope(self.inner.pool.clone(), future))
    }

    fn enter_scope(&self) -> TestDbScopeGuard {
        TEST_DB_SCOPE_STACK.with(|stack| {
            stack.borrow_mut().push(self.inner.pool.clone());
        });
        TestDbScopeGuard
    }
}

#[cfg(test)]
pub(crate) fn open_test_sqlite_connection(file_name: &str) -> (TempDir, SqliteConnection) {
    let (temp_dir, db_url) = create_test_sqlite_db(file_name);
    let mut connection =
        SqliteConnection::establish(&db_url).expect("sqlite connection should be established");
    apply_test_sqlite_pragmas(&mut connection).expect("sqlite test pragmas should apply");
    (temp_dir, connection)
}

#[cfg(test)]
pub(crate) fn open_test_sqlite_connection_with_migrations(
    file_name: &str,
) -> (TempDir, SqliteConnection) {
    let (temp_dir, mut connection) = open_test_sqlite_connection(file_name);
    run_sqlite_migrations(&mut connection).expect("sqlite migrations should run");
    (temp_dir, connection)
}

#[cfg(test)]
pub(crate) fn open_test_sqlite_pooled_connection_with_migrations(
    file_name: &str,
) -> (
    TempDir,
    PooledConnection<ConnectionManager<SqliteConnection>>,
) {
    let (temp_dir, db_url) = create_test_sqlite_db(file_name);
    let manager = ConnectionManager::<SqliteConnection>::new(db_url);
    let pool = Pool::builder()
        .max_size(test_sqlite_pool_size())
        .build(manager)
        .expect("sqlite pool should be created");
    let mut connection = pool
        .get()
        .expect("sqlite pooled connection should be checked out");
    apply_test_sqlite_pragmas(&mut connection).expect("sqlite test pragmas should apply");
    run_sqlite_migrations(&mut connection).expect("sqlite migrations should run");
    (temp_dir, connection)
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
        Self::establish_for_url(&CONFIG.db_url)
    }

    fn establish_for_url(db_url: &str) -> Self {
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

#[cfg_attr(test, allow(dead_code))]
static DB_POOL: LazyLock<DbPool> = LazyLock::new(DbPool::establish);
const SQLITE_UPGRADE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");
const POSTGRES_UPGRADE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/postgres");
const SQLITE_CLEAN_BASELINE_MIGRATIONS: EmbeddedMigrations =
    embed_migrations!("migrations/sqlite_clean");
const POSTGRES_CLEAN_BASELINE_MIGRATIONS: EmbeddedMigrations =
    embed_migrations!("migrations/postgres_clean");

#[cfg(test)]
const SQLITE_CLEAN_BASELINE_VERSION: &str = "20260423180000";

const SQLITE_ARCHIVED_UPGRADE_VERSIONS: &[&str] = &[
    "20250320062357",
    "20250702140210",
    "20260128233111",
    "20260203230221",
    "20260408090000",
    "20260410120000",
    "20260414090000",
    "20260417100000",
    "20260417120000",
    "20260417130000",
    "20260420120000",
    "20260421090000",
    "20260422120000",
    "20260423120000",
];

const POSTGRES_ARCHIVED_UPGRADE_VERSIONS: &[&str] = &[
    "20250320062357",
    "20250702140210",
    "20250710221420",
    "20250923220412",
    "20260128233111",
    "20260203230221",
    "20260408083622",
    "20260410120000",
    "20260414090000",
    "20260417100000",
    "20260417120000",
    "20260417130000",
    "20260420120000",
    "20260421090000",
    "20260422120000",
    "20260423120000",
];

type MigrationBootstrapResult = Result<(), Box<dyn StdError + Send + Sync>>;

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

#[derive(QueryableByName)]
struct DbCountRow {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
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

fn sqlite_user_table_count(
    connection: &mut SqliteConnection,
) -> Result<i64, diesel::result::Error> {
    diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM sqlite_master
         WHERE type = 'table'
           AND name NOT LIKE 'sqlite_%'
           AND name <> '__diesel_schema_migrations'",
    )
    .get_result::<DbCountRow>(connection)
    .map(|row| row.count)
}

fn postgres_user_table_count(connection: &mut PgConnection) -> Result<i64, diesel::result::Error> {
    diesel::sql_query(
        "SELECT COUNT(*) AS count
         FROM information_schema.tables
         WHERE table_schema = current_schema()
           AND table_type = 'BASE TABLE'
           AND table_name <> '__diesel_schema_migrations'",
    )
    .get_result::<DbCountRow>(connection)
    .map(|row| row.count)
}

fn record_sqlite_migration_versions(
    connection: &mut SqliteConnection,
    versions: &[&str],
) -> MigrationBootstrapResult {
    for version in versions {
        diesel::sql_query("INSERT OR IGNORE INTO __diesel_schema_migrations (version) VALUES (?)")
            .bind::<Text, _>(*version)
            .execute(connection)?;
    }

    Ok(())
}

fn record_postgres_migration_versions(
    connection: &mut PgConnection,
    versions: &[&str],
) -> MigrationBootstrapResult {
    for version in versions {
        diesel::sql_query(
            "INSERT INTO __diesel_schema_migrations (version)
             VALUES ($1)
             ON CONFLICT (version) DO NOTHING",
        )
        .bind::<Text, _>(*version)
        .execute(connection)?;
    }

    Ok(())
}

fn run_sqlite_migrations(connection: &mut SqliteConnection) -> MigrationBootstrapResult {
    repair_legacy_sqlite_schema(connection)?;

    if sqlite_user_table_count(connection)? == 0 {
        connection.run_pending_migrations(SQLITE_CLEAN_BASELINE_MIGRATIONS)?;
        record_sqlite_migration_versions(connection, SQLITE_ARCHIVED_UPGRADE_VERSIONS)?;
    }

    connection.run_pending_migrations(SQLITE_UPGRADE_MIGRATIONS)?;
    Ok(())
}

fn run_postgres_migrations(connection: &mut PgConnection) -> MigrationBootstrapResult {
    if postgres_user_table_count(connection)? == 0 {
        connection.run_pending_migrations(POSTGRES_CLEAN_BASELINE_MIGRATIONS)?;
        record_postgres_migration_versions(connection, POSTGRES_ARCHIVED_UPGRADE_VERSIONS)?;
    }

    connection.run_pending_migrations(POSTGRES_UPGRADE_MIGRATIONS)?;
    Ok(())
}

fn ensure_sqlite_db_file(db_url: &str) {
    let db_path = Path::new(db_url);
    if !db_path.exists() {
        if let Some(parent_dir) = db_path.parent() {
            if !parent_dir.exists() {
                std::fs::create_dir_all(parent_dir).expect("failed to create database directory");
            }
        }
        File::create(db_path).expect("failed to create database file");
    }
}

#[cfg(test)]
const SQLITE_TEST_BUSY_TIMEOUT_MS: u64 = 5_000;

#[cfg(test)]
fn test_sqlite_pool_size() -> u32 {
    2
}

#[cfg(test)]
fn create_test_sqlite_db(file_name: &str) -> (TempDir, String) {
    let temp_dir = tempfile::tempdir().expect("temp dir should be created");
    let db_url = temp_dir
        .path()
        .join(file_name)
        .to_string_lossy()
        .into_owned();
    ensure_sqlite_db_file(&db_url);
    (temp_dir, db_url)
}

#[cfg(test)]
fn apply_test_sqlite_pragmas(
    connection: &mut SqliteConnection,
) -> Result<(), diesel::result::Error> {
    connection.batch_execute(&format!(
        "PRAGMA journal_mode = WAL; PRAGMA busy_timeout = {SQLITE_TEST_BUSY_TIMEOUT_MS};"
    ))
}

fn init_sqlite_pool(db_url: &str) -> Pool<ConnectionManager<SqliteConnection>> {
    ensure_sqlite_db_file(db_url);

    let mut connection =
        SqliteConnection::establish(db_url).expect("failed to establish migration connection");

    #[cfg(test)]
    apply_test_sqlite_pragmas(&mut connection).expect("sqlite test pragmas should apply");

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

    run_sqlite_migrations(&mut connection).expect("failed to run sqlite migrations");
    backfill_api_key_shadow_sqlite(&mut connection)
        .expect("failed to backfill api_key shadow table");

    let manager = ConnectionManager::<SqliteConnection>::new(db_url);
    Pool::builder()
        .test_on_check_out(true)
        .max_size({
            #[cfg(test)]
            {
                test_sqlite_pool_size()
            }

            #[cfg(not(test))]
            {
                CONFIG.db_pool_size
            }
        })
        .build(manager)
        .expect("Failed to create pool.")
}

fn init_pg_pool(db_url: &str) -> Pool<ConnectionManager<PgConnection>> {
    let mut connection =
        PgConnection::establish(db_url).expect("failed to establish migration connection");

    run_postgres_migrations(&mut connection).expect("failed to run postgres migrations");
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

    fn apply_sql(connection: &mut SqliteConnection, sql_text: &str) {
        if let Err(err) = connection.batch_execute(sql_text) {
            panic!("sql should execute successfully: {err}\n{sql_text}");
        }
    }

    fn provider_exists(connection: &mut SqliteConnection, provider_id: i64) -> bool {
        diesel::sql_query("SELECT COUNT(*) AS count FROM provider WHERE id = ?")
            .bind::<diesel::sql_types::BigInt, _>(provider_id)
            .get_result::<DbCountRow>(connection)
            .map(|row| row.count > 0)
            .expect("provider existence query should succeed")
    }

    fn insert_provider_marker(connection: &mut SqliteConnection, provider_id: i64) {
        diesel::sql_query(
            "INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at, created_at,
                updated_at, provider_type, provider_api_key_mode
            ) VALUES (?, ?, ?, ?, 0, 1, NULL, 1, 1, 'OPENAI', 'QUEUE')",
        )
        .bind::<diesel::sql_types::BigInt, _>(provider_id)
        .bind::<diesel::sql_types::Text, _>(format!("provider-{provider_id}"))
        .bind::<diesel::sql_types::Text, _>(format!("Provider {provider_id}"))
        .bind::<diesel::sql_types::Text, _>("https://example.com")
        .execute(connection)
        .expect("provider marker should insert");
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
        let (_temp_dir, mut connection) = open_test_sqlite_connection("legacy.sqlite");

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

        run_sqlite_migrations(&mut connection).expect("remaining sqlite migrations should succeed");

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
    struct ApiKeyShadowRow {
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

    #[test]
    fn sqlite_api_key_shadow_backfill_populates_hash_and_request_log_links() {
        let (_temp_dir, mut connection) =
            open_test_sqlite_connection_with_migrations("api-key-shadow.sqlite");

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
                10, 1, NULL, 'demo-model', 'demo-model', 1, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                3, 'cyder-abcdefghijklmnopqrstuvwxyz', NULL, 'cyder-abcdef', 'wxyz', 'demo', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO api_key_acl_rule (
                id, api_key_id, effect, scope, provider_id, model_id, priority, is_enabled,
                description, created_at, updated_at, deleted_at
            ) VALUES (
                31, 3, 'DENY', 'MODEL', 1, 10, 5, 1, 'deny demo model', 1, 1, NULL
            );",
        );
        apply_sql(
            &mut connection,
            "INSERT INTO request_log (
                id, api_key_id, requested_model_name, resolved_name_scope, user_api_type,
                overall_status, attempt_count, retry_count, fallback_count, request_received_at,
                created_at, updated_at, has_transform_diagnostics, transform_diagnostic_count
            ) VALUES (
                20, 3, 'demo-model', 'direct', 'OPENAI',
                'SUCCESS', 1, 0, 0, 123456,
                123456, 123456, 0, 0
            );",
        );

        backfill_api_key_shadow_sqlite(&mut connection)
            .expect("api_key shadow backfill should succeed");

        let api_key = diesel::sql_query(
            "SELECT id, api_key_hash, key_prefix, key_last4, default_action
             FROM api_key
             WHERE id = 3",
        )
        .get_result::<ApiKeyShadowRow>(&mut connection)
        .expect("api_key row should be readable");

        assert_eq!(api_key.id, 3);
        assert_eq!(
            api_key.api_key_hash,
            compute_api_key_hash("cyder-abcdefghijklmnopqrstuvwxyz")
        );
        assert_eq!(api_key.key_prefix, "cyder-abcdef");
        assert_eq!(api_key.key_last4, "wxyz");
        assert_eq!(api_key.default_action, "ALLOW");

        let acl_rule_count = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM api_key_acl_rule
             WHERE api_key_id = 3
               AND scope = 'MODEL'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("api_key_acl_rule count should be readable")
        .count;
        assert_eq!(acl_rule_count, 1);

        let join_count = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM request_log AS rl
             JOIN api_key AS ak
               ON rl.api_key_id = ak.id
             WHERE rl.id = 20
               AND ak.id = 3",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("request_log/api_key join count should be readable")
        .count;
        assert_eq!(join_count, 1);
    }

    #[test]
    fn sqlite_fresh_install_bootstraps_clean_baseline_and_marks_upgrade_history_applied() {
        let (_temp_dir, mut connection) = open_test_sqlite_connection("clean-baseline.sqlite");
        let legacy_tables = [
            ["system", "_api_key"].concat(),
            ["access", "_control_rule"].concat(),
            ["access", "_control_policy"].concat(),
            ["model", "_alias"].concat(),
        ];

        run_sqlite_migrations(&mut connection)
            .expect("fresh install should bootstrap the clean baseline");

        for table_name in [
            "api_key",
            "request_log",
            "request_attempt",
            "request_patch_rule",
        ] {
            assert!(
                sqlite_table_exists(&mut connection, table_name),
                "{table_name} should exist after clean baseline bootstrap"
            );
        }

        for table_name in &legacy_tables {
            assert!(
                !sqlite_table_exists(&mut connection, table_name.as_str()),
                "{table_name} should not exist after clean baseline bootstrap"
            );
        }

        let baseline_count = diesel::sql_query(format!(
            "SELECT COUNT(*) AS count
             FROM __diesel_schema_migrations
             WHERE version = '{SQLITE_CLEAN_BASELINE_VERSION}'"
        ))
        .get_result::<CountRow>(&mut connection)
        .expect("clean baseline migration count should be readable")
        .count;
        assert_eq!(baseline_count, 1);

        let archived_upgrade_count = diesel::sql_query(
            "SELECT COUNT(*) AS count
             FROM __diesel_schema_migrations
             WHERE version = '20250702140210'
                OR version = '20260423120000'",
        )
        .get_result::<CountRow>(&mut connection)
        .expect("archived upgrade migration count should be readable")
        .count;
        assert_eq!(archived_upgrade_count, 2);
    }

    fn sqlite_table_exists(connection: &mut SqliteConnection, table_name: &str) -> bool {
        super::sqlite_table_has_column(connection, table_name, "id")
            .expect("table existence should be readable")
            .unwrap_or(false)
    }

    fn apply_sqlite_migrations_through_request_diagnostics_replay(
        connection: &mut SqliteConnection,
    ) {
        for (version, sql_text) in [
            (
                "20250320062357",
                include_str!("../../migrations/sqlite/2025-03-20-062357_initial_setup/up.sql"),
            ),
            (
                "20250702140210",
                include_str!("../../migrations/sqlite/2025-07-02-140210_api_key_jwt/up.sql"),
            ),
            (
                "20260128233111",
                include_str!(
                    "../../migrations/sqlite/2026-01-28-233111_request_log_optimize/up.sql"
                ),
            ),
            (
                "20260203230221",
                include_str!(
                    "../../migrations/sqlite/2026-02-03-230221_request_log_field_opt/up.sql"
                ),
            ),
            (
                "20260408090000",
                include_str!(
                    "../../migrations/sqlite/2026-04-08-090000_expand_llm_api_type_for_request_log/up.sql"
                ),
            ),
            (
                "20260410120000",
                include_str!(
                    "../../migrations/sqlite/2026-04-10-120000_cost_schema_foundation/up.sql"
                ),
            ),
            (
                "20260414090000",
                include_str!(
                    "../../migrations/sqlite/2026-04-14-090000_cost_catalog_version_freeze_flags/up.sql"
                ),
            ),
            (
                "20260417100000",
                include_str!(
                    "../../migrations/sqlite/2026-04-17-100000_model_route_foundation/up.sql"
                ),
            ),
            (
                "20260417120000",
                include_str!(
                    "../../migrations/sqlite/2026-04-17-120000_api_key_governance_foundation/up.sql"
                ),
            ),
            (
                "20260417130000",
                include_str!(
                    "../../migrations/sqlite/2026-04-17-130000_request_log_route_trace/up.sql"
                ),
            ),
            (
                "20260420120000",
                include_str!(
                    "../../migrations/sqlite/2026-04-20-120000_request_patch_rule_foundation/up.sql"
                ),
            ),
            (
                "20260421090000",
                include_str!(
                    "../../migrations/sqlite/2026-04-21-090000_routing_resilience_foundation/up.sql"
                ),
            ),
            (
                "20260422120000",
                include_str!(
                    "../../migrations/sqlite/2026-04-22-120000_request_diagnostics_replay_foundation/up.sql"
                ),
            ),
        ] {
            apply_sql(connection, sql_text);
            mark_sqlite_migration_applied(connection, version);
        }
    }

    #[test]
    fn sqlite_drop_legacy_tables_migration_removes_legacy_tables_on_existing_schema() {
        let (_temp_dir, mut connection) = open_test_sqlite_connection("drop-legacy.sqlite");
        let legacy_tables = [
            ["system", "_api_key"].concat(),
            ["access", "_control_rule"].concat(),
            ["access", "_control_policy"].concat(),
            ["model", "_alias"].concat(),
        ];

        apply_sqlite_migrations_through_request_diagnostics_replay(&mut connection);

        for table_name in &legacy_tables {
            assert!(
                sqlite_table_exists(&mut connection, table_name.as_str()),
                "{table_name} should exist before the drop migration runs"
            );
        }

        run_sqlite_migrations(&mut connection)
            .expect("drop migration should succeed on the current sqlite schema");

        for table_name in &legacy_tables {
            assert!(
                !sqlite_table_exists(&mut connection, table_name.as_str()),
                "{table_name} should be removed by the drop migration"
            );
        }

        assert!(sqlite_table_exists(&mut connection, "api_key"));
        assert!(sqlite_table_exists(&mut connection, "request_log"));
    }

    #[test]
    fn sqlite_request_patch_rule_migration_replaces_legacy_tables_and_adds_request_log_trace_columns()
     {
        let (_temp_dir, mut connection) =
            open_test_sqlite_connection_with_migrations("request-patch-rule.sqlite");

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
                 FROM pragma_table_info('request_attempt')
                 WHERE name = '{column}'"
            ))
            .get_result::<CountRow>(&mut connection)
            .expect("request_attempt column count should be readable")
            .count;
            assert_eq!(column_count, 1, "{column} should exist on request_attempt");
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

    #[test]
    fn test_db_context_run_sync_uses_scoped_pool_and_restores_default_after_exit() {
        let scoped = TestDbContext::new_sqlite("scoped-run-sync.sqlite");
        let marker_id = 991_001;

        scoped.run_sync(|| {
            let DbConnection::Sqlite(mut connection) =
                get_connection().expect("scoped sqlite connection should be available")
            else {
                panic!("expected sqlite connection");
            };
            insert_provider_marker(&mut connection, marker_id);
            assert!(provider_exists(&mut connection, marker_id));
        });

        let DbConnection::Sqlite(mut default_connection) =
            get_connection().expect("default sqlite connection should be available")
        else {
            panic!("expected sqlite connection");
        };
        assert!(!provider_exists(&mut default_connection, marker_id));
    }

    #[tokio::test]
    async fn test_db_context_run_async_uses_scoped_pool() {
        let scoped = TestDbContext::new_sqlite("scoped-run-async.sqlite");
        let marker_id = 991_002;

        scoped
            .run_async(async {
                let DbConnection::Sqlite(mut connection) =
                    get_connection().expect("scoped sqlite connection should be available")
                else {
                    panic!("expected sqlite connection");
                };
                insert_provider_marker(&mut connection, marker_id);
                assert!(provider_exists(&mut connection, marker_id));
            })
            .await;

        let DbConnection::Sqlite(mut default_connection) =
            get_connection().expect("default sqlite connection should be available")
        else {
            panic!("expected sqlite connection");
        };
        assert!(!provider_exists(&mut default_connection, marker_id));
    }

    #[test]
    fn nested_test_db_context_scopes_restore_outer_pool() {
        let outer = TestDbContext::new_sqlite("outer-scope.sqlite");
        let inner = TestDbContext::new_sqlite("inner-scope.sqlite");
        let outer_marker = 991_003;
        let inner_marker = 991_004;

        outer.run_sync(|| {
            let DbConnection::Sqlite(mut outer_connection) =
                get_connection().expect("outer sqlite connection should be available")
            else {
                panic!("expected sqlite connection");
            };
            insert_provider_marker(&mut outer_connection, outer_marker);
            assert!(provider_exists(&mut outer_connection, outer_marker));
            assert!(!provider_exists(&mut outer_connection, inner_marker));
            drop(outer_connection);

            inner.run_sync(|| {
                let DbConnection::Sqlite(mut inner_connection) =
                    get_connection().expect("inner sqlite connection should be available")
                else {
                    panic!("expected sqlite connection");
                };
                insert_provider_marker(&mut inner_connection, inner_marker);
                assert!(provider_exists(&mut inner_connection, inner_marker));
                assert!(!provider_exists(&mut inner_connection, outer_marker));
            });

            let DbConnection::Sqlite(mut restored_outer_connection) =
                get_connection().expect("outer sqlite connection should be restored")
            else {
                panic!("expected sqlite connection");
            };
            assert!(provider_exists(
                &mut restored_outer_connection,
                outer_marker
            ));
            assert!(!provider_exists(
                &mut restored_outer_connection,
                inner_marker
            ));
        });
    }
}
