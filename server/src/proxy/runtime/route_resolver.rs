use std::{fmt, sync::Arc};

use crate::{
    schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::{
            CacheModel, CacheModelsCatalog, CacheProvider, CacheRequestPatchVariant,
            CacheUpstreamSource,
        },
        source_selector::select_source as select_model_source,
    },
};
use cyder_tools::log::error;

pub use crate::service::source_selector::SourceSelectionReason;

use super::super::{
    requested_model::{
        RequestedModelParseStatus, ResolvedRequestedModelName, enabled_patch_suffixes,
        parse_patch_suffix,
    },
    util::determine_upstream_protocol,
};

#[derive(Debug, Clone)]
pub struct ExecutionTarget {
    pub provider: Arc<CacheProvider>,
    pub model: Arc<CacheModel>,
    pub upstream_source: Arc<CacheUpstreamSource>,
    pub downstream_protocol: DownstreamProtocol,
    pub upstream_protocol: UpstreamProtocol,
    pub selection_reason: SourceSelectionReason,
    pub requested_patch_suffix: Option<String>,
}

impl ExecutionTarget {
    pub(in crate::proxy) fn is_openai_compatible_generation(&self) -> bool {
        self.upstream_protocol == UpstreamProtocol::Openai
            && matches!(
                self.upstream_source.profile_type,
                UpstreamProfileType::Openai
                    | UpstreamProfileType::VertexOpenai
                    | UpstreamProfileType::GeminiOpenai
            )
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub requested_name: String,
    pub base_requested_name: String,
    pub resolved_patch_suffix: Option<String>,
    pub requested_model_parse_status: RequestedModelParseStatus,
    pub target: ExecutionTarget,
    pub(crate) request_patch_variants: Arc<Vec<CacheRequestPatchVariant>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionPlanBuildError {
    InvalidModelFormat(String),
    TargetNotFound(String),
    ProviderConfiguration(String),
    CatalogUnavailable(String),
}

impl ExecutionPlanBuildError {
    fn message(&self) -> &str {
        match self {
            Self::InvalidModelFormat(message)
            | Self::TargetNotFound(message)
            | Self::ProviderConfiguration(message)
            | Self::CatalogUnavailable(message) => message,
        }
    }
}

impl fmt::Display for ExecutionPlanBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.message())
    }
}

impl std::error::Error for ExecutionPlanBuildError {}

impl ExecutionPlan {
    #[cfg(test)]
    pub fn target_summary_for_log(&self) -> String {
        let target = &self.target;
        format!(
            "base_name={}; provider={}/{}; model={}/{}; source={}({:?}); downstream_protocol={:?}; upstream_protocol={:?}; selection_reason={}; patch_suffix={:?}",
            self.base_requested_name,
            target.provider.id,
            target.provider.provider_key,
            target.model.id,
            target.model.model_name,
            target.upstream_source.id,
            target.upstream_source.profile_type,
            target.downstream_protocol,
            target.upstream_protocol,
            target.selection_reason.as_key(),
            self.resolved_patch_suffix,
        )
    }

    fn apply_resolved_requested_model_name(&mut self, resolved: ResolvedRequestedModelName) {
        self.requested_name = resolved.original_requested_name;
        self.base_requested_name = resolved.base_requested_name;
        self.resolved_patch_suffix = resolved.requested_suffix.clone();
        self.requested_model_parse_status = resolved.parse_status;
        self.target.requested_patch_suffix = resolved.requested_suffix;
    }
}

fn parse_provider_model(value: &str) -> (&str, &str) {
    let mut parts = value.splitn(2, '/');
    (parts.next().unwrap_or(""), parts.next().unwrap_or(""))
}

