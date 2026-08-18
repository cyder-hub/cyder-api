use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::controller::BaseError;
use crate::database::model::Model;
use crate::database::provider::{
    BootstrapProviderInput, BootstrapProviderResult, NewProvider, NewProviderApiKey, Provider,
    ProviderAggregate, ProviderApiKeyRepository, ProviderApiKeySummary,
    UpdateProviderApiKeyMetadata, UpdateProviderData,
};
use crate::database::request_patch::RequestPatchVariantRepository;
use crate::database::upstream_source::{
    NewUpstreamSource, UpdateUpstreamSourceData, UpstreamSource,
};
use crate::schema::enum_def::{ModelKind, ProviderApiKeyMode, UpstreamProfileType};
use crate::service::admin::model::load_cache_model_snapshots;
use crate::service::cache::types::{CacheModel, CacheProvider};
use crate::service::provider_http::{normalize_operation_path, normalize_source_base_url};
use crate::service::secret_encryption::{SecretDomain, SecretEncryptionService, SensitiveSecret};
use crate::service::source_selector::{select_source, select_source_with_sources};
use crate::service::vertex::invalidate_vertex_token;
use crate::utils::ID_GENERATOR;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone)]
pub struct ProviderUpsertInput {
    pub name: String,
    pub key: String,
    pub is_enabled: Option<bool>,
    pub initial_source: Option<UpstreamSourceCreateInput>,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

#[derive(Debug, Clone)]
pub struct ProviderUpdateInput {
    pub name: String,
    pub is_enabled: Option<bool>,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

#[derive(Debug, Clone)]
pub struct UpstreamSourceCreateInput {
    pub profile_type: UpstreamProfileType,
    pub base_url: Option<String>,
    pub use_proxy: bool,
    pub chat_completions_enabled: Option<bool>,
    pub chat_completions_path_override: Option<String>,
    pub embeddings_enabled: Option<bool>,
    pub embeddings_path_override: Option<String>,
    pub rerank_enabled: Option<bool>,
    pub rerank_path_override: Option<String>,
    pub is_enabled: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone)]
pub struct UpstreamSourceUpdateInput {
    pub base_url: Option<Option<String>>,
    pub use_proxy: Option<bool>,
    pub chat_completions_enabled: Option<bool>,
    pub chat_completions_path_override: Option<Option<String>>,
    pub embeddings_enabled: Option<bool>,
    pub embeddings_path_override: Option<Option<String>>,
    pub rerank_enabled: Option<bool>,
    pub rerank_path_override: Option<Option<String>>,
    pub is_enabled: Option<bool>,
    pub is_default: Option<bool>,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SourceImpactAction {
    Disable,
    Delete,
    SetDefault,
    UnsetDefault,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceImpactProtocolSummary {
    pub downstream_protocol: crate::schema::enum_def::DownstreamProtocol,
    pub selection_changed_count: usize,
    pub would_become_unselectable_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SourceImpactReport {
    pub action: SourceImpactAction,
    pub provider_id: i64,
    pub source_id: i64,
    pub inherit_all_model_count: usize,
    pub explicit_binding_model_count: usize,
    pub explicit_default_model_count: usize,
    pub source_variant_count: usize,
    pub model_variant_count: usize,
    pub request_patch_rule_count: usize,
    pub protocols: Vec<SourceImpactProtocolSummary>,
}

fn remove_simulated_source_bindings(snapshots: &mut HashMap<i64, CacheModel>, source_id: i64) {
    for snapshot in snapshots.values_mut() {
        snapshot
            .source_bindings
            .retain(|binding| binding.source_id != source_id);
    }
}

#[derive(Clone)]
pub struct CreateProviderApiKeyInput {
    pub api_key: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateProviderApiKeyInput {
    pub description: Option<String>,
    pub is_enabled: bool,
}

#[derive(Clone)]
pub struct ReplaceProviderApiKeyInput {
    pub api_key: String,
}

#[derive(Serialize)]
pub struct ProviderApiKeyReveal {
    #[serde(flatten)]
    pub summary: ProviderApiKeySummary,
    pub api_key: String,
}

#[derive(Clone)]
pub struct BootstrapProviderCommand {
    pub provider_id: i64,
    pub provider_key: String,
    pub name: String,
    pub source: UpstreamSourceCreateInput,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub api_key: String,
    pub api_key_description: Option<String>,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_kind: ModelKind,
}

#[derive(Debug, Clone)]
struct NormalizedSourceOperations {
    chat_completions_enabled: Option<bool>,
    chat_completions_path_override: Option<String>,
    embeddings_enabled: Option<bool>,
    embeddings_path_override: Option<String>,
    rerank_enabled: Option<bool>,
    rerank_path_override: Option<String>,
}

fn normalize_path_override(value: Option<String>) -> Result<Option<String>, BaseError> {
    value
        .map(|value| {
            normalize_operation_path(&value).map_err(|error| {
                BaseError::ParamInvalid(Some(format!("upstream operation path {error}")))
            })
        })
        .transpose()
}

fn reject_native_operation_fields(
    chat_completions_enabled: Option<bool>,
    chat_completions_path_override: &Option<String>,
    embeddings_enabled: Option<bool>,
    embeddings_path_override: &Option<String>,
    rerank_enabled: Option<bool>,
    rerank_path_override: &Option<String>,
) -> Result<(), BaseError> {
    if chat_completions_enabled.is_some()
        || chat_completions_path_override.is_some()
        || embeddings_enabled.is_some()
        || embeddings_path_override.is_some()
        || rerank_enabled.is_some()
        || rerank_path_override.is_some()
    {
        return Err(BaseError::ParamInvalid(Some(
            "native upstream Profiles must not define OpenAI operation fields".to_string(),
        )));
    }
    Ok(())
}

fn normalize_source_operations(
    input: &UpstreamSourceCreateInput,
) -> Result<NormalizedSourceOperations, BaseError> {
    let chat_path = normalize_path_override(input.chat_completions_path_override.clone())?;
    let embeddings_path = normalize_path_override(input.embeddings_path_override.clone())?;
    let rerank_path = normalize_path_override(input.rerank_path_override.clone())?;

    match input.profile_type {
        UpstreamProfileType::Openai => {
            if input.rerank_enabled.is_some() || rerank_path.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "OPENAI does not support the Rerank operation".to_string(),
                )));
            }
            Ok(NormalizedSourceOperations {
                chat_completions_enabled: Some(input.chat_completions_enabled.unwrap_or(true)),
                chat_completions_path_override: chat_path,
                embeddings_enabled: Some(input.embeddings_enabled.unwrap_or(true)),
                embeddings_path_override: embeddings_path,
                rerank_enabled: Some(false),
                rerank_path_override: None,
            })
        }
        UpstreamProfileType::OpenaiCompatible => Ok(NormalizedSourceOperations {
            chat_completions_enabled: Some(input.chat_completions_enabled.unwrap_or(true)),
            chat_completions_path_override: chat_path,
            embeddings_enabled: Some(input.embeddings_enabled.unwrap_or(false)),
            embeddings_path_override: embeddings_path,
            rerank_enabled: Some(input.rerank_enabled.unwrap_or(false)),
            rerank_path_override: rerank_path,
        }),
        UpstreamProfileType::GeminiOpenai => {
            if input.rerank_enabled.is_some() || rerank_path.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "GEMINI_OPENAI does not support the Rerank operation".to_string(),
                )));
            }
            Ok(NormalizedSourceOperations {
                chat_completions_enabled: Some(input.chat_completions_enabled.unwrap_or(true)),
                chat_completions_path_override: chat_path,
                embeddings_enabled: Some(input.embeddings_enabled.unwrap_or(true)),
                embeddings_path_override: embeddings_path,
                rerank_enabled: Some(false),
                rerank_path_override: None,
            })
        }
        _ => {
            reject_native_operation_fields(
                input.chat_completions_enabled,
                &chat_path,
                input.embeddings_enabled,
                &embeddings_path,
                input.rerank_enabled,
                &rerank_path,
            )?;
            Ok(NormalizedSourceOperations {
                chat_completions_enabled: None,
                chat_completions_path_override: None,
                embeddings_enabled: None,
                embeddings_path_override: None,
                rerank_enabled: None,
                rerank_path_override: None,
            })
        }
    }
}

