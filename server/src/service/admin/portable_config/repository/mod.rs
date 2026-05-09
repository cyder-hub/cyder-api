use diesel::{Connection, PgConnection, SqliteConnection};

use crate::{controller::BaseError, database::DbConnection};

pub(crate) mod api_key;
pub(crate) mod cost;
pub(crate) mod model_route;
pub(crate) mod provider;
pub(crate) mod reasoning_config;
pub(crate) mod request_patch;

pub(crate) enum PortableRepositoryConnection<'a> {
    Postgres(&'a mut PgConnection),
    Sqlite(&'a mut SqliteConnection),
}

pub(crate) fn with_transaction<T>(
    conn: &mut DbConnection,
    operation: impl FnOnce(&mut PortableRepositoryConnection<'_>) -> Result<T, BaseError>,
) -> Result<T, BaseError> {
    match conn {
        DbConnection::Postgres(conn) => conn.transaction::<T, BaseError, _>(|conn| {
            let mut repository_conn = PortableRepositoryConnection::Postgres(conn);
            operation(&mut repository_conn)
        }),
        DbConnection::Sqlite(conn) => conn.transaction::<T, BaseError, _>(|conn| {
            let mut repository_conn = PortableRepositoryConnection::Sqlite(conn);
            operation(&mut repository_conn)
        }),
    }
}

fn map_write_error(context: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}
