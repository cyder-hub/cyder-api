use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = cost_catalogs)]
    pub struct CostCatalog {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub deleted_at: Option<i64>,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = cost_catalogs)]
    pub struct NewCostCatalog {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = cost_catalogs)]
    pub struct UpdateCostCatalogData {
        pub name: Option<String>,
        pub description: Option<Option<String>>,
    }

    #[derive(Queryable, Selectable, Identifiable, Associations, Debug, Clone, serde::Serialize)]
    #[diesel(belongs_to(CostCatalog, foreign_key = catalog_id))]
    #[diesel(table_name = cost_catalog_versions)]
    pub struct CostCatalogVersion {
        pub id: i64,
        pub catalog_id: i64,
        pub version: String,
        pub currency: String,
        pub source: Option<String>,
        pub effective_from: i64,
        pub effective_until: Option<i64>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = cost_catalog_versions)]
    pub struct NewCostCatalogVersion {
        pub id: i64,
        pub catalog_id: i64,
        pub version: String,
        pub currency: String,
        pub source: Option<String>,
        pub effective_from: i64,
        pub effective_until: Option<i64>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = cost_catalog_versions)]
    pub struct UpdateCostCatalogVersionData {
        pub currency: Option<String>,
        pub source: Option<Option<String>>,
        pub effective_from: Option<i64>,
        pub effective_until: Option<Option<i64>>,
        pub is_enabled: Option<bool>,
    }

    #[derive(Queryable, Selectable, Identifiable, Associations, Debug, Clone, serde::Serialize)]
    #[diesel(belongs_to(CostCatalogVersion, foreign_key = catalog_version_id))]
    #[diesel(table_name = cost_components)]
    pub struct CostComponent {
        pub id: i64,
        pub catalog_version_id: i64,
        pub meter_key: String,
        pub charge_kind: String,
        pub unit_price_nanos: Option<i64>,
        pub flat_fee_nanos: Option<i64>,
        pub tier_config_json: Option<String>,
        pub match_attributes_json: Option<String>,
        pub priority: i32,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = cost_components)]
    pub struct NewCostComponent {
        pub id: i64,
        pub catalog_version_id: i64,
        pub meter_key: String,
        pub charge_kind: String,
        pub unit_price_nanos: Option<i64>,
        pub flat_fee_nanos: Option<i64>,
        pub tier_config_json: Option<String>,
        pub match_attributes_json: Option<String>,
        pub priority: i32,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = cost_components)]
    pub struct UpdateCostComponentData {
        pub meter_key: Option<String>,
        pub charge_kind: Option<String>,
        pub unit_price_nanos: Option<Option<i64>>,
        pub flat_fee_nanos: Option<Option<i64>>,
        pub tier_config_json: Option<Option<String>>,
        pub match_attributes_json: Option<Option<String>>,
        pub priority: Option<i32>,
        pub description: Option<Option<String>>,
    }
}

#[derive(Deserialize, Debug)]
pub struct NewCostCatalogPayload {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Deserialize, Debug)]
pub struct NewCostCatalogVersionPayload {
    pub catalog_id: i64,
    pub version: String,
    pub currency: String,
    pub source: Option<String>,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub is_enabled: bool,
}

