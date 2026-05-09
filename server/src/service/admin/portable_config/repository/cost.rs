use diesel::prelude::*;

use crate::database::{
    DbResult,
    cost::{
        CostCatalog, CostCatalogVersion, CostComponent, NewCostCatalog, NewCostCatalogVersion,
        NewCostComponent, UpdateCostCatalogData, UpdateCostCatalogVersionData,
        UpdateCostComponentData,
    },
    model::{Model, UpdateModelData},
};

use super::provider as provider_repository;
use super::{PortableRepositoryConnection, map_write_error};

#[derive(Debug, Clone)]
pub(crate) struct ModelCostBindingExport {
    pub model: Model,
    pub provider_key: String,
    pub cost_catalog: CostCatalog,
}

pub(crate) fn list_cost_catalogs_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
) -> DbResult<Vec<CostCatalog>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalogs;
            use crate::database::cost::_postgres_model::CostCatalogDb;

            cost_catalogs::table
                .filter(cost_catalogs::dsl::deleted_at.is_null())
                .order((cost_catalogs::dsl::name.asc(), cost_catalogs::dsl::id.asc()))
                .select(CostCatalogDb::as_select())
                .load::<CostCatalogDb>(*conn)
                .map(|rows| rows.into_iter().map(CostCatalogDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list cost catalogs for portable export", err)
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalogs;
            use crate::database::cost::_sqlite_model::CostCatalogDb;

            cost_catalogs::table
                .filter(cost_catalogs::dsl::deleted_at.is_null())
                .order((cost_catalogs::dsl::name.asc(), cost_catalogs::dsl::id.asc()))
                .select(CostCatalogDb::as_select())
                .load::<CostCatalogDb>(*conn)
                .map(|rows| rows.into_iter().map(CostCatalogDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list cost catalogs for portable export", err)
                })
        }
    }
}

pub(crate) fn list_cost_catalog_versions_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    catalog_id: i64,
) -> DbResult<Vec<CostCatalogVersion>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalog_versions;
            use crate::database::cost::_postgres_model::CostCatalogVersionDb;

            cost_catalog_versions::table
                .filter(cost_catalog_versions::dsl::catalog_id.eq(catalog_id))
                .order((
                    cost_catalog_versions::dsl::version.asc(),
                    cost_catalog_versions::dsl::id.asc(),
                ))
                .select(CostCatalogVersionDb::as_select())
                .load::<CostCatalogVersionDb>(*conn)
                .map(|rows| {
                    rows.into_iter()
                        .map(CostCatalogVersionDb::from_db)
                        .collect()
                })
                .map_err(|err| {
                    map_write_error(
                        "Failed to list cost catalog versions for portable export",
                        err,
                    )
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalog_versions;
            use crate::database::cost::_sqlite_model::CostCatalogVersionDb;

            cost_catalog_versions::table
                .filter(cost_catalog_versions::dsl::catalog_id.eq(catalog_id))
                .order((
                    cost_catalog_versions::dsl::version.asc(),
                    cost_catalog_versions::dsl::id.asc(),
                ))
                .select(CostCatalogVersionDb::as_select())
                .load::<CostCatalogVersionDb>(*conn)
                .map(|rows| {
                    rows.into_iter()
                        .map(CostCatalogVersionDb::from_db)
                        .collect()
                })
                .map_err(|err| {
                    map_write_error(
                        "Failed to list cost catalog versions for portable export",
                        err,
                    )
                })
        }
    }
}

pub(crate) fn list_cost_components_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    catalog_version_id: i64,
) -> DbResult<Vec<CostComponent>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_components;
            use crate::database::cost::_postgres_model::CostComponentDb;

            cost_components::table
                .filter(cost_components::dsl::catalog_version_id.eq(catalog_version_id))
                .order((
                    cost_components::dsl::priority.asc(),
                    cost_components::dsl::meter_key.asc(),
                    cost_components::dsl::created_at.asc(),
                    cost_components::dsl::id.asc(),
                ))
                .select(CostComponentDb::as_select())
                .load::<CostComponentDb>(*conn)
                .map(|rows| rows.into_iter().map(CostComponentDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list cost components for portable export", err)
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_components;
            use crate::database::cost::_sqlite_model::CostComponentDb;

            cost_components::table
                .filter(cost_components::dsl::catalog_version_id.eq(catalog_version_id))
                .order((
                    cost_components::dsl::priority.asc(),
                    cost_components::dsl::meter_key.asc(),
                    cost_components::dsl::created_at.asc(),
                    cost_components::dsl::id.asc(),
                ))
                .select(CostComponentDb::as_select())
                .load::<CostComponentDb>(*conn)
                .map(|rows| rows.into_iter().map(CostComponentDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list cost components for portable export", err)
                })
        }
    }
}

