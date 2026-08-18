use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;

use super::{
    DbResult,
    runtime::{DatabaseRuntime, DatabaseWorkload, db_execute as async_db_execute},
};
use crate::controller::BaseError;
use crate::database::request_patch::{RequestPatchVariantAggregate, RequestPatchVariantRepository};
use crate::db_object_no_default;
use crate::schema::enum_def::ModelKind;
use crate::utils::ID_GENERATOR;

use serde::Serialize;

// `Model` is the canonical provider-scoped candidate identity used at execution time.
// Shared logical names and key-scoped overrides are intentionally modeled elsewhere.

db_object_no_default! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = model)]
    pub struct Model {
        pub id: i64,
        pub provider_id: i64,
        pub model_name: String,
        pub real_model_name: Option<String>,
        pub model_kind: ModelKind,
        pub cost_catalog_id: Option<i64>,
        pub source_selection_mode: String,
        pub deleted_at: Option<i64>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

#[derive(Insertable, Deserialize, Debug, Clone)]
#[diesel(table_name = model)]
pub struct NewModel {
    pub id: i64,
    pub provider_id: i64,
        pub model_name: String,
        pub real_model_name: Option<String>,
        pub model_kind: ModelKind,
        pub source_selection_mode: String,
        pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(AsChangeset, Deserialize, Debug, Default, Clone)]
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
    pub request_patch_variants: Vec<RequestPatchVariantAggregate>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSummaryItem {
    pub id: i64,
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_kind: ModelKind,
    pub source_selection_mode: String,
    pub is_enabled: bool,
}

impl Model {
    pub async fn get_by_name_and_provider_id(
        database: &DatabaseRuntime,
        model_name_val: &str,
        provider_id_val: i64,
    ) -> DbResult<Option<Model>> {
        let model_name_val = model_name_val.to_string();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = model::table
                            .filter(
                                model::dsl::model_name
                                    .eq(&model_name_val)
                                    .and(model::dsl::provider_id.eq(provider_id_val))
                                    .and(model::dsl::deleted_at.is_null()),
                            )
                            .select(ModelDb::as_select());
                        let result: Result<ModelDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result
                            .optional()
                            .map(|row| row.map(ModelDb::from_db))
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Error fetching model '{}' for provider {}: {}",
                                    model_name_val, provider_id_val, error
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn get_by_id(database: &DatabaseRuntime, id_value: i64) -> DbResult<Model> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = model::table
                            .filter(
                                model::dsl::id
                                    .eq(id_value)
                                    .and(model::dsl::deleted_at.is_null()),
                            )
                            .select(ModelDb::as_select());
                        let result: Result<ModelDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result.map(ModelDb::from_db).map_err(|error| match error {
                            diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(
                                format!("Model with id {} not found or deleted", id_value),
                            )),
                            _ => BaseError::DatabaseFatal(Some(format!(
                                "Error fetching model {}: {}",
                                id_value, error
                            ))),
                        })
                    })
                })
            })
            .await
    }

    pub async fn list_all(database: &DatabaseRuntime) -> DbResult<Vec<Model>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = model::table
                            .left_join(
                                provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)),
                            )
                            .filter(provider::dsl::deleted_at.is_null())
                            .filter(model::dsl::deleted_at.is_null())
                            .order(model::dsl::created_at.desc())
                            .select(ModelDb::as_select());
                        let result: Result<Vec<ModelDb>, diesel::result::Error> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn).await;
                        result
                            .map(|rows| rows.into_iter().map(ModelDb::from_db).collect())
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to list all models: {}",
                                    error
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn create_with_source_config(
        database: &DatabaseRuntime,
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
        model_kind_val: ModelKind,
        is_enabled_val: bool,
        source_config: Option<&crate::database::model_source_binding::ModelSourceConfig>,
    ) -> DbResult<Model> {
        let now = Utc::now().timestamp_millis();
        let source_selection_mode = source_config
            .map(|config| config.source_selection_mode.clone())
            .unwrap_or_else(|| "INHERIT_ALL".to_string());
        let new_model = NewModel {
            id: ID_GENERATOR.generate_id(),
            provider_id: provider_id_val,
            model_name: model_name_val.to_string(),
            real_model_name: real_model_name_val.map(str::to_string),
            model_kind: model_kind_val,
            source_selection_mode,
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };
        crate::database::model_source_binding::create_model_with_config(
            database,
            &new_model,
            source_config,
        )
        .await
    }

    pub async fn update(
        database: &DatabaseRuntime,
        id_value: i64,
        data: &UpdateModelData,
    ) -> DbResult<Model> {
        let current_time = Utc::now().timestamp_millis();
        let data = data.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(model::table.find(id_value))
                            .set((
                                UpdateModelDataDb::to_db(&data),
                                model::dsl::updated_at.eq(current_time),
                            ))
                            .returning(ModelDb::as_returning());
                        diesel_async::RunQueryDsl::get_result::<ModelDb>(query, &mut **conn)
                            .await
                            .map(ModelDb::from_db)
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to update model {}: {}",
                                    id_value, error
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn delete_with_dependents(
        database: &DatabaseRuntime,
        id_value: i64,
    ) -> DbResult<usize> {
        let current_time = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(model::table.find(id_value)).set((
                            model::dsl::deleted_at.eq(current_time),
                            model::dsl::is_enabled.eq(false),
                            model::dsl::updated_at.eq(current_time),
                        ));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to delete model {}: {}",
                                    id_value, error
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn list_by_provider_id(
        database: &DatabaseRuntime,
        provider_id_val: i64,
    ) -> DbResult<Vec<Model>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = model::table
                            .filter(
                                model::dsl::provider_id
                                    .eq(provider_id_val)
                                    .and(model::dsl::deleted_at.is_null()),
                            )
                            .order(model::dsl::created_at.desc())
                            .select(ModelDb::as_select());
                        diesel_async::RunQueryDsl::load::<ModelDb>(query, &mut **conn)
                            .await
                            .map(|rows| rows.into_iter().map(ModelDb::from_db).collect())
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to list models for provider {}: {}",
                                    provider_id_val, error
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn get_detail_by_id(
        database: &DatabaseRuntime,
        model_id_val: i64,
    ) -> DbResult<ModelDetail> {
        let model = Self::get_by_id(database, model_id_val).await?;
        let request_patch_variants =
            RequestPatchVariantRepository::list_by_model_ids(database, &[model_id_val]).await?;
        Ok(ModelDetail {
            model,
            request_patch_variants,
        })
    }

    pub async fn list_summary(database: &DatabaseRuntime) -> DbResult<Vec<ModelSummaryItem>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = model::table
                            .inner_join(
                                provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)),
                            )
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
                                model::dsl::model_kind,
                                model::dsl::source_selection_mode,
                                model::dsl::is_enabled,
                            ));
                        let rows = diesel_async::RunQueryDsl::load::<(
                            i64,
                            i64,
                            String,
                            String,
                            String,
                            Option<String>,
                            ModelKind,
                            String,
                            bool,
                        )>(query, &mut **conn)
                        .await
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to list model summaries: {}",
                                error
                            )))
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
                                    model_kind,
                                    source_selection_mode,
                                    is_enabled,
                                )| ModelSummaryItem {
                                    id,
                                    provider_id,
                                    provider_key,
                                    provider_name,
                                    model_name,
                                    real_model_name,
                                    model_kind,
                                    source_selection_mode,
                                    is_enabled,
                                },
                            )
                            .collect())
                    })
                })
            })
            .await
    }

    /// Creates a new model record.
    pub async fn create(
        database: &DatabaseRuntime,
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
        model_kind_val: ModelKind,
        is_enabled_val: bool,
    ) -> DbResult<Model> {
        Self::create_with_source_config(
            database,
            provider_id_val,
            model_name_val,
            real_model_name_val,
            model_kind_val,
            is_enabled_val,
            None,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::_sqlite_schema::*;
    use crate::database::TestDatabase;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use serde_json::Value;

    struct TestSqliteDb {
        _temp_dir: tempfile::TempDir,
        conn: diesel::SqliteConnection,
    }

    fn sqlite_connection() -> TestSqliteDb {
        let (temp_dir, conn) =
            crate::database::open_test_sqlite_connection_with_migrations("model-summary.sqlite");
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
        use crate::database::upstream_source::{
            _sqlite_model::NewUpstreamSourceDb, NewUpstreamSource,
        };

        let now = 1_000_000;
        let data = NewProvider {
            id,
            provider_key: provider_key_val.to_string(),
            name: name_val.to_string(),
            is_enabled: true,
            created_at: now,
            updated_at: now,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        };

        diesel::insert_into(provider::table)
            .values(NewProviderDb::to_db(&data))
            .execute(conn)
            .expect("provider seed should succeed");

        let source = NewUpstreamSource {
            id,
            provider_id: id,
            profile_type: UpstreamProfileType::Openai,
            base_url: "https://example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            is_default: true,
            created_at: now,
            updated_at: now,
            ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
        };
        diesel::insert_into(upstream_source::table)
            .values(NewUpstreamSourceDb::to_db(&source))
            .execute(conn)
            .expect("upstream source seed should succeed");
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
            model_kind: ModelKind::Chat,
            source_selection_mode: "INHERIT_ALL".to_string(),
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
                model::dsl::model_kind,
                model::dsl::source_selection_mode,
                model::dsl::is_enabled,
            ))
            .load::<(
                i64,
                i64,
                String,
                String,
                String,
                Option<String>,
                ModelKind,
                String,
                bool,
            )>(conn)
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
                    model_kind,
                    source_selection_mode,
                    is_enabled,
                )| ModelSummaryItem {
                    id,
                    provider_id,
                    provider_key,
                    provider_name,
                    model_name,
                    real_model_name,
                    model_kind,
                    source_selection_mode,
                    is_enabled,
                },
            )
            .collect()
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
    fn model_detail_contract_uses_variant_aggregate_fields() {
        let detail = ModelDetail {
            model: Model {
                id: 22,
                provider_id: 11,
                model_name: "alpha-model".to_string(),
                real_model_name: None,
                model_kind: ModelKind::Chat,
                cost_catalog_id: None,
                source_selection_mode: "INHERIT_ALL".to_string(),
                deleted_at: None,
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
            },
            request_patch_variants: vec![],
        };

        let value = serde_json::to_value(detail).expect("model detail should serialize");
        let object = value
            .as_object()
            .expect("detail should serialize as object");
        assert!(matches!(
            object.get("request_patch_variants"),
            Some(Value::Array(_))
        ));
        assert!(object.get("request_patches").is_none());
        assert!(object.get("custom_fields").is_none());
    }

    #[tokio::test]
    async fn model_soft_delete_preserves_source_bound_variants_for_recovery() {
        let database =
            TestDatabase::new_sqlite_default("model-delete-preserves-variants.sqlite").await;
        (async {
            let provider = Provider::create(
                &database,
                &NewProvider {
                    id: 71,
                    provider_key: "model-delete-provider".to_string(),
                    name: "Model Delete Provider".to_string(),
                    is_enabled: true,
                    created_at: 1,
                    updated_at: 1,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                },
                &crate::database::upstream_source::NewUpstreamSource {
                    id: 72,
                    provider_id: 71,
                    profile_type: UpstreamProfileType::Openai,
                    base_url: "https://model-delete.example/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                    ..crate::database::upstream_source::NewUpstreamSource::test_defaults(
                        UpstreamProfileType::Openai,
                    )
                },
            )
            .await
            .expect("provider should be created");
            let model = Model::create(
                &database,
                provider.provider.id,
                "recoverable-model",
                None,
                ModelKind::Chat,
                true,
            )
            .await
            .expect("model should be created");
            let variant = RequestPatchVariantRepository::create(
                &database,
                &RequestPatchVariantInput {
                    source_id: 72,
                    model_id: Some(model.id),
                    suffix: Some("fast".to_string()),
                    enabled: true,
                    expose_in_models: false,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/options/temperature".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(serde_json::json!(0.2))),
                        description: None,
                    }],
                },
            )
            .await
            .expect("model Variant should be created");

            assert_eq!(
                Model::delete_with_dependents(&database, model.id)
                    .await
                    .expect("model delete"),
                1
            );
            assert!(Model::get_by_id(&database, model.id).await.is_err());
            let historical = RequestPatchVariantRepository::list_by_model_ids_including_deleted(
                &database,
                &[model.id],
            )
            .await
            .expect("historical model Variants should load");
            assert_eq!(historical.len(), 1);
            assert_eq!(historical[0].variant.id, variant.variant.id);
            assert!(historical[0].variant.deleted_at.is_none());
            assert_eq!(historical[0].rules.len(), 1);
        })
        .await;
    }
}
