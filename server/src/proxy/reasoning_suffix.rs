use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    database::reasoning_profile::{ReasoningPatchFamily, ReasoningPreset},
    schema::enum_def::{LlmApiType, RequestPatchOperation, RequestPatchPlacement},
    service::cache::types::CacheModel,
};

const OPENAI_DEFAULT_REASONING_EFFORT: &str = "medium";
const ANTHROPIC_THINKING_BUDGET_LOW: i64 = 1024;
const ANTHROPIC_THINKING_BUDGET_MEDIUM: i64 = 4096;
const ANTHROPIC_THINKING_BUDGET_HIGH: i64 = 10_000;
const GEMINI_THINKING_BUDGET_LOW: i64 = 1024;
const GEMINI_THINKING_BUDGET_MEDIUM: i64 = 4096;
const GEMINI_THINKING_BUDGET_HIGH: i64 = 8192;
const GEMINI_THINKING_BUDGET_AUTO: i64 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReasoningOperationKind {
    Generation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReasoningPresetRuntimeMetadata {
    pub preset: ReasoningPreset,
    pub preset_key: String,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<ReasoningOperationKind>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReasoningPatchContext<'a> {
    pub target_api_type: LlmApiType,
    pub model_id: Option<i64>,
    pub model_name: Option<&'a str>,
    pub supports_reasoning: bool,
}

impl<'a> ReasoningPatchContext<'a> {
    pub(crate) fn for_model(target_api_type: LlmApiType, model: &'a CacheModel) -> Self {
        Self {
            target_api_type,
            model_id: Some(model.id),
            model_name: Some(&model.model_name),
            supports_reasoning: model.supports_reasoning,
        }
    }

    #[cfg(test)]
    fn test(target_api_type: LlmApiType) -> Self {
        Self {
            target_api_type,
            model_id: Some(1),
            model_name: Some("test-model"),
            supports_reasoning: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GeneratedReasoningPatch {
    pub family: ReasoningPatchFamily,
    pub preset: ReasoningPreset,
    pub suffix: String,
    pub placement: RequestPatchPlacement,
    pub target: String,
    pub operation: RequestPatchOperation,
    pub value_json: Option<String>,
    pub description: Option<String>,
}

impl GeneratedReasoningPatch {
    fn new(
        family: ReasoningPatchFamily,
        preset: ReasoningPreset,
        placement: RequestPatchPlacement,
        target: impl Into<String>,
        operation: RequestPatchOperation,
        value: Option<Value>,
        description: impl Into<Option<String>>,
    ) -> Result<Self, String> {
        let target = target.into();
        if placement == RequestPatchPlacement::Body && target == "/model" {
            return Err("generated reasoning patch cannot modify /model".to_string());
        }

        let value_json = value
            .map(|value| serde_json::to_string(&value))
            .transpose()
            .map_err(|err| format!("failed to serialize generated reasoning patch value: {err}"))?;

        Ok(Self {
            family,
            preset,
            suffix: preset.canonical_suffix().to_string(),
            placement,
            target,
            operation,
            value_json,
            description: description.into(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReasoningPresetUnsupported {
    pub family: ReasoningPatchFamily,
    pub preset: ReasoningPreset,
    pub target_api_type: LlmApiType,
    pub model_id: Option<i64>,
    pub model_name: Option<String>,
    pub reason: String,
}

impl ReasoningPresetUnsupported {
    fn new(
        family: ReasoningPatchFamily,
        preset: ReasoningPreset,
        context: ReasoningPatchContext<'_>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            family,
            preset,
            target_api_type: context.target_api_type,
            model_id: context.model_id,
            model_name: context.model_name.map(str::to_string),
            reason: reason.into(),
        }
    }
}

impl std::fmt::Display for ReasoningPresetUnsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.model_name {
            Some(model_name) => write!(
                f,
                "reasoning preset '{}' is unsupported for family '{}' on {:?} model '{}': {}",
                self.preset, self.family, self.target_api_type, model_name, self.reason
            ),
            None => write!(
                f,
                "reasoning preset '{}' is unsupported for family '{}' on {:?}: {}",
                self.preset, self.family, self.target_api_type, self.reason
            ),
        }
    }
}

impl std::error::Error for ReasoningPresetUnsupported {}

pub(crate) fn reasoning_preset_runtime_metadata(
    preset: ReasoningPreset,
) -> ReasoningPresetRuntimeMetadata {
    ReasoningPresetRuntimeMetadata {
        preset,
        preset_key: preset.as_key().to_string(),
        suffix: preset.canonical_suffix().to_string(),
        requires_reasoning: preset.requires_reasoning(),
        allowed_operation_kinds: vec![ReasoningOperationKind::Generation],
    }
}

pub(crate) fn generate_reasoning_patches(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    if preset.requires_reasoning() && !context.supports_reasoning {
        return Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "model capability does not include reasoning",
        ));
    }

    match family {
        ReasoningPatchFamily::OpenAiChatReasoningEffort => {
            generate_openai_chat_reasoning_effort_patch(family, preset, context)
        }
        ReasoningPatchFamily::OpenAiResponsesReasoning => {
            generate_openai_responses_reasoning_patch(family, preset, context)
        }
        ReasoningPatchFamily::AnthropicThinkingBudget => {
            generate_anthropic_thinking_budget_patch(family, preset, context)
        }
        ReasoningPatchFamily::Gemini25ThinkingBudget => {
            generate_gemini25_thinking_budget_patch(family, preset, context)
        }
        ReasoningPatchFamily::Gemini3ThinkingLevel => {
            generate_gemini3_thinking_level_patch(family, preset, context)
        }
    }
}

fn ensure_protocol(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
    allowed: &[LlmApiType],
) -> Result<(), ReasoningPresetUnsupported> {
    if allowed.contains(&context.target_api_type) {
        Ok(())
    } else {
        Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            format!(
                "family targets {:?}, got {:?}",
                allowed, context.target_api_type
            ),
        ))
    }
}

fn body_set_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    target: &'static str,
    value: Value,
    description: &'static str,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    let patch = GeneratedReasoningPatch::new(
        family,
        preset,
        RequestPatchPlacement::Body,
        target,
        RequestPatchOperation::Set,
        Some(value),
        Some(description.to_string()),
    )
    .map_err(|reason| ReasoningPresetUnsupported::new(family, preset, context, reason))?;
    Ok(vec![patch])
}