fn new_upstream_source(
    provider_id: i64,
    input: UpstreamSourceCreateInput,
    now: i64,
) -> Result<NewUpstreamSource, BaseError> {
    let base_url = normalize_source_base_url(&input.profile_type, input.base_url.as_deref())
        .map_err(|error| {
            BaseError::ParamInvalid(Some(format!("upstream source base URL {error}")))
        })?;
    let operations = normalize_source_operations(&input)?;
    if input.is_default && !input.is_enabled {
        return Err(BaseError::ParamInvalid(Some(
            "a default upstream source must be enabled".to_string(),
        )));
    }

    Ok(NewUpstreamSource {
        id: ID_GENERATOR.generate_id(),
        provider_id,
        profile_type: input.profile_type,
        base_url,
        use_proxy: input.use_proxy,
        chat_completions_enabled: operations.chat_completions_enabled,
        chat_completions_path_override: operations.chat_completions_path_override,
        embeddings_enabled: operations.embeddings_enabled,
        embeddings_path_override: operations.embeddings_path_override,
        rerank_enabled: operations.rerank_enabled,
        rerank_path_override: operations.rerank_path_override,
        is_enabled: input.is_enabled,
        is_default: input.is_default,
        created_at: now,
        updated_at: now,
    })
}

fn normalize_source_update(
    before: &UpstreamSource,
    input: UpstreamSourceUpdateInput,
) -> Result<UpdateUpstreamSourceData, BaseError> {
    if matches!(
        before.profile_type,
        UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai
    ) && (input.rerank_enabled.is_some() || input.rerank_path_override.is_some())
    {
        return Err(BaseError::ParamInvalid(Some(format!(
            "{:?} does not support the Rerank operation",
            before.profile_type
        ))));
    }

    let merged = UpstreamSourceCreateInput {
        profile_type: before.profile_type,
        base_url: None,
        use_proxy: input.use_proxy.unwrap_or(before.use_proxy),
        chat_completions_enabled: input
            .chat_completions_enabled
            .or(before.chat_completions_enabled),
        chat_completions_path_override: input
            .chat_completions_path_override
            .clone()
            .unwrap_or_else(|| before.chat_completions_path_override.clone()),
        embeddings_enabled: input.embeddings_enabled.or(before.embeddings_enabled),
        embeddings_path_override: input
            .embeddings_path_override
            .clone()
            .unwrap_or_else(|| before.embeddings_path_override.clone()),
        rerank_enabled: if matches!(
            before.profile_type,
            UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai
        ) {
            None
        } else {
            input.rerank_enabled.or(before.rerank_enabled)
        },
        rerank_path_override: if matches!(
            before.profile_type,
            UpstreamProfileType::Openai | UpstreamProfileType::GeminiOpenai
        ) {
            None
        } else {
            input
                .rerank_path_override
                .clone()
                .unwrap_or_else(|| before.rerank_path_override.clone())
        },
        is_enabled: input.is_enabled.unwrap_or(before.is_enabled),
        is_default: input.is_default.unwrap_or(before.is_default),
    };
    let operations = normalize_source_operations(&merged)?;
    let base_url = input
        .base_url
        .map(|value| {
            normalize_source_base_url(&before.profile_type, value.as_deref()).map_err(|error| {
                BaseError::ParamInvalid(Some(format!("upstream source base URL {error}")))
            })
        })
        .transpose()?;

    Ok(UpdateUpstreamSourceData {
        base_url,
        use_proxy: input.use_proxy,
        chat_completions_enabled: input
            .chat_completions_enabled
            .map(|_| operations.chat_completions_enabled)
            .flatten(),
        chat_completions_path_override: input
            .chat_completions_path_override
            .map(|_| operations.chat_completions_path_override),
        embeddings_enabled: input
            .embeddings_enabled
            .map(|_| operations.embeddings_enabled)
            .flatten(),
        embeddings_path_override: input
            .embeddings_path_override
            .map(|_| operations.embeddings_path_override),
        rerank_enabled: input
            .rerank_enabled
            .map(|_| operations.rerank_enabled)
            .flatten(),
        rerank_path_override: input
            .rerank_path_override
            .map(|_| operations.rerank_path_override),
        is_enabled: input.is_enabled,
        is_default: input.is_default,
        updated_at: Utc::now().timestamp_millis(),
    })
}