#[derive(Deserialize, Debug)]
pub struct NewCostComponentPayload {
    pub catalog_version_id: i64,
    pub meter_key: String,
    pub charge_kind: String,
    pub unit_price_nanos: Option<i64>,
    pub flat_fee_nanos: Option<i64>,
    pub tier_config_json: Option<String>,
    pub match_attributes_json: Option<String>,
    pub priority: i32,
    pub description: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CostTemplateComponentImportPayload {
    pub meter_key: String,
    pub charge_kind: String,
    pub unit_price_nanos: Option<i64>,
    pub flat_fee_nanos: Option<i64>,
    pub tier_config_json: Option<String>,
    pub match_attributes_json: Option<String>,
    pub priority: i32,
    pub description: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub struct CostCatalogTemplateImportPayload {
    pub catalog_name: String,
    pub catalog_description: Option<String>,
    pub version: String,
    pub currency: String,
    pub source: Option<String>,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub is_enabled: bool,
    pub components: Vec<CostTemplateComponentImportPayload>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ImportedCostCatalogTemplate {
    pub catalog: CostCatalog,
    pub version: CostCatalogVersion,
    pub components: Vec<CostComponent>,
    pub created_catalog: bool,
}

impl CostCatalog {
    pub fn create(data: &NewCostCatalogPayload) -> DbResult<CostCatalog> {
        let now = Utc::now().timestamp_millis();
        let new_catalog = NewCostCatalog {
            id: ID_GENERATOR.generate_id(),
            name: data.name.clone(),
            description: data.description.clone(),
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let inserted = diesel::insert_into(cost_catalogs::table)
                .values(NewCostCatalogDb::to_db(&new_catalog))
                .returning(CostCatalogDb::as_returning())
                .get_result::<CostCatalogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to create cost catalog: {}", e)))
                })?;
            Ok(inserted.from_db())
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<CostCatalog> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let catalog = cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::id
                        .eq(id_value)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Cost catalog with id {} not found or deleted",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Failed to get cost catalog {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(catalog.from_db())
        })
    }

    pub fn get_by_name(name_value: &str) -> DbResult<Option<CostCatalog>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::name
                        .eq(name_value)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to get cost catalog by name {}: {}",
                        name_value, e
                    )))
                })?;
            Ok(row.map(|catalog| catalog.from_db()))
        })
    }

    pub fn update(id_value: i64, data: &UpdateCostCatalogData) -> DbResult<CostCatalog> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let updated = diesel::update(cost_catalogs::table.find(id_value))
                .set((
                    UpdateCostCatalogDataDb::to_db(data),
                    cost_catalogs::dsl::updated_at.eq(now),
                ))
                .returning(CostCatalogDb::as_returning())
                .get_result::<CostCatalogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update cost catalog {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            diesel::update(cost_catalogs::table.find(id_value))
                .set((
                    cost_catalogs::dsl::deleted_at.eq(now),
                    cost_catalogs::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete cost catalog {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    pub fn list_all() -> DbResult<Vec<CostCatalog>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = cost_catalogs::table
                .filter(cost_catalogs::dsl::deleted_at.is_null())
                .order(cost_catalogs::dsl::created_at.desc())
                .select(CostCatalogDb::as_select())
                .load::<CostCatalogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list cost catalogs: {}", e)))
                })?;
            Ok(rows.into_iter().map(|row| row.from_db()).collect())
        })
    }
}

