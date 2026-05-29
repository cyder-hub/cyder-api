use std::str::FromStr;
use std::sync::Arc;

use serde::Serialize;

use crate::controller::BaseError;
use crate::database::model::Model;
use crate::database::provider::Provider;
use crate::database::runtime_feature_config::{
    RuntimeFeatureConfig, RuntimeFeatureConfigScope, RuntimeFeatureConfigView, RuntimeFeatureKey,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFeatureCatalog {
    pub features: Vec<RuntimeFeatureCatalogItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFeatureCatalogItem {
    pub feature_key: String,
    pub label: String,
    pub description: String,
    pub default_enabled: bool,
    pub supported_scope_kinds: Vec<RuntimeFeatureConfigScope>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFeatureEffectiveSource {
    DefaultFalse,
    ProviderDefault,
    ModelOverride,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFeatureConfigAdminView {
    pub id: i64,
    pub scope_kind: RuntimeFeatureConfigScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub feature_key: String,
    pub enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFeatureConfigFeatureResponse {
    pub feature_key: String,
    pub owner_config: Option<RuntimeFeatureConfigAdminView>,
    pub provider_config: Option<RuntimeFeatureConfigAdminView>,
    pub effective_enabled: bool,
    pub effective_source: RuntimeFeatureEffectiveSource,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeFeatureConfigAdminResponse {
    pub owner_kind: RuntimeFeatureConfigScope,
    pub owner_id: i64,
    pub features: Vec<RuntimeFeatureConfigFeatureResponse>,
}

#[derive(Debug, Clone)]
pub struct UpsertRuntimeFeatureConfigInput {
    pub enabled: bool,
}

pub struct RuntimeFeatureConfigAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl RuntimeFeatureConfigAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub fn catalog(&self) -> RuntimeFeatureCatalog {
        RuntimeFeatureCatalog {
            features: RuntimeFeatureKey::ALL
                .into_iter()
                .map(runtime_feature_catalog_item)
                .collect(),
        }
    }

    pub fn get_provider_config(
        &self,
        provider_id: i64,
    ) -> Result<RuntimeFeatureConfigAdminResponse, BaseError> {
        ensure_provider(provider_id)?;
        provider_config_response(provider_id)
    }

    pub async fn upsert_provider_config(
        &self,
        provider_id: i64,
        feature_key: &str,
        input: UpsertRuntimeFeatureConfigInput,
    ) -> Result<RuntimeFeatureConfigAdminResponse, BaseError> {
        ensure_provider(provider_id)?;
        let feature_key = parse_feature_key(feature_key)?;
        let config =
            RuntimeFeatureConfig::upsert_provider_config(provider_id, feature_key, input.enabled)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::RuntimeFeatureProviderConfig { provider_id },
            ),
            AdminMutationEffect::audit(runtime_feature_config_audit_event(
                "provider_upserted",
                RuntimeFeatureConfigScope::Provider,
                provider_id,
                feature_key,
                Some(&config),
                input.enabled,
            )),
        ])
        .await;

        provider_config_response(provider_id)
    }

    pub async fn delete_provider_config(
        &self,
        provider_id: i64,
        feature_key: &str,
    ) -> Result<(), BaseError> {
        ensure_provider(provider_id)?;
        let feature_key = parse_feature_key(feature_key)?;
        let before = RuntimeFeatureConfig::get_active_provider_config(provider_id, feature_key)?;
        RuntimeFeatureConfig::delete_provider_config(provider_id, feature_key)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::RuntimeFeatureProviderConfig { provider_id },
            ),
            AdminMutationEffect::audit(runtime_feature_config_audit_event(
                "provider_deleted",
                RuntimeFeatureConfigScope::Provider,
                provider_id,
                feature_key,
                before.as_ref(),
                before
                    .as_ref()
                    .map(|config| config.config.enabled)
                    .unwrap_or(false),
            )),
        ])
        .await;

        Ok(())
    }

    pub fn get_model_config(
        &self,
        model_id: i64,
    ) -> Result<RuntimeFeatureConfigAdminResponse, BaseError> {
        ensure_model(model_id)?;
        model_config_response(model_id)
    }

    pub async fn upsert_model_config(
        &self,
        model_id: i64,
        feature_key: &str,
        input: UpsertRuntimeFeatureConfigInput,
    ) -> Result<RuntimeFeatureConfigAdminResponse, BaseError> {
        ensure_model(model_id)?;
        let feature_key = parse_feature_key(feature_key)?;
        let config =
            RuntimeFeatureConfig::upsert_model_config(model_id, feature_key, input.enabled)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::RuntimeFeatureModelConfig { model_id },
            ),
            AdminMutationEffect::audit(runtime_feature_config_audit_event(
                "model_upserted",
                RuntimeFeatureConfigScope::Model,
                model_id,
                feature_key,
                Some(&config),
                input.enabled,
            )),
        ])
        .await;

        model_config_response(model_id)
    }

    pub async fn delete_model_config(
        &self,
        model_id: i64,
        feature_key: &str,
    ) -> Result<(), BaseError> {
        ensure_model(model_id)?;
        let feature_key = parse_feature_key(feature_key)?;
        let before = RuntimeFeatureConfig::get_active_model_config(model_id, feature_key)?;
        RuntimeFeatureConfig::delete_model_config(model_id, feature_key)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::RuntimeFeatureModelConfig { model_id },
            ),
            AdminMutationEffect::audit(runtime_feature_config_audit_event(
                "model_deleted",
                RuntimeFeatureConfigScope::Model,
                model_id,
                feature_key,
                before.as_ref(),
                before
                    .as_ref()
                    .map(|config| config.config.enabled)
                    .unwrap_or(false),
            )),
        ])
        .await;

        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn runtime_feature_catalog_item(feature_key: RuntimeFeatureKey) -> RuntimeFeatureCatalogItem {
    match feature_key {
        RuntimeFeatureKey::OpenAiReasoningContentRepair => RuntimeFeatureCatalogItem {
            feature_key: feature_key.as_key().to_string(),
            label: "OpenAI reasoning_content repair".to_string(),
            description:
                "Restore observed assistant reasoning_content for compatible OpenAI tool continuations"
                    .to_string(),
            default_enabled: false,
            supported_scope_kinds: vec![
                RuntimeFeatureConfigScope::Provider,
                RuntimeFeatureConfigScope::Model,
            ],
        },
    }
}