pub struct ProviderAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
    secret_encryption: Arc<SecretEncryptionService>,
}

impl ProviderAdminService {
    pub(crate) fn new(
        mutation_runner: Arc<AdminMutationRunner>,
        secret_encryption: Arc<SecretEncryptionService>,
    ) -> Self {
        Self {
            mutation_runner,
            secret_encryption,
        }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_provider(
        &self,
        input: ProviderUpsertInput,
    ) -> Result<ProviderAggregate, BaseError> {
        let current_time = Utc::now().timestamp_millis();
        let provider_id = ID_GENERATOR.generate_id();
        let new_provider_data = NewProvider {
            id: provider_id,
            provider_key: input.key,
            name: input.name,
            is_enabled: input.is_enabled.unwrap_or(true),
            created_at: current_time,
            updated_at: current_time,
            provider_api_key_mode: input
                .provider_api_key_mode
                .unwrap_or(ProviderApiKeyMode::Queue),
        };
        let new_source_data = input
            .initial_source
            .map(|source| new_upstream_source(provider_id, source, current_time))
            .transpose()?;
        let database = self.mutation_runner.database();
        let created_provider =
            Provider::create_optional(&database, &new_provider_data, new_source_data.as_ref())
                .await?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: created_provider.id,
                key: Some(created_provider.provider_key.clone()),
            }),
            AdminMutationEffect::audit(provider_audit_event("create", &created_provider)),
        ])
        .await;

        Ok(created_provider)
    }

    pub async fn update_provider(
        &self,
        id: i64,
        input: ProviderUpdateInput,
    ) -> Result<ProviderAggregate, BaseError> {
        let update_data = UpdateProviderData {
            provider_key: None,
            name: Some(input.name),
            is_enabled: input.is_enabled,
            provider_api_key_mode: input.provider_api_key_mode,
        };
        let database = self.mutation_runner.database();
        let updated_provider = Provider::update(&database, id, &update_data).await?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: updated_provider.id,
                key: Some(updated_provider.provider_key.clone()),
            }),
            AdminMutationEffect::audit(provider_audit_event("update", &updated_provider)),
        ])
        .await;

        Ok(updated_provider)
    }

    pub async fn create_source(
        &self,
        provider_id: i64,
        input: UpstreamSourceCreateInput,
    ) -> Result<UpstreamSource, BaseError> {
        let now = Utc::now().timestamp_millis();
        let new_source = new_upstream_source(provider_id, input, now)?;
        let database = self.mutation_runner.database();
        let source = UpstreamSource::create(&database, &new_source).await?;
        self.run_runtime_refresh_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: provider_id,
                key: None,
            }),
            AdminMutationEffect::audit(source_audit_event("create", &source)),
        ])
        .await?;
        Ok(source)
    }

    pub async fn update_source(
        &self,
        provider_id: i64,
        source_id: i64,
        input: UpstreamSourceUpdateInput,
    ) -> Result<UpstreamSource, BaseError> {
        let database = self.mutation_runner.database();
        let before =
            UpstreamSource::get_active_by_id_for_provider(&database, source_id, provider_id)
                .await?;
        if input.is_enabled == Some(true) && !before.is_enabled {
            RequestPatchVariantRepository::validate_source_reactivation(&database, source_id)
                .await?;
        }
        let update = normalize_source_update(&before, input)?;
        let source = UpstreamSource::update(&database, source_id, provider_id, &update).await?;
        let effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: provider_id,
                key: None,
            }),
            AdminMutationEffect::audit(source_audit_event("update", &source)),
        ];
        self.run_runtime_refresh_post_commit(effects).await?;
        Ok(source)
    }

    pub async fn delete_source(&self, provider_id: i64, source_id: i64) -> Result<(), BaseError> {
        let database = self.mutation_runner.database();
        let source = UpstreamSource::delete(&database, source_id, provider_id).await?;
        self.run_runtime_refresh_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: provider_id,
                key: None,
            }),
            AdminMutationEffect::audit(source_audit_event("delete", &source)),
        ])
        .await?;
        Ok(())
    }

    pub async fn preview_source_impact(
        &self,
        provider_id: i64,
        source_id: i64,
        action: SourceImpactAction,
    ) -> Result<SourceImpactReport, BaseError> {
        let database = self.mutation_runner.database();
        let provider = Provider::get_by_id(&database, provider_id).await?;
        if !provider
            .upstream_sources
            .iter()
            .any(|source| source.id == source_id)
        {
            return Err(BaseError::NotFound(Some(format!(
                "upstream source {source_id} not found for provider {provider_id}"
            ))));
        }

        let models = Model::list_by_provider_id(&database, provider_id).await?;
        let source_variants =
            RequestPatchVariantRepository::list_by_source_ids(&database, &[source_id]).await?;
        let model_variants = RequestPatchVariantRepository::list_by_model_ids(
            &database,
            &models.iter().map(|model| model.id).collect::<Vec<_>>(),
        )
        .await?
        .into_iter()
        .filter(|variant| variant.variant.source_id == source_id)
        .collect::<Vec<_>>();
        let snapshots = load_cache_model_snapshots(&database, &models).await?;
        let cache_provider = CacheProvider::from(provider.clone());
        let mut simulated_sources = cache_provider.upstream_sources.clone();
        let mut simulated_snapshots = snapshots.clone();
        match action {
            SourceImpactAction::Disable => {
                let source = simulated_sources
                    .iter_mut()
                    .find(|source| source.id == source_id)
                    .expect("validated source should exist in simulation");
                source.is_enabled = false;
                source.is_default = false;
            }
            SourceImpactAction::Delete => {
                simulated_sources.retain(|source| source.id != source_id);
                remove_simulated_source_bindings(&mut simulated_snapshots, source_id);
            }
            SourceImpactAction::SetDefault => {
                for source in &mut simulated_sources {
                    source.is_default = source.id == source_id;
                    if source.id == source_id {
                        source.is_enabled = true;
                    }
                }
            }
            SourceImpactAction::UnsetDefault => {
                let source = simulated_sources
                    .iter_mut()
                    .find(|source| source.id == source_id)
                    .expect("validated source should exist in simulation");
                source.is_default = false;
            }
        }

        let mut inherit_all_model_count = 0;
        let mut explicit_binding_model_count = 0;
        let mut explicit_default_model_count = 0;
        for model in &models {
            if model.source_selection_mode == "INHERIT_ALL" {
                inherit_all_model_count += 1;
            } else if model.source_selection_mode == "EXPLICIT" {
                explicit_binding_model_count += 1;
                if snapshots.get(&model.id).is_some_and(|snapshot| {
                    snapshot
                        .source_bindings
                        .iter()
                        .any(|binding| binding.is_default)
                }) {
                    explicit_default_model_count += 1;
                }
            }
        }

        let protocols = crate::schema::enum_def::DownstreamProtocol::ALL
            .into_iter()
            .map(|downstream_protocol| {
                let mut selection_changed_count = 0;
                let mut would_become_unselectable_count = 0;
                for model in &models {
                    let Some(cache_model) = snapshots.get(&model.id) else {
                        continue;
                    };
                    let before = select_source(&cache_provider, cache_model, downstream_protocol)
                        .ok()
                        .map(|selection| selection.source.id);
                    let after = select_source_with_sources(
                        &cache_provider,
                        simulated_snapshots
                            .get(&model.id)
                            .expect("simulated snapshot should exist for loaded model"),
                        downstream_protocol,
                        &simulated_sources,
                    )
                    .ok()
                    .map(|selection| selection.source.id);
                    if before != after {
                        selection_changed_count += 1;
                    }
                    if before.is_some() && after.is_none() {
                        would_become_unselectable_count += 1;
                    }
                }
                SourceImpactProtocolSummary {
                    downstream_protocol,
                    selection_changed_count,
                    would_become_unselectable_count,
                }
            })
            .collect();

        Ok(SourceImpactReport {
            action,
            provider_id,
            source_id,
            inherit_all_model_count,
            explicit_binding_model_count,
            explicit_default_model_count,
            source_variant_count: source_variants.len(),
            model_variant_count: model_variants.len(),
            request_patch_rule_count: source_variants
                .iter()
                .chain(model_variants.iter())
                .map(|variant| variant.rules.len())
                .sum(),
            protocols,
        })
    }

    pub async fn create_provider_api_key(
        &self,
        provider_id: i64,
        input: CreateProviderApiKeyInput,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let database = self.mutation_runner.database();
        let _provider = Provider::get_by_id(&database, provider_id).await?;
        let current_time = Utc::now().timestamp_millis();
        let key_id = ID_GENERATOR.generate_id();
        let secret = validate_provider_secret(input.api_key)?;
        let (key_prefix, key_last4) = secret_mask_parts(secret.expose());
        let encrypted_secret = self
            .secret_encryption
            .encrypt_current(SecretDomain::ProviderApiKey(key_id), &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let secret_hmac = self
            .secret_encryption
            .provider_secret_fingerprint(provider_id, &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let new_key_data = NewProviderApiKey {
            id: key_id,
            provider_id,
            description: input.description,
            key_prefix,
            key_last4,
            encrypted_secret,
            secret_hmac,
            is_enabled: true,
            created_at: current_time,
            updated_at: current_time,
        };
        let created_key = ProviderApiKeyRepository::insert(&database, &new_key_data).await?;

        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("create", &created_key)),
        ])
        .await?;

        Ok(created_key)
    }

    pub async fn update_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
        input: UpdateProviderApiKeyInput,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let key_to_update = self
            .validate_provider_key_membership(provider_id, key_id)
            .await?;
        let update_data = UpdateProviderApiKeyMetadata {
            description: input.description,
            is_enabled: input.is_enabled,
        };
        let database = self.mutation_runner.database();
        let updated_key =
            ProviderApiKeyRepository::update_metadata(&database, provider_id, key_id, &update_data)
                .await?;

        invalidate_vertex_token(key_id);
        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("update", &updated_key)),
        ])
        .await?;

        // Keep the fetched row in scope so membership validation remains explicit.
        let _ = key_to_update;

        Ok(updated_key)
    }

    pub async fn delete_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<(), BaseError> {
        let key_to_delete = self
            .validate_provider_key_membership(provider_id, key_id)
            .await?;
        let database = self.mutation_runner.database();
        ProviderApiKeyRepository::soft_delete(&database, provider_id, key_id).await?;

        invalidate_vertex_token(key_id);
        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("delete", &key_to_delete)),
        ])
        .await?;

        Ok(())
    }

    pub(crate) async fn decrypt_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<SensitiveSecret, BaseError> {
        let database = self.mutation_runner.database();
        let _provider = Provider::get_by_id(&database, provider_id).await?;
        let stored =
            ProviderApiKeyRepository::get_stored_by_id(&database, provider_id, key_id).await?;
        let encrypted = stored
            .encrypted_secret()
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        self.secret_encryption
            .decrypt_current(SecretDomain::ProviderApiKey(key_id), &encrypted)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)
    }

    pub async fn list_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<Vec<ProviderApiKeySummary>, BaseError> {
        let database = self.mutation_runner.database();
        let _provider = Provider::get_by_id(&database, provider_id).await?;
        ProviderApiKeyRepository::list_summaries_by_provider_id(&database, provider_id).await
    }

    pub async fn get_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        self.validate_provider_key_membership(provider_id, key_id)
            .await
    }

    pub async fn replace_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
        input: ReplaceProviderApiKeyInput,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let database = self.mutation_runner.database();
        let _provider = Provider::get_by_id(&database, provider_id).await?;
        let _existing = self
            .validate_provider_key_membership(provider_id, key_id)
            .await?;
        let secret = validate_provider_secret(input.api_key)?;
        let (key_prefix, key_last4) = secret_mask_parts(secret.expose());
        let encrypted = self
            .secret_encryption
            .encrypt_current(SecretDomain::ProviderApiKey(key_id), &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let hmac = self
            .secret_encryption
            .provider_secret_fingerprint(provider_id, &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let updated = ProviderApiKeyRepository::replace_secret(
            &database,
            provider_id,
            key_id,
            key_prefix,
            key_last4,
            &encrypted,
            &hmac,
        )
        .await?;
        invalidate_vertex_token(key_id);
        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("replace", &updated)),
        ])
        .await?;
        Ok(updated)
    }

    pub async fn reveal_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<ProviderApiKeyReveal, BaseError> {
        let summary = self
            .validate_provider_key_membership(provider_id, key_id)
            .await?;
        let secret = self.decrypt_provider_api_key(provider_id, key_id).await?;
        self.run_post_commit_effects(vec![AdminMutationEffect::audit(
            provider_api_key_audit_event("reveal", &summary),
        )])
        .await;
        Ok(ProviderApiKeyReveal {
            summary,
            api_key: secret.to_unprotected_string(),
        })
    }

    pub async fn delete_provider(&self, id: i64) -> Result<(), BaseError> {
        let database = self.mutation_runner.database();
        let provider_to_delete = Provider::get_by_id(&database, id).await?;
        let provider_key_ids =
            ProviderApiKeyRepository::list_summaries_by_provider_id(&database, id)
                .await?
                .into_iter()
                .map(|key| key.id)
                .collect::<Vec<_>>();
        let num_deleted_db = Provider::delete_with_dependents(&database, id).await?;

        if num_deleted_db == 0 {
            return Ok(());
        }
        for key_id in provider_key_ids {
            invalidate_vertex_token(key_id);
        }

        let mut effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id,
                key: Some(provider_to_delete.provider_key.clone()),
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id: id,
            }),
        ];
        effects.push(AdminMutationEffect::audit(provider_audit_event(
            "delete",
            &provider_to_delete,
        )));

        self.run_runtime_refresh_post_commit(effects).await?;

        Ok(())
    }

    pub async fn bootstrap_provider_persist(
        &self,
        input: BootstrapProviderCommand,
    ) -> Result<BootstrapProviderResult, BaseError> {
        let source = new_upstream_source(
            input.provider_id,
            input.source,
            Utc::now().timestamp_millis(),
        )?;
        let key_id = ID_GENERATOR.generate_id();
        let secret = validate_provider_secret(input.api_key)?;
        let (key_prefix, key_last4) = secret_mask_parts(secret.expose());
        let encrypted_secret = self
            .secret_encryption
            .encrypt_current(SecretDomain::ProviderApiKey(key_id), &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let secret_hmac = self
            .secret_encryption
            .provider_secret_fingerprint(input.provider_id, &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let database = self.mutation_runner.database();
        let created = Provider::bootstrap(
            &database,
            &BootstrapProviderInput {
                provider_id: input.provider_id,
                provider_key: input.provider_key,
                name: input.name,
                source,
                provider_api_key_mode: input.provider_api_key_mode,
                provider_api_key_id: key_id,
                api_key_description: input.api_key_description,
                key_prefix,
                key_last4,
                encrypted_secret,
                secret_hmac,
                model_name: input.model_name,
                real_model_name: input.real_model_name,
                model_kind: input.model_kind,
            },
        )
        .await?;

        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: created.provider.id,
                key: Some(created.provider.provider_key.clone()),
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id: created.provider.id,
            }),
        ])
        .await?;

        Ok(created)
    }

    pub async fn record_bootstrap_audit(
        &self,
        created: &BootstrapProviderResult,
        check_success: Option<bool>,
    ) {
        self.run_post_commit_effects(vec![AdminMutationEffect::audit(
            provider_bootstrap_audit_event(created, check_success),
        )])
        .await;
    }

    async fn validate_provider_key_membership(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let database = self.mutation_runner.database();
        let _provider = Provider::get_by_id(&database, provider_id).await?;
        ProviderApiKeyRepository::get_summary_by_id(&database, provider_id, key_id).await
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }

    async fn run_runtime_refresh_post_commit(
        &self,
        effects: Vec<AdminMutationEffect>,
    ) -> Result<(), BaseError> {
        let report = self.mutation_runner.execute(&effects).await;
        if report.has_catalog_failures() {
            return Err(BaseError::ProviderRuntimeRefreshFailed);
        }
        Ok(())
    }

    async fn run_provider_key_post_commit(
        &self,
        effects: Vec<AdminMutationEffect>,
    ) -> Result<(), BaseError> {
        let report = self.mutation_runner.execute(&effects).await;
        if report.has_catalog_failures() {
            return Err(BaseError::ProviderRuntimeRefreshFailed);
        }
        Ok(())
    }
}

