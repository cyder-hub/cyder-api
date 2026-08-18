use std::{path::PathBuf, time::Duration};

use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl, SqliteConnection,
    connection::SimpleConnection,
    sql_types::{BigInt, Text},
};

use super::{
    DatabaseInitError, ensure_sqlite_db_file, parse_db_type, run_postgres_migrations,
    run_sqlite_migrations,
};
use crate::controller::BaseError;

pub(crate) type StoredDownstreamSecret = (
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
);

pub(crate) type StoredProviderSecret = (
    i64,
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<String>,
);

pub(crate) type StoredManagerTotpSecret = (
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

pub(crate) struct DownstreamSecretRotation {
    pub id: i64,
    pub expected_ciphertext: Vec<u8>,
    pub expected_nonce: Vec<u8>,
    pub expected_format_version: i32,
    pub expected_fingerprint: String,
    pub replacement_ciphertext: Vec<u8>,
    pub replacement_nonce: Vec<u8>,
    pub replacement_format_version: i32,
    pub replacement_fingerprint: String,
}

pub(crate) struct ManagerTotpSecretRotation {
    pub expected_epoch: String,
    pub expected_ciphertext: Vec<u8>,
    pub expected_nonce: Vec<u8>,
    pub expected_format_version: i32,
    pub expected_fingerprint: String,
    pub expected_last_accepted_step: i64,
    pub expected_enabled_at: i64,
    pub replacement_ciphertext: Vec<u8>,
    pub replacement_nonce: Vec<u8>,
    pub replacement_format_version: i32,
    pub replacement_fingerprint: String,
}

pub(crate) struct ProviderSecretRotation {
    pub id: i64,
    pub provider_id: i64,
    pub expected_ciphertext: Vec<u8>,
    pub expected_nonce: Vec<u8>,
    pub expected_format_version: i32,
    pub expected_fingerprint: String,
    pub expected_hmac: String,
    pub replacement_ciphertext: Vec<u8>,
    pub replacement_nonce: Vec<u8>,
    pub replacement_format_version: i32,
    pub replacement_fingerprint: String,
    pub replacement_hmac: String,
}

pub struct StartupDatabaseConnection {
    backend: StartupDatabaseBackend,
}

pub(crate) enum StartupDatabaseBackend {
    Postgres(PgConnection),
    Sqlite(SqliteConnection),
}

#[derive(QueryableByName)]
struct TextValue {
    #[diesel(sql_type = Text)]
    value: String,
}

#[derive(QueryableByName)]
struct CountValue {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

impl StartupDatabaseConnection {
    pub fn establish(
        db_url: &str,
        sqlite_busy_timeout: Duration,
    ) -> Result<Self, DatabaseInitError> {
        match parse_db_type(db_url) {
            super::DbType::Postgres => {
                let mut connection = PgConnection::establish(db_url)
                    .map_err(|source| DatabaseInitError::PostgresConnection { source })?;
                run_postgres_migrations(&mut connection).map_err(|source| {
                    DatabaseInitError::Migration {
                        backend: "postgres",
                        source: source.to_string(),
                    }
                })?;
                Ok(Self {
                    backend: StartupDatabaseBackend::Postgres(connection),
                })
            }
            super::DbType::Sqlite => {
                ensure_sqlite_db_file(db_url)?;
                let path = PathBuf::from(db_url);
                let mut connection = SqliteConnection::establish(db_url).map_err(|source| {
                    DatabaseInitError::SqliteConnection {
                        path: path.clone(),
                        source,
                    }
                })?;
                configure_sqlite(&mut connection, &path, sqlite_busy_timeout)?;
                run_sqlite_migrations(&mut connection).map_err(|source| {
                    DatabaseInitError::Migration {
                        backend: "sqlite",
                        source: source.to_string(),
                    }
                })?;
                verify_sqlite_foreign_keys(&mut connection, &path)?;
                Ok(Self {
                    backend: StartupDatabaseBackend::Sqlite(connection),
                })
            }
        }
    }

    pub(crate) fn backend_mut(&mut self) -> &mut StartupDatabaseBackend {
        &mut self.backend
    }

    #[cfg(test)]
    pub(crate) fn sqlite_connection(&mut self) -> Option<&mut SqliteConnection> {
        match &mut self.backend {
            StartupDatabaseBackend::Sqlite(connection) => Some(connection),
            StartupDatabaseBackend::Postgres(_) => None,
        }
    }
}

pub(crate) enum StartupSecretTransaction<'a> {
    Postgres(&'a mut PgConnection),
    Sqlite(&'a mut SqliteConnection),
}

macro_rules! startup_secret_execute {
    ($transaction:expr, $conn:ident, $block:block) => {{
        match $transaction {
            StartupSecretTransaction::Postgres($conn) => {
                use crate::database::_postgres_schema::*;
                use diesel::prelude::*;
                $block
            }
            StartupSecretTransaction::Sqlite($conn) => {
                use crate::database::_sqlite_schema::*;
                use diesel::prelude::*;
                $block
            }
        }
    }};
}

pub(crate) fn with_startup_secret_transaction<T>(
    connection: &mut StartupDatabaseConnection,
    operation: impl FnOnce(&mut StartupSecretTransaction<'_>) -> Result<T, BaseError>,
) -> Result<T, BaseError> {
    let mut operation = Some(operation);
    match connection.backend_mut() {
        StartupDatabaseBackend::Postgres(connection) => connection.transaction(|connection| {
            let mut transaction = StartupSecretTransaction::Postgres(connection);
            operation
                .take()
                .expect("startup secret transaction operation should run once")(
                &mut transaction
            )
        }),
        StartupDatabaseBackend::Sqlite(connection) => connection.transaction(|connection| {
            let mut transaction = StartupSecretTransaction::Sqlite(connection);
            operation
                .take()
                .expect("startup secret transaction operation should run once")(
                &mut transaction
            )
        }),
    }
}

impl StartupSecretTransaction<'_> {
    pub(crate) fn load_downstream_secrets(
        &mut self,
    ) -> Result<Vec<StoredDownstreamSecret>, BaseError> {
        startup_secret_execute!(self, connection, {
            api_key::table
                .filter(api_key::dsl::secret_ciphertext.is_not_null())
                .order(api_key::dsl::id.asc())
                .select((
                    api_key::dsl::id,
                    api_key::dsl::secret_ciphertext,
                    api_key::dsl::secret_nonce,
                    api_key::dsl::secret_format_version,
                    api_key::dsl::secret_key_fingerprint,
                ))
                .load::<StoredDownstreamSecret>(&mut **connection)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to scan downstream api key secrets: {error}"
                    )))
                })
        })
    }

    pub(crate) fn rotate_downstream_secret(
        &mut self,
        rotation: DownstreamSecretRotation,
    ) -> Result<usize, BaseError> {
        startup_secret_execute!(self, connection, {
            diesel::update(
                api_key::table.filter(
                    api_key::dsl::id
                        .eq(rotation.id)
                        .and(api_key::dsl::secret_ciphertext.eq(Some(rotation.expected_ciphertext)))
                        .and(api_key::dsl::secret_nonce.eq(Some(rotation.expected_nonce)))
                        .and(
                            api_key::dsl::secret_format_version
                                .eq(Some(rotation.expected_format_version)),
                        )
                        .and(
                            api_key::dsl::secret_key_fingerprint
                                .eq(Some(rotation.expected_fingerprint)),
                        ),
                ),
            )
            .set((
                api_key::dsl::secret_ciphertext.eq(Some(rotation.replacement_ciphertext)),
                api_key::dsl::secret_nonce.eq(Some(rotation.replacement_nonce)),
                api_key::dsl::secret_format_version.eq(Some(rotation.replacement_format_version)),
                api_key::dsl::secret_key_fingerprint.eq(Some(rotation.replacement_fingerprint)),
            ))
            .execute(&mut **connection)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to rotate downstream api key secret {}: {error}",
                    rotation.id
                )))
            })
        })
    }

    pub(crate) fn load_manager_totp_secret(
        &mut self,
    ) -> Result<Option<StoredManagerTotpSecret>, BaseError> {
        startup_secret_execute!(self, connection, {
            manager_credential::table
                .filter(manager_credential::dsl::manager_id.eq(0_i64))
                .select((
                    manager_credential::dsl::credential_epoch,
                    manager_credential::dsl::totp_secret_ciphertext,
                    manager_credential::dsl::totp_secret_nonce,
                    manager_credential::dsl::totp_secret_format_version,
                    manager_credential::dsl::totp_secret_key_fingerprint,
                    manager_credential::dsl::totp_last_accepted_step,
                    manager_credential::dsl::totp_enabled_at,
                ))
                .first::<StoredManagerTotpSecret>(&mut **connection)
                .optional()
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to scan manager TOTP secret: {error}"
                    )))
                })
        })
    }

    pub(crate) fn rotate_manager_totp_secret(
        &mut self,
        rotation: ManagerTotpSecretRotation,
    ) -> Result<usize, BaseError> {
        startup_secret_execute!(self, connection, {
            diesel::update(
                manager_credential::table.filter(
                    manager_credential::dsl::manager_id
                        .eq(0_i64)
                        .and(manager_credential::dsl::credential_epoch.eq(rotation.expected_epoch))
                        .and(
                            manager_credential::dsl::totp_secret_ciphertext
                                .eq(Some(rotation.expected_ciphertext)),
                        )
                        .and(
                            manager_credential::dsl::totp_secret_nonce
                                .eq(Some(rotation.expected_nonce)),
                        )
                        .and(
                            manager_credential::dsl::totp_secret_format_version
                                .eq(Some(rotation.expected_format_version)),
                        )
                        .and(
                            manager_credential::dsl::totp_secret_key_fingerprint
                                .eq(Some(rotation.expected_fingerprint)),
                        )
                        .and(
                            manager_credential::dsl::totp_last_accepted_step
                                .eq(Some(rotation.expected_last_accepted_step)),
                        )
                        .and(
                            manager_credential::dsl::totp_enabled_at
                                .eq(Some(rotation.expected_enabled_at)),
                        ),
                ),
            )
            .set((
                manager_credential::dsl::totp_secret_ciphertext
                    .eq(Some(rotation.replacement_ciphertext)),
                manager_credential::dsl::totp_secret_nonce.eq(Some(rotation.replacement_nonce)),
                manager_credential::dsl::totp_secret_format_version
                    .eq(Some(rotation.replacement_format_version)),
                manager_credential::dsl::totp_secret_key_fingerprint
                    .eq(Some(rotation.replacement_fingerprint)),
            ))
            .execute(&mut **connection)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to rotate manager TOTP secret: {error}"
                )))
            })
        })
    }

    pub(crate) fn load_provider_secrets(&mut self) -> Result<Vec<StoredProviderSecret>, BaseError> {
        startup_secret_execute!(self, connection, {
            provider_api_key::table
                .filter(provider_api_key::dsl::deleted_at.is_null())
                .order(provider_api_key::dsl::id.asc())
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::secret_ciphertext,
                    provider_api_key::dsl::secret_nonce,
                    provider_api_key::dsl::secret_format_version,
                    provider_api_key::dsl::secret_key_fingerprint,
                    provider_api_key::dsl::secret_hmac,
                ))
                .load::<StoredProviderSecret>(&mut **connection)
                .map_err(|_| {
                    BaseError::DatabaseFatal(Some(
                        "Failed to scan provider API key secrets".to_string(),
                    ))
                })
        })
    }

    pub(crate) fn rotate_provider_secret(
        &mut self,
        rotation: ProviderSecretRotation,
    ) -> Result<usize, BaseError> {
        startup_secret_execute!(self, connection, {
            diesel::update(
                provider_api_key::table.filter(
                    provider_api_key::dsl::id
                        .eq(rotation.id)
                        .and(provider_api_key::dsl::provider_id.eq(rotation.provider_id))
                        .and(provider_api_key::dsl::deleted_at.is_null())
                        .and(
                            provider_api_key::dsl::secret_ciphertext
                                .eq(Some(rotation.expected_ciphertext)),
                        )
                        .and(provider_api_key::dsl::secret_nonce.eq(Some(rotation.expected_nonce)))
                        .and(
                            provider_api_key::dsl::secret_format_version
                                .eq(Some(rotation.expected_format_version)),
                        )
                        .and(
                            provider_api_key::dsl::secret_key_fingerprint
                                .eq(Some(rotation.expected_fingerprint)),
                        )
                        .and(provider_api_key::dsl::secret_hmac.eq(Some(rotation.expected_hmac))),
                ),
            )
            .set((
                provider_api_key::dsl::secret_ciphertext.eq(Some(rotation.replacement_ciphertext)),
                provider_api_key::dsl::secret_nonce.eq(Some(rotation.replacement_nonce)),
                provider_api_key::dsl::secret_format_version
                    .eq(Some(rotation.replacement_format_version)),
                provider_api_key::dsl::secret_key_fingerprint
                    .eq(Some(rotation.replacement_fingerprint)),
                provider_api_key::dsl::secret_hmac.eq(Some(rotation.replacement_hmac)),
            ))
            .execute(&mut **connection)
            .map_err(|_| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to rotate provider API key secret {}",
                    rotation.id
                )))
            })
        })
    }
}