fn parse_feature_key(value: &str) -> Result<RuntimeFeatureKey, BaseError> {
    RuntimeFeatureKey::from_str(value).map_err(|err| BaseError::ParamInvalid(Some(err)))
}

fn ensure_provider(provider_id: i64) -> Result<Provider, BaseError> {
    Provider::get_by_id(provider_id)
        .map_err(|err| map_owner_not_found(err, "provider", provider_id))
}

fn ensure_model(model_id: i64) -> Result<Model, BaseError> {
    Model::get_by_id(model_id).map_err(|err| map_owner_not_found(err, "model", model_id))
}

fn map_owner_not_found(err: BaseError, owner_kind: &'static str, owner_id: i64) -> BaseError {
    match err {
        BaseError::ParamInvalid(_) => {
            BaseError::NotFound(Some(format!("{owner_kind} {owner_id} not found")))
        }
        other => other,
    }
}

fn provider_config_response(
    provider_id: i64,
) -> Result<RuntimeFeatureConfigAdminResponse, BaseError> {
    let mut features = Vec::with_capacity(RuntimeFeatureKey::ALL.len());
    for feature_key in RuntimeFeatureKey::ALL {
        let owner_config =
            RuntimeFeatureConfig::get_active_provider_config(provider_id, feature_key)?
                .map(config_view);
        let (effective_enabled, effective_source) = match owner_config.as_ref() {
            Some(config) => (
                config.enabled,
                RuntimeFeatureEffectiveSource::ProviderDefault,
            ),
            None => (false, RuntimeFeatureEffectiveSource::DefaultFalse),
        };
        features.push(RuntimeFeatureConfigFeatureResponse {
            feature_key: feature_key.as_key().to_string(),
            owner_config,
            provider_config: None,
            effective_enabled,
            effective_source,
        });
    }

    Ok(RuntimeFeatureConfigAdminResponse {
        owner_kind: RuntimeFeatureConfigScope::Provider,
        owner_id: provider_id,
        features,
    })
}

