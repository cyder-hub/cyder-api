use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use serde::Serialize;

use crate::controller::BaseError;
use crate::database::model::{Model, UpdateModelData};
use crate::database::model_source_binding::{
    ModelSourceConfig, get_config as get_model_source_config, list_visible_by_model_ids,
    replace_for_model as replace_model_source_config,
};
use crate::database::provider::Provider;
use crate::database::request_patch::RequestPatchVariantRepository;
use crate::database::upstream_source::UpstreamSource;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};
use crate::service::cache::types::{CacheModel, CacheModelSourceBinding, CacheProvider};
use crate::service::source_selector::{
    SourceSelectionTraceEntry, select_source, upstream_wire_family,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{
    AdminCatalogInvalidation, AdminModelCacheName, AdminMutationEffect, AdminMutationRunner,
};

#[derive(Debug, Clone)]
pub struct CreateModelInput {
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateModelInput {
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub cost_catalog_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSourceBindingSummary {
    pub source_id: i64,
    pub is_default: bool,
    pub profile_type: UpstreamProfileType,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSourceConfigSummary {
    pub source_selection_mode: String,
    pub bindings: Vec<ModelSourceBindingSummary>,
    pub declared_source_count: usize,
    pub enabled_source_count: usize,
    pub model_default_source_id: Option<i64>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSourceProtocolExplain {
    pub downstream_protocol: DownstreamProtocol,
    pub selection_status: String,
    pub source_id: Option<i64>,
    pub profile_type: Option<UpstreamProfileType>,
    pub upstream_protocol: Option<UpstreamProtocol>,
    pub selection_reason: Option<String>,
    pub transform_required: Option<bool>,
    pub decision_trace: Vec<SourceSelectionTraceEntry>,
    pub failure_reason: Option<String>,
    pub generation_execution_status: String,
    pub generation_execution_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelSourceExplain {
    pub model_id: i64,
    pub provider_id: i64,
    pub source_selection_mode: String,
    pub provider_enabled: bool,
    pub model_enabled: bool,
    pub declared_source_count: usize,
    pub enabled_source_count: usize,
    pub model_default_source_id: Option<i64>,
    pub warnings: Vec<String>,
    pub protocols: Vec<ModelSourceProtocolExplain>,
}

#[derive(Debug, Clone)]
pub struct ModelSourceSnapshotOwner {
    pub model_id: i64,
    pub provider_id: i64,
    pub source_selection_mode: String,
}

pub fn load_cache_model_snapshots(models: &[Model]) -> Result<HashMap<i64, CacheModel>, BaseError> {
    let model_ids = models.iter().map(|model| model.id).collect::<Vec<_>>();
    let bindings_by_model = list_visible_by_model_ids(&model_ids)?;
    Ok(models
        .iter()
        .map(|model| {
            let source_bindings = bindings_by_model
                .get(&model.id)
                .into_iter()
                .flatten()
                .map(|binding| CacheModelSourceBinding {
                    source_id: binding.source_id,
                    is_default: binding.is_default,
                })
                .collect();
            (
                model.id,
                CacheModel {
                    id: model.id,
                    provider_id: model.provider_id,
                    model_name: model.model_name.clone(),
                    real_model_name: model.real_model_name.clone(),
                    cost_catalog_id: model.cost_catalog_id,
                    source_selection_mode: model.source_selection_mode.clone(),
                    source_bindings,
                    is_enabled: model.is_enabled,
                },
            )
        })
        .collect())
}

pub fn load_model_source_config_summaries(
    owners: &[ModelSourceSnapshotOwner],
) -> Result<HashMap<i64, ModelSourceConfigSummary>, BaseError> {
    if owners.is_empty() {
        return Ok(HashMap::new());
    }

    let models = owners
        .iter()
        .map(|owner| Model {
            id: owner.model_id,
            provider_id: owner.provider_id,
            model_name: String::new(),
            real_model_name: None,
            cost_catalog_id: None,
            source_selection_mode: owner.source_selection_mode.clone(),
            deleted_at: None,
            is_enabled: true,
            created_at: 0,
            updated_at: 0,
        })
        .collect::<Vec<_>>();
    let snapshots = load_cache_model_snapshots(&models)?;
    let providers = Provider::list_all()?
        .into_iter()
        .map(|aggregate| (aggregate.id, CacheProvider::from(aggregate)))
        .collect::<HashMap<_, _>>();

    owners
        .iter()
        .map(|owner| {
            let provider = providers.get(&owner.provider_id).ok_or_else(|| {
                BaseError::DatabaseFatal(Some(format!(
                    "provider {} for model {} was not found while loading source config",
                    owner.provider_id, owner.model_id
                )))
            })?;
            let model = snapshots.get(&owner.model_id).ok_or_else(|| {
                BaseError::DatabaseFatal(Some(format!(
                    "model {} source snapshot was not loaded",
                    owner.model_id
                )))
            })?;
            Ok((
                owner.model_id,
                build_model_source_config_summary(provider, model),
            ))
        })
        .collect()
}

pub fn build_model_source_config_summary(
    provider: &CacheProvider,
    model: &CacheModel,
) -> ModelSourceConfigSummary {
    let mut warnings = BTreeSet::new();
    let explicit = model.source_selection_mode == "EXPLICIT";
    let inherit = model.source_selection_mode == "INHERIT_ALL";
    if !explicit && !inherit {
        warnings.insert("invalid_mode_binding_state".to_string());
    }
    if inherit && !model.source_bindings.is_empty() {
        warnings.insert("invalid_mode_binding_state".to_string());
    }

    let source_by_id = provider
        .upstream_sources
        .iter()
        .map(|source| (source.id, source))
        .collect::<HashMap<_, _>>();
    let declared_sources = if inherit {
        provider.upstream_sources.iter().collect::<Vec<_>>()
    } else {
        model
            .source_bindings
            .iter()
            .filter_map(|binding| source_by_id.get(&binding.source_id).copied())
            .collect::<Vec<_>>()
    };
    if explicit && model.source_bindings.is_empty() {
        warnings.insert("explicit_empty".to_string());
    }
    if declared_sources.is_empty() && !explicit {
        warnings.insert("no_visible_source".to_string());
    }

    let enabled_source_count = declared_sources
        .iter()
        .filter(|source| source.is_enabled)
        .count();
    if !declared_sources.is_empty() && enabled_source_count == 0 {
        warnings.insert("no_enabled_source".to_string());
    }

    let model_default_source_id = if explicit {
        model
            .source_bindings
            .iter()
            .find(|binding| binding.is_default)
            .map(|binding| binding.source_id)
    } else {
        None
    };
    if let Some(default_source_id) = model_default_source_id {
        if source_by_id
            .get(&default_source_id)
            .is_some_and(|source| !source.is_enabled)
        {
            warnings.insert("model_default_unavailable".to_string());
        }
    }

    let bindings = if explicit {
        model
            .source_bindings
            .iter()
            .filter_map(|binding| {
                source_by_id
                    .get(&binding.source_id)
                    .map(|source| ModelSourceBindingSummary {
                        source_id: binding.source_id,
                        is_default: binding.is_default,
                        profile_type: source.profile_type,
                        is_enabled: source.is_enabled,
                    })
            })
            .collect()
    } else {
        Vec::new()
    };

    ModelSourceConfigSummary {
        source_selection_mode: model.source_selection_mode.clone(),
        bindings,
        declared_source_count: declared_sources.len(),
        enabled_source_count,
        model_default_source_id,
        warnings: warnings.into_iter().collect(),
    }
}

pub fn build_model_source_explain(
    provider: &CacheProvider,
    model: &CacheModel,
) -> ModelSourceExplain {
    let summary = build_model_source_config_summary(provider, model);
    let protocols = DownstreamProtocol::ALL
        .into_iter()
        .map(|downstream_protocol| {
            match select_source(provider, model, downstream_protocol) {
                Ok(selection) => ModelSourceProtocolExplain {
                    downstream_protocol,
                    selection_status: "selected".to_string(),
                    source_id: Some(selection.source.id),
                    profile_type: Some(selection.source.profile_type),
                    upstream_protocol: Some(upstream_wire_family(&selection.source)),
                    selection_reason: Some(selection.reason.as_key().to_string()),
                    transform_required: Some(selection.transform_required),
                    decision_trace: selection.trace,
                    failure_reason: None,
                    generation_execution_status: "runtime_validation_required".to_string(),
                    generation_execution_reason: "Gateway Materializer, credential, Circuit, and upstream checks run after Source selection and do not participate in selection.".to_string(),
                },
                Err(error) => ModelSourceProtocolExplain {
                    downstream_protocol,
                    selection_status: "unselectable".to_string(),
                    source_id: None,
                    profile_type: None,
                    upstream_protocol: None,
                    selection_reason: None,
                    transform_required: None,
                    decision_trace: error.trace,
                    failure_reason: Some(error.failure.as_key().to_string()),
                    generation_execution_status: "not_reached".to_string(),
                    generation_execution_reason: "Source selection failed before gateway generation execution could be evaluated.".to_string(),
                },
            }
        })
        .collect();

    ModelSourceExplain {
        model_id: model.id,
        provider_id: model.provider_id,
        source_selection_mode: summary.source_selection_mode.clone(),
        provider_enabled: provider.is_enabled,
        model_enabled: model.is_enabled,
        declared_source_count: summary.declared_source_count,
        enabled_source_count: summary.enabled_source_count,
        model_default_source_id: summary.model_default_source_id,
        warnings: summary.warnings,
        protocols,
    }
}

pub struct ModelAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ModelAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_model(&self, input: CreateModelInput) -> Result<Model, BaseError> {
        self.create_model_with_source_config(input, None).await
    }

    pub async fn create_model_with_source_config(
        &self,
        input: CreateModelInput,
        source_config: Option<ModelSourceConfig>,
    ) -> Result<Model, BaseError> {
        let provider = Provider::get_by_id(input.provider_id)?;
        let created = Model::create_with_source_config(
            input.provider_id,
            &input.model_name,
            input.real_model_name.as_deref(),
            input.is_enabled,
            source_config.as_ref(),
        )?;
        let committed_source_config = source_config
            .clone()
            .unwrap_or_else(ModelSourceConfig::inherit_all);

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id: created.id,
                name: Some(model_cache_name(&provider, &created.model_name)),
                previous_name: None,
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: provider.id,
                key: Some(provider.provider_key.clone()),
            }),
            AdminMutationEffect::audit(model_audit_event("create", &created)),
            AdminMutationEffect::audit(model_source_config_audit_event(
                "create",
                &created,
                &committed_source_config,
            )),
        ])
        .await;

        Ok(created)
    }

    pub async fn get_model_source_config(
        &self,
        model_id: i64,
    ) -> Result<ModelSourceConfig, BaseError> {
        Ok(get_model_source_config(model_id)?)
    }

    pub fn get_model_source_config_summary(
        &self,
        model_id: i64,
    ) -> Result<ModelSourceConfigSummary, BaseError> {
        let model = Model::get_by_id(model_id)?;
        let summary = load_model_source_config_summaries(&[ModelSourceSnapshotOwner {
            model_id: model.id,
            provider_id: model.provider_id,
            source_selection_mode: model.source_selection_mode,
        }])?;
        summary.get(&model.id).cloned().ok_or_else(|| {
            BaseError::DatabaseFatal(Some(format!(
                "source config summary for model {} was not loaded",
                model.id
            )))
        })
    }

    pub fn explain_model_source_config(
        &self,
        model_id: i64,
    ) -> Result<ModelSourceExplain, BaseError> {
        let model = Model::get_by_id(model_id)?;
        let provider = Provider::get_by_id(model.provider_id)?;
        let cache_provider = CacheProvider::from(provider);
        let cache_model = load_cache_model_snapshots(std::slice::from_ref(&model))?
            .remove(&model.id)
            .ok_or_else(|| {
                BaseError::DatabaseFatal(Some(format!(
                    "model {} source snapshot was not loaded",
                    model.id
                )))
            })?;
        Ok(build_model_source_explain(&cache_provider, &cache_model))
    }

    pub async fn replace_model_source_config(
        &self,
        model_id: i64,
        source_config: ModelSourceConfig,
    ) -> Result<ModelSourceConfig, BaseError> {
        let model = Model::get_by_id(model_id)?;
        let provider = Provider::get_by_id(model.provider_id)?;
        let reactivated_source_ids = if source_config.source_selection_mode == "EXPLICIT" {
            source_config
                .bindings
                .iter()
                .map(|binding| binding.source_id)
                .collect::<Vec<_>>()
        } else if source_config.source_selection_mode == "INHERIT_ALL" {
            UpstreamSource::list_active_by_provider_id(provider.id)?
                .into_iter()
                .filter(|source| source.is_enabled)
                .map(|source| source.id)
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        for source_id in reactivated_source_ids {
            RequestPatchVariantRepository::validate_model_source_reactivation(model_id, source_id)?;
        }
        let replaced = replace_model_source_config(model_id, &source_config)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id: model.id,
                name: Some(model_cache_name(&provider, &model.model_name)),
                previous_name: None,
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: provider.id,
                key: Some(provider.provider_key.clone()),
            }),
            AdminMutationEffect::audit(model_source_config_audit_event(
                "replace", &model, &replaced,
            )),
        ])
        .await;

        Ok(replaced)
    }

    pub async fn update_model(&self, id: i64, input: UpdateModelInput) -> Result<Model, BaseError> {
        let existing = Model::get_by_id(id)?;
        let provider = Provider::get_by_id(existing.provider_id)?;
        let updated = Model::update(
            id,
            &UpdateModelData {
                model_name: Some(input.model_name),
                real_model_name: Some(input.real_model_name),
                is_enabled: Some(input.is_enabled),
                cost_catalog_id: Some(input.cost_catalog_id),
            },
        )?;

        let previous_name = if existing.model_name != updated.model_name {
            Some(model_cache_name(&provider, &existing.model_name))
        } else {
            None
        };

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id: updated.id,
                name: Some(model_cache_name(&provider, &updated.model_name)),
                previous_name,
            }),
            AdminMutationEffect::audit(model_audit_event("update", &updated)),
        ])
        .await;

        Ok(updated)
    }

    pub async fn delete_model(&self, id: i64) -> Result<(), BaseError> {
        let model = Model::get_by_id(id)?;
        let provider = Provider::get_by_id(model.provider_id)?;
        let num_deleted = Model::delete_with_dependents(id)?;

        if num_deleted == 0 {
            return Ok(());
        }

        let effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id,
                name: Some(model_cache_name(&provider, &model.model_name)),
                previous_name: None,
            }),
            AdminMutationEffect::audit(model_audit_event("delete", &model)),
        ];

        self.run_post_commit_effects(effects).await;

        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn model_cache_name(provider: &Provider, model_name: &str) -> AdminModelCacheName {
    AdminModelCacheName::new(provider.provider_key.clone(), model_name.to_string())
}