fn build_direct_execution_plan(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
    downstream_protocol: DownstreamProtocol,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    let (provider_key, model_name) = parse_provider_model(requested_name);
    if provider_key.is_empty() || model_name.is_empty() {
        return Err(ExecutionPlanBuildError::InvalidModelFormat(format!(
            "Invalid model format: '{}'. Expected 'provider/model'.",
            requested_name
        )));
    }
    let provider = catalog
        .providers
        .iter()
        .find(|provider| provider.provider_key == provider_key && provider.is_enabled)
        .cloned()
        .ok_or_else(|| {
            ExecutionPlanBuildError::TargetNotFound(format!(
                "Enabled provider '{}' was not found.",
                provider_key
            ))
        })?;
    let model = catalog
        .models
        .iter()
        .find(|model| {
            model.provider_id == provider.id && model.model_name == model_name && model.is_enabled
        })
        .cloned()
        .ok_or_else(|| {
            ExecutionPlanBuildError::TargetNotFound(format!(
                "Enabled model '{}' was not found.",
                requested_name
            ))
        })?;
    let selection =
        select_model_source(&provider, &model, downstream_protocol).map_err(|error| {
            ExecutionPlanBuildError::ProviderConfiguration(format!(
                "source selection failed: {}",
                error.failure.as_key()
            ))
        })?;
    let upstream_source = Arc::new(selection.source);
    Ok(ExecutionPlan {
        requested_name: requested_name.to_string(),
        base_requested_name: requested_name.to_string(),
        resolved_patch_suffix: None,
        requested_model_parse_status: RequestedModelParseStatus::Exact,
        target: ExecutionTarget {
            provider: Arc::new(provider),
            model: Arc::new(model),
            upstream_protocol: determine_upstream_protocol(&upstream_source),
            upstream_source,
            downstream_protocol,
            selection_reason: selection.reason,
            requested_patch_suffix: None,
        },
        request_patch_variants: Arc::new(catalog.request_patch_variants.clone()),
    })
}

fn build_execution_plan_from_catalog(
    catalog: &CacheModelsCatalog,
    requested_name: &str,
    downstream_protocol: DownstreamProtocol,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    match build_direct_execution_plan(catalog, requested_name, downstream_protocol) {
        Ok(plan) => Ok(plan),
        Err(exact_error) => {
            let suffixes = enabled_patch_suffixes(catalog);
            let Some(resolved_name) = parse_patch_suffix(requested_name, &suffixes) else {
                return Err(exact_error);
            };
            let mut plan = match build_direct_execution_plan(
                catalog,
                &resolved_name.base_requested_name,
                downstream_protocol,
            ) {
                Ok(plan) => plan,
                Err(_) => return Err(exact_error),
            };
            plan.apply_resolved_requested_model_name(resolved_name);
            Ok(plan)
        }
    }
}