pub(crate) fn list_model_cost_bindings_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
) -> DbResult<Vec<ModelCostBindingExport>> {
    let mut rows = Vec::new();

    for provider in provider_repository::list_providers_for_export(conn)? {
        for model in provider_repository::list_models_for_provider_export(conn, provider.id)? {
            let Some(cost_catalog_id) = model.cost_catalog_id else {
                continue;
            };
            if let Some(cost_catalog) = find_active_cost_catalog_by_id(conn, cost_catalog_id)? {
                rows.push(ModelCostBindingExport {
                    model,
                    provider_key: provider.provider_key.clone(),
                    cost_catalog,
                });
            }
        }
    }

    rows.sort_by(|left, right| {
        (
            left.provider_key.as_str(),
            left.model.model_name.as_str(),
            left.cost_catalog.name.as_str(),
        )
            .cmp(&(
                right.provider_key.as_str(),
                right.model.model_name.as_str(),
                right.cost_catalog.name.as_str(),
            ))
    });
    Ok(rows)
}

pub(crate) fn find_active_cost_catalog_by_name(
    conn: &mut PortableRepositoryConnection<'_>,
    name: &str,
) -> DbResult<Option<CostCatalog>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalogs;
            use crate::database::cost::_postgres_model::CostCatalogDb;

            cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::name
                        .eq(name)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(*conn)
                .optional()
                .map(|row| row.map(CostCatalogDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup cost catalog by name", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalogs;
            use crate::database::cost::_sqlite_model::CostCatalogDb;

            cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::name
                        .eq(name)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(*conn)
                .optional()
                .map(|row| row.map(CostCatalogDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup cost catalog by name", err))
        }
    }
}

pub(crate) fn find_active_cost_catalog_by_id(
    conn: &mut PortableRepositoryConnection<'_>,
    id: i64,
) -> DbResult<Option<CostCatalog>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalogs;
            use crate::database::cost::_postgres_model::CostCatalogDb;

            cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::id
                        .eq(id)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(*conn)
                .optional()
                .map(|row| row.map(CostCatalogDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup cost catalog by id", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalogs;
            use crate::database::cost::_sqlite_model::CostCatalogDb;

            cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::id
                        .eq(id)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(*conn)
                .optional()
                .map(|row| row.map(CostCatalogDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup cost catalog by id", err))
        }
    }
}

pub(crate) fn find_cost_catalog_version_by_catalog_and_version(
    conn: &mut PortableRepositoryConnection<'_>,
    catalog_id: i64,
    version: &str,
) -> DbResult<Option<CostCatalogVersion>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalog_versions;
            use crate::database::cost::_postgres_model::CostCatalogVersionDb;

            cost_catalog_versions::table
                .filter(
                    cost_catalog_versions::dsl::catalog_id
                        .eq(catalog_id)
                        .and(cost_catalog_versions::dsl::version.eq(version)),
                )
                .select(CostCatalogVersionDb::as_select())
                .first::<CostCatalogVersionDb>(*conn)
                .optional()
                .map(|row| row.map(CostCatalogVersionDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup cost catalog version", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalog_versions;
            use crate::database::cost::_sqlite_model::CostCatalogVersionDb;

            cost_catalog_versions::table
                .filter(
                    cost_catalog_versions::dsl::catalog_id
                        .eq(catalog_id)
                        .and(cost_catalog_versions::dsl::version.eq(version)),
                )
                .select(CostCatalogVersionDb::as_select())
                .first::<CostCatalogVersionDb>(*conn)
                .optional()
                .map(|row| row.map(CostCatalogVersionDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup cost catalog version", err))
        }
    }
}

pub(crate) fn list_cost_catalog_version_ids(
    conn: &mut PortableRepositoryConnection<'_>,
    catalog_id: i64,
) -> DbResult<Vec<i64>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalog_versions;

            cost_catalog_versions::table
                .filter(cost_catalog_versions::dsl::catalog_id.eq(catalog_id))
                .select(cost_catalog_versions::dsl::id)
                .load::<i64>(*conn)
                .map_err(|err| map_write_error("Failed to list cost catalog version ids", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalog_versions;

            cost_catalog_versions::table
                .filter(cost_catalog_versions::dsl::catalog_id.eq(catalog_id))
                .select(cost_catalog_versions::dsl::id)
                .load::<i64>(*conn)
                .map_err(|err| map_write_error("Failed to list cost catalog version ids", err))
        }
    }
}

pub(crate) fn insert_cost_catalog(
    conn: &mut PortableRepositoryConnection<'_>,
    new_catalog: &NewCostCatalog,
) -> DbResult<CostCatalog> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalogs;
            use crate::database::cost::_postgres_model::{CostCatalogDb, NewCostCatalogDb};

            diesel::insert_into(cost_catalogs::table)
                .values(NewCostCatalogDb::to_db(new_catalog))
                .returning(CostCatalogDb::as_returning())
                .get_result::<CostCatalogDb>(*conn)
                .map(CostCatalogDb::from_db)
                .map_err(|err| map_write_error("Failed to import cost catalog", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalogs;
            use crate::database::cost::_sqlite_model::{CostCatalogDb, NewCostCatalogDb};

            diesel::insert_into(cost_catalogs::table)
                .values(NewCostCatalogDb::to_db(new_catalog))
                .returning(CostCatalogDb::as_returning())
                .get_result::<CostCatalogDb>(*conn)
                .map(CostCatalogDb::from_db)
                .map_err(|err| map_write_error("Failed to import cost catalog", err))
        }
    }
}

pub(crate) fn update_cost_catalog(
    conn: &mut PortableRepositoryConnection<'_>,
    catalog_id: i64,
    data: &UpdateCostCatalogData,
    updated_at: i64,
) -> DbResult<CostCatalog> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalogs;
            use crate::database::cost::_postgres_model::{CostCatalogDb, UpdateCostCatalogDataDb};

            diesel::update(cost_catalogs::table.find(catalog_id))
                .set((
                    UpdateCostCatalogDataDb::to_db(data),
                    cost_catalogs::dsl::updated_at.eq(updated_at),
                ))
                .returning(CostCatalogDb::as_returning())
                .get_result::<CostCatalogDb>(*conn)
                .map(CostCatalogDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported cost catalog", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalogs;
            use crate::database::cost::_sqlite_model::{CostCatalogDb, UpdateCostCatalogDataDb};

            diesel::update(cost_catalogs::table.find(catalog_id))
                .set((
                    UpdateCostCatalogDataDb::to_db(data),
                    cost_catalogs::dsl::updated_at.eq(updated_at),
                ))
                .returning(CostCatalogDb::as_returning())
                .get_result::<CostCatalogDb>(*conn)
                .map(CostCatalogDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported cost catalog", err))
        }
    }
}