fn openai_reasoning_effort(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<&'static str, ReasoningPresetUnsupported> {
    match preset {
        ReasoningPreset::Disabled => Ok("none"),
        ReasoningPreset::Enabled => Ok(OPENAI_DEFAULT_REASONING_EFFORT),
        ReasoningPreset::Low => Ok("low"),
        ReasoningPreset::Medium => Ok("medium"),
        ReasoningPreset::High => Ok("high"),
        ReasoningPreset::XHigh => Ok("xhigh"),
        ReasoningPreset::Auto => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "OpenAI reasoning effort does not define a provider-managed auto value",
        )),
    }
}

fn generate_openai_chat_reasoning_effort_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(
        family,
        preset,
        context,
        &[LlmApiType::Openai, LlmApiType::GeminiOpenai],
    )?;
    let effort = openai_reasoning_effort(family, preset, context)?;
    body_set_patch(
        family,
        preset,
        "/reasoning_effort",
        json!(effort),
        "generated OpenAI Chat reasoning effort patch",
        context,
    )
}

fn generate_openai_responses_reasoning_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(family, preset, context, &[LlmApiType::Responses])?;
    let effort = openai_reasoning_effort(family, preset, context)?;
    body_set_patch(
        family,
        preset,
        "/reasoning/effort",
        json!(effort),
        "generated OpenAI Responses reasoning effort patch",
        context,
    )
}

