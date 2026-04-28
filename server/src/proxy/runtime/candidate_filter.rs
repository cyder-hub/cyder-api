use serde_json::Value;

use crate::{
    database::reasoning_config::ReasoningPreset,
    proxy::{
        ProxyError,
        reasoning_suffix::{ReasoningOperationKind, reasoning_preset_runtime_metadata},
        runtime::{
            attempt::RequestAttemptDraft,
            route_resolver::{ExecutionCandidate, ExecutionPlan},
        },
    },
    schema::enum_def::LlmApiType,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::proxy) struct ExecutionRequirement {
    pub requires_streaming: bool,
    pub requires_tools: bool,
    pub requires_reasoning: bool,
    pub requires_image_input: bool,
    pub requires_embeddings: bool,
    pub requires_rerank: bool,
}

impl ExecutionRequirement {
    fn required_capability_names(&self) -> Vec<&'static str> {
        [
            (self.requires_streaming, "streaming"),
            (self.requires_tools, "tools"),
            (self.requires_reasoning, "reasoning"),
            (self.requires_image_input, "image_input"),
            (self.requires_embeddings, "embeddings"),
            (self.requires_rerank, "rerank"),
        ]
        .into_iter()
        .filter_map(|(required, name)| required.then_some(name))
        .collect()
    }
}

#[derive(Debug, Clone)]
pub(in crate::proxy) struct PrefilteredExecutionPlan {
    pub execution_plan: ExecutionPlan,
    pub skipped_attempts: Vec<RequestAttemptDraft>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::proxy) enum RequestedOperationKind {
    Generation,
    Utility,
}

impl RequestedOperationKind {
    fn label(self) -> &'static str {
        match self {
            Self::Generation => "generation",
            Self::Utility => "utility",
        }
    }

    fn is_allowed_by(self, allowed: &[ReasoningOperationKind]) -> bool {
        match self {
            Self::Generation => allowed.contains(&ReasoningOperationKind::Generation),
            Self::Utility => false,
        }
    }
}

pub(in crate::proxy) fn ensure_reasoning_preset_allows_operation(
    execution_plan: &ExecutionPlan,
    operation_kind: RequestedOperationKind,
    operation_name: &str,
) -> Result<(), ProxyError> {
    let Some(preset) = execution_plan.resolved_reasoning_preset else {
        return Ok(());
    };

    let metadata = reasoning_preset_runtime_metadata(preset);
    if operation_kind.is_allowed_by(&metadata.allowed_operation_kinds) {
        return Ok(());
    }

    let suffix = execution_plan
        .resolved_reasoning_suffix
        .as_deref()
        .unwrap_or(metadata.suffix.as_str());
    Err(ProxyError::BadRequest(format!(
        "Reasoning suffix '{}' (preset '{}') on model '{}' is only supported for generation requests; '{}' is a {} operation.",
        suffix,
        preset,
        execution_plan.requested_name,
        operation_name,
        operation_kind.label()
    )))
}

pub(in crate::proxy) fn derive_generation_requirement(
    data: &Value,
    _user_api_type: LlmApiType,
    is_stream: bool,
    resolved_reasoning_preset: Option<ReasoningPreset>,
) -> ExecutionRequirement {
    let preset_requires_reasoning = resolved_reasoning_preset
        .map(reasoning_preset_runtime_metadata)
        .map(|metadata| metadata.requires_reasoning)
        .unwrap_or(false);

    ExecutionRequirement {
        requires_streaming: is_stream,
        requires_tools: request_uses_tools(data),
        requires_reasoning: request_uses_reasoning(data) || preset_requires_reasoning,
        requires_image_input: request_uses_image_input(data),
        requires_embeddings: false,
        requires_rerank: false,
    }
}

pub(in crate::proxy) fn derive_utility_requirement(
    operation_name: &str,
    data: &Value,
) -> ExecutionRequirement {
    let normalized_name = operation_name.to_ascii_lowercase();
    ExecutionRequirement {
        requires_streaming: false,
        requires_tools: false,
        requires_reasoning: false,
        requires_image_input: request_uses_image_input(data),
        requires_embeddings: normalized_name == "embeddings",
        requires_rerank: normalized_name == "rerank",
    }
}