pub(crate) fn insert_cost_catalog_version(
    conn: &mut PortableRepositoryConnection<'_>,
    new_version: &NewCostCatalogVersion,
) -> DbResult<CostCatalogVersion> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalog_versions;
            use crate::database::cost::_postgres_model::{
                CostCatalogVersionDb, NewCostCatalogVersionDb,
            };

            diesel::insert_into(cost_catalog_versions::table)
                .values(NewCostCatalogVersionDb::to_db(new_version))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(*conn)
                .map(CostCatalogVersionDb::from_db)
                .map_err(|err| map_write_error("Failed to import cost catalog version", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalog_versions;
            use crate::database::cost::_sqlite_model::{
                CostCatalogVersionDb, NewCostCatalogVersionDb,
            };

            diesel::insert_into(cost_catalog_versions::table)
                .values(NewCostCatalogVersionDb::to_db(new_version))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(*conn)
                .map(CostCatalogVersionDb::from_db)
                .map_err(|err| map_write_error("Failed to import cost catalog version", err))
        }
    }
}

pub(crate) fn update_cost_catalog_version(
    conn: &mut PortableRepositoryConnection<'_>,
    version_id: i64,
    data: &UpdateCostCatalogVersionData,
    updated_at: i64,
) -> DbResult<CostCatalogVersion> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_catalog_versions;
            use crate::database::cost::_postgres_model::{
                CostCatalogVersionDb, UpdateCostCatalogVersionDataDb,
            };

            diesel::update(cost_catalog_versions::table.find(version_id))
                .set((
                    UpdateCostCatalogVersionDataDb::to_db(data),
                    cost_catalog_versions::dsl::updated_at.eq(updated_at),
                ))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(*conn)
                .map(CostCatalogVersionDb::from_db)
                .map_err(|err| {
                    map_write_error("Failed to update imported cost catalog version", err)
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_catalog_versions;
            use crate::database::cost::_sqlite_model::{
                CostCatalogVersionDb, UpdateCostCatalogVersionDataDb,
            };

            diesel::update(cost_catalog_versions::table.find(version_id))
                .set((
                    UpdateCostCatalogVersionDataDb::to_db(data),
                    cost_catalog_versions::dsl::updated_at.eq(updated_at),
                ))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(*conn)
                .map(CostCatalogVersionDb::from_db)
                .map_err(|err| {
                    map_write_error("Failed to update imported cost catalog version", err)
                })
        }
    }
}