fn anthropic_thinking_value(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Value, ReasoningPresetUnsupported> {
    match preset {
        ReasoningPreset::Disabled => Ok(json!({ "type": "disabled" })),
        ReasoningPreset::Enabled | ReasoningPreset::Low => Ok(json!({
            "type": "enabled",
            "budget_tokens": ANTHROPIC_THINKING_BUDGET_LOW,
        })),
        ReasoningPreset::Medium => Ok(json!({
            "type": "enabled",
            "budget_tokens": ANTHROPIC_THINKING_BUDGET_MEDIUM,
        })),
        ReasoningPreset::High => Ok(json!({
            "type": "enabled",
            "budget_tokens": ANTHROPIC_THINKING_BUDGET_HIGH,
        })),
        ReasoningPreset::XHigh => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Anthropic budget family does not define an xhigh budget",
        )),
        ReasoningPreset::Auto => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Anthropic budget family does not define provider-managed auto thinking",
        )),
    }
}

fn generate_anthropic_thinking_budget_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(family, preset, context, &[LlmApiType::Anthropic])?;
    let value = anthropic_thinking_value(family, preset, context)?;
    body_set_patch(
        family,
        preset,
        "/thinking",
        value,
        "generated Anthropic thinking budget patch",
        context,
    )
}

fn gemini25_thinking_budget(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<i64, ReasoningPresetUnsupported> {
    match preset {
        ReasoningPreset::Disabled => Ok(0),
        ReasoningPreset::Enabled | ReasoningPreset::Low => Ok(GEMINI_THINKING_BUDGET_LOW),
        ReasoningPreset::Medium => Ok(GEMINI_THINKING_BUDGET_MEDIUM),
        ReasoningPreset::High => Ok(GEMINI_THINKING_BUDGET_HIGH),
        ReasoningPreset::Auto => Ok(GEMINI_THINKING_BUDGET_AUTO),
        ReasoningPreset::XHigh => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Gemini 2.5 budget family does not define an xhigh budget",
        )),
    }
}

fn generate_gemini25_thinking_budget_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(family, preset, context, &[LlmApiType::Gemini])?;
    let budget = gemini25_thinking_budget(family, preset, context)?;
    body_set_patch(
        family,
        preset,
        "/generationConfig/thinkingConfig/thinkingBudget",
        json!(budget),
        "generated Gemini 2.5 thinking budget patch",
        context,
    )
}

fn gemini3_thinking_level(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<&'static str, ReasoningPresetUnsupported> {
    match preset {
        ReasoningPreset::Enabled | ReasoningPreset::High => Ok("high"),
        ReasoningPreset::Low => Ok("low"),
        ReasoningPreset::Disabled => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Gemini 3 thinking level family does not support disabling thinking",
        )),
        ReasoningPreset::Medium => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Gemini 3 thinking level family only exposes low/high in the gateway preset surface",
        )),
        ReasoningPreset::XHigh => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Gemini 3 thinking level family does not define xhigh",
        )),
        ReasoningPreset::Auto => Err(ReasoningPresetUnsupported::new(
            family,
            preset,
            context,
            "Gemini 3 thinking level family does not define an explicit auto value",
        )),
    }
}

