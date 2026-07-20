use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl, SqliteConnection,
    connection::SimpleConnection,
    r2d2::{ConnectionManager, Pool, PooledConnection},
    sql_types::Text,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use sha2::{Digest, Sha256};
use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
#[cfg(test)]
use std::sync::LazyLock;
use std::sync::{Mutex, OnceLock};

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
pub mod manager_auth_instance;
pub mod manager_credential;
pub mod metrics;
pub mod model;
pub mod provider;
pub mod provider_runtime;
pub mod reasoning_config;
pub mod request_log;
pub mod request_patch;
pub mod runtime_feature_config;
pub mod stat;
//pub mod record; // Assuming this will be replaced or removed if request_log supersedes it

#[cfg(test)]
mod migration_smoke_tests;

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
        match global_db_pool() {
            Ok(pool) => get_connection_from_pool(pool),
            Err(err) => Err(BaseError::DatabaseFatal(Some(err.to_string()))),
        }
    }
}

#[cfg(not(test))]
fn global_db_pool() -> Result<&'static DbPool, DatabaseInitError> {
    get_or_try_init_retryable(&DB_POOL, &DB_POOL_INIT_LOCK, DbPool::establish)
}

fn get_or_try_init_retryable<T: 'static, E>(
    cell: &'static OnceLock<T>,
    init_lock: &'static Mutex<()>,
    init: impl FnOnce() -> Result<T, E>,
) -> Result<&'static T, E> {
    if let Some(value) = cell.get() {
        return Ok(value);
    }

    let _guard = init_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(value) = cell.get() {
        return Ok(value);
    }

    let value = init()?;
    if cell.set(value).is_err() {
        return Ok(cell
            .get()
            .expect("retryable global initializer should be set"));
    }
    Ok(cell
        .get()
        .expect("retryable global initializer should be set"))
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

#[derive(Debug)]
pub enum DatabaseInitError {
    Io {
        operation: &'static str,
        path: PathBuf,
        source: io::Error,
    },
    SqliteConnection {
        path: PathBuf,
        source: diesel::ConnectionError,
    },
    PostgresConnection {
        source: diesel::ConnectionError,
    },
    SqlitePragma {
        path: PathBuf,
        source: diesel::result::Error,
    },
    Migration {
        backend: &'static str,
        source: String,
    },
    Backfill {
        backend: &'static str,
        source: diesel::result::Error,
    },
    Pool {
        backend: &'static str,
        source: String,
    },
}

impl fmt::Display for DatabaseInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                path,
                source,
            } => write!(f, "failed to {operation} '{}': {source}", path.display()),
            Self::SqliteConnection { path, source } => write!(
                f,
                "failed to establish sqlite migration connection for '{}': {source}",
                path.display()
            ),
            Self::PostgresConnection { source } => {
                write!(
                    f,
                    "failed to establish postgres migration connection: {source}"
                )
            }
            Self::SqlitePragma { path, source } => write!(
                f,
                "failed to apply sqlite pragmas for '{}': {source}",
                path.display()
            ),
            Self::Migration { backend, source } => {
                write!(f, "failed to run {backend} migrations: {source}")
            }
            Self::Backfill { backend, source } => {
                write!(
                    f,
                    "failed to backfill {backend} api_key shadow table: {source}"
                )
            }
            Self::Pool { backend, source } => {
                write!(f, "failed to create {backend} database pool: {source}")
            }
        }
    }
}

impl StdError for DatabaseInitError {}

impl DbPool {
    pub fn establish() -> Result<Self, DatabaseInitError> {
        Self::try_establish_for_url(&CONFIG.db_url)
    }

    #[cfg(test)]
    fn establish_for_url(db_url: &str) -> Self {
        Self::try_establish_for_url(db_url)
            .unwrap_or_else(|err| panic!("failed to initialize database: {err}"))
    }

