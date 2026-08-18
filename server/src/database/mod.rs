use crate::controller::BaseError;
#[cfg(test)]
use diesel::Connection;
use diesel::{
    PgConnection, QueryableByName, RunQueryDsl, SqliteConnection, connection::SimpleConnection,
    sql_types::Text,
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
use serde::Serialize;
use std::error::Error as StdError;
use std::fmt;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

#[cfg(test)]
use tempfile::TempDir;

pub mod api_key;
pub mod api_key_acl_rule;
pub mod api_key_rollup;
pub mod cost;
pub mod error;
pub mod manager_auth_instance;
pub mod manager_credential;
pub mod manager_totp_recovery_code;
pub mod metrics;
pub mod model;
pub mod model_source_binding;
pub mod provider;
pub mod provider_runtime;
pub mod request_log;
pub mod request_patch;
pub mod runtime;
pub mod startup;
pub mod stat;
#[cfg(test)]
pub mod test_support;
#[cfg(test)]
pub(crate) use test_support::TestDatabase;
pub mod upstream_source;
//pub mod record; // Assuming this will be replaced or removed if request_log supersedes it

#[cfg(test)]
mod migration_smoke_tests;
#[cfg(test)]
mod r3_22_postgres_boundary_tests;

pub enum DbType {
    Postgres,
    Sqlite,
}

fn parse_db_type(db_url: &str) -> DbType {
    if db_url.starts_with("postgres") {
        DbType::Postgres
    } else {
        DbType::Sqlite
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
    SqliteForeignKeyCheck {
        path: PathBuf,
        violations: i64,
    },
    Migration {
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
            Self::SqliteForeignKeyCheck { path, violations } => write!(
                f,
                "sqlite foreign key check failed for '{}': {violations} violation(s)",
                path.display()
            ),
            Self::Migration { backend, source } => {
                write!(f, "failed to run {backend} migrations: {source}")
            }
        }
    }
}

impl DatabaseInitError {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::SqliteConnection { .. } => "sqlite_connection",
            Self::PostgresConnection { .. } => "postgres_connection",
            Self::SqlitePragma { .. } => "sqlite_pragma",
            Self::SqliteForeignKeyCheck { .. } => "sqlite_foreign_key_check",
            Self::Migration { .. } => "migration",
        }
    }
}

impl StdError for DatabaseInitError {}

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

/// Variant of `db_object!` for aggregates that contain required domain enums
/// and therefore must not acquire an implicit Rust `Default` contract.
#[macro_export]
macro_rules! db_object_no_default {
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
            #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
            pub struct $name { $( $vis $field : $typ, )+ }
        )+

        pub mod _postgres_model {
            $( $crate::db_object! { @expand postgres |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
        pub mod _sqlite_model {
            $( $crate::db_object! { @expand sqlite |  $( #[$attr] )* | $name |  $( $( #[$field_attr] )* $field : $typ ),+ } )+
        }
    };
}

const SQLITE_UPGRADE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");
const POSTGRES_UPGRADE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/postgres");
// Clean baselines and ordered upgrades remain separate embedded migration sources.
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

pub(super) fn run_sqlite_migrations(connection: &mut SqliteConnection) -> MigrationBootstrapResult {
    repair_legacy_sqlite_schema(connection)?;

    if sqlite_user_table_count(connection)? == 0 {
        connection.run_pending_migrations(SQLITE_CLEAN_BASELINE_MIGRATIONS)?;
        record_sqlite_migration_versions(connection, SQLITE_ARCHIVED_UPGRADE_VERSIONS)?;
    }

    connection.run_pending_migrations(SQLITE_UPGRADE_MIGRATIONS)?;
    Ok(())
}

pub(super) fn run_postgres_migrations(connection: &mut PgConnection) -> MigrationBootstrapResult {
    if postgres_user_table_count(connection)? == 0 {
        connection.run_pending_migrations(POSTGRES_CLEAN_BASELINE_MIGRATIONS)?;
        record_postgres_migration_versions(connection, POSTGRES_ARCHIVED_UPGRADE_VERSIONS)?;
    }

    connection.run_pending_migrations(POSTGRES_UPGRADE_MIGRATIONS)?;
    Ok(())
}

pub(super) fn ensure_sqlite_db_file(db_url: &str) -> Result<(), DatabaseInitError> {
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

pub type DbResult<T> = Result<T, BaseError>;

#[derive(Serialize)]
pub struct ListResult<T> {
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
    pub list: Vec<T>,
}
