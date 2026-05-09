use diesel::prelude::*;

use crate::database::{DbResult, model_route::ModelRoute};

use super::{PortableRepositoryConnection, map_write_error};

pub(crate) fn find_active_model_route_by_name(
    conn: &mut PortableRepositoryConnection<'_>,
    route_name: &str,
) -> DbResult<Option<ModelRoute>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::model_route;
            use crate::database::model_route::_postgres_model::ModelRouteDb;

            model_route::table
                .filter(
                    model_route::dsl::route_name
                        .eq(route_name)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(ModelRouteDb::as_select())
                .first::<ModelRouteDb>(*conn)
                .optional()
                .map(|row| row.map(ModelRouteDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup model route by name", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::model_route;
            use crate::database::model_route::_sqlite_model::ModelRouteDb;

            model_route::table
                .filter(
                    model_route::dsl::route_name
                        .eq(route_name)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(ModelRouteDb::as_select())
                .first::<ModelRouteDb>(*conn)
                .optional()
                .map(|row| row.map(ModelRouteDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup model route by name", err))
        }
    }
}