impl CostCatalogVersion {
    pub fn create(data: &NewCostCatalogVersionPayload) -> DbResult<CostCatalogVersion> {
        let now = Utc::now().timestamp_millis();
        let new_version = NewCostCatalogVersion {
            id: ID_GENERATOR.generate_id(),
            catalog_id: data.catalog_id,
            version: data.version.clone(),
            currency: data.currency.clone(),
            source: data.source.clone(),
            effective_from: data.effective_from,
            effective_until: data.effective_until,
            is_enabled: data.is_enabled,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let inserted = diesel::insert_into(cost_catalog_versions::table)
                .values(NewCostCatalogVersionDb::to_db(&new_version))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to create cost catalog version: {}",
                        e
                    )))
                })?;
            Ok(inserted.from_db())
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<CostCatalogVersion> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = cost_catalog_versions::table
                .find(id_value)
                .select(CostCatalogVersionDb::as_select())
                .first::<CostCatalogVersionDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Cost catalog version with id {} not found",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Failed to get cost catalog version {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(row.from_db())
        })
    }

    pub fn update(
        id_value: i64,
        data: &UpdateCostCatalogVersionData,
    ) -> DbResult<CostCatalogVersion> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let updated = diesel::update(cost_catalog_versions::table.find(id_value))
                .set((
                    UpdateCostCatalogVersionDataDb::to_db(data),
                    cost_catalog_versions::dsl::updated_at.eq(now),
                ))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update cost catalog version {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::delete(cost_catalog_versions::table.find(id_value))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete cost catalog version {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    pub fn list_all() -> DbResult<Vec<CostCatalogVersion>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = cost_catalog_versions::table
                .order(cost_catalog_versions::dsl::created_at.desc())
                .select(CostCatalogVersionDb::as_select())
                .load::<CostCatalogVersionDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list cost catalog versions: {}",
                        e
                    )))
                })?;
            Ok(rows.into_iter().map(|row| row.from_db()).collect())
        })
    }

    pub fn list_by_catalog_id(catalog_id_value: i64) -> DbResult<Vec<CostCatalogVersion>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = cost_catalog_versions::table
                .filter(cost_catalog_versions::dsl::catalog_id.eq(catalog_id_value))
                .order(cost_catalog_versions::dsl::effective_from.desc())
                .select(CostCatalogVersionDb::as_select())
                .load::<CostCatalogVersionDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list cost catalog versions by catalog {}: {}",
                        catalog_id_value, e
                    )))
                })?;
            Ok(rows.into_iter().map(|row| row.from_db()).collect())
        })
    }

    pub fn get_active_by_catalog_id(
        catalog_id_value: i64,
        at_time_ms: i64,
    ) -> DbResult<Option<CostCatalogVersion>> {
        let versions = Self::list_by_catalog_id(catalog_id_value)?;
        select_active_cost_catalog_version(versions, at_time_ms)
    }
}

pub fn enabled_version_conflicts(
    versions: &[CostCatalogVersion],
    candidate: &CostCatalogVersion,
) -> Vec<CostCatalogVersion> {
    if !candidate.is_enabled {
        return Vec::new();
    }

    versions
        .iter()
        .filter(|version| {
            version.id != candidate.id
                && version.is_enabled
                && intervals_overlap(
                    version.effective_from,
                    version.effective_until,
                    candidate.effective_from,
                    candidate.effective_until,
                )
        })
        .cloned()
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnabledVersionResolution {
    Disable { version_id: i64 },
    Truncate { version_id: i64, effective_until: i64 },
}

pub fn reconcile_enabled_version_conflicts(
    versions: &[CostCatalogVersion],
    candidate: &CostCatalogVersion,
) -> Vec<EnabledVersionResolution> {
    enabled_version_conflicts(versions, candidate)
        .into_iter()
        .map(|version| {
            if version.effective_from < candidate.effective_from {
                EnabledVersionResolution::Truncate {
                    version_id: version.id,
                    effective_until: candidate.effective_from,
                }
            } else {
                EnabledVersionResolution::Disable {
                    version_id: version.id,
                }
            }
        })
        .collect()
}

impl CostComponent {
    pub fn create(data: &NewCostComponentPayload) -> DbResult<CostComponent> {
        let now = Utc::now().timestamp_millis();
        let new_component = NewCostComponent {
            id: ID_GENERATOR.generate_id(),
            catalog_version_id: data.catalog_version_id,
            meter_key: data.meter_key.clone(),
            charge_kind: data.charge_kind.clone(),
            unit_price_nanos: data.unit_price_nanos,
            flat_fee_nanos: data.flat_fee_nanos,
            tier_config_json: data.tier_config_json.clone(),
            match_attributes_json: data.match_attributes_json.clone(),
            priority: data.priority,
            description: data.description.clone(),
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let inserted = diesel::insert_into(cost_components::table)
                .values(NewCostComponentDb::to_db(&new_component))
                .returning(CostComponentDb::as_returning())
                .get_result::<CostComponentDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to create cost component: {}",
                        e
                    )))
                })?;
            Ok(inserted.from_db())
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<CostComponent> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = cost_components::table
                .find(id_value)
                .select(CostComponentDb::as_select())
                .first::<CostComponentDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Cost component with id {} not found",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Failed to get cost component {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(row.from_db())
        })
    }

    pub fn update(id_value: i64, data: &UpdateCostComponentData) -> DbResult<CostComponent> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let updated = diesel::update(cost_components::table.find(id_value))
                .set((
                    UpdateCostComponentDataDb::to_db(data),
                    cost_components::dsl::updated_at.eq(now),
                ))
                .returning(CostComponentDb::as_returning())
                .get_result::<CostComponentDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update cost component {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::delete(cost_components::table.find(id_value))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete cost component {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    pub fn list_all() -> DbResult<Vec<CostComponent>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = cost_components::table
                .order((
                    cost_components::dsl::catalog_version_id.asc(),
                    cost_components::dsl::priority.asc(),
                    cost_components::dsl::created_at.asc(),
                ))
                .select(CostComponentDb::as_select())
                .load::<CostComponentDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list cost components: {}", e)))
                })?;
            Ok(rows.into_iter().map(|row| row.from_db()).collect())
        })
    }

    pub fn list_by_catalog_version_id(
        catalog_version_id_value: i64,
    ) -> DbResult<Vec<CostComponent>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = cost_components::table
                .filter(cost_components::dsl::catalog_version_id.eq(catalog_version_id_value))
                .order((
                    cost_components::dsl::priority.asc(),
                    cost_components::dsl::created_at.asc(),
                ))
                .select(CostComponentDb::as_select())
                .load::<CostComponentDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list cost components for version {}: {}",
                        catalog_version_id_value, e
                    )))
                })?;
            Ok(rows.into_iter().map(|row| row.from_db()).collect())
        })
    }
}

