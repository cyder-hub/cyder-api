use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::database::model_route::ModelRoute;
use crate::database::request_patch::{RequestPatchRule, RequestPatchRuleResponse};
use crate::service::cache::types::{
    CacheInheritedRequestPatch, CacheRequestPatchConflict, CacheRequestPatchExplainEntry,
    CacheRequestPatchRule, CacheResolvedRequestPatch,
};
use crate::service::request_patch::resolve_effective_request_patches;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use serde::Serialize;

// `Model` is the canonical provider-scoped candidate identity used at execution time.
// Shared logical names and key-scoped overrides are intentionally modeled elsewhere.

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = model)]
    pub struct Model {
        pub id: i64,
        pub provider_id: i64,
        pub model_name: String,
        pub real_model_name: Option<String>,
        pub cost_catalog_id: Option<i64>,
        pub deleted_at: Option<i64>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

#[derive(Insertable, Deserialize, Debug)]
#[diesel(table_name = model)]
pub struct NewModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(AsChangeset, Deserialize, Debug, Default)]
#[diesel(table_name = model)]
pub struct UpdateModelData {
    pub model_name: Option<String>,
    pub real_model_name: Option<Option<String>>, // Allow setting to NULL
    pub is_enabled: Option<bool>,
    pub cost_catalog_id: Option<Option<i64>>,
}

}

