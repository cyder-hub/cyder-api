use std::fmt;
use std::str::FromStr;

use bincode::{Decode, Encode};
use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFeatureKey {
    #[serde(rename = "openai_reasoning_content_repair")]
    OpenAiReasoningContentRepair,
}

impl RuntimeFeatureKey {
    pub const ALL: [RuntimeFeatureKey; 1] = [RuntimeFeatureKey::OpenAiReasoningContentRepair];

    pub fn as_key(self) -> &'static str {
        match self {
            Self::OpenAiReasoningContentRepair => "openai_reasoning_content_repair",
        }
    }
}

impl fmt::Display for RuntimeFeatureKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

impl FromStr for RuntimeFeatureKey {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "openai_reasoning_content_repair" => Ok(Self::OpenAiReasoningContentRepair),
            other => Err(format!("unknown runtime feature key '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Encode, Decode)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFeatureConfigScope {
    Provider,
    Model,
}

impl RuntimeFeatureConfigScope {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Provider => "provider",
            Self::Model => "model",
        }
    }
}

impl fmt::Display for RuntimeFeatureConfigScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_key())
    }
}

impl FromStr for RuntimeFeatureConfigScope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "provider" => Ok(Self::Provider),
            "model" => Ok(Self::Model),
            other => Err(format!("unknown runtime feature config scope '{other}'")),
        }
    }
}

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = runtime_feature_config)]
    pub struct RuntimeFeatureConfig {
        pub id: i64,
        pub scope_kind: String,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub feature_key: String,
        pub enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = runtime_feature_config)]
    pub struct NewRuntimeFeatureConfig {
        pub id: i64,
        pub scope_kind: String,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub feature_key: String,
        pub enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFeatureConfigView {
    pub config: RuntimeFeatureConfig,
    pub scope: RuntimeFeatureConfigScope,
    pub feature_key: RuntimeFeatureKey,
}

impl RuntimeFeatureConfigView {
    fn from_row(config: RuntimeFeatureConfig) -> DbResult<Self> {
        let scope = parse_scope_key(&config.scope_kind)?;
        let feature_key = parse_feature_key(&config.feature_key)?;
        Ok(Self {
            config,
            scope,
            feature_key,
        })
    }
}

fn database_config_error(message: impl Into<String>) -> BaseError {
    BaseError::DatabaseFatal(Some(message.into()))
}

fn parse_scope_key(value: &str) -> DbResult<RuntimeFeatureConfigScope> {
    RuntimeFeatureConfigScope::from_str(value).map_err(database_config_error)
}

fn parse_feature_key(value: &str) -> DbResult<RuntimeFeatureKey> {
    RuntimeFeatureKey::from_str(value).map_err(database_config_error)
}

fn map_write_error(action: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            info,
        ) => BaseError::DatabaseDup(Some(format!("{action}: {}", info.message()))),
        other => BaseError::DatabaseFatal(Some(format!("{action}: {other}"))),
    }
}

impl RuntimeFeatureConfig {
    pub fn upsert_provider_config(
        provider_id_value: i64,
        feature_key_value: RuntimeFeatureKey,
        enabled_value: bool,
    ) -> DbResult<RuntimeFeatureConfigView> {
        let config_id = Self::upsert_owner_config(
            RuntimeFeatureConfigScope::Provider,
            provider_id_value,
            feature_key_value,
            enabled_value,
        )?;
        Self::get_active_by_id(config_id)?.ok_or_else(|| {
            BaseError::DatabaseFatal(Some(format!(
                "runtime feature config {} disappeared after provider upsert",
                config_id
            )))
        })
    }

    pub fn upsert_model_config(
        model_id_value: i64,
        feature_key_value: RuntimeFeatureKey,
        enabled_value: bool,
    ) -> DbResult<RuntimeFeatureConfigView> {
        let config_id = Self::upsert_owner_config(
            RuntimeFeatureConfigScope::Model,
            model_id_value,
            feature_key_value,
            enabled_value,
        )?;
        Self::get_active_by_id(config_id)?.ok_or_else(|| {
            BaseError::DatabaseFatal(Some(format!(
                "runtime feature config {} disappeared after model upsert",
                config_id
            )))
        })
    }

    pub fn delete_provider_config(
        provider_id_value: i64,
        feature_key_value: RuntimeFeatureKey,
    ) -> DbResult<usize> {
        Self::delete_owner_config(
            RuntimeFeatureConfigScope::Provider,
            provider_id_value,
            feature_key_value,
        )
    }