fn model_config_response(model_id: i64) -> Result<RuntimeFeatureConfigAdminResponse, BaseError> {
    let model = ensure_model(model_id)?;
    let _provider = ensure_provider(model.provider_id)?;
    let mut features = Vec::with_capacity(RuntimeFeatureKey::ALL.len());

    for feature_key in RuntimeFeatureKey::ALL {
        let owner_config =
            RuntimeFeatureConfig::get_active_model_config(model_id, feature_key)?.map(config_view);
        let provider_config =
            RuntimeFeatureConfig::get_active_provider_config(model.provider_id, feature_key)?
                .map(config_view);

        let (effective_enabled, effective_source) = match owner_config.as_ref() {
            Some(config) => (config.enabled, RuntimeFeatureEffectiveSource::ModelOverride),
            None => match provider_config.as_ref() {
                Some(config) => (
                    config.enabled,
                    RuntimeFeatureEffectiveSource::ProviderDefault,
                ),
                None => (false, RuntimeFeatureEffectiveSource::DefaultFalse),
            },
        };

        features.push(RuntimeFeatureConfigFeatureResponse {
            feature_key: feature_key.as_key().to_string(),
            owner_config,
            provider_config,
            effective_enabled,
            effective_source,
        });
    }

    Ok(RuntimeFeatureConfigAdminResponse {
        owner_kind: RuntimeFeatureConfigScope::Model,
        owner_id: model_id,
        features,
    })
}

fn config_view(config: RuntimeFeatureConfigView) -> RuntimeFeatureConfigAdminView {
    RuntimeFeatureConfigAdminView {
        id: config.config.id,
        scope_kind: config.scope,
        provider_id: config.config.provider_id,
        model_id: config.config.model_id,
        feature_key: config.feature_key.as_key().to_string(),
        enabled: config.config.enabled,
        created_at: config.config.created_at,
        updated_at: config.config.updated_at,
    }
}