fn provider_audit_event(action: &'static str, provider: &ProviderAggregate) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.provider_created",
        "update" => "manager.provider_updated",
        "delete" => "manager.provider_deleted",
        _ => unreachable!("unsupported provider audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("provider_id", provider.id),
            AdminAuditField::new("provider_key", &provider.provider_key),
            AdminAuditField::new("provider_name", &provider.name),
            AdminAuditField::new("is_enabled", provider.is_enabled),
            AdminAuditField::new("source_count", provider.upstream_sources.len()),
        ],
    )
}

fn provider_bootstrap_audit_event(
    created: &BootstrapProviderResult,
    check_success: Option<bool>,
) -> AdminAuditEvent {
    let mut fields = vec![
        AdminAuditField::new("action", "bootstrap"),
        AdminAuditField::new("provider_id", created.provider.id),
        AdminAuditField::new("provider_key", &created.provider.provider_key),
        AdminAuditField::new("provider_name", &created.provider.name),
        AdminAuditField::new("is_enabled", created.provider.is_enabled),
        AdminAuditField::new("source_count", created.provider.upstream_sources.len()),
        AdminAuditField::new("provider_api_key_id", created.created_key.id),
        AdminAuditField::new("model_id", created.created_model.id),
        AdminAuditField::new("model_name", &created.created_model.model_name),
        AdminAuditField::new("check_performed", check_success.is_some()),
    ];
    fields.extend(AdminAuditField::optional("check_success", check_success));
    AdminAuditEvent::with_fields("manager.provider_bootstrapped", fields)
}