fn configure_sqlite(
    connection: &mut SqliteConnection,
    path: &PathBuf,
    busy_timeout: Duration,
) -> Result<(), DatabaseInitError> {
    let busy_timeout_ms = u64::try_from(busy_timeout.as_millis()).unwrap_or(u64::MAX);
    connection
        .batch_execute(&format!(
            "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; PRAGMA busy_timeout = {busy_timeout_ms};"
        ))
        .map_err(|source| DatabaseInitError::SqlitePragma {
            path: path.clone(),
            source,
        })?;

    let journal_mode = diesel::sql_query("SELECT journal_mode AS value FROM pragma_journal_mode")
        .get_result::<TextValue>(connection)
        .map_err(|source| DatabaseInitError::SqlitePragma {
            path: path.clone(),
            source,
        })?;
    let foreign_keys = diesel::sql_query("SELECT foreign_keys AS count FROM pragma_foreign_keys")
        .get_result::<CountValue>(connection)
        .map_err(|source| DatabaseInitError::SqlitePragma {
            path: path.clone(),
            source,
        })?;
    if !journal_mode.value.eq_ignore_ascii_case("wal") || foreign_keys.count != 1 {
        return Err(DatabaseInitError::SqlitePragma {
            path: path.clone(),
            source: diesel::result::Error::QueryBuilderError(
                format!(
                    "sqlite startup pragmas were not applied (journal_mode={}, foreign_keys={})",
                    journal_mode.value, foreign_keys.count
                )
                .into(),
            ),
        });
    }
    Ok(())
}