pub fn import_cost_catalog_template(
    data: &CostCatalogTemplateImportPayload,
) -> DbResult<ImportedCostCatalogTemplate> {
    let conn = &mut get_connection()?;
    let now = Utc::now().timestamp_millis();

    db_execute!(conn, {
        conn.transaction::<ImportedCostCatalogTemplate, BaseError, _>(|conn| {
            let existing_catalog = cost_catalogs::table
                .filter(
                    cost_catalogs::dsl::name
                        .eq(&data.catalog_name)
                        .and(cost_catalogs::dsl::deleted_at.is_null()),
                )
                .select(CostCatalogDb::as_select())
                .first::<CostCatalogDb>(conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to lookup cost catalog {} during template import: {}",
                        data.catalog_name, e
                    )))
                })?;

            let (catalog, created_catalog) = match existing_catalog {
                Some(catalog) => (catalog.from_db(), false),
                None => {
                    let new_catalog = NewCostCatalog {
                        id: ID_GENERATOR.generate_id(),
                        name: data.catalog_name.clone(),
                        description: data.catalog_description.clone(),
                        created_at: now,
                        updated_at: now,
                    };
                    let inserted = diesel::insert_into(cost_catalogs::table)
                        .values(NewCostCatalogDb::to_db(&new_catalog))
                        .returning(CostCatalogDb::as_returning())
                        .get_result::<CostCatalogDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to create cost catalog during template import: {}",
                                e
                            )))
                        })?;
                    (inserted.from_db(), true)
                }
            };

            let existing_version = cost_catalog_versions::table
                .filter(
                    cost_catalog_versions::dsl::catalog_id
                        .eq(catalog.id)
                        .and(cost_catalog_versions::dsl::version.eq(&data.version)),
                )
                .select(CostCatalogVersionDb::as_select())
                .first::<CostCatalogVersionDb>(conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to lookup version {} for catalog {} during template import: {}",
                        data.version, catalog.id, e
                    )))
                })?;

            if existing_version.is_some() {
                return Err(BaseError::DatabaseDup(Some(format!(
                    "Cost catalog '{}' already has version '{}'",
                    catalog.name, data.version
                ))));
            }

            let new_version = NewCostCatalogVersion {
                id: ID_GENERATOR.generate_id(),
                catalog_id: catalog.id,
                version: data.version.clone(),
                currency: data.currency.clone(),
                source: data.source.clone(),
                effective_from: data.effective_from,
                effective_until: data.effective_until,
                is_enabled: data.is_enabled,
                created_at: now,
                updated_at: now,
            };
            let version = diesel::insert_into(cost_catalog_versions::table)
                .values(NewCostCatalogVersionDb::to_db(&new_version))
                .returning(CostCatalogVersionDb::as_returning())
                .get_result::<CostCatalogVersionDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to create cost catalog version during template import: {}",
                        e
                    )))
                })?
                .from_db();

            if version.is_enabled {
                let existing_enabled_versions = cost_catalog_versions::table
                    .filter(
                        cost_catalog_versions::dsl::catalog_id
                            .eq(catalog.id)
                            .and(cost_catalog_versions::dsl::is_enabled.eq(true)),
                    )
                    .select(CostCatalogVersionDb::as_select())
                    .load::<CostCatalogVersionDb>(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to load enabled cost catalog versions during template import: {}",
                            e
                        )))
                    })?
                    .into_iter()
                    .map(CostCatalogVersionDb::from_db)
                    .collect::<Vec<_>>();

                for resolution in
                    reconcile_enabled_version_conflicts(&existing_enabled_versions, &version)
                {
                    match resolution {
                        EnabledVersionResolution::Disable { version_id } => {
                            diesel::update(cost_catalog_versions::table.find(version_id))
                                .set((
                                    cost_catalog_versions::dsl::is_enabled.eq(false),
                                    cost_catalog_versions::dsl::updated_at.eq(now),
                                ))
                                .execute(conn)
                                .map_err(|e| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to disable conflicting cost catalog version {} during template import: {}",
                                        version_id, e
                                    )))
                                })?;
                        }
                        EnabledVersionResolution::Truncate {
                            version_id,
                            effective_until,
                        } => {
                            diesel::update(cost_catalog_versions::table.find(version_id))
                                .set((
                                    cost_catalog_versions::dsl::effective_until
                                        .eq(Some(effective_until)),
                                    cost_catalog_versions::dsl::updated_at.eq(now),
                                ))
                                .execute(conn)
                                .map_err(|e| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to truncate conflicting cost catalog version {} during template import: {}",
                                        version_id, e
                                    )))
                                })?;
                        }
                    }
                }
            }

            let mut components = Vec::with_capacity(data.components.len());
            for component in &data.components {
                let new_component = NewCostComponent {
                    id: ID_GENERATOR.generate_id(),
                    catalog_version_id: version.id,
                    meter_key: component.meter_key.clone(),
                    charge_kind: component.charge_kind.clone(),
                    unit_price_nanos: component.unit_price_nanos,
                    flat_fee_nanos: component.flat_fee_nanos,
                    tier_config_json: component.tier_config_json.clone(),
                    match_attributes_json: component.match_attributes_json.clone(),
                    priority: component.priority,
                    description: component.description.clone(),
                    created_at: now,
                    updated_at: now,
                };
                let inserted = diesel::insert_into(cost_components::table)
                    .values(NewCostComponentDb::to_db(&new_component))
                    .returning(CostComponentDb::as_returning())
                    .get_result::<CostComponentDb>(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to create cost component during template import: {}",
                            e
                        )))
                    })?;
                components.push(inserted.from_db());
            }

            Ok(ImportedCostCatalogTemplate {
                catalog,
                version,
                components,
                created_catalog,
            })
        })
    })
}

