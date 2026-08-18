use std::sync::Arc;

use diesel::{
    Connection, PgConnection, QueryableByName, RunQueryDsl, connection::SimpleConnection,
    sql_types::Text,
};
use tempfile::TempDir;

use crate::config::DatabaseIoConfig;

use super::{
    runtime::{DatabaseRuntime, DatabaseWorkload, RuntimeConnection},
    startup::StartupDatabaseConnection,
};

struct TestDatabaseInner {
    _temp_dir: Option<TempDir>,
    db_url: String,
    runtime: Arc<DatabaseRuntime>,
}

#[derive(QueryableByName)]
struct DatabaseNameRow {
    #[diesel(sql_type = Text)]
    name: String,
}

#[derive(QueryableByName)]
struct PostgresIdentityRow {
    #[diesel(sql_type = Text)]
    database_name: String,
    #[diesel(sql_type = Text)]
    user_name: String,
    #[diesel(sql_type = Text)]
    server_version_num: String,
}

#[derive(Clone)]
pub(crate) struct TestDatabase {
    inner: Arc<TestDatabaseInner>,
}

impl TestDatabase {
    pub(crate) fn reset_dedicated_postgres_schema(
        database_url: &str,
        expected_database_name: &str,
    ) {
        let mut connection = PgConnection::establish(database_url)
            .expect("dedicated postgres smoke database should be reachable");
        let database_name = diesel::sql_query("SELECT current_database()::text AS name")
            .get_result::<DatabaseNameRow>(&mut connection)
            .expect("postgres database name should query")
            .name;
        assert_eq!(database_name, expected_database_name);
        connection
            .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .expect("postgres smoke schema should reset");
    }

    pub(crate) fn reset_dedicated_postgres17_schema(
        database_url: &str,
        expected_database_name: &str,
        expected_user_name: &str,
    ) -> String {
        let mut connection = PgConnection::establish(database_url)
            .expect("dedicated PostgreSQL 17 boundary database should be reachable");
        let identity = diesel::sql_query(
            "SELECT current_database()::text AS database_name, \
                    current_user::text AS user_name, \
                    current_setting('server_version_num')::text AS server_version_num",
        )
        .get_result::<PostgresIdentityRow>(&mut connection)
        .expect("dedicated PostgreSQL 17 boundary identity should query");
        assert_eq!(identity.database_name, expected_database_name);
        assert_eq!(identity.user_name, expected_user_name);
        let version_number = identity
            .server_version_num
            .parse::<u32>()
            .expect("PostgreSQL server_version_num should be numeric");
        assert_eq!(
            version_number / 10_000,
            17,
            "boundary database must be PG17"
        );
        connection
            .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .expect("PostgreSQL 17 boundary schema should reset");
        identity.server_version_num
    }

    pub(crate) async fn new_sqlite(
        file_name: &str,
        pool_size: u32,
        config: DatabaseIoConfig,
    ) -> Self {
        let temp_dir = tempfile::tempdir().expect("async test database directory should create");
        let db_url = temp_dir
            .path()
            .join(file_name)
            .to_string_lossy()
            .into_owned();
        let startup = StartupDatabaseConnection::establish(&db_url, config.sqlite_busy_timeout())
            .expect("async test startup database should establish");
        drop(startup);
        let runtime = Arc::new(
            DatabaseRuntime::connect(&db_url, pool_size, config)
                .await
                .expect("async test database runtime should connect"),
        );
        Self {
            inner: Arc::new(TestDatabaseInner {
                _temp_dir: Some(temp_dir),
                db_url,
                runtime,
            }),
        }
    }

    pub(crate) async fn new_sqlite_default(file_name: &str) -> Self {
        Self::new_sqlite(
            file_name,
            crate::config::CONFIG.db_pool_size,
            crate::config::CONFIG.database_io.clone(),
        )
        .await
    }

    pub(crate) async fn new_postgres(database_url: &str) -> Self {
        Self::new_postgres_with_config(
            database_url,
            crate::config::CONFIG.db_pool_size,
            crate::config::CONFIG.database_io.clone(),
        )
        .await
    }

    pub(crate) async fn new_postgres_with_config(
        database_url: &str,
        pool_size: u32,
        config: DatabaseIoConfig,
    ) -> Self {
        let startup =
            StartupDatabaseConnection::establish(database_url, config.sqlite_busy_timeout())
                .expect("test PostgreSQL startup database should establish");
        drop(startup);
        let runtime = Arc::new(
            DatabaseRuntime::connect(database_url, pool_size, config)
                .await
                .expect("test PostgreSQL runtime should connect"),
        );
        Self {
            inner: Arc::new(TestDatabaseInner {
                _temp_dir: None,
                db_url: database_url.to_string(),
                runtime,
            }),
        }
    }

    pub(crate) fn runtime(&self) -> Arc<DatabaseRuntime> {
        Arc::clone(&self.inner.runtime)
    }

    pub(crate) fn startup_connection(&self) -> StartupDatabaseConnection {
        StartupDatabaseConnection::establish(
            &self.inner.db_url,
            crate::config::DatabaseIoConfig::default().sqlite_busy_timeout(),
        )
        .unwrap_or_else(|error| panic!("test startup connection should establish: {error}"))
    }

    pub(crate) async fn execute_sqlite_batch(
        &self,
        statement: impl Into<String>,
    ) -> crate::database::DbResult<()> {
        let statement = statement.into();
        self.inner
            .runtime
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    let RuntimeConnection::Sqlite(connection) = connection else {
                        return Err(crate::controller::BaseError::InternalServerError(Some(
                            "SQLite test fixture used with a non-SQLite runtime".to_string(),
                        )));
                    };
                    diesel_async::SimpleAsyncConnection::batch_execute(
                        &mut **connection,
                        &statement,
                    )
                    .await
                    .map_err(|error| {
                        crate::controller::BaseError::DatabaseFatal(Some(format!(
                            "SQLite test fixture statement failed: {error}"
                        )))
                    })
                })
            })
            .await
    }
}

impl std::ops::Deref for TestDatabase {
    type Target = DatabaseRuntime;

    fn deref(&self) -> &Self::Target {
        self.inner.runtime.as_ref()
    }
}