pub(in crate::proxy) fn prefilter_execution_plan(
    execution_plan: ExecutionPlan,
    requirement: &ExecutionRequirement,
) -> PrefilteredExecutionPlan {
    let mut compatible_candidates = Vec::with_capacity(execution_plan.candidates.len());
    let mut skipped_attempts = Vec::new();

    for candidate in execution_plan.candidates {
        let missing_capabilities = missing_capabilities(&candidate, requirement);
        if missing_capabilities.is_empty() {
            compatible_candidates.push(candidate);
        } else {
            skipped_attempts.push(RequestAttemptDraft::skipped_for_capability_mismatch(
                &candidate,
                &missing_capabilities,
            ));
        }
    }

    PrefilteredExecutionPlan {
        execution_plan: ExecutionPlan {
            requested_name: execution_plan.requested_name,
            base_requested_name: execution_plan.base_requested_name,
            resolved_reasoning_suffix: execution_plan.resolved_reasoning_suffix,
            resolved_reasoning_preset: execution_plan.resolved_reasoning_preset,
            requested_model_parse_status: execution_plan.requested_model_parse_status,
            resolved_scope: execution_plan.resolved_scope,
            resolved_route_id: execution_plan.resolved_route_id,
            resolved_route_name: execution_plan.resolved_route_name,
            candidates: compatible_candidates,
        },
        skipped_attempts,
    }
}

pub(in crate::proxy) fn no_candidate_error_message(requirement: &ExecutionRequirement) -> String {
    let required = requirement.required_capability_names();
    if required.is_empty() {
        "No execution candidate is available for this request.".to_string()
    } else {
        format!(
            "No execution candidate supports the required capabilities: {}",
            required.join(", ")
        )
    }
}

fn missing_capabilities(
    candidate: &ExecutionCandidate,
    requirement: &ExecutionRequirement,
) -> Vec<&'static str> {
    let model = candidate.model.as_ref();
    [
        (
            requirement.requires_streaming && !model.supports_streaming,
            "streaming",
        ),
        (requirement.requires_tools && !model.supports_tools, "tools"),
        (
            requirement.requires_reasoning && !model.supports_reasoning,
            "reasoning",
        ),
        (
            requirement.requires_image_input && !model.supports_image_input,
            "image_input",
        ),
        (
            requirement.requires_embeddings && !model.supports_embeddings,
            "embeddings",
        ),
        (
            requirement.requires_rerank && !model.supports_rerank,
            "rerank",
        ),
    ]
    .into_iter()
    .filter_map(|(missing, name)| missing.then_some(name))
    .collect()
}

fn request_uses_tools(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let key = key.as_str();
            if matches!(key, "tools" | "functions") {
                return match value {
                    Value::Array(items) => !items.is_empty(),
                    Value::Object(items) => !items.is_empty(),
                    Value::Null => false,
                    _ => true,
                };
            }
            if key == "tool_choice" || key == "function_call" {
                return !matches!(value, Value::Null)
                    && value.as_str().map_or(true, |choice| choice != "none");
            }
            request_uses_tools(value)
        }),
        Value::Array(items) => items.iter().any(request_uses_tools),
        _ => false,
    }
}

fn request_uses_reasoning(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            if matches!(
                key.as_str(),
                "reasoning"
                    | "reasoning_effort"
                    | "thinking"
                    | "thinking_config"
                    | "thinkingConfig"
                    | "enable_thinking"
                    | "enableThinking"
                    | "include_reasoning"
                    | "includeReasoning"
            ) {
                return !matches!(value, Value::Null | Value::Bool(false));
            }
            request_uses_reasoning(value)
        }),
        Value::Array(items) => items.iter().any(request_uses_reasoning),
        _ => false,
    }
}

