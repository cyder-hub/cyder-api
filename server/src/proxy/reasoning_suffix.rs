use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    database::reasoning_config::{ReasoningPatchFamily, ReasoningPreset},
    schema::enum_def::{LlmApiType, ProviderType, RequestPatchOperation, RequestPatchPlacement},
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

#[derive(Debug, Clone, Copy)]
pub(crate) struct ReasoningPresetPreviewInput {
    pub preset: ReasoningPreset,
    pub enabled: bool,
    pub expose_in_models: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ReasoningGeneratedPatchPreview {
    pub placement: String,
    pub target: String,
    pub operation: String,
    pub value_json: Option<Value>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ReasoningPresetPatchPreview {
    pub preset_key: String,
    pub suffix: String,
    pub requires_reasoning: bool,
    pub allowed_operation_kinds: Vec<String>,
    pub family_supported: bool,
    pub enabled: bool,
    pub expose_in_models: bool,
    pub runtime_supported: bool,
    pub unsupported_reason: Option<String>,
    pub generated_patches: Vec<ReasoningGeneratedPatchPreview>,
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
        ReasoningPatchFamily::DeepSeekOpenAiReasoning => {
            generate_deepseek_openai_reasoning_patch(family, preset, context)
        }
        ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking => {
            generate_siliconflow_openai_enable_thinking_patch(family, preset, context)
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

pub(crate) fn target_api_type_for_provider_type(provider_type: &ProviderType) -> LlmApiType {
    match provider_type {
        ProviderType::Vertex | ProviderType::Gemini => LlmApiType::Gemini,
        ProviderType::Ollama => LlmApiType::Ollama,
        ProviderType::Anthropic => LlmApiType::Anthropic,
        ProviderType::Responses => LlmApiType::Responses,
        ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
        ProviderType::Openai | ProviderType::VertexOpenai => LlmApiType::Openai,
    }
}

pub(crate) fn target_api_types_for_reasoning_family(
    family: ReasoningPatchFamily,
) -> &'static [LlmApiType] {
    match family {
        ReasoningPatchFamily::OpenAiChatReasoningEffort => {
            &[LlmApiType::Openai, LlmApiType::GeminiOpenai]
        }
        ReasoningPatchFamily::OpenAiResponsesReasoning => &[LlmApiType::Responses],
        ReasoningPatchFamily::DeepSeekOpenAiReasoning => &[LlmApiType::Openai],
        ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking => &[LlmApiType::Openai],
        ReasoningPatchFamily::AnthropicThinkingBudget => &[LlmApiType::Anthropic],
        ReasoningPatchFamily::Gemini25ThinkingBudget
        | ReasoningPatchFamily::Gemini3ThinkingLevel => &[LlmApiType::Gemini],
    }
}

pub(crate) fn preview_reasoning_patches(
    family: ReasoningPatchFamily,
    preset_inputs: &[ReasoningPresetPreviewInput],
    context: ReasoningPatchContext<'_>,
) -> Vec<ReasoningPresetPatchPreview> {
    ReasoningPreset::ALL
        .into_iter()
        .map(|preset| {
            let metadata = preset.metadata();
            let input = preset_inputs.iter().find(|input| input.preset == preset);
            let enabled = input.map(|input| input.enabled).unwrap_or(false);
            let expose_in_models = input.map(|input| input.expose_in_models).unwrap_or(false);
            let family_supported = family.supports_preset(preset);

            if let Some(reason) = family.unsupported_preset_reason(preset) {
                return ReasoningPresetPatchPreview {
                    preset_key: metadata.preset_key,
                    suffix: metadata.suffix,
                    requires_reasoning: metadata.requires_reasoning,
                    allowed_operation_kinds: metadata.allowed_operation_kinds,
                    family_supported,
                    enabled,
                    expose_in_models,
                    runtime_supported: false,
                    unsupported_reason: Some(reason.to_string()),
                    generated_patches: Vec::new(),
                };
            }

            match generate_reasoning_patches(family, preset, context) {
                Ok(patches) => {
                    let generated_patches = patches
                        .into_iter()
                        .map(ReasoningGeneratedPatchPreview::from_generated)
                        .collect();
                    ReasoningPresetPatchPreview {
                        preset_key: metadata.preset_key,
                        suffix: metadata.suffix,
                        requires_reasoning: metadata.requires_reasoning,
                        allowed_operation_kinds: metadata.allowed_operation_kinds,
                        family_supported,
                        enabled,
                        expose_in_models,
                        runtime_supported: enabled,
                        unsupported_reason: (!enabled)
                            .then(|| "preset is not enabled for this config".to_string()),
                        generated_patches,
                    }
                }
                Err(err) => ReasoningPresetPatchPreview {
                    preset_key: metadata.preset_key,
                    suffix: metadata.suffix,
                    requires_reasoning: metadata.requires_reasoning,
                    allowed_operation_kinds: metadata.allowed_operation_kinds,
                    family_supported,
                    enabled,
                    expose_in_models,
                    runtime_supported: false,
                    unsupported_reason: Some(err.reason),
                    generated_patches: Vec::new(),
                },
            }
        })
        .collect()
}

impl ReasoningGeneratedPatchPreview {
    fn from_generated(patch: GeneratedReasoningPatch) -> Self {
        Self {
            placement: serialize_patch_enum(patch.placement),
            target: patch.target,
            operation: serialize_patch_enum(patch.operation),
            value_json: patch
                .value_json
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .unwrap_or(None),
            description: patch.description,
        }
    }
}

fn serialize_patch_enum<T>(value: T) -> String
where
    T: Serialize + std::fmt::Debug,
{
    serde_json::to_value(&value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{value:?}"))
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

fn generate_deepseek_openai_reasoning_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(family, preset, context, &[LlmApiType::Openai])?;

    let thinking_type = match preset {
        ReasoningPreset::Disabled => "disabled",
        ReasoningPreset::Enabled | ReasoningPreset::High | ReasoningPreset::XHigh => "enabled",
        ReasoningPreset::Low | ReasoningPreset::Medium => {
            return Err(ReasoningPresetUnsupported::new(
                family,
                preset,
                context,
                "DeepSeek OpenAI reasoning only exposes enabled/high/xhigh strengths",
            ));
        }
        ReasoningPreset::Auto => {
            return Err(ReasoningPresetUnsupported::new(
                family,
                preset,
                context,
                "DeepSeek OpenAI reasoning does not define provider-managed auto",
            ));
        }
    };

    let mut patches = vec![
        GeneratedReasoningPatch::new(
            family,
            preset,
            RequestPatchPlacement::Body,
            "/thinking/type",
            RequestPatchOperation::Set,
            Some(json!(thinking_type)),
            Some("generated DeepSeek OpenAI thinking switch patch".to_string()),
        )
        .map_err(|reason| ReasoningPresetUnsupported::new(family, preset, context, reason))?,
    ];

    let effort = match preset {
        ReasoningPreset::High => Some("high"),
        ReasoningPreset::XHigh => Some("xhigh"),
        _ => None,
    };

    if let Some(effort) = effort {
        patches.push(
            GeneratedReasoningPatch::new(
                family,
                preset,
                RequestPatchPlacement::Body,
                "/reasoning_effort",
                RequestPatchOperation::Set,
                Some(json!(effort)),
                Some("generated DeepSeek OpenAI reasoning effort patch".to_string()),
            )
            .map_err(|reason| ReasoningPresetUnsupported::new(family, preset, context, reason))?,
        );
    } else if preset == ReasoningPreset::Disabled {
        patches.push(
            GeneratedReasoningPatch::new(
                family,
                preset,
                RequestPatchPlacement::Body,
                "/reasoning_effort",
                RequestPatchOperation::Remove,
                None,
                Some("generated DeepSeek OpenAI reasoning effort removal patch".to_string()),
            )
            .map_err(|reason| ReasoningPresetUnsupported::new(family, preset, context, reason))?,
        );
    }

    Ok(patches)
}

fn generate_siliconflow_openai_enable_thinking_patch(
    family: ReasoningPatchFamily,
    preset: ReasoningPreset,
    context: ReasoningPatchContext<'_>,
) -> Result<Vec<GeneratedReasoningPatch>, ReasoningPresetUnsupported> {
    ensure_protocol(family, preset, context, &[LlmApiType::Openai])?;

    let enable_thinking = match preset {
        ReasoningPreset::Disabled => false,
        ReasoningPreset::Enabled => true,
        ReasoningPreset::Low
        | ReasoningPreset::Medium
        | ReasoningPreset::High
        | ReasoningPreset::XHigh => {
            return Err(ReasoningPresetUnsupported::new(
                family,
                preset,
                context,
                "SiliconFlow OpenAI reasoning only supports enable_thinking on/off",
            ));
        }
        ReasoningPreset::Auto => {
            return Err(ReasoningPresetUnsupported::new(
                family,
                preset,
                context,
                "SiliconFlow OpenAI reasoning does not define provider-managed auto",
            ));
        }
    };

    body_set_patch(
        family,
        preset,
        "/enable_thinking",
        json!(enable_thinking),
        "generated SiliconFlow OpenAI enable_thinking patch",
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

    fn preview_entry(
        family: ReasoningPatchFamily,
        preset: ReasoningPreset,
        api_type: LlmApiType,
    ) -> ReasoningPresetPatchPreview {
        preview_reasoning_patches(
            family,
            &[ReasoningPresetPreviewInput {
                preset,
                enabled: true,
                expose_in_models: true,
            }],
            ReasoningPatchContext::test(api_type),
        )
        .into_iter()
        .find(|entry| entry.preset_key == preset.as_key())
        .expect("preview entry should exist")
    }

    fn only_preview_patch(
        family: ReasoningPatchFamily,
        preset: ReasoningPreset,
        api_type: LlmApiType,
    ) -> ReasoningGeneratedPatchPreview {
        let entry = preview_entry(family, preset, api_type);
        assert!(
            entry.runtime_supported,
            "preview should be runtime-supported: {entry:?}"
        );
        assert_eq!(entry.generated_patches.len(), 1);
        entry.generated_patches.into_iter().next().unwrap()
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
    fn deepseek_openai_reasoning_generates_thinking_switch_patches() {
        let disabled = generate_reasoning_patches(
            ReasoningPatchFamily::DeepSeekOpenAiReasoning,
            ReasoningPreset::Disabled,
            ReasoningPatchContext::test(LlmApiType::Openai),
        )
        .expect("DeepSeek disabled patch should generate");
        assert_eq!(disabled.len(), 2);
        assert_eq!(disabled[0].target, "/thinking/type");
        assert_eq!(disabled[0].operation, RequestPatchOperation::Set);
        assert_eq!(patch_value(&disabled[0]), json!("disabled"));
        assert_eq!(disabled[1].target, "/reasoning_effort");
        assert_eq!(disabled[1].operation, RequestPatchOperation::Remove);
        assert!(disabled[1].value_json.is_none());

        let enabled = only_patch(
            ReasoningPatchFamily::DeepSeekOpenAiReasoning,
            ReasoningPreset::Enabled,
            LlmApiType::Openai,
        );
        assert_eq!(enabled.target, "/thinking/type");
        assert_eq!(patch_value(&enabled), json!("enabled"));
    }

    #[test]
    fn deepseek_openai_reasoning_high_and_xhigh_generate_effort_patches() {
        for (preset, effort) in [
            (ReasoningPreset::High, "high"),
            (ReasoningPreset::XHigh, "xhigh"),
        ] {
            let patches = generate_reasoning_patches(
                ReasoningPatchFamily::DeepSeekOpenAiReasoning,
                preset,
                ReasoningPatchContext::test(LlmApiType::Openai),
            )
            .expect("DeepSeek high strength patch should generate");
            assert_eq!(patches.len(), 2);
            assert_eq!(patches[0].target, "/thinking/type");
            assert_eq!(patch_value(&patches[0]), json!("enabled"));
            assert_eq!(patches[1].target, "/reasoning_effort");
            assert_eq!(patch_value(&patches[1]), json!(effort));
        }
    }

    #[test]
    fn deepseek_openai_reasoning_rejects_unsupported_strengths() {
        for preset in [
            ReasoningPreset::Low,
            ReasoningPreset::Medium,
            ReasoningPreset::Auto,
        ] {
            assert!(
                !ReasoningPatchFamily::DeepSeekOpenAiReasoning.supports_preset(preset),
                "{preset:?} should not be supported"
            );
        }
    }

    #[test]
    fn siliconflow_openai_enable_thinking_generates_switch_patches() {
        let disabled = only_patch(
            ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking,
            ReasoningPreset::Disabled,
            LlmApiType::Openai,
        );
        assert_eq!(disabled.target, "/enable_thinking");
        assert_eq!(patch_value(&disabled), json!(false));

        let enabled = only_patch(
            ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking,
            ReasoningPreset::Enabled,
            LlmApiType::Openai,
        );
        assert_eq!(enabled.target, "/enable_thinking");
        assert_eq!(patch_value(&enabled), json!(true));
    }

    #[test]
    fn siliconflow_openai_enable_thinking_rejects_strength_presets() {
        for preset in [
            ReasoningPreset::Low,
            ReasoningPreset::Medium,
            ReasoningPreset::High,
            ReasoningPreset::XHigh,
            ReasoningPreset::Auto,
        ] {
            assert!(
                !ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking.supports_preset(preset),
                "{preset:?} should not be supported"
            );
        }
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
                ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking,
                LlmApiType::Openai,
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
                ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking,
                LlmApiType::Openai,
                ReasoningPreset::Enabled,
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

    #[test]
    fn preview_openai_chat_high_exposes_generated_patch() {
        let patch = only_preview_patch(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            ReasoningPreset::High,
            LlmApiType::Openai,
        );
        assert_eq!(patch.placement, "BODY");
        assert_eq!(patch.operation, "SET");
        assert_eq!(patch.target, "/reasoning_effort");
        assert_eq!(patch.value_json, Some(json!("high")));
    }

    #[test]
    fn preview_openai_responses_high_exposes_generated_patch() {
        let patch = only_preview_patch(
            ReasoningPatchFamily::OpenAiResponsesReasoning,
            ReasoningPreset::High,
            LlmApiType::Responses,
        );
        assert_eq!(patch.target, "/reasoning/effort");
        assert_eq!(patch.value_json, Some(json!("high")));
    }

    #[test]
    fn preview_siliconflow_enabled_exposes_enable_thinking_patch() {
        let patch = only_preview_patch(
            ReasoningPatchFamily::SiliconFlowOpenAiEnableThinking,
            ReasoningPreset::Enabled,
            LlmApiType::Openai,
        );
        assert_eq!(patch.target, "/enable_thinking");
        assert_eq!(patch.value_json, Some(json!(true)));
    }

    #[test]
    fn preview_anthropic_high_exposes_generated_patch() {
        let patch = only_preview_patch(
            ReasoningPatchFamily::AnthropicThinkingBudget,
            ReasoningPreset::High,
            LlmApiType::Anthropic,
        );
        assert_eq!(patch.target, "/thinking");
        assert_eq!(
            patch.value_json,
            Some(json!({
                "type": "enabled",
                "budget_tokens": ANTHROPIC_THINKING_BUDGET_HIGH,
            }))
        );
    }

    #[test]
    fn preview_gemini25_auto_exposes_generated_budget_patch() {
        let patch = only_preview_patch(
            ReasoningPatchFamily::Gemini25ThinkingBudget,
            ReasoningPreset::Auto,
            LlmApiType::Gemini,
        );
        assert_eq!(
            patch.target,
            "/generationConfig/thinkingConfig/thinkingBudget"
        );
        assert_eq!(patch.value_json, Some(json!(GEMINI_THINKING_BUDGET_AUTO)));
    }

    #[test]
    fn preview_gemini3_high_exposes_generated_level_patch() {
        let patch = only_preview_patch(
            ReasoningPatchFamily::Gemini3ThinkingLevel,
            ReasoningPreset::High,
            LlmApiType::Gemini,
        );
        assert_eq!(
            patch.target,
            "/generationConfig/thinkingConfig/thinkingLevel"
        );
        assert_eq!(patch.value_json, Some(json!("high")));
    }

    #[test]
    fn preview_marks_model_capability_unsupported_for_reasoning_required_preset() {
        let context = ReasoningPatchContext {
            target_api_type: LlmApiType::Openai,
            model_id: Some(99),
            model_name: Some("plain-model"),
            supports_reasoning: false,
        };
        let entry = preview_reasoning_patches(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            &[ReasoningPresetPreviewInput {
                preset: ReasoningPreset::High,
                enabled: true,
                expose_in_models: true,
            }],
            context,
        )
        .into_iter()
        .find(|entry| entry.preset_key == "high")
        .expect("high preview entry should exist");

        assert!(!entry.runtime_supported);
        assert!(entry.generated_patches.is_empty());
        assert!(
            entry
                .unsupported_reason
                .as_deref()
                .unwrap_or_default()
                .contains("capability")
        );
    }

    #[test]
    fn preview_generates_review_patch_for_not_enabled_preset() {
        let entry = preview_reasoning_patches(
            ReasoningPatchFamily::OpenAiChatReasoningEffort,
            &[ReasoningPresetPreviewInput {
                preset: ReasoningPreset::High,
                enabled: false,
                expose_in_models: false,
            }],
            ReasoningPatchContext::test(LlmApiType::Openai),
        )
        .into_iter()
        .find(|entry| entry.preset_key == "high")
        .expect("high preview entry should exist");

        assert!(!entry.enabled);
        assert!(!entry.runtime_supported);
        assert_eq!(
            entry.unsupported_reason.as_deref(),
            Some("preset is not enabled for this config")
        );
        assert_eq!(entry.generated_patches.len(), 1);
        assert_eq!(entry.generated_patches[0].target, "/reasoning_effort");
        assert_eq!(entry.generated_patches[0].value_json, Some(json!("high")));
    }
}