    pub fn delete_model_config(
        model_id_value: i64,
        feature_key_value: RuntimeFeatureKey,
    ) -> DbResult<usize> {
        Self::delete_owner_config(
            RuntimeFeatureConfigScope::Model,
            model_id_value,
            feature_key_value,
        )
    }

    pub fn get_active_provider_config(
        provider_id_value: i64,
        feature_key_value: RuntimeFeatureKey,
    ) -> DbResult<Option<RuntimeFeatureConfigView>> {
        Self::get_active_by_owner(
            RuntimeFeatureConfigScope::Provider,
            provider_id_value,
            feature_key_value,
        )
    }

    pub fn get_active_model_config(
        model_id_value: i64,
        feature_key_value: RuntimeFeatureKey,
    ) -> DbResult<Option<RuntimeFeatureConfigView>> {
        Self::get_active_by_owner(
            RuntimeFeatureConfigScope::Model,
            model_id_value,
            feature_key_value,
        )
    }

    pub fn list_active() -> DbResult<Vec<RuntimeFeatureConfigView>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = runtime_feature_config::table
                .filter(runtime_feature_config::dsl::deleted_at.is_null())
                .order((
                    runtime_feature_config::dsl::scope_kind.asc(),
                    runtime_feature_config::dsl::provider_id.asc(),
                    runtime_feature_config::dsl::model_id.asc(),
                    runtime_feature_config::dsl::feature_key.asc(),
                    runtime_feature_config::dsl::id.asc(),
                ))
                .select(RuntimeFeatureConfigDb::as_select())
                .load::<RuntimeFeatureConfigDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active runtime feature configs: {err}"
                    )))
                })?;
            rows.into_iter()
                .map(RuntimeFeatureConfigDb::from_db)
                .map(RuntimeFeatureConfigView::from_row)
                .collect()
        })
    }

    pub fn list_active_provider_configs(
        provider_ids: &[i64],
    ) -> DbResult<Vec<RuntimeFeatureConfigView>> {
        if provider_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_active_by_owner_ids(RuntimeFeatureConfigScope::Provider, provider_ids)
    }

    pub fn list_active_model_configs(model_ids: &[i64]) -> DbResult<Vec<RuntimeFeatureConfigView>> {
        if model_ids.is_empty() {
            return Ok(Vec::new());
        }
        Self::list_active_by_owner_ids(RuntimeFeatureConfigScope::Model, model_ids)
    }

    fn get_active_by_id(id_value: i64) -> DbResult<Option<RuntimeFeatureConfigView>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            runtime_feature_config::table
                .filter(
                    runtime_feature_config::dsl::id
                        .eq(id_value)
                        .and(runtime_feature_config::dsl::deleted_at.is_null()),
                )
                .select(RuntimeFeatureConfigDb::as_select())
                .first::<RuntimeFeatureConfigDb>(conn)
                .optional()
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch runtime feature config {id_value}: {err}"
                    )))
                })?
                .map(RuntimeFeatureConfigDb::from_db)
                .map(RuntimeFeatureConfigView::from_row)
                .transpose()
        })
    }

    fn get_active_by_owner(
        scope: RuntimeFeatureConfigScope,
        owner_id: i64,
        feature_key: RuntimeFeatureKey,
    ) -> DbResult<Option<RuntimeFeatureConfigView>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = runtime_feature_config::table
                .filter(
                    runtime_feature_config::dsl::scope_kind
                        .eq(scope.as_key())
                        .and(runtime_feature_config::dsl::feature_key.eq(feature_key.as_key()))
                        .and(runtime_feature_config::dsl::deleted_at.is_null()),
                )
                .into_boxed();
            query = match scope {
                RuntimeFeatureConfigScope::Provider => {
                    query.filter(runtime_feature_config::dsl::provider_id.eq(owner_id))
                }
                RuntimeFeatureConfigScope::Model => {
                    query.filter(runtime_feature_config::dsl::model_id.eq(owner_id))
                }
            };
            query
                .select(RuntimeFeatureConfigDb::as_select())
                .first::<RuntimeFeatureConfigDb>(conn)
                .optional()
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch active {} runtime feature config for owner {} and feature {}: {err}",
                        scope.as_key(),
                        owner_id,
                        feature_key.as_key()
                    )))
                })?
                .map(RuntimeFeatureConfigDb::from_db)
                .map(RuntimeFeatureConfigView::from_row)
                .transpose()
        })
    }

    fn list_active_by_owner_ids(
        scope: RuntimeFeatureConfigScope,
        owner_ids: &[i64],
    ) -> DbResult<Vec<RuntimeFeatureConfigView>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = runtime_feature_config::table
                .filter(
                    runtime_feature_config::dsl::scope_kind
                        .eq(scope.as_key())
                        .and(runtime_feature_config::dsl::deleted_at.is_null()),
                )
                .into_boxed();
            query = match scope {
                RuntimeFeatureConfigScope::Provider => {
                    query.filter(runtime_feature_config::dsl::provider_id.eq_any(owner_ids))
                }
                RuntimeFeatureConfigScope::Model => {
                    query.filter(runtime_feature_config::dsl::model_id.eq_any(owner_ids))
                }
            };

            let rows = query
                .order((
                    runtime_feature_config::dsl::provider_id.asc(),
                    runtime_feature_config::dsl::model_id.asc(),
                    runtime_feature_config::dsl::feature_key.asc(),
                    runtime_feature_config::dsl::id.asc(),
                ))
                .select(RuntimeFeatureConfigDb::as_select())
                .load::<RuntimeFeatureConfigDb>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to list active {} runtime feature configs: {err}",
                        scope.as_key()
                    )))
                })?;
            rows.into_iter()
                .map(RuntimeFeatureConfigDb::from_db)
                .map(RuntimeFeatureConfigView::from_row)
                .collect()
        })
    }

    fn upsert_owner_config(
        scope: RuntimeFeatureConfigScope,
        owner_id: i64,
        feature_key: RuntimeFeatureKey,
        enabled_value: bool,
    ) -> DbResult<i64> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<i64, BaseError, _>(|conn| {
                let mut query = runtime_feature_config::table
                    .filter(
                        runtime_feature_config::dsl::scope_kind
                            .eq(scope.as_key())
                            .and(
                                runtime_feature_config::dsl::feature_key.eq(feature_key.as_key()),
                            )
                            .and(runtime_feature_config::dsl::deleted_at.is_null()),
                    )
                    .into_boxed();
                query = match scope {
                    RuntimeFeatureConfigScope::Provider => {
                        query.filter(runtime_feature_config::dsl::provider_id.eq(owner_id))
                    }
                    RuntimeFeatureConfigScope::Model => {
                        query.filter(runtime_feature_config::dsl::model_id.eq(owner_id))
                    }
                };

                let existing = query
                    .select(RuntimeFeatureConfigDb::as_select())
                    .first::<RuntimeFeatureConfigDb>(conn)
                    .optional()
                    .map_err(|err| {
                        BaseError::DatabaseFatal(Some(format!(
                            "failed to fetch existing {} runtime feature config for owner {} and feature {}: {err}",
                            scope.as_key(),
                            owner_id,
                            feature_key.as_key()
                        )))
                    })?
                    .map(RuntimeFeatureConfigDb::from_db);

                if let Some(existing) = existing {
                    diesel::update(runtime_feature_config::table.find(existing.id))
                        .set((
                            runtime_feature_config::dsl::enabled.eq(enabled_value),
                            runtime_feature_config::dsl::updated_at.eq(now),
                        ))
                        .returning(runtime_feature_config::dsl::id)
                        .get_result::<i64>(conn)
                        .map_err(|err| {
                            map_write_error("failed to update runtime feature config", err)
                        })
                } else {
                    let new_config = NewRuntimeFeatureConfig {
                        id: ID_GENERATOR.generate_id(),
                        scope_kind: scope.as_key().to_string(),
                        provider_id: matches!(scope, RuntimeFeatureConfigScope::Provider)
                            .then_some(owner_id),
                        model_id: matches!(scope, RuntimeFeatureConfigScope::Model)
                            .then_some(owner_id),
                        feature_key: feature_key.as_key().to_string(),
                        enabled: enabled_value,
                        created_at: now,
                        updated_at: now,
                    };
                    diesel::insert_into(runtime_feature_config::table)
                        .values(NewRuntimeFeatureConfigDb::to_db(&new_config))
                        .returning(runtime_feature_config::dsl::id)
                        .get_result::<i64>(conn)
                        .map_err(|err| {
                            map_write_error("failed to create runtime feature config", err)
                        })
                }
            })
        })
    }

    fn delete_owner_config(
        scope: RuntimeFeatureConfigScope,
        owner_id: i64,
        feature_key: RuntimeFeatureKey,
    ) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let mut query = runtime_feature_config::table
                .filter(
                    runtime_feature_config::dsl::scope_kind
                        .eq(scope.as_key())
                        .and(runtime_feature_config::dsl::feature_key.eq(feature_key.as_key()))
                        .and(runtime_feature_config::dsl::deleted_at.is_null()),
                )
                .into_boxed();
            query = match scope {
                RuntimeFeatureConfigScope::Provider => {
                    query.filter(runtime_feature_config::dsl::provider_id.eq(owner_id))
                }
                RuntimeFeatureConfigScope::Model => {
                    query.filter(runtime_feature_config::dsl::model_id.eq(owner_id))
                }
            };
            let config_ids = query
                .select(runtime_feature_config::dsl::id)
                .load::<i64>(conn)
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "failed to fetch {} runtime feature configs for deletion: {err}",
                        scope.as_key()
                    )))
                })?;

            if config_ids.is_empty() {
                return Ok(0);
            }

            diesel::update(
                runtime_feature_config::table
                    .filter(runtime_feature_config::dsl::id.eq_any(&config_ids)),
            )
            .set((
                runtime_feature_config::dsl::deleted_at.eq(Some(now)),
                runtime_feature_config::dsl::updated_at.eq(now),
            ))
            .execute(conn)
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "failed to delete {} runtime feature config for owner {} and feature {}: {err}",
                    scope.as_key(),
                    owner_id,
                    feature_key.as_key()
                )))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};

    fn provider_input(id: i64, key: &str) -> NewProvider {
        let now = Utc::now().timestamp_millis();
        NewProvider {
            id,
            provider_key: key.to_string(),
            name: key.to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: now,
            updated_at: now,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }

    fn create_provider(id: i64, key: &str) -> Provider {
        Provider::create(&provider_input(id, key)).expect("provider")
    }

    fn create_model(provider_id: i64, name: &str) -> Model {
        Model::create(
            provider_id,
            name,
            None,
            true,
            ModelCapabilityFlags::default(),
        )
        .expect("model")
    }

    #[test]
    fn runtime_feature_key_registry_accepts_only_known_features() {
        assert_eq!(
            RuntimeFeatureKey::from_str("openai_reasoning_content_repair").expect("feature key"),
            RuntimeFeatureKey::OpenAiReasoningContentRepair
        );
        assert_eq!(
            RuntimeFeatureKey::OpenAiReasoningContentRepair.as_key(),
            "openai_reasoning_content_repair"
        );
        assert_eq!(
            serde_json::to_string(&RuntimeFeatureKey::OpenAiReasoningContentRepair)
                .expect("serialize feature key"),
            "\"openai_reasoning_content_repair\""
        );
        assert_eq!(RuntimeFeatureKey::ALL.len(), 1);
        assert!(RuntimeFeatureKey::from_str("deepseek_reasoning_content_repair").is_err());
    }

    #[test]
    fn provider_config_upsert_delete_and_reinsert_uses_active_soft_delete() {
        let db = TestDbContext::new_sqlite("runtime-feature-provider.sqlite");
        db.run_sync(|| {
            let provider = create_provider(4101, "openai-runtime-feature");
            let feature = RuntimeFeatureKey::OpenAiReasoningContentRepair;

            assert!(
                RuntimeFeatureConfig::get_active_provider_config(provider.id, feature)
                    .expect("initial provider feature")
                    .is_none()
            );

            let created = RuntimeFeatureConfig::upsert_provider_config(provider.id, feature, true)
                .expect("provider feature");
            assert_eq!(created.scope, RuntimeFeatureConfigScope::Provider);
            assert_eq!(created.feature_key, feature);
            assert_eq!(created.config.provider_id, Some(provider.id));
            assert_eq!(created.config.model_id, None);
            assert!(created.config.enabled);

            let updated = RuntimeFeatureConfig::upsert_provider_config(provider.id, feature, false)
                .expect("provider feature update");
            assert_eq!(updated.config.id, created.config.id);
            assert!(!updated.config.enabled);

            let listed =
                RuntimeFeatureConfig::list_active_provider_configs(&[provider.id]).expect("list");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].config.id, created.config.id);

            let deleted = RuntimeFeatureConfig::delete_provider_config(provider.id, feature)
                .expect("provider feature delete");
            assert_eq!(deleted, 1);
            assert!(
                RuntimeFeatureConfig::get_active_provider_config(provider.id, feature)
                    .expect("after delete")
                    .is_none()
            );

            let recreated =
                RuntimeFeatureConfig::upsert_provider_config(provider.id, feature, true)
                    .expect("provider feature reinsert");
            assert_ne!(recreated.config.id, created.config.id);
            assert!(recreated.config.enabled);
        });
    }

    #[test]
    fn model_config_false_is_an_explicit_override_and_delete_returns_to_inherit() {
        let db = TestDbContext::new_sqlite("runtime-feature-model.sqlite");
        db.run_sync(|| {
            let provider = create_provider(4201, "openai-runtime-feature-model");
            let model = create_model(provider.id, "gpt-5");
            let feature = RuntimeFeatureKey::OpenAiReasoningContentRepair;

            assert!(
                RuntimeFeatureConfig::get_active_model_config(model.id, feature)
                    .expect("initial model feature")
                    .is_none()
            );

            let disabled = RuntimeFeatureConfig::upsert_model_config(model.id, feature, false)
                .expect("model feature override");
            assert_eq!(disabled.scope, RuntimeFeatureConfigScope::Model);
            assert_eq!(disabled.config.model_id, Some(model.id));
            assert_eq!(disabled.config.provider_id, None);
            assert!(!disabled.config.enabled);

            let listed =
                RuntimeFeatureConfig::list_active_model_configs(&[model.id]).expect("model list");
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[0].config.id, disabled.config.id);

            let deleted = RuntimeFeatureConfig::delete_model_config(model.id, feature)
                .expect("model feature delete");
            assert_eq!(deleted, 1);
            assert!(
                RuntimeFeatureConfig::get_active_model_config(model.id, feature)
                    .expect("after inherit delete")
                    .is_none()
            );
        });
    }

    #[test]
    fn list_active_returns_provider_and_model_owner_rows_without_deleted_rows() {
        let db = TestDbContext::new_sqlite("runtime-feature-list-active.sqlite");
        db.run_sync(|| {
            let provider_a = create_provider(4301, "openai-runtime-feature-list-a");
            let provider_b = create_provider(4302, "openai-runtime-feature-list-b");
            let model_a = create_model(provider_a.id, "gpt-5-mini");
            let model_b = create_model(provider_b.id, "gpt-5-nano");
            let feature = RuntimeFeatureKey::OpenAiReasoningContentRepair;

            let provider_row =
                RuntimeFeatureConfig::upsert_provider_config(provider_a.id, feature, true)
                    .expect("provider row");
            let deleted_provider_row =
                RuntimeFeatureConfig::upsert_provider_config(provider_b.id, feature, true)
                    .expect("deleted provider row");
            let model_row = RuntimeFeatureConfig::upsert_model_config(model_a.id, feature, false)
                .expect("model row");
            let deleted_model_row =
                RuntimeFeatureConfig::upsert_model_config(model_b.id, feature, false)
                    .expect("deleted model row");

            assert_eq!(
                RuntimeFeatureConfig::delete_provider_config(provider_b.id, feature)
                    .expect("delete provider b"),
                1
            );
            assert_eq!(
                RuntimeFeatureConfig::delete_model_config(model_b.id, feature)
                    .expect("delete model b"),
                1
            );

            let all = RuntimeFeatureConfig::list_active().expect("all active");
            let active_ids: Vec<i64> = all.iter().map(|row| row.config.id).collect();
            assert!(active_ids.contains(&provider_row.config.id));
            assert!(active_ids.contains(&model_row.config.id));
            assert!(!active_ids.contains(&deleted_provider_row.config.id));
            assert!(!active_ids.contains(&deleted_model_row.config.id));

            let providers =
                RuntimeFeatureConfig::list_active_provider_configs(&[provider_a.id, provider_b.id])
                    .expect("provider list");
            assert_eq!(providers.len(), 1);
            assert_eq!(providers[0].config.provider_id, Some(provider_a.id));

            let models = RuntimeFeatureConfig::list_active_model_configs(&[model_a.id, model_b.id])
                .expect("model list");
            assert_eq!(models.len(), 1);
            assert_eq!(models[0].config.model_id, Some(model_a.id));
        });
    }
}