pub(crate) fn insert_cost_component(
    conn: &mut PortableRepositoryConnection<'_>,
    new_component: &NewCostComponent,
) -> DbResult<CostComponent> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_components;
            use crate::database::cost::_postgres_model::{CostComponentDb, NewCostComponentDb};

            diesel::insert_into(cost_components::table)
                .values(NewCostComponentDb::to_db(new_component))
                .returning(CostComponentDb::as_returning())
                .get_result::<CostComponentDb>(*conn)
                .map(CostComponentDb::from_db)
                .map_err(|err| map_write_error("Failed to import cost component", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_components;
            use crate::database::cost::_sqlite_model::{CostComponentDb, NewCostComponentDb};

            diesel::insert_into(cost_components::table)
                .values(NewCostComponentDb::to_db(new_component))
                .returning(CostComponentDb::as_returning())
                .get_result::<CostComponentDb>(*conn)
                .map(CostComponentDb::from_db)
                .map_err(|err| map_write_error("Failed to import cost component", err))
        }
    }
}

pub(crate) fn update_cost_component(
    conn: &mut PortableRepositoryConnection<'_>,
    component_id: i64,
    data: &UpdateCostComponentData,
    updated_at: i64,
) -> DbResult<CostComponent> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::cost_components;
            use crate::database::cost::_postgres_model::{
                CostComponentDb, UpdateCostComponentDataDb,
            };

            diesel::update(cost_components::table.find(component_id))
                .set((
                    UpdateCostComponentDataDb::to_db(data),
                    cost_components::dsl::updated_at.eq(updated_at),
                ))
                .returning(CostComponentDb::as_returning())
                .get_result::<CostComponentDb>(*conn)
                .map(CostComponentDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported cost component", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::cost_components;
            use crate::database::cost::_sqlite_model::{
                CostComponentDb, UpdateCostComponentDataDb,
            };

            diesel::update(cost_components::table.find(component_id))
                .set((
                    UpdateCostComponentDataDb::to_db(data),
                    cost_components::dsl::updated_at.eq(updated_at),
                ))
                .returning(CostComponentDb::as_returning())
                .get_result::<CostComponentDb>(*conn)
                .map(CostComponentDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported cost component", err))
        }
    }
}

pub(crate) fn update_model_cost_catalog(
    conn: &mut PortableRepositoryConnection<'_>,
    model_id: i64,
    cost_catalog_id: Option<i64>,
    updated_at: i64,
) -> DbResult<Model> {
    provider_repository::update_model(
        conn,
        model_id,
        &UpdateModelData {
            cost_catalog_id: Some(cost_catalog_id),
            ..UpdateModelData::default()
        },
        updated_at,
    )
}