fn source_audit_event(action: &'static str, source: &UpstreamSource) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.provider_source_created",
        "update" => "manager.provider_source_updated",
        "delete" => "manager.provider_source_deleted",
        _ => unreachable!("unsupported source audit action: {action}"),
    };
    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("provider_id", source.provider_id),
            AdminAuditField::new("source_id", source.id),
            AdminAuditField::new("profile_type", format!("{:?}", source.profile_type)),
            AdminAuditField::new("is_enabled", source.is_enabled),
            AdminAuditField::new("is_default", source.is_default),
        ],
    )
}

fn provider_api_key_audit_event(
    action: &'static str,
    key: &ProviderApiKeySummary,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.provider_api_key_created",
        "update" => "manager.provider_api_key_updated",
        "delete" => "manager.provider_api_key_deleted",
        "replace" => "manager.provider_api_key_replaced",
        "reveal" => "manager.provider_api_key_revealed",
        _ => unreachable!("unsupported provider api key audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("provider_id", key.provider_id),
            AdminAuditField::new("provider_api_key_id", key.id),
            AdminAuditField::new("is_enabled", key.is_enabled),
            AdminAuditField::new("description_present", key.description.is_some()),
        ],
    )
}

fn validate_provider_secret(value: String) -> Result<SensitiveSecret, BaseError> {
    if value.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "provider API key must not be empty".to_string(),
        )));
    }
    Ok(SensitiveSecret::new(value))
}