fn request_uses_image_input(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("type")
                .and_then(Value::as_str)
                .map_or(false, |kind| {
                    matches!(kind, "image" | "image_url" | "input_image")
                })
            {
                return true;
            }

            if map.contains_key("image_url") {
                return true;
            }

            if map.iter().any(|(key, value)| {
                matches!(key.as_str(), "mime_type" | "mimeType")
                    && value
                        .as_str()
                        .map_or(false, |mime_type| mime_type.starts_with("image/"))
            }) {
                return true;
            }

            map.values().any(request_uses_image_input)
        }
        Value::Array(items) => items.iter().any(request_uses_image_input),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::json;

    use super::*;
    use crate::{
        database::reasoning_config::ReasoningPreset,
        proxy::runtime::{
            attempt::CAPABILITY_MISMATCH_SKIPPED_ERROR,
            route_resolver::{ExecutionCandidate, ExecutionPlan, ResolvedNameScope},
        },
        schema::enum_def::{ProviderApiKeyMode, ProviderType, SchedulerAction},
        service::cache::types::{CacheModel, CacheProvider},
    };

    fn provider(id: i64) -> Arc<CacheProvider> {
        Arc::new(CacheProvider {
            id,
            provider_key: format!("provider-{id}"),
            name: format!("Provider {id}"),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        })
    }

    fn model(id: i64, supports_tools: bool, supports_image_input: bool) -> Arc<CacheModel> {
        model_with_reasoning(id, supports_tools, true, supports_image_input)
    }

    fn model_with_reasoning(
        id: i64,
        supports_tools: bool,
        supports_reasoning: bool,
        supports_image_input: bool,
    ) -> Arc<CacheModel> {
        Arc::new(CacheModel {
            id,
            provider_id: id,
            model_name: format!("model-{id}"),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools,
            supports_reasoning,
            supports_image_input,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        })
    }

    fn candidate(
        position: usize,
        supports_tools: bool,
        supports_image_input: bool,
    ) -> ExecutionCandidate {
        ExecutionCandidate {
            candidate_position: position,
            route_id: Some(1),
            route_name: Some("route".to_string()),
            route_candidate_priority: Some(position as i32),
            provider: provider(position as i64),
            model: model(position as i64, supports_tools, supports_image_input),
            llm_api_type: LlmApiType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            reasoning_config_id: None,
            reasoning_config_scope: None,
            reasoning_config_source: None,
            reasoning_config_preset_id: None,
            reasoning_family: None,
            reasoning_preset: None,
            reasoning_suffix: None,
        }
    }

    fn plan() -> ExecutionPlan {
        ExecutionPlan {
            requested_name: "route".to_string(),
            base_requested_name: "route".to_string(),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            requested_model_parse_status:
                crate::proxy::requested_model::RequestedModelParseStatus::Exact,
            resolved_scope: ResolvedNameScope::GlobalRoute,
            resolved_route_id: Some(1),
            resolved_route_name: Some("route".to_string()),
            candidates: vec![candidate(1, false, true), candidate(2, true, true)],
        }
    }

    #[test]
    fn derive_generation_requirement_detects_tools_reasoning_images_and_streaming() {
        let requirement = derive_generation_requirement(
            &json!({
                "stream": true,
                "tools": [{"type": "function"}],
                "reasoning_effort": "medium",
                "messages": [{
                    "role": "user",
                    "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}}]
                }]
            }),
            LlmApiType::Openai,
            true,
            None,
        );

        assert!(requirement.requires_streaming);
        assert!(requirement.requires_tools);
        assert!(requirement.requires_reasoning);
        assert!(requirement.requires_image_input);
        assert!(!requirement.requires_embeddings);
        assert!(!requirement.requires_rerank);
    }

    #[test]
    fn derive_generation_requirement_uses_resolved_reasoning_preset_metadata() {
        let high = derive_generation_requirement(
            &json!({ "messages": [{ "role": "user", "content": "hello" }] }),
            LlmApiType::Openai,
            false,
            Some(ReasoningPreset::High),
        );
        assert!(high.requires_reasoning);

        let disabled = derive_generation_requirement(
            &json!({ "messages": [{ "role": "user", "content": "hello" }] }),
            LlmApiType::Openai,
            false,
            Some(ReasoningPreset::Disabled),
        );
        assert!(!disabled.requires_reasoning);
    }

    #[test]
    fn derive_generation_requirement_detects_siliconflow_enable_thinking() {
        let enabled = derive_generation_requirement(
            &json!({
                "messages": [{ "role": "user", "content": "hello" }],
                "enable_thinking": true,
            }),
            LlmApiType::Openai,
            false,
            None,
        );
        assert!(enabled.requires_reasoning);

        let disabled = derive_generation_requirement(
            &json!({
                "messages": [{ "role": "user", "content": "hello" }],
                "enable_thinking": false,
            }),
            LlmApiType::Openai,
            false,
            None,
        );
        assert!(!disabled.requires_reasoning);
    }

    #[test]
    fn reasoning_suffix_generation_requirement_skips_non_reasoning_candidates() {
        let mut execution_plan = plan();
        execution_plan.resolved_reasoning_suffix = Some("high".to_string());
        execution_plan.resolved_reasoning_preset = Some(ReasoningPreset::High);
        execution_plan.candidates[0].model = model_with_reasoning(1, true, false, true);
        execution_plan.candidates[1].model = model_with_reasoning(2, true, true, true);

        let requirement = derive_generation_requirement(
            &json!({ "messages": [{ "role": "user", "content": "hello" }] }),
            LlmApiType::Openai,
            false,
            execution_plan.resolved_reasoning_preset,
        );
        let prefiltered = prefilter_execution_plan(execution_plan, &requirement);

        assert_eq!(prefiltered.execution_plan.candidate_model_ids(), vec![2]);
        assert_eq!(prefiltered.skipped_attempts.len(), 1);
        assert_eq!(
            prefiltered.skipped_attempts[0].error_code.as_deref(),
            Some(CAPABILITY_MISMATCH_SKIPPED_ERROR)
        );
        assert_eq!(
            prefiltered.skipped_attempts[0].scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
    }

    #[test]
    fn derive_utility_requirement_detects_embeddings_and_rerank() {
        let embeddings = derive_utility_requirement("embeddings", &json!({ "input": "hello" }));
        let rerank = derive_utility_requirement("rerank", &json!({ "query": "hello" }));

        assert!(embeddings.requires_embeddings);
        assert!(!embeddings.requires_rerank);
        assert!(rerank.requires_rerank);
    }

    #[test]
    fn reasoning_suffix_rejects_utility_operation_kind() {
        let mut execution_plan = plan();
        execution_plan.requested_name = "route-high".to_string();
        execution_plan.resolved_reasoning_suffix = Some("high".to_string());
        execution_plan.resolved_reasoning_preset = Some(ReasoningPreset::High);

        let error = ensure_reasoning_preset_allows_operation(
            &execution_plan,
            RequestedOperationKind::Utility,
            "embeddings",
        )
        .unwrap_err();

        match error {
            ProxyError::BadRequest(message) => {
                assert!(message.contains("Reasoning suffix 'high'"));
                assert!(message.contains("generation requests"));
                assert!(message.contains("embeddings"));
            }
            other => panic!("expected BadRequest, got {other:?}"),
        }
    }

    #[test]
    fn prefilter_execution_plan_skips_incompatible_candidates_without_reordering() {
        let requirement = ExecutionRequirement {
            requires_tools: true,
            ..ExecutionRequirement::default()
        };

        let prefiltered = prefilter_execution_plan(plan(), &requirement);

        assert_eq!(prefiltered.execution_plan.candidate_model_ids(), vec![2]);
        assert_eq!(prefiltered.skipped_attempts.len(), 1);
        assert_eq!(prefiltered.skipped_attempts[0].candidate_position, 1);
        assert_eq!(
            prefiltered.skipped_attempts[0].error_code.as_deref(),
            Some(CAPABILITY_MISMATCH_SKIPPED_ERROR)
        );
        assert_eq!(
            prefiltered.skipped_attempts[0].scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
    }
}