fn verify_sqlite_foreign_keys(
    connection: &mut SqliteConnection,
    path: &PathBuf,
) -> Result<(), DatabaseInitError> {
    let violations = diesel::sql_query("SELECT COUNT(*) AS count FROM pragma_foreign_key_check")
        .get_result::<CountValue>(connection)
        .map_err(|source| DatabaseInitError::SqlitePragma {
            path: path.clone(),
            source,
        })?
        .count;
    if violations != 0 {
        return Err(DatabaseInitError::SqliteForeignKeyCheck {
            path: path.clone(),
            violations,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use diesel::{RunQueryDsl, sql_types::BigInt};

    use super::*;

    #[derive(QueryableByName)]
    struct IntegerValue {
        #[diesel(sql_type = BigInt)]
        value: i64,
    }

    #[test]
    fn sqlite_startup_connection_applies_migrations_and_fixed_pragmas() {
        let temp_dir = tempfile::tempdir().expect("sqlite startup temp dir should create");
        let db_url = temp_dir.path().join("startup.sqlite");
        let mut startup = StartupDatabaseConnection::establish(
            db_url.to_str().expect("sqlite path should be UTF-8"),
            Duration::from_secs(5),
        )
        .expect("sqlite startup connection should establish");
        let connection = startup
            .sqlite_connection()
            .expect("startup backend should be sqlite");

        let busy_timeout = diesel::sql_query("SELECT timeout AS value FROM pragma_busy_timeout")
            .get_result::<IntegerValue>(connection)
            .expect("busy timeout should query")
            .value;
        let migration_count =
            diesel::sql_query("SELECT COUNT(*) AS value FROM __diesel_schema_migrations")
                .get_result::<IntegerValue>(connection)
                .expect("migration count should query")
                .value;
        let journal_mode =
            diesel::sql_query("SELECT journal_mode AS value FROM pragma_journal_mode")
                .get_result::<TextValue>(connection)
                .expect("journal mode should query")
                .value;
        let foreign_keys =
            diesel::sql_query("SELECT foreign_keys AS value FROM pragma_foreign_keys")
                .get_result::<IntegerValue>(connection)
                .expect("foreign key mode should query")
                .value;
        let foreign_key_violations =
            diesel::sql_query("SELECT COUNT(*) AS value FROM pragma_foreign_key_check")
                .get_result::<IntegerValue>(connection)
                .expect("foreign key check should query")
                .value;
        let synchronous = diesel::sql_query("SELECT synchronous AS value FROM pragma_synchronous")
            .get_result::<IntegerValue>(connection)
            .expect("synchronous mode should query")
            .value;

        assert_eq!(busy_timeout, 5_000);
        assert_eq!(journal_mode, "wal");
        assert_eq!(foreign_keys, 1);
        assert_eq!(foreign_key_violations, 0);
        assert_eq!(synchronous, 2, "startup must retain SQLite FULL default");
        assert!(migration_count > 0);
    }

    #[test]
    fn sqlite_startup_rejects_existing_foreign_key_violations() {
        let temp_dir = tempfile::tempdir().expect("sqlite startup temp dir should create");
        let db_url = temp_dir.path().join("invalid-foreign-key.sqlite");
        let db_url = db_url.to_str().expect("sqlite path should be UTF-8");
        drop(
            StartupDatabaseConnection::establish(db_url, Duration::from_secs(5))
                .expect("initial sqlite startup should establish"),
        );

        let mut fixture =
            SqliteConnection::establish(db_url).expect("sqlite fixture should connect");
        fixture
            .batch_execute(
                "PRAGMA foreign_keys = OFF;
                 INSERT INTO api_key_rollup_daily (
                     api_key_id, day_bucket, currency, request_count,
                     total_input_tokens, total_output_tokens, total_reasoning_tokens,
                     total_tokens, billed_amount_nanos, created_at, updated_at
                 ) VALUES (999999, 0, 'USD', 0, 0, 0, 0, 0, 0, 1, 1);",
            )
            .expect("invalid foreign key fixture should insert");
        drop(fixture);

        let error = StartupDatabaseConnection::establish(db_url, Duration::from_secs(5))
            .err()
            .expect("startup must reject existing foreign key violations");
        assert!(matches!(
            error,
            DatabaseInitError::SqliteForeignKeyCheck { violations: 1, .. }
        ));
    }
}