fn model_audit_event(action: &'static str, model: &Model) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.model_created",
        "update" => "manager.model_updated",
        "delete" => "manager.model_deleted",
        _ => unreachable!("unsupported model audit action: {action}"),
    };

    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("model_id", model.id),
        AdminAuditField::new("provider_id", model.provider_id),
        AdminAuditField::new("model_name", &model.model_name),
        AdminAuditField::new("is_enabled", model.is_enabled),
    ];
    fields.extend(AdminAuditField::optional(
        "real_model_name",
        model.real_model_name.as_deref(),
    ));
    AdminAuditEvent::with_fields(event_name, fields)
}

fn model_source_config_audit_event(
    action: &'static str,
    model: &Model,
    config: &ModelSourceConfig,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.model_source_config_created",
        "replace" => "manager.model_source_config_replaced",
        _ => unreachable!("unsupported model source config audit action: {action}"),
    };
    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("model_id", model.id),
        AdminAuditField::new("provider_id", model.provider_id),
        AdminAuditField::new("source_selection_mode", &config.source_selection_mode),
        AdminAuditField::new("binding_count", config.bindings.len()),
    ];
    fields.extend(AdminAuditField::optional(
        "default_source_id",
        config
            .bindings
            .iter()
            .find(|binding| binding.is_default)
            .map(|binding| binding.source_id),
    ));
    AdminAuditEvent::with_fields(event_name, fields)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::database::TestDbContext;
    use crate::database::model_source_binding::{ModelSourceBindingInput, ModelSourceConfig};
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::database::upstream_source::{NewUpstreamSource, UpstreamSource};
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use crate::service::catalog::CatalogService;

    fn provider_input() -> NewProvider {
        NewProvider {
            id: 8101,
            provider_key: "admin-provider".to_string(),
            name: "Admin Provider".to_string(),
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        }
    }

    fn source_input() -> NewUpstreamSource {
        NewUpstreamSource {
            id: 8102,
            provider_id: 8101,
            profile_type: UpstreamProfileType::Openai,
            endpoint: "https://admin.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            is_default: true,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn explicit_config() -> ModelSourceConfig {
        ModelSourceConfig::explicit(vec![ModelSourceBindingInput {
            source_id: 8102,
            is_default: true,
        }])
    }

    #[tokio::test]
    async fn admin_model_source_config_is_atomic_and_core_update_preserves_it() {
        let db = TestDbContext::new_sqlite("admin-model-source-config.sqlite");
        db.run_async(async {
            Provider::create(&provider_input(), &source_input()).expect("provider should seed");
            let catalog = Arc::new(CatalogService::new(true).await);
            let runner = Arc::new(AdminMutationRunner::new(Arc::clone(&catalog)));
            let service = ModelAdminService::new(Arc::clone(&runner));

            let model = service
                .create_model_with_source_config(
                    CreateModelInput {
                        provider_id: 8101,
                        model_name: "admin-model".to_string(),
                        real_model_name: None,
                        is_enabled: true,
                    },
                    Some(explicit_config()),
                )
                .await
                .expect("model create should commit with source config");
            assert_eq!(
                service
                    .get_model_source_config(model.id)
                    .await
                    .expect("source config should load"),
                explicit_config()
            );
            let cached_before = catalog
                .get_model_by_id(model.id)
                .await
                .expect("model cache should load")
                .expect("created model should be cacheable");
            assert_eq!(
                cached_before.source_selection_mode, "EXPLICIT",
                "cache should include the committed source mode"
            );

            service
                .update_model(
                    model.id,
                    UpdateModelInput {
                        model_name: "admin-model-renamed".to_string(),
                        real_model_name: None,
                        is_enabled: true,
                        cost_catalog_id: None,
                    },
                )
                .await
                .expect("core update should commit");
            assert_eq!(
                service
                    .get_model_source_config(model.id)
                    .await
                    .expect("source config should remain"),
                explicit_config()
            );

            service
                .replace_model_source_config(model.id, ModelSourceConfig::inherit_all())
                .await
                .expect("source config replacement should commit");
            let cached_after = catalog
                .get_model_by_id(model.id)
                .await
                .expect("invalidated model cache should reload")
                .expect("model should remain cacheable");
            assert_eq!(
                cached_after.source_selection_mode, "INHERIT_ALL",
                "source config replacement must invalidate the model snapshot"
            );
            assert_eq!(
                service
                    .get_model_source_config(model.id)
                    .await
                    .expect("replaced config should load"),
                ModelSourceConfig::inherit_all()
            );
            let audit_events = runner.drain_audit_events();
            let source_config_event = audit_events
                .iter()
                .find(|event| event.event_name() == "manager.model_source_config_replaced")
                .expect("source config replacement should be audited");
            for (key, value) in [
                ("model_id", model.id.to_string()),
                ("provider_id", "8101".to_string()),
                ("source_selection_mode", "INHERIT_ALL".to_string()),
                ("binding_count", "0".to_string()),
            ] {
                assert!(
                    source_config_event
                        .fields()
                        .iter()
                        .any(|field| field.key() == key && field.value() == value)
                );
            }
            assert!(
                !source_config_event
                    .fields()
                    .iter()
                    .any(|field| field.key() == "endpoint" || field.key() == "api_key")
            );
        })
        .await;
    }

    #[tokio::test]
    async fn inherit_all_rejects_dormant_model_variant_without_effective_suffix_rules() {
        let db = TestDbContext::new_sqlite("admin-model-inherit-all-request-patch.sqlite");
        db.run_async(async {
            Provider::create(&provider_input(), &source_input()).expect("provider should seed");
            let second_source = UpstreamSource::create(&NewUpstreamSource {
                id: 8103,
                provider_id: 8101,
                profile_type: UpstreamProfileType::Anthropic,
                endpoint: "https://secondary.example.com/v1".to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: false,
                created_at: 1,
                updated_at: 1,
            })
            .expect("second Source should seed");
            let catalog = Arc::new(CatalogService::new(true).await);
            let runner = Arc::new(AdminMutationRunner::new(Arc::clone(&catalog)));
            let service = ModelAdminService::new(Arc::clone(&runner));
            let model = service
                .create_model_with_source_config(
                    CreateModelInput {
                        provider_id: 8101,
                        model_name: "inherit-all-model".to_string(),
                        real_model_name: None,
                        is_enabled: true,
                    },
                    Some(ModelSourceConfig::inherit_all()),
                )
                .await
                .expect("Model should be created in INHERIT_ALL mode");
            let source_variant = RequestPatchVariantRepository::create(&RequestPatchVariantInput {
                source_id: second_source.id,
                model_id: None,
                suffix: Some("fast".to_string()),
                enabled: true,
                expose_in_models: true,
                rules: vec![RequestPatchRuleInput {
                    placement: RequestPatchPlacement::Body,
                    target: "/options/temperature".to_string(),
                    operation: RequestPatchOperation::Set,
                    value_json: Some(Some(serde_json::json!(0.2))),
                    description: None,
                    confirm_dangerous_target: false,
                }],
            })
            .expect("Source suffix should be created");
            RequestPatchVariantRepository::create(&RequestPatchVariantInput {
                source_id: second_source.id,
                model_id: Some(model.id),
                suffix: Some("fast".to_string()),
                enabled: true,
                expose_in_models: false,
                rules: Vec::new(),
            })
            .expect("empty Model suffix should inherit Source Rules");

            service
                .replace_model_source_config(model.id, explicit_config())
                .await
                .expect("switching to the primary explicit Source should make S2 dormant");
            RequestPatchVariantRepository::soft_delete(source_variant.variant.id)
                .expect("dormant Source suffix may be removed");

            assert!(
                service
                    .replace_model_source_config(model.id, ModelSourceConfig::inherit_all())
                    .await
                    .is_err(),
                "INHERIT_ALL must preflight the reactivated S2 Model Variant"
            );
            assert_eq!(
                service
                    .get_model_source_config(model.id)
                    .await
                    .expect("failed replacement must preserve source config"),
                explicit_config()
            );
        })
        .await;
    }
}