fn secret_mask_parts(secret: &str) -> (String, String) {
    let characters = secret.chars().collect::<Vec<_>>();
    let visible_budget = characters.len().saturating_sub(1);
    let prefix_len = visible_budget.div_ceil(2).min(4);
    let suffix_len = visible_budget.saturating_sub(prefix_len).min(4);
    let prefix = characters.iter().take(prefix_len).collect::<String>();
    let last4 = characters
        .iter()
        .rev()
        .take(suffix_len)
        .rev()
        .collect::<String>();
    (prefix, last4)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::config::SecretEncryptionConfig;
    use crate::database::TestDatabase;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::database::upstream_source::NewUpstreamSource;
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use crate::service::admin::mutation::AdminMutationRunner;
    use crate::service::catalog::CatalogService;
    use crate::service::secret_encryption::SecretEncryptionService;
    use serde_json::json;

    #[test]
    fn provider_secret_masks_always_hide_at_least_one_character() {
        let cases = [
            ("a", "", ""),
            ("ab", "a", ""),
            ("abc", "a", "c"),
            ("abcd", "ab", "d"),
            ("abcde", "ab", "de"),
            ("abcdef", "abc", "ef"),
            ("abcdefg", "abc", "efg"),
            ("abcdefgh", "abcd", "fgh"),
            ("abcdefghi", "abcd", "fghi"),
            ("abcdefghij", "abcd", "ghij"),
        ];

        for (secret, expected_prefix, expected_suffix) in cases {
            let (prefix, suffix) = secret_mask_parts(secret);
            assert_eq!(prefix, expected_prefix, "unexpected prefix for {secret}");
            assert_eq!(suffix, expected_suffix, "unexpected suffix for {secret}");
            assert!(
                prefix.chars().count() + suffix.chars().count() < secret.chars().count(),
                "mask fragments must not expose the complete secret: {secret}"
            );
        }
    }

    #[test]
    fn provider_secret_masks_count_unicode_characters_without_overlap() {
        let secret = "密钥甲乙丙";
        let (prefix, suffix) = secret_mask_parts(secret);

        assert_eq!(prefix, "密钥");
        assert_eq!(suffix, "乙丙");
    }

    #[test]
    fn provider_secret_validation_accepts_non_empty_secrets_of_any_length() {
        let secret = validate_provider_secret("x".to_string())
            .expect("single-character provider credentials remain valid");

        assert_eq!(secret.expose(), "x");
    }

    #[test]
    fn provider_credentials_are_opaque_for_every_source_profile() {
        for profile in [
            UpstreamProfileType::Vertex,
            UpstreamProfileType::Openai,
            UpstreamProfileType::Gemini,
        ] {
            let secret =
                validate_provider_secret(format!("not-json-or-profile-specific-{profile:?}"))
                    .expect("non-empty provider credentials should remain opaque");
            assert!(!secret.expose().is_empty());
        }
    }

    #[tokio::test]
    async fn source_reactivation_validates_variants_and_refreshes_catalog() {
        let database = TestDatabase::new_sqlite_default("admin-source-reactivation.sqlite").await;
        let runtime = database.runtime();
        (async {
            Provider::create(
                &runtime,
                &NewProvider {
                    id: 9201,
                    provider_key: "source-reactivation".to_string(),
                    name: "Source Reactivation".to_string(),
                    is_enabled: true,
                    created_at: 1,
                    updated_at: 1,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                },
                &NewUpstreamSource {
                    id: 9202,
                    provider_id: 9201,
                    profile_type: UpstreamProfileType::Openai,
                    base_url: "https://source-reactivation.example/v1".to_string(),
                    use_proxy: false,
                    is_enabled: false,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                    ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
                },
            )
            .await
            .expect("provider should be seeded");
            RequestPatchVariantRepository::create(
                &runtime,
                &RequestPatchVariantInput {
                    source_id: 9202,
                    model_id: None,
                    suffix: Some("fast".to_string()),
                    enabled: true,
                    expose_in_models: true,
                    rules: vec![RequestPatchRuleInput {
                        placement: RequestPatchPlacement::Body,
                        target: "/options/temperature".to_string(),
                        operation: RequestPatchOperation::Set,
                        value_json: Some(Some(json!(0.2))),
                        description: None,
                    }],
                },
            )
            .await
            .expect("disabled source should accept preconfigured Variant");

            let catalog = Arc::new(CatalogService::new(runtime, true).await);
            let runner = Arc::new(AdminMutationRunner::new(Arc::clone(&catalog)));
            let service = ProviderAdminService::new(
                Arc::clone(&runner),
                Arc::new(SecretEncryptionService::from_config(
                    &SecretEncryptionConfig::default(),
                )),
            );
            let source = service
                .update_source(
                    9201,
                    9202,
                    UpstreamSourceUpdateInput {
                        base_url: None,
                        use_proxy: None,
                        chat_completions_enabled: None,
                        chat_completions_path_override: None,
                        embeddings_enabled: None,
                        embeddings_path_override: None,
                        rerank_enabled: None,
                        rerank_path_override: None,
                        is_enabled: Some(true),
                        is_default: Some(true),
                    },
                )
                .await
                .expect("source reactivation should validate and commit");
            assert!(source.is_enabled && source.is_default);
            assert!(
                runner
                    .drain_audit_events()
                    .iter()
                    .any(|event| { event.event_name() == "manager.provider_source_updated" })
            );
            assert_eq!(
                catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload after source update")
                    .request_patch_variants
                    .len(),
                1
            );
        })
        .await;
    }

    #[test]
    fn deleting_a_source_removes_its_bindings_from_the_simulated_snapshots() {
        let mut snapshots = HashMap::from([(
            1,
            CacheModel {
                id: 1,
                provider_id: 2,
                model_name: "model".to_string(),
                real_model_name: None,
                model_kind: crate::schema::enum_def::ModelKind::Chat,
                cost_catalog_id: None,
                source_selection_mode: "EXPLICIT".to_string(),
                source_bindings: vec![
                    crate::service::source_selector::source_binding(10, true),
                    crate::service::source_selector::source_binding(20, false),
                ],
                is_enabled: true,
            },
        )]);

        remove_simulated_source_bindings(&mut snapshots, 10);

        assert_eq!(
            snapshots[&1].source_bindings,
            vec![crate::service::source_selector::source_binding(20, false)]
        );
    }
}