fn version_is_active(version: &CostCatalogVersion, at_time_ms: i64) -> bool {
    version.is_enabled
        && version.effective_from <= at_time_ms
        && version
            .effective_until
            .map(|until| at_time_ms < until)
            .unwrap_or(true)
}

fn intervals_overlap(
    left_start: i64,
    left_end: Option<i64>,
    right_start: i64,
    right_end: Option<i64>,
) -> bool {
    let left_end = left_end.unwrap_or(i64::MAX);
    let right_end = right_end.unwrap_or(i64::MAX);
    left_start < right_end && right_start < left_end
}

fn select_active_cost_catalog_version(
    versions: Vec<CostCatalogVersion>,
    at_time_ms: i64,
) -> DbResult<Option<CostCatalogVersion>> {
    let active_versions: Vec<CostCatalogVersion> = versions
        .into_iter()
        .filter(|version| version_is_active(version, at_time_ms))
        .collect();

    match active_versions.len() {
        0 => Ok(None),
        1 => Ok(active_versions.into_iter().next()),
        count => Err(BaseError::ParamInvalid(Some(format!(
            "Expected at most one active cost catalog version, found {}",
            count
        )))),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        CostCatalogVersion, EnabledVersionResolution, enabled_version_conflicts,
        reconcile_enabled_version_conflicts, select_active_cost_catalog_version,
    };

    fn version(
        id: i64,
        effective_from: i64,
        effective_until: Option<i64>,
        is_enabled: bool,
    ) -> CostCatalogVersion {
        CostCatalogVersion {
            id,
            catalog_id: 1,
            version: format!("v{}", id),
            currency: "USD".to_string(),
            source: None,
            effective_from,
            effective_until,
            is_enabled,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn active_version_selector_returns_single_match() {
        let selected = select_active_cost_catalog_version(
            vec![
                version(1, 0, Some(1000), true),
                version(2, 1000, None, true),
                version(3, 0, None, false),
            ],
            1500,
        )
        .unwrap()
        .expect("version should match");

        assert_eq!(selected.id, 2);
    }

    #[test]
    fn active_version_selector_rejects_overlapping_matches() {
        let err = select_active_cost_catalog_version(
            vec![version(1, 0, None, true), version(2, 100, None, true)],
            500,
        )
        .expect_err("overlap should fail");

        assert!(
            matches!(err, crate::controller::BaseError::ParamInvalid(Some(message)) if message.contains("at most one active"))
        );
    }

    #[test]
    fn enabled_version_conflicts_ignores_non_overlapping_future_release() {
        let conflicts = enabled_version_conflicts(
            &[
                version(1, 0, Some(2_000), true),
                version(2, 2_000, None, true),
                version(3, 0, None, false),
            ],
            &version(2, 2_000, None, true),
        );

        assert!(conflicts.is_empty());
    }

    #[test]
    fn enabled_version_conflicts_returns_only_overlapping_enabled_versions() {
        let conflicts = enabled_version_conflicts(
            &[
                version(1, 0, None, true),
                version(2, 500, Some(900), true),
                version(3, 900, None, true),
                version(4, 0, None, false),
            ],
            &version(2, 500, Some(900), true),
        );

        assert_eq!(conflicts.into_iter().map(|version| version.id).collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn reconcile_enabled_version_conflicts_truncates_current_for_future_release() {
        let resolutions = reconcile_enabled_version_conflicts(
            &[version(1, 0, None, true), version(2, 2_000, None, true)],
            &version(2, 2_000, None, true),
        );

        assert_eq!(
            resolutions,
            vec![EnabledVersionResolution::Truncate {
                version_id: 1,
                effective_until: 2_000,
            }]
        );
    }

    #[test]
    fn reconcile_enabled_version_conflicts_disables_later_overlapping_versions() {
        let resolutions = reconcile_enabled_version_conflicts(
            &[
                version(1, 0, Some(1_000), true),
                version(2, 500, Some(900), true),
                version(3, 800, None, true),
            ],
            &version(2, 500, Some(900), true),
        );

        assert_eq!(
            resolutions,
            vec![
                EnabledVersionResolution::Truncate {
                    version_id: 1,
                    effective_until: 500,
                },
                EnabledVersionResolution::Disable { version_id: 3 },
            ]
        );
    }
}