fn generate_gemini3_thinking_level_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(family, preset, context, &[LlmApiType::Gemini])?;
    let level = gemini3_thinking_level(family, preset, context)?;
    body_set_patch(
        family,
        preset,
        "/generationConfig/thinkingConfig/thinkingLevel",
        json!(level),
        "generated Gemini 3 thinking level patch",
        context,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::cache::types::CacheModel;

    fn only_patch(
        family: ReasoningPatchFamily,
        preset: ReasoningPreset,
        api_type: LlmApiType,
    ) -> GeneratedReasoningPatch {
        let patches =
            generate_reasoning_patches(family, preset, ReasoningPatchContext::test(api_type))
                .expect("patch should generate");
        assert_eq!(patches.len(), 1);
        patches.into_iter().next().unwrap()
    }

    fn patch_value(patch: &GeneratedReasoningPatch) -> Value {
        serde_json::from_str(
            patch
                .value_json
                .as_deref()
                .expect("patch should have value"),
        )
        .expect("patch value should decode")
    }

    #[test]
    fn preset_runtime_metadata_is_derived_from_builtin_preset() {
        let metadata = reasoning_preset_runtime_metadata(ReasoningPreset::Disabled);
        assert_eq!(metadata.preset_key, "disabled");
        assert_eq!(metadata.suffix, "no-think");
        assert!(!metadata.requires_reasoning);
        assert_eq!(
            metadata.allowed_operation_kinds,
            vec![ReasoningOperationKind::Generation]
        );
    }

    #[test]
    fn openai_chat_high_generates_reasoning_effort_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            ReasoningPreset::High,
            LlmApiType::Openai,
        );
        assert_eq!(patch.target, "/reasoning_effort");
        assert_eq!(patch.operation, RequestPatchOperation::Set);
        assert_eq!(patch_value(&patch), json!("high"));
    }

    #[test]
    fn openai_responses_high_generates_reasoning_effort_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::OpenAiResponsesReasoning,
            ReasoningPreset::High,
            LlmApiType::Responses,
        );
        assert_eq!(patch.target, "/reasoning/effort");
        assert_eq!(patch_value(&patch), json!("high"));
    }

    #[test]
    fn enabled_uses_explicit_default_for_openai_effort_family() {
        let patch = only_patch(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            ReasoningPreset::Enabled,
            LlmApiType::Openai,
        );
        assert_eq!(patch.target, "/reasoning_effort");
        assert_eq!(patch_value(&patch), json!("medium"));
    }

    #[test]
    fn anthropic_high_generates_builtin_budget_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::AnthropicThinkingBudget,
            ReasoningPreset::High,
            LlmApiType::Anthropic,
        );
        assert_eq!(patch.target, "/thinking");
        assert_eq!(
            patch_value(&patch),
            json!({
                "type": "enabled",
                "budget_tokens": ANTHROPIC_THINKING_BUDGET_HIGH,
            })
        );
    }

    #[test]
    fn anthropic_enabled_generates_minimum_builtin_budget_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::AnthropicThinkingBudget,
            ReasoningPreset::Enabled,
            LlmApiType::Anthropic,
        );
        assert_eq!(
            patch_value(&patch),
            json!({
                "type": "enabled",
                "budget_tokens": ANTHROPIC_THINKING_BUDGET_LOW,
            })
        );
    }

    #[test]
    fn gemini25_high_generates_builtin_budget_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::Gemini25ThinkingBudget,
            ReasoningPreset::High,
            LlmApiType::Gemini,
        );
        assert_eq!(
            patch.target,
            "/generationConfig/thinkingConfig/thinkingBudget"
        );
        assert_eq!(patch_value(&patch), json!(GEMINI_THINKING_BUDGET_HIGH));
    }

    #[test]
    fn gemini25_auto_generates_dynamic_budget_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::Gemini25ThinkingBudget,
            ReasoningPreset::Auto,
            LlmApiType::Gemini,
        );
        assert_eq!(patch_value(&patch), json!(GEMINI_THINKING_BUDGET_AUTO));
    }

    #[test]
    fn gemini25_xhigh_returns_unsupported() {
        let err = generate_reasoning_patches(
            ReasoningPatchFamily::Gemini25ThinkingBudget,
            ReasoningPreset::XHigh,
            ReasoningPatchContext::test(LlmApiType::Gemini),
        )
        .expect_err("Gemini 2.5 budget family does not define xhigh");

        assert!(err.reason.contains("xhigh"));
    }

    #[test]
    fn gemini3_low_generates_thinking_level_patch() {
        let patch = only_patch(
            ReasoningPatchFamily::Gemini3ThinkingLevel,
            ReasoningPreset::Low,
            LlmApiType::Gemini,
        );
        assert_eq!(
            patch.target,
            "/generationConfig/thinkingConfig/thinkingLevel"
        );
        assert_eq!(patch_value(&patch), json!("low"));
    }

    #[test]
    fn disabled_patch_is_generated_only_for_families_with_disable_semantics() {
        let openai = only_patch(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            ReasoningPreset::Disabled,
            LlmApiType::Openai,
        );
        assert_eq!(patch_value(&openai), json!("none"));

        let anthropic = only_patch(
            ReasoningPatchFamily::AnthropicThinkingBudget,
            ReasoningPreset::Disabled,
            LlmApiType::Anthropic,
        );
        assert_eq!(patch_value(&anthropic), json!({ "type": "disabled" }));

        let gemini = only_patch(
            ReasoningPatchFamily::Gemini25ThinkingBudget,
            ReasoningPreset::Disabled,
            LlmApiType::Gemini,
        );
        assert_eq!(patch_value(&gemini), json!(0));

        let err = generate_reasoning_patches(
            ReasoningPatchFamily::Gemini3ThinkingLevel,
            ReasoningPreset::Disabled,
            ReasoningPatchContext::test(LlmApiType::Gemini),
        )
        .expect_err("Gemini 3 cannot disable thinking");
        assert!(err.reason.contains("does not support disabling"));
    }

    #[test]
    fn auto_is_unsupported_when_family_has_no_explicit_provider_auto_value() {
        for (family, api_type) in [
            (
                ReasoningPatchFamily::OpenAiChatReasoningEffort,
                LlmApiType::Openai,
            ),
            (
                ReasoningPatchFamily::OpenAiResponsesReasoning,
                LlmApiType::Responses,
            ),
            (
                ReasoningPatchFamily::AnthropicThinkingBudget,
                LlmApiType::Anthropic,
            ),
            (
                ReasoningPatchFamily::Gemini3ThinkingLevel,
                LlmApiType::Gemini,
            ),
        ] {
            let err = generate_reasoning_patches(
                family,
                ReasoningPreset::Auto,
                ReasoningPatchContext::test(api_type),
            )
            .expect_err("auto should be unsupported");
            assert!(
                err.reason.contains("auto") || err.reason.contains("provider-managed"),
                "unexpected reason: {}",
                err.reason
            );
        }
    }

    #[test]
    fn protocol_mismatch_returns_clear_unsupported_error() {
        let err = generate_reasoning_patches(
            ReasoningPatchFamily::OpenAiResponsesReasoning,
            ReasoningPreset::High,
            ReasoningPatchContext::test(LlmApiType::Openai),
        )
        .expect_err("wrong protocol should be unsupported");

        assert_eq!(err.target_api_type, LlmApiType::Openai);
        assert!(err.reason.contains("family targets"));
    }

    #[test]
    fn model_without_reasoning_capability_rejects_reasoning_required_preset() {
        let model = CacheModel {
            id: 7,
            provider_id: 1,
            model_name: "plain-model".to_string(),
            real_model_name: None,
            cost_catalog_id: None,
            reasoning_profile_override_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: false,
            supports_image_input: true,
            supports_embeddings: false,
            supports_rerank: false,
            is_enabled: true,
        };
        let context = ReasoningPatchContext::for_model(LlmApiType::Openai, &model);

        let err = generate_reasoning_patches(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            ReasoningPreset::High,
            context,
        )
        .expect_err("model without reasoning should reject high preset");

        assert_eq!(err.model_id, Some(7));
        assert_eq!(err.model_name.as_deref(), Some("plain-model"));
        assert!(err.reason.contains("capability"));
    }

    #[test]
    fn generated_patches_never_target_model_field() {
        let valid = [
            (
                ReasoningPatchFamily::OpenAiChatReasoningEffort,
                LlmApiType::Openai,
                ReasoningPreset::High,
            ),
            (
                ReasoningPatchFamily::OpenAiResponsesReasoning,
                LlmApiType::Responses,
                ReasoningPreset::High,
            ),
            (
                ReasoningPatchFamily::AnthropicThinkingBudget,
                LlmApiType::Anthropic,
                ReasoningPreset::High,
            ),
            (
                ReasoningPatchFamily::Gemini25ThinkingBudget,
                LlmApiType::Gemini,
                ReasoningPreset::Auto,
            ),
            (
                ReasoningPatchFamily::Gemini3ThinkingLevel,
                LlmApiType::Gemini,
                ReasoningPreset::High,
            ),
        ];

        for (family, api_type, preset) in valid {
            let patches =
                generate_reasoning_patches(family, preset, ReasoningPatchContext::test(api_type))
                    .expect("valid patch should generate");
            assert!(patches.iter().all(|patch| patch.target != "/model"));
        }
    }
}