#[derive(Debug, Serialize)]
pub struct ModelDetail {
    pub model: Model,
    pub request_patches: Vec<CacheRequestPatchRule>,
    pub inherited_request_patches: Vec<CacheInheritedRequestPatch>,
    pub effective_request_patches: Vec<CacheResolvedRequestPatch>,
    pub request_patch_explain: Vec<CacheRequestPatchExplainEntry>,
    pub request_patch_conflicts: Vec<CacheRequestPatchConflict>,
    pub has_request_patch_conflicts: bool,
    pub route_references: Vec<ModelRoute>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSummaryItem {
    pub id: i64,
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
}

impl Model {
    fn cache_request_patch_rules(
        rows: Vec<RequestPatchRuleResponse>,
    ) -> DbResult<Vec<CacheRequestPatchRule>> {
        rows.into_iter()
            .map(|row| {
                CacheRequestPatchRule::try_from(row).map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to convert request patch rule into cache snapshot: {}",
                        err
                    )))
                })
            })
            .collect()
    }

    /// Creates a new model record.
    pub fn create(
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
        is_enabled_val: bool,
    ) -> DbResult<Model> {
        let now = Utc::now().timestamp_millis();
        let new_id = ID_GENERATOR.generate_id();

        let new_model_data = NewModel {
            id: new_id,
            provider_id: provider_id_val,
            model_name: model_name_val.to_string(),
            real_model_name: real_model_name_val.map(|s| s.to_string()),
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let inserted_db_model = diesel::insert_into(model::table)
                .values(NewModelDb::to_db(&new_model_data))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to create model: {}", e)))
                })?;
            Ok(inserted_db_model.from_db())
        })
    }

    /// Updates an existing model record.
    pub fn update(id_value: i64, data: &UpdateModelData) -> DbResult<Model> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_db_model = diesel::update(model::table.find(id_value))
                .set((
                    UpdateModelDataDb::to_db(data),
                    model::dsl::updated_at.eq(current_time),
                ))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update model {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_model.from_db())
        })
    }

    /// Soft-deletes a model by ID (sets deleted_at to current time and is_enabled to false).
    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(model::table.find(id_value))
                .set((
                    model::dsl::deleted_at.eq(current_time),
                    model::dsl::is_enabled.eq(false), // Typically disable on delete
                    model::dsl::updated_at.eq(current_time),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete model {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    /// Retrieves a model by its name and provider ID, if not deleted.
    pub fn get_by_name_and_provider_id(
        model_name_val: &str,
        provider_id_val: i64,
    ) -> DbResult<Option<Model>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_model_opt = model::table
                .filter(
                    model::dsl::model_name
                        .eq(model_name_val)
                        .and(model::dsl::provider_id.eq(provider_id_val))
                        .and(model::dsl::deleted_at.is_null()),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(conn)
                .optional() // This makes it return Ok(None) if not found, instead of Err
                .map_err(|e| {
                    // We only expect NotFound to be handled by optional(), other errors are fatal
                    BaseError::DatabaseFatal(Some(format!(
                        "Error fetching model '{}' for provider {}: {}",
                        model_name_val, provider_id_val, e
                    )))
                })?;

            Ok(db_model_opt.map(|db_m| db_m.from_db()))
        })
    }

    /// Retrieves a model by its ID, if not deleted.
    pub fn get_by_id(id_value: i64) -> DbResult<Model> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_model = model::table
                .filter(
                    model::dsl::id
                        .eq(id_value)
                        .and(model::dsl::deleted_at.is_null()),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Model with id {} not found or deleted",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching model {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(db_model.from_db())
        })
    }

    pub fn get_detail_by_id(model_id_val: i64) -> DbResult<ModelDetail> {
        let model = Model::get_by_id(model_id_val)?;
        let request_patches =
            Self::cache_request_patch_rules(RequestPatchRule::list_by_model_id(model_id_val)?)?;
        let provider_request_patches = Self::cache_request_patch_rules(
            RequestPatchRule::list_by_provider_id(model.provider_id)?,
        )?;
        let resolved = resolve_effective_request_patches(
            model.provider_id,
            model_id_val,
            &provider_request_patches,
            &request_patches,
        );
        let route_references = ModelRoute::list_by_model_id(model_id_val)?;
        Ok(ModelDetail {
            model,
            request_patches,
            inherited_request_patches: resolved.inherited_rules,
            effective_request_patches: resolved.effective_rules,
            request_patch_explain: resolved.explain,
            request_patch_conflicts: resolved.conflicts,
            has_request_patch_conflicts: resolved.has_conflicts,
            route_references,
        })
    }

    /// Lists all models for a given provider_id that are not marked as deleted.
    pub fn list_by_provider_id(provider_id_val: i64) -> DbResult<Vec<Model>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_models = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id_val)
                        .and(model::dsl::deleted_at.is_null()),
                )
                .order(model::dsl::model_name.asc()) // Or by created_at, etc.
                .select(ModelDb::as_select())
                .load::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list models for provider {}: {}",
                        provider_id_val, e
                    )))
                })?;
            Ok(db_models.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Lists all models that are not marked as deleted.
    pub fn list_all() -> DbResult<Vec<Model>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_models = model::table
                .left_join(provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)))
                .filter(provider::dsl::deleted_at.is_null())
                .filter(model::dsl::deleted_at.is_null())
                .order(model::dsl::created_at.desc())
                .select(ModelDb::as_select())
                .load::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list all models: {}", e)))
                })?;
            Ok(db_models.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Lists lightweight model summary rows for table views and selectors.
    pub fn list_summary() -> DbResult<Vec<ModelSummaryItem>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = model::table
                .inner_join(provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)))
                .filter(provider::dsl::deleted_at.is_null())
                .filter(model::dsl::deleted_at.is_null())
                .order((provider::dsl::name.asc(), model::dsl::model_name.asc()))
                .select((
                    model::dsl::id,
                    model::dsl::provider_id,
                    provider::dsl::provider_key,
                    provider::dsl::name,
                    model::dsl::model_name,
                    model::dsl::real_model_name,
                    model::dsl::is_enabled,
                ))
                .load::<(i64, i64, String, String, String, Option<String>, bool)>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list model summaries: {}", e)))
                })?;

            Ok(rows
                .into_iter()
                .map(
                    |(
                        id,
                        provider_id,
                        provider_key,
                        provider_name,
                        model_name,
                        real_model_name,
                        is_enabled,
                    )| {
                        ModelSummaryItem {
                            id,
                            provider_id,
                            provider_key,
                            provider_name,
                            model_name,
                            real_model_name,
                            is_enabled,
                        }
                    },
                )
                .collect())
        })
    }

    /// Lists all active (not deleted and enabled) models for a given provider_id.
    pub fn list_active_by_provider_id(provider_id_val: i64) -> DbResult<Vec<Model>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_models = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id_val)
                        .and(model::dsl::deleted_at.is_null())
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .order(model::dsl::model_name.asc())
                .select(ModelDb::as_select())
                .load::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list active models for provider {}: {}",
                        provider_id_val, e
                    )))
                })?;
            Ok(db_models.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Upserts a model based on provider_id and model_name.
    /// If it exists, it updates real_model_name, updated_at, and ensures is_deleted is false and is_enabled is true.
    /// If it doesn't exist, it creates a new model.
    pub fn upsert_by_provider_and_name(
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
    ) -> DbResult<Model> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let existing_model_db = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id_val)
                        .and(model::dsl::model_name.eq(model_name_val)),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(conn)
                .optional() // Makes it return Option<ModelDb>
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Error checking for existing model: {}",
                        e
                    )))
                })?;

            let now = Utc::now().timestamp_millis();

            match existing_model_db {
                Some(db_model) => {
                    let model_item = ModelDb::from_db(db_model);

                    // Update existing model
                    let update_data = UpdateModelData {
                        real_model_name: Some(real_model_name_val.map(|s| s.to_string())),
                        is_enabled: Some(true), // Ensure it's enabled
                        model_name: None,       // Not changing model_name itself here
                        cost_catalog_id: None,  // Do not update cost_catalog_id during upsert
                    };

                    // Also ensure it's not deleted
                    let updated_db_model = diesel::update(model::table.find(model_item.id))
                        .set((
                            UpdateModelDataDb::to_db(&update_data),
                            model::dsl::deleted_at.eq(None::<i64>),
                            model::dsl::updated_at.eq(now),
                        ))
                        .returning(ModelDb::as_returning())
                        .get_result::<ModelDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to update existing model during upsert: {}",
                                e
                            )))
                        })?;
                    Ok(updated_db_model.from_db())
                }
                None => {
                    // Create new model
                    let new_id = ID_GENERATOR.generate_id();
                    let new_model_data = NewModel {
                        id: new_id,
                        provider_id: provider_id_val,
                        model_name: model_name_val.to_string(),
                        real_model_name: real_model_name_val.map(|s| s.to_string()),
                        is_enabled: true,
                        created_at: now,
                        updated_at: now,
                    };
                    let inserted_db_model = diesel::insert_into(model::table)
                        .values(NewModelDb::to_db(&new_model_data))
                        .returning(ModelDb::as_returning())
                        .get_result::<ModelDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to insert new model during upsert: {}",
                                e
                            )))
                        })?;
                    Ok(inserted_db_model.from_db())
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::_sqlite_schema::*;
    use crate::database::model_route::{NewModelRoute, NewModelRouteCandidate};
    use crate::database::provider::NewProvider;
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use diesel::Connection;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use serde_json::Value;
    use tempfile::tempdir;

    const SQLITE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");

    struct TestSqliteDb {
        _temp_dir: tempfile::TempDir,
        conn: diesel::SqliteConnection,
    }

    fn sqlite_connection() -> TestSqliteDb {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("model-summary.sqlite");
        std::fs::File::create(&db_path).expect("db file should be created");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut conn = diesel::SqliteConnection::establish(&db_url)
            .expect("sqlite connection should be established");
        conn.run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("migrations should run");
        TestSqliteDb {
            _temp_dir: temp_dir,
            conn,
        }
    }

    fn seed_provider(
        conn: &mut diesel::SqliteConnection,
        id: i64,
        provider_key_val: &str,
        name_val: &str,
    ) {
        use crate::database::provider::_sqlite_model::*;

        let now = 1_000_000;
        let data = NewProvider {
            id,
            provider_key: provider_key_val.to_string(),
            name: name_val.to_string(),
            endpoint: "https://example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: now,
            updated_at: now,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        };

        diesel::insert_into(provider::table)
            .values(NewProviderDb::to_db(&data))
            .execute(conn)
            .expect("provider seed should succeed");
    }

    fn seed_model(
        conn: &mut diesel::SqliteConnection,
        id: i64,
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
        is_enabled_val: bool,
    ) {
        use crate::database::model::_sqlite_model::*;

        let now = 1_000_000;
        let data = NewModel {
            id,
            provider_id: provider_id_val,
            model_name: model_name_val.to_string(),
            real_model_name: real_model_name_val.map(ToString::to_string),
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        diesel::insert_into(model::table)
            .values(NewModelDb::to_db(&data))
            .execute(conn)
            .expect("model seed should succeed");
    }

    fn model_summaries(conn: &mut diesel::SqliteConnection) -> Vec<ModelSummaryItem> {
        let rows = model::table
            .inner_join(provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)))
            .filter(provider::dsl::deleted_at.is_null())
            .filter(model::dsl::deleted_at.is_null())
            .order((provider::dsl::name.asc(), model::dsl::model_name.asc()))
            .select((
                model::dsl::id,
                model::dsl::provider_id,
                provider::dsl::provider_key,
                provider::dsl::name,
                model::dsl::model_name,
                model::dsl::real_model_name,
                model::dsl::is_enabled,
            ))
            .load::<(i64, i64, String, String, String, Option<String>, bool)>(conn)
            .expect("model summary rows should load");

        rows.into_iter()
            .map(
                |(
                    id,
                    provider_id,
                    provider_key,
                    provider_name,
                    model_name,
                    real_model_name,
                    is_enabled,
                )| ModelSummaryItem {
                    id,
                    provider_id,
                    provider_key,
                    provider_name,
                    model_name,
                    real_model_name,
                    is_enabled,
                },
            )
            .collect()
    }

    fn seed_model_route(
        conn: &mut diesel::SqliteConnection,
        route_id: i64,
        model_id_val: i64,
        route_name_val: &str,
    ) {
        use crate::database::model_route::_sqlite_model::*;

        let now = 1_000_000;
        let route = NewModelRoute {
            id: route_id,
            route_name: route_name_val.to_string(),
            description: Some("route description".to_string()),
            is_enabled: true,
            expose_in_models: true,
            created_at: now,
            updated_at: now,
        };

        diesel::insert_into(model_route::table)
            .values(NewModelRouteDb::to_db(&route))
            .execute(conn)
            .expect("model route seed should succeed");

        let candidate = NewModelRouteCandidate {
            id: route_id + 1000,
            route_id,
            model_id: model_id_val,
            priority: 0,
            is_enabled: true,
            created_at: now,
            updated_at: now,
        };

        diesel::insert_into(model_route_candidate::table)
            .values(NewModelRouteCandidateDb::to_db(&candidate))
            .execute(conn)
            .expect("model route candidate seed should succeed");
    }

    #[test]
    fn model_summary_list_returns_provider_context() {
        let mut db = sqlite_connection();
        seed_provider(&mut db.conn, 11, "alpha", "Alpha Provider");
        seed_provider(&mut db.conn, 12, "beta", "Beta Provider");
        seed_model(&mut db.conn, 21, 12, "zeta", Some("zeta-real"), true);
        seed_model(&mut db.conn, 22, 11, "alpha-model", None, false);
        seed_model(&mut db.conn, 23, 11, "beta-model", Some("beta-real"), true);

        let rows = model_summaries(&mut db.conn);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].provider_name, "Alpha Provider");
        assert_eq!(rows[0].model_name, "alpha-model");
        assert_eq!(rows[0].provider_key, "alpha");
        assert_eq!(rows[0].real_model_name, None);
        assert!(!rows[0].is_enabled);

        assert_eq!(rows[1].model_name, "beta-model");
        assert_eq!(rows[1].provider_key, "alpha");
        assert_eq!(rows[2].provider_name, "Beta Provider");
        assert_eq!(rows[2].model_name, "zeta");
        assert_eq!(rows[2].real_model_name.as_deref(), Some("zeta-real"));
    }

    #[test]
    fn model_detail_can_carry_direct_route_references() {
        let detail = ModelDetail {
            model: Model {
                id: 22,
                provider_id: 11,
                model_name: "alpha-model".to_string(),
                real_model_name: None,
                cost_catalog_id: None,
                deleted_at: None,
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
            },
            request_patches: vec![],
            inherited_request_patches: vec![],
            effective_request_patches: vec![],
            request_patch_explain: vec![],
            request_patch_conflicts: vec![],
            has_request_patch_conflicts: false,
            route_references: vec![ModelRoute {
                id: 31,
                route_name: "alpha-route".to_string(),
                description: Some("route description".to_string()),
                is_enabled: true,
                expose_in_models: true,
                deleted_at: None,
                created_at: 1,
                updated_at: 1,
            }],
        };

        assert_eq!(detail.route_references.len(), 1);
        assert_eq!(detail.route_references[0].route_name, "alpha-route");
        assert_eq!(
            detail.route_references[0].description.as_deref(),
            Some("route description")
        );
        assert!(detail.route_references[0].is_enabled);
        assert!(detail.route_references[0].expose_in_models);
    }

    #[test]
    fn model_detail_contract_uses_request_patch_fields() {
        let detail = ModelDetail {
            model: Model {
                id: 22,
                provider_id: 11,
                model_name: "alpha-model".to_string(),
                real_model_name: None,
                cost_catalog_id: None,
                deleted_at: None,
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
            },
            request_patches: vec![],
            inherited_request_patches: vec![],
            effective_request_patches: vec![],
            request_patch_explain: vec![],
            request_patch_conflicts: vec![],
            has_request_patch_conflicts: false,
            route_references: vec![],
        };

        let value = serde_json::to_value(detail).expect("model detail should serialize");
        let object = value
            .as_object()
            .expect("detail should serialize as object");
        assert!(matches!(
            object.get("request_patches"),
            Some(Value::Array(_))
        ));
        assert!(matches!(
            object.get("inherited_request_patches"),
            Some(Value::Array(_))
        ));
        assert!(matches!(
            object.get("effective_request_patches"),
            Some(Value::Array(_))
        ));
        assert!(matches!(
            object.get("request_patch_explain"),
            Some(Value::Array(_))
        ));
        assert!(matches!(
            object.get("request_patch_conflicts"),
            Some(Value::Array(_))
        ));
        assert_eq!(
            object.get("has_request_patch_conflicts"),
            Some(&Value::Bool(false))
        );
        assert!(object.get("custom_fields").is_none());
    }
}