pub(crate) async fn build_execution_plan(
    app_state: &Arc<AppState>,
    requested_name: &str,
    downstream_protocol: DownstreamProtocol,
) -> Result<ExecutionPlan, ExecutionPlanBuildError> {
    let catalog = app_state
        .catalog
        .get_models_catalog()
        .await
        .map_err(|err| {
            error!(
                "Error loading models catalog while resolving '{}': {:?}",
                requested_name, err
            );
            ExecutionPlanBuildError::CatalogUnavailable(format!(
                "Internal server error while loading model catalog for '{}'.",
                requested_name
            ))
        })?;
    build_execution_plan_from_catalog(catalog.as_ref(), requested_name, downstream_protocol)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
    use crate::service::cache::types::{CacheRequestPatchRule, CacheRequestPatchVariant};

    fn catalog() -> CacheModelsCatalog {
        CacheModelsCatalog {
            providers: vec![CacheProvider {
                id: 1,
                provider_key: "openai".to_string(),
                name: "OpenAI".to_string(),
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                is_enabled: true,
                upstream_sources: vec![CacheUpstreamSource {
                    id: 2,
                    profile_type: UpstreamProfileType::Openai,
                    endpoint: "https://example.test".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                }],
            }],
            models: vec![CacheModel {
                id: 3,
                provider_id: 1,
                model_name: "gpt-4o".to_string(),
                real_model_name: None,
                cost_catalog_id: None,
                source_selection_mode: "INHERIT_ALL".to_string(),
                source_bindings: vec![],
                is_enabled: true,
            }],
            request_patch_variants: Vec::new(),
        }
    }

    fn suffix_variant(
        id: i64,
        model_id: Option<i64>,
        suffix: &str,
        enabled: bool,
    ) -> CacheRequestPatchVariant {
        CacheRequestPatchVariant {
            id,
            source_id: 2,
            model_id,
            suffix: Some(suffix.to_string()),
            enabled,
            expose_in_models: true,
            rules: vec![CacheRequestPatchRule {
                id: id + 100,
                variant_id: id,
                placement: crate::schema::enum_def::RequestPatchPlacement::Body,
                target: "/options/temperature".to_string(),
                operation: crate::schema::enum_def::RequestPatchOperation::Set,
                value_json: Some("0.2".to_string()),
                description: None,
                created_at: id,
                updated_at: id,
            }],
        }
    }

    #[test]
    fn exact_model_name_wins_before_suffix_fallback() {
        let mut catalog = catalog();
        catalog.models.push(CacheModel {
            id: 4,
            provider_id: 1,
            model_name: "gpt-4o-fast".to_string(),
            real_model_name: None,
            cost_catalog_id: None,
            source_selection_mode: "INHERIT_ALL".to_string(),
            source_bindings: vec![],
            is_enabled: true,
        });
        assert_eq!(
            build_execution_plan_from_catalog(
                &catalog,
                "openai/gpt-4o-fast",
                DownstreamProtocol::Openai,
            )
            .unwrap()
            .target
            .model
            .id,
            4
        );
    }

    #[test]
    fn suffix_resolution_uses_longest_enabled_token_after_exact_lookup() {
        let mut catalog = catalog();
        catalog.request_patch_variants = vec![
            suffix_variant(10, None, "fast", true),
            suffix_variant(11, None, "fast-long", true),
        ];
        let plan = build_execution_plan_from_catalog(
            &catalog,
            "openai/gpt-4o-fast-long",
            DownstreamProtocol::Openai,
        )
        .expect("known longest suffix should resolve");
        assert_eq!(plan.target.model.id, 3);
        assert_eq!(plan.resolved_patch_suffix.as_deref(), Some("fast-long"));
        assert_eq!(
            plan.requested_model_parse_status,
            RequestedModelParseStatus::PatchSuffix
        );
    }

    #[test]
    fn disabled_and_unknown_suffixes_fail_while_masked_suffix_is_deferred_to_execution() {
        let mut disabled = catalog();
        disabled.request_patch_variants = vec![suffix_variant(20, None, "disabled", false)];
        assert!(matches!(
            build_execution_plan_from_catalog(
                &disabled,
                "openai/gpt-4o-disabled",
                DownstreamProtocol::Openai,
            ),
            Err(ExecutionPlanBuildError::TargetNotFound(_))
        ));
        assert!(matches!(
            build_execution_plan_from_catalog(
                &catalog(),
                "openai/gpt-4o-unknown",
                DownstreamProtocol::Openai,
            ),
            Err(ExecutionPlanBuildError::TargetNotFound(_))
        ));

        let mut masked = catalog();
        masked.request_patch_variants = vec![
            suffix_variant(21, None, "fast", true),
            suffix_variant(22, Some(3), "fast", false),
        ];
        let masked_plan = build_execution_plan_from_catalog(
            &masked,
            "openai/gpt-4o-fast",
            DownstreamProtocol::Openai,
        )
        .expect("masked suffix validation must be deferred until after API key ACL");
        assert_eq!(masked_plan.resolved_patch_suffix.as_deref(), Some("fast"));
    }

    #[test]
    fn direct_provider_keys_with_hyphens_remain_exactly_addressable() {
        let mut catalog = catalog();
        catalog.providers[0].provider_key = "provider-with-hyphen".to_string();
        let plan = build_execution_plan_from_catalog(
            &catalog,
            "provider-with-hyphen/gpt-4o",
            DownstreamProtocol::Openai,
        )
        .expect("hyphenated provider key should resolve");
        assert_eq!(plan.target.provider.provider_key, "provider-with-hyphen");
        assert_eq!(
            plan.requested_model_parse_status,
            RequestedModelParseStatus::Exact
        );
    }
}