    fn try_establish_for_url(db_url: &str) -> Result<Self, DatabaseInitError> {
        let db_type = parse_db_type(db_url);
        Ok(match db_type {
            DbType::Postgres => {
                let pool = init_pg_pool(db_url)?;
                DbPool::Postgres(pool)
            }
            DbType::Sqlite => {
                let pool = init_sqlite_pool(db_url)?;
                DbPool::Sqlite(pool)
            }
        })
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
static DB_POOL: OnceLock<DbPool> = OnceLock::new();
#[cfg_attr(test, allow(dead_code))]
static DB_POOL_INIT_LOCK: Mutex<()> = Mutex::new(());
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

fn ensure_sqlite_db_file(db_url: &str) -> Result<(), DatabaseInitError> {
    let db_path = Path::new(db_url);
    if db_path.exists() {
        if db_path.is_file() {
            return Ok(());
        }
        return Err(DatabaseInitError::Io {
            operation: "create sqlite database file",
            path: db_path.to_path_buf(),
            source: io::Error::new(
                io::ErrorKind::AlreadyExists,
                "path exists but is not a file",
            ),
        });
    }

    if let Some(parent_dir) = db_path.parent().filter(|path| !path.as_os_str().is_empty()) {
        if parent_dir.exists() && !parent_dir.is_dir() {
            return Err(DatabaseInitError::Io {
                operation: "create sqlite database directory",
                path: parent_dir.to_path_buf(),
                source: io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "path exists but is not a directory",
                ),
            });
        }
        if !parent_dir.exists() {
            std::fs::create_dir_all(parent_dir).map_err(|source| DatabaseInitError::Io {
                operation: "create sqlite database directory",
                path: parent_dir.to_path_buf(),
                source,
            })?;
        }
    }

    File::create(db_path).map_err(|source| DatabaseInitError::Io {
        operation: "create sqlite database file",
        path: db_path.to_path_buf(),
        source,
    })?;
    Ok(())
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
    ensure_sqlite_db_file(&db_url).expect("test sqlite db file should be created");
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

fn init_sqlite_pool(
    db_url: &str,
) -> Result<Pool<ConnectionManager<SqliteConnection>>, DatabaseInitError> {
    ensure_sqlite_db_file(db_url)?;
    let db_path = PathBuf::from(db_url);

    let mut connection = SqliteConnection::establish(db_url).map_err(|source| {
        DatabaseInitError::SqliteConnection {
            path: db_path.clone(),
            source,
        }
    })?;

    #[cfg(test)]
    apply_test_sqlite_pragmas(&mut connection).map_err(|source| {
        DatabaseInitError::SqlitePragma {
            path: db_path.clone(),
            source,
        }
    })?;

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

    run_sqlite_migrations(&mut connection).map_err(|source| DatabaseInitError::Migration {
        backend: "sqlite",
        source: source.to_string(),
    })?;
    backfill_api_key_shadow_sqlite(&mut connection).map_err(|source| {
        DatabaseInitError::Backfill {
            backend: "sqlite",
            source,
        }
    })?;

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
        .map_err(|source| DatabaseInitError::Pool {
            backend: "sqlite",
            source: source.to_string(),
        })
}

fn init_pg_pool(db_url: &str) -> Result<Pool<ConnectionManager<PgConnection>>, DatabaseInitError> {
    let mut connection = PgConnection::establish(db_url)
        .map_err(|source| DatabaseInitError::PostgresConnection { source })?;

    run_postgres_migrations(&mut connection).map_err(|source| DatabaseInitError::Migration {
        backend: "postgres",
        source: source.to_string(),
    })?;
    backfill_api_key_shadow_postgres(&mut connection).map_err(|source| {
        DatabaseInitError::Backfill {
            backend: "postgres",
            source,
        }
    })?;

    let manager = ConnectionManager::<PgConnection>::new(db_url);
    Pool::builder()
        .max_size(CONFIG.db_pool_size)
        .build(manager)
        .map_err(|source| DatabaseInitError::Pool {
            backend: "postgres",
            source: source.to_string(),
        })
}

pub type DbResult<T> = Result<T, BaseError>;

#[derive(Serialize)]
pub struct ListResult<T> {
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub list: Vec<T>,
}