fn runtime_feature_config_audit_event(
    action: &'static str,
    scope: RuntimeFeatureConfigScope,
    owner_id: i64,
    feature_key: RuntimeFeatureKey,
    config: Option<&RuntimeFeatureConfigView>,
    enabled: bool,
) -> AdminAuditEvent {
    let event_name = match action {
        "provider_upserted" => "manager.runtime_feature_config_provider_upserted",
        "provider_deleted" => "manager.runtime_feature_config_provider_deleted",
        "model_upserted" => "manager.runtime_feature_config_model_upserted",
        "model_deleted" => "manager.runtime_feature_config_model_deleted",
        _ => unreachable!("unsupported runtime feature config audit action: {action}"),
    };
    let config_id = config.map(|config| config.config.id);

    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("scope", scope.as_key()),
        AdminAuditField::new("owner_id", owner_id),
        AdminAuditField::new("feature_key", feature_key.as_key()),
        AdminAuditField::new("enabled", enabled),
    ];
    fields.extend(AdminAuditField::optional(
        "runtime_feature_config_id",
        config_id,
    ));

    AdminAuditEvent::with_fields(event_name, fields)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::create_test_app_state;

    use super::{
        RuntimeFeatureEffectiveSource, UpsertRuntimeFeatureConfigInput,
        runtime_feature_config_audit_event,
    };
    use crate::database::runtime_feature_config::{RuntimeFeatureConfigScope, RuntimeFeatureKey};

    fn seed_provider(id: i64, provider_key: &str) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_model(provider_id: i64, model_name: &str) -> Model {
        Model::create(
            provider_id,
            model_name,
            None,
            true,
            ModelCapabilityFlags::default(),
        )
        .expect("model seed should succeed")
    }

    #[tokio::test]
    async fn provider_runtime_feature_lifecycle_refreshes_models_catalog() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-runtime-feature-config-provider.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(44101, "provider-runtime-feature-config");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load before mutation");

                let created = app_state
                    .admin
                    .runtime_feature_config
                    .upsert_provider_config(
                        provider.id,
                        RuntimeFeatureKey::OpenAiReasoningContentRepair.as_key(),
                        UpsertRuntimeFeatureConfigInput { enabled: true },
                    )
                    .await
                    .expect("provider runtime feature should create");
                assert_eq!(created.owner_kind, RuntimeFeatureConfigScope::Provider);
                assert_eq!(
                    created.features[0].effective_source,
                    RuntimeFeatureEffectiveSource::ProviderDefault
                );
                assert!(created.features[0].effective_enabled);

                let cached_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload after invalidation");
                assert!(!Arc::ptr_eq(&cached_before, &cached_after));

                app_state
                    .admin
                    .runtime_feature_config
                    .delete_provider_config(
                        provider.id,
                        RuntimeFeatureKey::OpenAiReasoningContentRepair.as_key(),
                    )
                    .await
                    .expect("provider runtime feature should delete");

                let deleted = app_state
                    .admin
                    .runtime_feature_config
                    .get_provider_config(provider.id)
                    .expect("provider config should load");
                assert_eq!(
                    deleted.features[0].effective_source,
                    RuntimeFeatureEffectiveSource::DefaultFalse
                );
                assert!(!deleted.features[0].effective_enabled);
            })
            .await;
    }

    #[tokio::test]
    async fn model_runtime_feature_inherits_provider_until_override_is_deleted() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-runtime-feature-config-model.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(44201, "model-runtime-feature-config");
                let model = seed_model(provider.id, "gpt-5-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                app_state
                    .admin
                    .runtime_feature_config
                    .upsert_provider_config(
                        provider.id,
                        RuntimeFeatureKey::OpenAiReasoningContentRepair.as_key(),
                        UpsertRuntimeFeatureConfigInput { enabled: true },
                    )
                    .await
                    .expect("provider runtime feature should create");

                let inherited = app_state
                    .admin
                    .runtime_feature_config
                    .get_model_config(model.id)
                    .expect("model config should load");
                assert_eq!(
                    inherited.features[0].effective_source,
                    RuntimeFeatureEffectiveSource::ProviderDefault
                );
                assert!(inherited.features[0].effective_enabled);
                assert!(inherited.features[0].owner_config.is_none());
                assert!(inherited.features[0].provider_config.is_some());

                let overridden = app_state
                    .admin
                    .runtime_feature_config
                    .upsert_model_config(
                        model.id,
                        RuntimeFeatureKey::OpenAiReasoningContentRepair.as_key(),
                        UpsertRuntimeFeatureConfigInput { enabled: false },
                    )
                    .await
                    .expect("model runtime feature should override");
                assert_eq!(
                    overridden.features[0].effective_source,
                    RuntimeFeatureEffectiveSource::ModelOverride
                );
                assert!(!overridden.features[0].effective_enabled);
                assert!(overridden.features[0].owner_config.is_some());

                app_state
                    .admin
                    .runtime_feature_config
                    .delete_model_config(
                        model.id,
                        RuntimeFeatureKey::OpenAiReasoningContentRepair.as_key(),
                    )
                    .await
                    .expect("model runtime feature should delete");

                let inherited_again = app_state
                    .admin
                    .runtime_feature_config
                    .get_model_config(model.id)
                    .expect("model config should load after delete");
                assert_eq!(
                    inherited_again.features[0].effective_source,
                    RuntimeFeatureEffectiveSource::ProviderDefault
                );
                assert!(inherited_again.features[0].effective_enabled);
                assert!(inherited_again.features[0].owner_config.is_none());
            })
            .await;
    }

    #[test]
    fn runtime_feature_audit_event_contains_scope_owner_feature_and_enabled() {
        let event = runtime_feature_config_audit_event(
            "provider_upserted",
            RuntimeFeatureConfigScope::Provider,
            7,
            RuntimeFeatureKey::OpenAiReasoningContentRepair,
            None,
            true,
        );

        assert_eq!(
            event.event_name(),
            "manager.runtime_feature_config_provider_upserted"
        );
        let field_value = |key: &str| {
            event
                .fields()
                .iter()
                .find(|field| field.key() == key)
                .map(|field| field.value().to_string())
                .expect("field should exist")
        };
        assert_eq!(field_value("scope"), "provider");
        assert_eq!(field_value("owner_id"), "7");
        assert_eq!(
            field_value("feature_key"),
            "openai_reasoning_content_repair"
        );
        assert_eq!(field_value("enabled"), "true");
    }
}
