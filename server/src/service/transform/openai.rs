use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    StreamTransformer, TransformProtocol, TransformValueKind, apply_transform_policy,
    build_stream_diagnostic_sse, unified::*,
};
use crate::schema::enum_def::{LlmApiType, ProviderType};
use crate::utils::sse::SseEvent;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenAiVariant {
    Standard,
    GeminiCompat,
    AzureCompat,
    OtherCompat,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RewriteAction {
    Remove,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FieldRewriteRule {
    field: &'static str,
    action: RewriteAction,
}

#[derive(Clone, Copy)]
pub(crate) struct DefaultInjectionRule {
    field: &'static str,
    value: fn() -> Value,
}

#[derive(Clone, Copy)]
pub(crate) struct ChannelSchemaPolicy {
    allowed_top_level_fields: &'static [&'static str],
    forbidden_top_level_fields: &'static [&'static str],
    rewrite_rules: &'static [FieldRewriteRule],
    default_injections: &'static [DefaultInjectionRule],
}

#[derive(Clone, Copy)]
pub(crate) struct OpenAiVariantPolicy {
    variant: OpenAiVariant,
    schema: ChannelSchemaPolicy,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct OpenAiSanitizeReport {
    pub removed_fields: Vec<String>,
    pub injected_defaults: Vec<String>,
}

const GEMINI_COMPAT_ALLOWED_FIELDS: &[&str] = &[
    "messages",
    "model",
    "detail",
    "max_completion_tokens",
    "modalities",
    "max_tokens",
    "n",
    "frequency_penalty",
    "presence_penalty",
    "reasoning_effort",
    "response_format",
    "seed",
    "stop",
    "stream",
    "temperature",
    "top_p",
    "tools",
    "tool_choice",
    "web_search_options",
    "function_call",
    "functions",
];

const GEMINI_COMPAT_REWRITE_RULES: &[FieldRewriteRule] = &[FieldRewriteRule {
    field: "stream_options",
    action: RewriteAction::Remove,
}];

const EMPTY_FIELDS: &[&str] = &[];
const EMPTY_REWRITE_RULES: &[FieldRewriteRule] = &[];
const EMPTY_DEFAULT_INJECTIONS: &[DefaultInjectionRule] = &[];

impl ChannelSchemaPolicy {
    fn standard_core() -> Self {
        Self {
            allowed_top_level_fields: EMPTY_FIELDS,
            forbidden_top_level_fields: EMPTY_FIELDS,
            rewrite_rules: EMPTY_REWRITE_RULES,
            default_injections: EMPTY_DEFAULT_INJECTIONS,
        }
    }

    fn gemini_compat() -> Self {
        Self {
            allowed_top_level_fields: GEMINI_COMPAT_ALLOWED_FIELDS,
            forbidden_top_level_fields: EMPTY_FIELDS,
            rewrite_rules: GEMINI_COMPAT_REWRITE_RULES,
            default_injections: EMPTY_DEFAULT_INJECTIONS,
        }
    }

    fn for_variant(variant: OpenAiVariant) -> Self {
        match variant {
            OpenAiVariant::Standard | OpenAiVariant::AzureCompat | OpenAiVariant::OtherCompat => {
                Self::standard_core()
            }
            OpenAiVariant::GeminiCompat => Self::gemini_compat(),
        }
    }
}

impl OpenAiVariantPolicy {
    pub(crate) fn for_variant(variant: OpenAiVariant) -> Self {
        Self {
            variant,
            schema: ChannelSchemaPolicy::for_variant(variant),
        }
    }

    pub(crate) fn variant(&self) -> OpenAiVariant {
        self.variant
    }

    pub(crate) fn sanitize_request_payload(&self, payload: &mut Value) -> OpenAiSanitizeReport {
        let policy = self.schema;
        let mut report = OpenAiSanitizeReport::default();
        let Some(obj) = payload.as_object_mut() else {
            return report;
        };

        for rule in policy.rewrite_rules {
            match rule.action {
                RewriteAction::Remove => {
                    if obj.remove(rule.field).is_some() {
                        report.removed_fields.push(rule.field.to_string());
                    }
                }
            }
        }

        if !policy.forbidden_top_level_fields.is_empty() {
            for field in policy.forbidden_top_level_fields {
                if obj.remove(*field).is_some() {
                    report.removed_fields.push((*field).to_string());
                }
            }
        }

        if !policy.allowed_top_level_fields.is_empty() {
            let keys_to_remove: Vec<String> = obj
                .keys()
                .filter(|key| !policy.allowed_top_level_fields.contains(&key.as_str()))
                .cloned()
                .collect();
            for key in keys_to_remove {
                obj.remove(&key);
                report.removed_fields.push(key);
            }
        }

        for injection in policy.default_injections {
            if !obj.contains_key(injection.field) {
                obj.insert(injection.field.to_string(), (injection.value)());
                report.injected_defaults.push(injection.field.to_string());
            }
        }

        report.removed_fields.sort();
        report.removed_fields.dedup();
        report.injected_defaults.sort();
        report.injected_defaults.dedup();
        report
    }
}

pub(crate) fn determine_openai_variant(
    provider_type: &ProviderType,
    downstream_path: &str,
) -> OpenAiVariant {
    match provider_type {
        ProviderType::VertexOpenai | ProviderType::GeminiOpenai
            if downstream_path == "chat/completions" =>
        {
            OpenAiVariant::GeminiCompat
        }
        _ => OpenAiVariant::Standard,
    }
}

pub(crate) fn resolve_openai_variant_policy(
    provider_type: &ProviderType,
    downstream_path: &str,
) -> OpenAiVariantPolicy {
    OpenAiVariantPolicy::for_variant(determine_openai_variant(provider_type, downstream_path))
}

pub(crate) fn finalize_openai_compatible_request_payload(
    payload: &mut Value,
    provider_type: &ProviderType,
    downstream_path: &str,
) -> (OpenAiVariant, OpenAiSanitizeReport) {
    let policy = resolve_openai_variant_policy(provider_type, downstream_path);
    let report = policy.sanitize_request_payload(payload);
    (policy.variant(), report)
}

pub(crate) fn sanitize_openai_request_payload(
    payload: &mut Value,
    variant: OpenAiVariant,
) -> OpenAiSanitizeReport {
    OpenAiVariantPolicy::for_variant(variant).sanitize_request_payload(payload)
}

// --- OpenAI to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiRequestPayload {
    #[serde(skip_serializing_if = "String::is_empty")]
    model: String,
    messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<UnifiedTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop: Option<OpenAiStop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logit_bias: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_logprobs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) enum ReasoningEffort {
    #[serde(rename = "none")]
    _None,
    #[serde(rename = "minimal")]
    Minimal,
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
    #[serde(rename = "xhigh")]
    Xhigh,
}

fn register_passthrough_field(
    passthrough_fields: &mut Vec<(String, Value)>,
    key: &str,
    value: Value,
    context: &str,
) {
    if is_registered_passthrough_key(key) {
        passthrough_fields.push((key.to_string(), value));
    } else {
        cyder_tools::log::warn!(
            "[transform][passthrough] rejected_unregistered_key key={} context={} registered_keys={:?}",
            key,
            context,
            REGISTERED_PASSTHROUGH_KEYS
        );
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(super) enum OpenAiStop {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiMessage {
    role: String,
    content: Option<OpenAiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(super) enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum OpenAiContentPart {
    Text { text: String },
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiImageUrl {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiToolCall {
    id: String,
    #[serde(rename = "type")]
    type_: String, // "function"
    function: OpenAiFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiFunction {
    name: String,
    arguments: String,
}

fn build_data_url(mime_type: &str, data: &str) -> String {
    format!("data:{mime_type};base64,{data}")
}

fn render_file_reference_text(
    url: &str,
    mime_type: Option<&str>,
    filename: Option<&str>,
) -> String {
    let mut lines = vec![format!("file_url: {url}")];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    if let Some(mime_type) = mime_type.filter(|value| !value.is_empty()) {
        lines.push(format!("mime_type: {mime_type}"));
    }
    lines.join("\n")
}

fn render_inline_file_data_text(data: &str, mime_type: &str, filename: Option<&str>) -> String {
    let mut lines = vec![
        format!("file_data: {data}"),
        format!("mime_type: {mime_type}"),
    ];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    lines.join("\n")
}

fn build_openai_stream_diagnostic(
    transformer: &mut StreamTransformer,
    kind: TransformValueKind,
    context: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        transformer,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Openai),
        kind,
        "openai_stream_encoding",
        context,
        None,
        Some(
            "Use a Responses or Anthropic target when structured reasoning/blob stream events must remain recoverable.".to_string(),
        ),
    )
}

fn render_executable_code_text(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiLogProbs {
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Vec<OpenAiLogProb>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiLogProb {
    token: String,
    logprob: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_logprobs: Option<Vec<OpenAiTopLogProb>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiTopLogProb {
    token: String,
    logprob: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    bytes: Option<Vec<u8>>,
}

impl From<OpenAiRequestPayload> for UnifiedRequest {
    fn from(openai_req: OpenAiRequestPayload) -> Self {
        let messages = openai_req
            .messages
            .into_iter()
            .map(|msg| {
                let role = match msg.role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User, // Default to user for unknown roles
                };

                let mut content = Vec::new();

                if let Some(c) = msg.content {
                    match c {
                        OpenAiContent::Text(text) => {
                            content.push(UnifiedContentPart::Text { text });
                        }
                        OpenAiContent::Parts(parts) => {
                            for part in parts {
                                match part {
                                    OpenAiContentPart::Text { text } => {
                                        content.push(UnifiedContentPart::Text { text });
                                    }
                                    OpenAiContentPart::ImageUrl { image_url } => {
                                        content.push(UnifiedContentPart::ImageUrl {
                                            url: image_url.url,
                                            detail: image_url.detail,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(refusal) = msg.refusal {
                    content.insert(0, UnifiedContentPart::Refusal { text: refusal });
                }

                if let Some(tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        let args: Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                        content.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: tc.id,
                            name: tc.function.name,
                            arguments: args,
                        }));
                    }
                }

                if let Some(tool_call_id) = msg.tool_call_id {
                    // If content was present, use it as the result content, otherwise empty string
                    let result_content = content
                        .iter()
                        .find_map(|p| match p {
                            UnifiedContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();

                    // Clear previous text content as it's now part of the tool result
                    content.retain(|p| !matches!(p, UnifiedContentPart::Text { .. }));

                    content.push(UnifiedContentPart::ToolResult(
                        UnifiedToolResult::from_legacy_content(
                            tool_call_id,
                            msg.name,
                            result_content,
                        ),
                    ));
                }

                UnifiedMessage { role, content }
            })
            .collect();

        let stop = openai_req.stop.map(|v| match v {
            OpenAiStop::String(s) => vec![s],
            OpenAiStop::Array(arr) => arr,
        });

        // Store OpenAI-specific fields that don't have unified equivalents in passthrough
        let mut passthrough_fields = Vec::new();
        if let Some(logprobs) = openai_req.logprobs {
            register_passthrough_field(
                &mut passthrough_fields,
                "logprobs",
                json!(logprobs),
                "openai_request_to_unified",
            );
        }
        if let Some(top_logprobs) = openai_req.top_logprobs {
            register_passthrough_field(
                &mut passthrough_fields,
                "top_logprobs",
                json!(top_logprobs),
                "openai_request_to_unified",
            );
        }
        if let Some(parallel_tool_calls) = openai_req.parallel_tool_calls {
            register_passthrough_field(
                &mut passthrough_fields,
                "parallel_tool_calls",
                json!(parallel_tool_calls),
                "openai_request_to_unified",
            );
        }
        if let Some(reasoning_effort) = openai_req.reasoning_effort {
            register_passthrough_field(
                &mut passthrough_fields,
                "reasoning_effort",
                json!(reasoning_effort),
                "openai_request_to_unified",
            );
        }

        let passthrough =
            build_registered_passthrough(passthrough_fields, "openai_request_to_unified");

        let openai_extension = UnifiedOpenAiRequestExtension {
            tool_choice: openai_req.tool_choice,
            n: openai_req.n,
            response_format: openai_req.response_format,
            logit_bias: openai_req.logit_bias,
            user: openai_req.user,
            passthrough,
        };

        UnifiedRequest {
            model: Some(openai_req.model),
            messages,
            tools: openai_req.tools,
            stream: openai_req.stream.unwrap_or(false),
            temperature: openai_req.temperature,
            max_tokens: openai_req.max_tokens,
            top_p: openai_req.top_p,
            stop,
            seed: openai_req.seed,
            presence_penalty: openai_req.presence_penalty,
            frequency_penalty: openai_req.frequency_penalty,
            extensions: (!openai_extension.is_empty()).then_some(UnifiedRequestExtensions {
                openai: Some(openai_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for OpenAiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let openai_extension = unified_req.openai_extension().cloned().unwrap_or_default();
        let messages = unified_req
            .messages
            .into_iter()
            .flat_map(|msg| {
                let role = match msg.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    UnifiedRole::Tool => "tool",
                }
                .to_string();

                // Group content by type to reconstruct OpenAI message structure
                let mut content_parts = Vec::new();
                let mut tool_calls = Vec::new();
                let mut tool_results = Vec::new();
                let mut refusal = None;
                let mut has_multimodal = false;

                for part in msg.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            if has_multimodal {
                                content_parts.push(OpenAiContentPart::Text { text });
                            } else {
                                content_parts.push(OpenAiContentPart::Text { text });
                            }
                        }
                        UnifiedContentPart::Refusal { text } => {
                            refusal = Some(text);
                        }
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl { url, detail },
                            });
                        }
                        UnifiedContentPart::Reasoning { text } => {
                            content_parts.push(OpenAiContentPart::Text { text });
                        }
                        UnifiedContentPart::ImageData { mime_type, data } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl {
                                    url: build_data_url(&mime_type, &data),
                                    detail: Some("auto".to_string()),
                                },
                            });
                        }
                        UnifiedContentPart::FileUrl {
                            url,
                            mime_type,
                            filename,
                        } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_file_reference_text(
                                    &url,
                                    mime_type.as_deref(),
                                    filename.as_deref(),
                                ),
                            });
                        }
                        UnifiedContentPart::FileData {
                            data,
                            mime_type,
                            filename,
                        } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_inline_file_data_text(
                                    &data,
                                    &mime_type,
                                    filename.as_deref(),
                                ),
                            });
                        }
                        UnifiedContentPart::ExecutableCode { language, code } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_executable_code_text(&language, &code),
                            });
                        }
                        UnifiedContentPart::ToolCall(call) => tool_calls.push(OpenAiToolCall {
                            id: call.id,
                            type_: "function".to_string(),
                            function: OpenAiFunction {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        }),
                        UnifiedContentPart::ToolResult(result) => tool_results.push(result),
                    }
                }

                let content_val = if content_parts.is_empty() {
                    None
                } else if content_parts.len() == 1 && !has_multimodal {
                    // Single text part - use simple string format
                    if let OpenAiContentPart::Text { text } = &content_parts[0] {
                        Some(OpenAiContent::Text(text.clone()))
                    } else {
                        Some(OpenAiContent::Parts(content_parts.clone()))
                    }
                } else {
                    // Multiple parts or has images - use parts format
                    Some(OpenAiContent::Parts(content_parts.clone()))
                };

                // If there are tool results, they must be separate messages in OpenAI
                // We also need to handle mixed content (e.g. Text + ToolResults) by creating separate messages
                let mut generated_messages = Vec::new();

                // 1. If there is text content, create a message for it first
                if let Some(c) = content_val {
                    generated_messages.push(OpenAiMessage {
                        role: role.clone(),
                        content: Some(c),
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls.clone())
                        },
                        name: None,
                        tool_call_id: None,
                        refusal: refusal.clone(),
                    });
                } else if !tool_calls.is_empty() {
                    // Case where there is no text but there are tool calls (Assistant invoking tool)
                    generated_messages.push(OpenAiMessage {
                        role: role.clone(),
                        content: None,
                        tool_calls: Some(tool_calls),
                        name: None,
                        tool_call_id: None,
                        refusal: refusal.clone(),
                    });
                }

                // 2. Add tool results as separate messages with 'tool' role
                for result in tool_results {
                    generated_messages.push(OpenAiMessage {
                        role: "tool".to_string(),
                        content: Some(OpenAiContent::Text(result.legacy_content())),
                        tool_calls: None,
                        name: result.name,
                        tool_call_id: Some(result.tool_call_id),
                        refusal: None,
                    });
                }

                generated_messages
            })
            .collect();

        let stop = unified_req.stop.clone().map(|v| {
            if v.len() == 1 {
                OpenAiStop::String(v.into_iter().next().unwrap())
            } else {
                OpenAiStop::Array(v)
            }
        });

        // Extract OpenAI-specific fields from passthrough if present
        let (logprobs, top_logprobs, parallel_tool_calls, reasoning_effort) =
            if let Some(passthrough) = openai_extension.passthrough.as_ref() {
                audit_passthrough_keys(passthrough, "unified_request_to_openai");
                (
                    passthrough.get("logprobs").and_then(|v| v.as_bool()),
                    passthrough
                        .get("top_logprobs")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                    passthrough
                        .get("parallel_tool_calls")
                        .and_then(|v| v.as_bool()),
                    passthrough
                        .get("reasoning_effort")
                        .and_then(|v| serde_json::from_value(v.clone()).ok()),
                )
            } else {
                (None, None, None, None)
            };

        OpenAiRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            messages,
            tools: unified_req.tools,
            tool_choice: openai_extension.tool_choice,
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
            stop,
            n: openai_extension.n,
            seed: unified_req.seed,
            presence_penalty: unified_req.presence_penalty,
            frequency_penalty: unified_req.frequency_penalty,
            logit_bias: openai_extension.logit_bias,
            logprobs,
            top_logprobs,
            response_format: openai_extension.response_format,
            user: openai_extension.user,
            parallel_tool_calls,
            reasoning_effort,
        }
    }
}

// --- OpenAI Response to Unified ---

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiCompletionTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct OpenAiPromptTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiUsage {
    completion_tokens: u32,
    prompt_tokens: u32,
    total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    completion_tokens_details: Option<OpenAiCompletionTokenDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_tokens_details: Option<OpenAiPromptTokenDetails>,
}

impl From<OpenAiUsage> for UnifiedUsage {
    fn from(openai_usage: OpenAiUsage) -> Self {
        let mut reasoning_tokens = openai_usage
            .completion_tokens_details
            .as_ref()
            .and_then(|d| d.reasoning_tokens);

        if reasoning_tokens.is_none() {
            let calculated_reasoning = openai_usage
                .total_tokens
                .saturating_sub(openai_usage.prompt_tokens)
                .saturating_sub(openai_usage.completion_tokens);
            if calculated_reasoning > 0 {
                reasoning_tokens = Some(calculated_reasoning);
            }
        }

        let cached_tokens = openai_usage
            .prompt_tokens_details
            .as_ref()
            .and_then(|d| d.cached_tokens);

        UnifiedUsage {
            input_tokens: openai_usage.prompt_tokens,
            output_tokens: openai_usage.completion_tokens,
            total_tokens: openai_usage.total_tokens,
            reasoning_tokens,
            cached_tokens,
            ..Default::default()
        }
    }
}

impl From<UnifiedUsage> for OpenAiUsage {
    fn from(unified_usage: UnifiedUsage) -> Self {
        let completion_tokens_details = unified_usage.reasoning_tokens.map(|rt| {
            OpenAiCompletionTokenDetails {
                reasoning_tokens: Some(rt),
                audio_tokens: None, // No source for this
            }
        });

        let prompt_tokens_details = unified_usage.cached_tokens.map(|ct| {
            OpenAiPromptTokenDetails {
                cached_tokens: Some(ct),
                audio_tokens: None, // No source for this
            }
        });

        OpenAiUsage {
            prompt_tokens: unified_usage.input_tokens,
            completion_tokens: unified_usage.output_tokens,
            total_tokens: unified_usage.total_tokens,
            completion_tokens_details,
            prompt_tokens_details,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiResponse {
    id: String,
    object: String, // Usually "chat.completion"
    created: i64,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_fingerprint: Option<String>,
    choices: Vec<OpenAiChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChoice {
    index: u32,
    message: OpenAiMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<OpenAiLogProbs>,
    finish_reason: Option<String>, // Can be null in some cases (e.g., content filtering)
}

impl From<OpenAiResponse> for UnifiedResponse {
    fn from(openai_res: OpenAiResponse) -> Self {
        let choices = openai_res
            .choices
            .into_iter()
            .map(|choice| {
                let role = match choice.message.role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::Assistant, // Default to assistant for response messages
                };

                let mut content = Vec::new();

                if let Some(c) = choice.message.content {
                    match c {
                        OpenAiContent::Text(text) => {
                            content.push(UnifiedContentPart::Text { text });
                        }
                        OpenAiContent::Parts(parts) => {
                            for part in parts {
                                match part {
                                    OpenAiContentPart::Text { text } => {
                                        content.push(UnifiedContentPart::Text { text });
                                    }
                                    OpenAiContentPart::ImageUrl { image_url } => {
                                        content.push(UnifiedContentPart::ImageUrl {
                                            url: image_url.url,
                                            detail: image_url.detail,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(refusal) = choice.message.refusal {
                    content.insert(0, UnifiedContentPart::Refusal { text: refusal });
                }

                if let Some(tool_calls) = choice.message.tool_calls {
                    for tc in tool_calls {
                        let args: Value =
                            serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                        content.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: tc.id,
                            name: tc.function.name,
                            arguments: args,
                        }));
                    }
                }

                if let Some(tool_call_id) = choice.message.tool_call_id {
                    // Extract text content if available to be the result content
                    let result_content = content
                        .iter()
                        .find_map(|p| match p {
                            UnifiedContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();

                    // Clear text parts as they are consumed
                    content.retain(|p| !matches!(p, UnifiedContentPart::Text { .. }));

                    content.push(UnifiedContentPart::ToolResult(
                        UnifiedToolResult::from_legacy_content(
                            tool_call_id,
                            choice.message.name,
                            result_content,
                        ),
                    ));
                }

                let message = UnifiedMessage {
                    role,
                    content,
                    ..Default::default()
                };

                UnifiedChoice {
                    index: choice.index,
                    message,
                    items: Vec::new(),
                    finish_reason: choice.finish_reason,
                    logprobs: choice
                        .logprobs
                        .map(|lp| serde_json::to_value(lp).unwrap_or(Value::Null)),
                }
            })
            .collect();

        UnifiedResponse {
            id: openai_res.id,
            model: Some(openai_res.model),
            choices,
            usage: openai_res.usage.map(|u| u.into()),
            created: Some(openai_res.created),
            object: Some(openai_res.object),
            system_fingerprint: openai_res.system_fingerprint,
            provider_response_metadata: None,
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedResponse> for OpenAiResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let choices = unified_res
            .choices
            .into_iter()
            .map(|choice| {
                let role = match choice.message.role {
                    UnifiedRole::System => "system",
                    UnifiedRole::User => "user",
                    UnifiedRole::Assistant => "assistant",
                    UnifiedRole::Tool => "tool",
                }
                .to_string();

                let mut content_parts = Vec::new();
                let mut tool_calls = Vec::new();
                // Note: OpenAI Response doesn't typically have ToolResult in choices, but handling for completeness
                let mut tool_call_id = None;
                let mut name = None;
                let mut refusal = None;
                let mut has_multimodal = false;

                for part in choice.message.content {
                    match part {
                        UnifiedContentPart::Text { text } => {
                            content_parts.push(OpenAiContentPart::Text { text });
                        }
                        UnifiedContentPart::Refusal { text } => {
                            refusal = Some(text);
                        }
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl { url, detail },
                            });
                        }
                        UnifiedContentPart::Reasoning { text } => {
                            content_parts.push(OpenAiContentPart::Text { text });
                        }
                        UnifiedContentPart::ImageData { mime_type, data } => {
                            has_multimodal = true;
                            content_parts.push(OpenAiContentPart::ImageUrl {
                                image_url: OpenAiImageUrl {
                                    url: build_data_url(&mime_type, &data),
                                    detail: Some("auto".to_string()),
                                },
                            });
                        }
                        UnifiedContentPart::FileUrl {
                            url,
                            mime_type,
                            filename,
                        } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_file_reference_text(
                                    &url,
                                    mime_type.as_deref(),
                                    filename.as_deref(),
                                ),
                            });
                        }
                        UnifiedContentPart::FileData {
                            data,
                            mime_type,
                            filename,
                        } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_inline_file_data_text(
                                    &data,
                                    &mime_type,
                                    filename.as_deref(),
                                ),
                            });
                        }
                        UnifiedContentPart::ExecutableCode { language, code } => {
                            content_parts.push(OpenAiContentPart::Text {
                                text: render_executable_code_text(&language, &code),
                            });
                        }
                        UnifiedContentPart::ToolCall(call) => tool_calls.push(OpenAiToolCall {
                            id: call.id,
                            type_: "function".to_string(),
                            function: OpenAiFunction {
                                name: call.name,
                                arguments: call.arguments.to_string(),
                            },
                        }),
                        UnifiedContentPart::ToolResult(result) => {
                            // If there's a tool result in the response, we treat it as content
                            // This is rare for a response object.
                            content_parts.push(OpenAiContentPart::Text {
                                text: result.legacy_content(),
                            });
                            tool_call_id = Some(result.tool_call_id);
                            name = result.name;
                        }
                    }
                }

                let content = if content_parts.is_empty() {
                    None
                } else if content_parts.len() == 1 && !has_multimodal {
                    // Single text part - use simple string format
                    if let OpenAiContentPart::Text { text } = &content_parts[0] {
                        Some(OpenAiContent::Text(text.clone()))
                    } else {
                        Some(OpenAiContent::Parts(content_parts.clone()))
                    }
                } else {
                    // Multiple parts or has images - use parts format
                    Some(OpenAiContent::Parts(content_parts.clone()))
                };

                let message = OpenAiMessage {
                    role,
                    content,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    name,
                    tool_call_id,
                    refusal,
                };

                OpenAiChoice {
                    index: choice.index,
                    message,
                    finish_reason: choice.finish_reason,
                    logprobs: choice.logprobs.and_then(|v| serde_json::from_value(v).ok()),
                }
            })
            .collect();

        OpenAiResponse {
            id: unified_res.id,
            object: unified_res
                .object
                .unwrap_or_else(|| "chat.completion".to_string()),
            created: unified_res
                .created
                .unwrap_or_else(|| chrono::Utc::now().timestamp()),
            model: unified_res.model.unwrap_or_default(),
            system_fingerprint: unified_res.system_fingerprint,
            choices,
            usage: unified_res.usage.map(|u| u.into()),
        }
    }
}

// --- OpenAI Response Chunk ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkResponse {
    id: String,
    object: String, // Usually "chat.completion.chunk"
    created: i64,
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_fingerprint: Option<String>,
    choices: Vec<OpenAiChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    usage: Option<OpenAiUsage>, // Usually only present in the last chunk
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkChoice {
    index: u32,
    delta: OpenAiChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logprobs: Option<OpenAiLogProbs>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) struct OpenAiChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAiChunkToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    refusal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>, // For tool messages
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiChunkToolCall {
    index: u32,         // OpenAI includes index in chunk tool calls
    id: Option<String>, // ID is optional in chunks
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    type_: Option<String>,
    function: OpenAiChunkFunction,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct OpenAiChunkFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<String>,
}

impl From<UnifiedChunkResponse> for OpenAiChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let choices = unified_chunk
            .choices
            .into_iter()
            .map(|choice| {
                let role = choice.delta.role.map(|r| {
                    match r {
                        UnifiedRole::System => "system",
                        UnifiedRole::User => "user",
                        UnifiedRole::Assistant => "assistant",
                        UnifiedRole::Tool => "tool",
                    }
                    .to_string()
                });

                let mut content = String::new();
                let mut tool_calls = Vec::new();

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => content.push_str(&text),
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Openai),
                                TransformValueKind::ImageDelta,
                                "Dropping unsupported image delta from OpenAI stream conversion.",
                            );
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tc) => {
                            tool_calls.push(OpenAiChunkToolCall {
                                index: tc.index,
                                id: tc.id,
                                type_: Some("function".to_string()),
                                function: OpenAiChunkFunction {
                                    name: tc.name,
                                    arguments: tc.arguments,
                                },
                            });
                        }
                    }
                }

                let delta = OpenAiChunkDelta {
                    role,
                    content: if content.is_empty() {
                        None
                    } else {
                        Some(content)
                    },
                    reasoning_content: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    refusal: None,
                    name: None,
                };

                OpenAiChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        OpenAiChunkResponse {
            id: unified_chunk.id,
            object: unified_chunk
                .object
                .unwrap_or_else(|| "chat.completion.chunk".to_string()),
            created: unified_chunk
                .created
                .unwrap_or_else(|| Utc::now().timestamp()),
            model: unified_chunk.model.unwrap_or_default(),
            system_fingerprint: None,
            choices,
            usage: unified_chunk.usage.map(|u| u.into()),
        }
    }
}

impl From<OpenAiChunkResponse> for UnifiedChunkResponse {
    fn from(openai_chunk: OpenAiChunkResponse) -> Self {
        let choices = openai_chunk
            .choices
            .into_iter()
            .map(|choice| {
                let role = choice.delta.role.map(|r| match r.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User,
                });

                let mut content = Vec::new();

                if let Some(text) = choice.delta.content {
                    if !text.is_empty() {
                        // Index 0 for text content for now
                        content.push(UnifiedContentPartDelta::TextDelta { index: 0, text });
                    }
                }

                if let Some(text) = choice.delta.reasoning_content {
                    if !text.is_empty() {
                        content.push(UnifiedContentPartDelta::TextDelta { index: 0, text });
                    }
                }

                if let Some(tool_calls) = choice.delta.tool_calls {
                    for tc in tool_calls {
                        content.push(UnifiedContentPartDelta::ToolCallDelta(
                            UnifiedToolCallDelta {
                                index: tc.index,
                                id: tc.id,
                                name: tc.function.name,
                                arguments: tc.function.arguments,
                            },
                        ));
                    }
                }

                let delta = UnifiedMessageDelta { role, content };

                UnifiedChunkChoice {
                    index: choice.index,
                    delta,
                    finish_reason: choice.finish_reason,
                }
            })
            .collect();

        UnifiedChunkResponse {
            id: openai_chunk.id,
            model: Some(openai_chunk.model),
            choices,
            usage: openai_chunk.usage.map(|u| u.into()),
            created: Some(openai_chunk.created),
            object: Some(openai_chunk.object),
            provider_session_metadata: None,
            synthetic_metadata: None,
        }
    }
}

pub(super) fn openai_chunk_to_unified_stream_events_with_state(
    openai_chunk: OpenAiChunkResponse,
    transformer: &mut StreamTransformer,
) -> Vec<UnifiedStreamEvent> {
    let OpenAiChunkResponse {
        id,
        model,
        choices,
        usage,
        ..
    } = openai_chunk;

    let mut events = Vec::with_capacity(choices.len() * 4 + usize::from(usage.is_some()));

    let mut reasoning_open = transformer.session.current_reasoning_block_index.is_some();
    let mut text_block_index = transformer.session.current_content_block_index;
    let mut reasoning_seen = transformer.session.openai_reasoning_seen;
    let mut active_tool_calls = transformer.session.openai_active_tool_calls.clone();

    for choice in choices {
        if let Some(role) = choice.delta.role {
            events.push(UnifiedStreamEvent::MessageStart {
                id: Some(id.clone()),
                model: Some(model.clone()),
                role: match role.as_str() {
                    "system" => UnifiedRole::System,
                    "user" => UnifiedRole::User,
                    "assistant" => UnifiedRole::Assistant,
                    "tool" => UnifiedRole::Tool,
                    _ => UnifiedRole::User,
                },
            });
        }

        if let Some(reasoning_text) = choice.delta.reasoning_content {
            if !reasoning_text.is_empty() {
                if text_block_index.is_some() {
                    apply_transform_policy(
                        TransformProtocol::Api(LlmApiType::Openai),
                        TransformProtocol::Unified,
                        TransformValueKind::ReasoningDelta,
                        "Dropping OpenAI reasoning delta that arrived after the text block started.",
                    );
                } else {
                    if !reasoning_open {
                        events.push(UnifiedStreamEvent::ReasoningStart { index: 0 });
                        reasoning_open = true;
                        reasoning_seen = true;
                    }
                    events.push(UnifiedStreamEvent::ReasoningDelta {
                        index: 0,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text: reasoning_text,
                    });
                }
            }
        }

        if let Some(text) = choice.delta.content {
            if !text.is_empty() {
                if reasoning_open {
                    events.push(UnifiedStreamEvent::ReasoningStop { index: 0 });
                    reasoning_open = false;
                }

                let index = if reasoning_seen { 1 } else { 0 };
                if text_block_index != Some(index) {
                    events.push(UnifiedStreamEvent::ContentBlockStart {
                        index,
                        kind: UnifiedBlockKind::Text,
                    });
                    text_block_index = Some(index);
                }
                events.push(UnifiedStreamEvent::ContentBlockDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text,
                });
            }
        }

        if let Some(tool_calls) = choice.delta.tool_calls {
            for tool_call in tool_calls {
                let OpenAiChunkToolCall {
                    index,
                    id,
                    function,
                    ..
                } = tool_call;
                let OpenAiChunkFunction { name, arguments } = function;

                if let (Some(id), Some(name)) = (id.clone(), name.clone()) {
                    active_tool_calls.insert(index, id.clone());
                    events.push(UnifiedStreamEvent::ToolCallStart { index, id, name });
                }

                if let Some(arguments) = arguments {
                    if let Some(id) = id.clone() {
                        active_tool_calls.insert(index, id);
                    }
                    events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        id,
                        name,
                        arguments,
                    });
                }
            }
        }

        if choice.finish_reason.is_some() {
            if reasoning_open {
                events.push(UnifiedStreamEvent::ReasoningStop { index: 0 });
                reasoning_open = false;
            }
            if let Some(index) = text_block_index.take() {
                events.push(UnifiedStreamEvent::ContentBlockStop { index });
            }
            if choice.finish_reason.as_deref() == Some("tool_calls") {
                let mut tool_call_indices: Vec<u32> = active_tool_calls.keys().copied().collect();
                tool_call_indices.sort_unstable();
                for tool_call_index in tool_call_indices {
                    let tool_call_id = active_tool_calls.remove(&tool_call_index);
                    events.push(UnifiedStreamEvent::ToolCallStop {
                        index: tool_call_index,
                        id: tool_call_id,
                    });
                }
            }
            events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: choice.finish_reason,
            });
        }
    }

    if let Some(usage) = usage {
        events.push(UnifiedStreamEvent::Usage {
            usage: usage.into(),
        });
    }

    events
}

pub(super) fn transform_unified_stream_events_to_openai_events(
    stream_events: Vec<UnifiedStreamEvent>,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut transformed = Vec::new();

    for event in stream_events {
        if let Some(event) = transform_unified_stream_event_to_openai_event(event, transformer) {
            transformed.push(event);
        }
    }

    if transformed.is_empty() {
        None
    } else {
        Some(transformed)
    }
}

pub(super) fn transform_unified_stream_event_to_openai_event(
    event: UnifiedStreamEvent,
    transformer: &mut StreamTransformer,
) -> Option<SseEvent> {
    let id = transformer.get_or_generate_stream_id();
    let model = transformer.get_or_default_stream_model();
    let created = Utc::now().timestamp();

    match event {
        UnifiedStreamEvent::MessageStart { role, .. } => {
            serde_json::to_string(&OpenAiChunkResponse {
                id,
                object: "chat.completion.chunk".to_string(),
                created,
                model,
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: Some(
                            match role {
                                UnifiedRole::System => "system",
                                UnifiedRole::User => "user",
                                UnifiedRole::Assistant => "assistant",
                                UnifiedRole::Tool => "tool",
                            }
                            .to_string(),
                        ),
                        content: None,
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            })
        }
        UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
            serde_json::to_string(&OpenAiChunkResponse {
                id,
                object: "chat.completion.chunk".to_string(),
                created,
                model,
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: Some(text),
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            })
        }
        UnifiedStreamEvent::ToolCallStart {
            index,
            id: tool_id,
            name,
        } => serde_json::to_string(&OpenAiChunkResponse {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: Some(vec![OpenAiChunkToolCall {
                        index,
                        id: Some(tool_id),
                        type_: Some("function".to_string()),
                        function: OpenAiChunkFunction {
                            name: Some(name),
                            arguments: None,
                        },
                    }]),
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        })
        .ok()
        .map(|data| SseEvent {
            data,
            ..Default::default()
        }),
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index: _,
            item_id: _,
            id: tool_id,
            name,
            arguments,
        } => serde_json::to_string(&OpenAiChunkResponse {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: Some(vec![OpenAiChunkToolCall {
                        index,
                        id: tool_id,
                        type_: Some("function".to_string()),
                        function: OpenAiChunkFunction {
                            name,
                            arguments: Some(arguments),
                        },
                    }]),
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        })
        .ok()
        .map(|data| SseEvent {
            data,
            ..Default::default()
        }),
        UnifiedStreamEvent::MessageDelta { finish_reason } => {
            serde_json::to_string(&OpenAiChunkResponse {
                id,
                object: "chat.completion.chunk".to_string(),
                created,
                model,
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: None,
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason,
                    logprobs: None,
                }],
                usage: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            })
        }
        UnifiedStreamEvent::Usage { usage } => serde_json::to_string(&OpenAiChunkResponse {
            id,
            object: "chat.completion.chunk".to_string(),
            created,
            model,
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: None,
                    content: None,
                    reasoning_content: None,
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: Some(usage.into()),
        })
        .ok()
        .map(|data| SseEvent {
            data,
            ..Default::default()
        }),
        UnifiedStreamEvent::ReasoningStart { index } => Some(build_openai_stream_diagnostic(
            transformer,
            TransformValueKind::ReasoningDelta,
            format!(
                "OpenAI chat completion chunks do not expose a native reasoning_start event; index={index} was downgraded to a structured transform diagnostic."
            ),
        )),
        UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
            Some(build_openai_stream_diagnostic(
                transformer,
                TransformValueKind::ReasoningDelta,
                format!(
                    "OpenAI chat completion chunks do not expose a native reasoning delta; index={index}, chars={} was downgraded to a structured transform diagnostic.",
                    text.chars().count()
                ),
            ))
        }
        UnifiedStreamEvent::ReasoningStop { index } => Some(build_openai_stream_diagnostic(
            transformer,
            TransformValueKind::ReasoningDelta,
            format!(
                "OpenAI chat completion chunks do not expose a native reasoning_stop event; index={index} was downgraded to a structured transform diagnostic."
            ),
        )),
        UnifiedStreamEvent::BlobDelta { index, data } => Some(build_openai_stream_diagnostic(
            transformer,
            TransformValueKind::BlobDelta,
            format!(
                "OpenAI chat completion chunks do not expose a native blob delta; index={index:?}, json_type={} was downgraded to a structured transform diagnostic.",
                match &data {
                    Value::Null => "null",
                    Value::Bool(_) => "bool",
                    Value::Number(_) => "number",
                    Value::String(_) => "string",
                    Value::Array(_) => "array",
                    Value::Object(_) => "object",
                }
            ),
        )),
        UnifiedStreamEvent::Error { error } => Some(SseEvent {
            event: Some("error".to_string()),
            data: serde_json::to_string(&error).unwrap_or_else(|_| {
                "{\"type\":\"transform_error\",\"message\":\"serialization failure\"}".to_string()
            }),
            ..Default::default()
        }),
        UnifiedStreamEvent::ItemAdded { .. }
        | UnifiedStreamEvent::ItemDone { .. }
        | UnifiedStreamEvent::MessageStop
        | UnifiedStreamEvent::ContentPartAdded { .. }
        | UnifiedStreamEvent::ContentPartDone { .. }
        | UnifiedStreamEvent::ContentBlockStart { .. }
        | UnifiedStreamEvent::ContentBlockStop { .. }
        | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
        | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
        | UnifiedStreamEvent::ToolCallStop { .. } => None,
    }
}

pub(super) fn transform_unified_chunk_to_openai_events(
    mut unified_chunk: UnifiedChunkResponse,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for choice in &mut unified_chunk.choices {
        let mut filtered = Vec::new();
        for part in std::mem::take(&mut choice.delta.content) {
            match part {
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    events.push(build_openai_stream_diagnostic(
                        transformer,
                        TransformValueKind::ImageDelta,
                        format!(
                            "OpenAI chat completion chunks do not expose native image deltas; index={index}, has_url={}, has_data={} was downgraded to a structured transform diagnostic.",
                            url.as_ref().is_some_and(|value| !value.is_empty()),
                            data.as_ref().is_some_and(|value| !value.is_empty())
                        ),
                    ));
                }
                other => filtered.push(other),
            }
        }
        choice.delta.content = filtered;
    }

    let has_chunk_payload = unified_chunk.usage.is_some()
        || unified_chunk.choices.iter().any(|choice| {
            choice.delta.role.is_some()
                || !choice.delta.content.is_empty()
                || choice.finish_reason.is_some()
        });

    if has_chunk_payload {
        if let Ok(data) = serde_json::to_string(&OpenAiChunkResponse::from(unified_chunk)) {
            events.push(SseEvent {
                data,
                ..Default::default()
            });
        }
    }

    (!events.is_empty()).then_some(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_openai_request_to_unified() {
        let openai_req = OpenAiRequestPayload {
            model: "gpt-4".to_string(),
            messages: vec![
                OpenAiMessage {
                    role: "system".to_string(),
                    content: Some(OpenAiContent::Text(
                        "You are a helpful assistant.".to_string(),
                    )),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: None,
                },
                OpenAiMessage {
                    role: "user".to_string(),
                    content: Some(OpenAiContent::Text("Hello".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: None,
                },
            ],
            tools: None,
            tool_choice: None,
            stream: Some(false),
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(OpenAiStop::String("stop".to_string())),
            n: None,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            logit_bias: None,
            logprobs: None,
            top_logprobs: None,
            response_format: None,
            user: None,
            parallel_tool_calls: None,
            reasoning_effort: None,
        };

        let unified_req: UnifiedRequest = openai_req.into();

        assert_eq!(unified_req.model, Some("gpt-4".to_string()));
        assert_eq!(unified_req.messages.len(), 2);
        assert_eq!(unified_req.messages[0].role, UnifiedRole::System);
        assert_eq!(
            unified_req.messages[0].content,
            vec![UnifiedContentPart::Text {
                text: "You are a helpful assistant.".to_string()
            }]
        );
        assert_eq!(unified_req.messages[1].role, UnifiedRole::User);
        assert_eq!(
            unified_req.messages[1].content,
            vec![UnifiedContentPart::Text {
                text: "Hello".to_string()
            }]
        );
        assert_eq!(unified_req.temperature, Some(0.8));
        assert_eq!(unified_req.max_tokens, Some(100));
        assert_eq!(unified_req.top_p, Some(0.9));
        assert_eq!(unified_req.stop, Some(vec!["stop".to_string()]));
        assert!(unified_req.openai_extension().is_none());
    }

    #[test]
    fn test_unified_request_to_openai() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4".to_string()),
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text {
                        text: "You are a helpful assistant.".to_string(),
                    }],
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![UnifiedContentPart::Text {
                        text: "Hello".to_string(),
                    }],
                },
            ],
            tools: None,
            stream: false,
            temperature: Some(0.8),
            max_tokens: Some(100),
            top_p: Some(0.9),
            stop: Some(vec!["stop".to_string()]),
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            ..Default::default()
        };

        let openai_req: OpenAiRequestPayload = unified_req.into();

        assert_eq!(openai_req.model, "gpt-4".to_string());
        assert_eq!(openai_req.messages.len(), 2);
        assert_eq!(openai_req.messages[0].role, "system");
        match openai_req.messages[0].content.as_ref().unwrap() {
            OpenAiContent::Text(t) => assert_eq!(t, "You are a helpful assistant."),
            _ => panic!("Expected text content"),
        }
        assert_eq!(openai_req.messages[1].role, "user");
        match openai_req.messages[1].content.as_ref().unwrap() {
            OpenAiContent::Text(t) => assert_eq!(t, "Hello"),
            _ => panic!("Expected text content"),
        }
        assert_eq!(openai_req.temperature, Some(0.8));
        assert_eq!(openai_req.max_tokens, Some(100));
        assert_eq!(openai_req.top_p, Some(0.9));
        match openai_req.stop.as_ref().unwrap() {
            OpenAiStop::String(s) => assert_eq!(s, "stop"),
            _ => panic!("Expected string stop"),
        }
    }

    #[test]
    fn test_unified_request_to_openai_preserves_reasoning_as_text() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "Question".to_string(),
                    },
                    UnifiedContentPart::Reasoning {
                        text: "hidden reasoning".to_string(),
                    },
                ],
            }],
            ..Default::default()
        };

        let openai_req: OpenAiRequestPayload = unified_req.into();
        match openai_req.messages[0].content.as_ref().unwrap() {
            OpenAiContent::Parts(parts) => {
                assert_eq!(parts.len(), 2);
                assert!(matches!(
                    &parts[1],
                    OpenAiContentPart::Text { text } if text == "hidden reasoning"
                ));
            }
            other => panic!("Expected multipart OpenAI content, got {:?}", other),
        }
    }

    #[test]
    fn test_unified_request_to_openai_preserves_image_data_file_and_code() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::ImageData {
                        mime_type: "image/png".to_string(),
                        data: "ZmFrZQ==".to_string(),
                    },
                    UnifiedContentPart::FileUrl {
                        url: "https://files.example.com/report.pdf".to_string(),
                        mime_type: Some("application/pdf".to_string()),
                        filename: None,
                    },
                    UnifiedContentPart::ExecutableCode {
                        language: "python".to_string(),
                        code: "print(1)".to_string(),
                    },
                ],
            }],
            ..Default::default()
        };

        let openai_req: OpenAiRequestPayload = unified_req.into();
        match openai_req.messages[0].content.as_ref().unwrap() {
            OpenAiContent::Parts(parts) => {
                assert!(matches!(
                    &parts[0],
                    OpenAiContentPart::ImageUrl { image_url }
                    if image_url.url == "data:image/png;base64,ZmFrZQ==" && image_url.detail.as_deref() == Some("auto")
                ));
                assert!(matches!(
                    &parts[1],
                    OpenAiContentPart::Text { text }
                    if text == "file_url: https://files.example.com/report.pdf\nmime_type: application/pdf"
                ));
                assert!(matches!(
                    &parts[2],
                    OpenAiContentPart::Text { text }
                    if text == "```python\nprint(1)\n```"
                ));
            }
            other => panic!("Expected multipart OpenAI content, got {:?}", other),
        }
    }

    #[test]
    fn test_openai_response_to_unified() {
        let openai_res = OpenAiResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAiContent::Text("Hi there!".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: None,
                },
                logprobs: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(OpenAiUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
                total_tokens: 30,
                completion_tokens_details: None,
                prompt_tokens_details: None,
            }),
        };

        let unified_res: UnifiedResponse = openai_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(
            choice.message.content,
            vec![UnifiedContentPart::Text {
                text: "Hi there!".to_string()
            }]
        );
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        assert!(unified_res.usage.is_some());
        let usage = unified_res.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_unified_response_to_openai() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: Some("gpt-4".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "Hi there!".to_string(),
                    }],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 20,
                total_tokens: 30,
                ..Default::default()
            }),
            created: Some(12345),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let openai_res: OpenAiResponse = unified_res.into();

        assert_eq!(openai_res.choices.len(), 1);
        let choice = &openai_res.choices[0];
        assert_eq!(choice.message.role, "assistant");
        match choice.message.content.as_ref().unwrap() {
            OpenAiContent::Text(t) => assert_eq!(t, "Hi there!"),
            _ => panic!("Expected text content"),
        }
        assert_eq!(choice.finish_reason, Some("stop".to_string()));
        assert!(openai_res.usage.is_some());
        let usage = openai_res.usage.unwrap();
        assert_eq!(usage.prompt_tokens, 10);
        assert_eq!(usage.completion_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
    }

    #[test]
    fn test_openai_response_to_unified_promotes_refusal() {
        let openai_res = OpenAiResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChoice {
                index: 0,
                message: OpenAiMessage {
                    role: "assistant".to_string(),
                    content: Some(OpenAiContent::Text("safe answer".to_string())),
                    tool_calls: None,
                    name: None,
                    tool_call_id: None,
                    refusal: Some("cannot comply".to_string()),
                },
                logprobs: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        };

        let unified_res: UnifiedResponse = openai_res.into();

        assert!(matches!(
            &unified_res.choices[0].message.content[..],
            [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
            if text == "cannot comply" && answer == "safe answer"
        ));
    }

    #[test]
    fn test_unified_response_to_openai_preserves_refusal_field() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: Some("gpt-4".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![
                        UnifiedContentPart::Refusal {
                            text: "cannot comply".to_string(),
                        },
                        UnifiedContentPart::Text {
                            text: "safe answer".to_string(),
                        },
                    ],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(12345),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let openai_res: OpenAiResponse = unified_res.into();

        assert_eq!(
            openai_res.choices[0].message.refusal.as_deref(),
            Some("cannot comply")
        );
        match openai_res.choices[0].message.content.as_ref().unwrap() {
            OpenAiContent::Text(text) => assert_eq!(text, "safe answer"),
            other => panic!("Expected text content, got {:?}", other),
        }
    }

    #[test]
    fn test_openai_chunk_to_unified() {
        let openai_chunk = OpenAiChunkResponse {
            id: "chatcmpl-123".to_string(),
            object: "chat.completion.chunk".to_string(),
            created: 12345,
            model: "gpt-4".to_string(),
            system_fingerprint: None,
            choices: vec![OpenAiChunkChoice {
                index: 0,
                delta: OpenAiChunkDelta {
                    role: Some("assistant".to_string()),
                    content: Some("Hello".to_string()),
                    reasoning_content: None,
                    tool_calls: None,
                    refusal: None,
                    name: None,
                },
                finish_reason: None,
                logprobs: None,
            }],
            usage: None,
        };

        let unified_chunk: UnifiedChunkResponse = openai_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));
        assert_eq!(
            choice.delta.content,
            vec![UnifiedContentPartDelta::TextDelta {
                index: 0,
                text: "Hello".to_string()
            }]
        );
        assert!(choice.finish_reason.is_none());
    }

    #[test]
    fn test_openai_chunk_to_unified_stream_events_with_reasoning_and_text() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);

        let reasoning_events = openai_chunk_to_unified_stream_events_with_state(
            OpenAiChunkResponse {
                id: "chatcmpl-123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: 12345,
                model: "gpt-4".to_string(),
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: Some("assistant".to_string()),
                        content: Some(String::new()),
                        reasoning_content: Some("step one".to_string()),
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            },
            &mut transformer,
        );

        assert_eq!(
            reasoning_events,
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("chatcmpl-123".to_string()),
                    model: Some("gpt-4".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ReasoningStart { index: 0 },
                UnifiedStreamEvent::ReasoningDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "step one".to_string(),
                },
            ]
        );
        transformer.update_session_from_stream_events(&reasoning_events);

        let text_events = openai_chunk_to_unified_stream_events_with_state(
            OpenAiChunkResponse {
                id: "chatcmpl-123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: 12346,
                model: "gpt-4".to_string(),
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: Some("Hello".to_string()),
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            },
            &mut transformer,
        );

        assert_eq!(
            text_events,
            vec![
                UnifiedStreamEvent::ReasoningStop { index: 0 },
                UnifiedStreamEvent::ContentBlockStart {
                    index: 1,
                    kind: UnifiedBlockKind::Text,
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 1,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "Hello".to_string(),
                },
            ]
        );
        transformer.update_session_from_stream_events(&text_events);

        let finish_events = openai_chunk_to_unified_stream_events_with_state(
            OpenAiChunkResponse {
                id: "chatcmpl-123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: 12347,
                model: "gpt-4".to_string(),
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: Some(String::new()),
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: Some("stop".to_string()),
                    logprobs: None,
                }],
                usage: None,
            },
            &mut transformer,
        );

        assert_eq!(
            finish_events,
            vec![
                UnifiedStreamEvent::ContentBlockStop { index: 1 },
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("stop".to_string()),
                },
            ]
        );
    }

    #[test]
    fn test_openai_chunk_to_unified_stream_events_drops_late_reasoning_after_text_started() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Anthropic);
        transformer.session.current_content_block_index = Some(0);

        let events = openai_chunk_to_unified_stream_events_with_state(
            OpenAiChunkResponse {
                id: "chatcmpl-123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: 12345,
                model: "gpt-4".to_string(),
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: None,
                        reasoning_content: Some("too late".to_string()),
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            },
            &mut transformer,
        );

        assert!(events.is_empty());
    }

    #[test]
    fn test_openai_chunk_to_unified_stream_events_emits_tool_call_stop_on_tool_calls_finish() {
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);

        let start_events = openai_chunk_to_unified_stream_events_with_state(
            OpenAiChunkResponse {
                id: "chatcmpl-123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: 12345,
                model: "gpt-4".to_string(),
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: Some("assistant".to_string()),
                        content: None,
                        reasoning_content: None,
                        tool_calls: Some(vec![OpenAiChunkToolCall {
                            index: 0,
                            id: Some("call_1".to_string()),
                            type_: Some("function".to_string()),
                            function: OpenAiChunkFunction {
                                name: Some("search_web".to_string()),
                                arguments: Some("{".to_string()),
                            },
                        }]),
                        refusal: None,
                        name: None,
                    },
                    finish_reason: None,
                    logprobs: None,
                }],
                usage: None,
            },
            &mut transformer,
        );

        assert_eq!(
            start_events,
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("chatcmpl-123".to_string()),
                    model: Some("gpt-4".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_1".to_string(),
                    name: "search_web".to_string(),
                },
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    id: Some("call_1".to_string()),
                    name: Some("search_web".to_string()),
                    arguments: "{".to_string(),
                },
            ]
        );
        transformer.update_session_from_stream_events(&start_events);

        let finish_events = openai_chunk_to_unified_stream_events_with_state(
            OpenAiChunkResponse {
                id: "chatcmpl-123".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: 12346,
                model: "gpt-4".to_string(),
                system_fingerprint: None,
                choices: vec![OpenAiChunkChoice {
                    index: 0,
                    delta: OpenAiChunkDelta {
                        role: None,
                        content: None,
                        reasoning_content: None,
                        tool_calls: None,
                        refusal: None,
                        name: None,
                    },
                    finish_reason: Some("tool_calls".to_string()),
                    logprobs: None,
                }],
                usage: None,
            },
            &mut transformer,
        );

        assert_eq!(
            finish_events,
            vec![
                UnifiedStreamEvent::ToolCallStop {
                    index: 0,
                    id: Some("call_1".to_string()),
                },
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("tool_calls".to_string()),
                },
            ]
        );
    }

    #[test]
    fn test_unified_chunk_to_openai() {
        let unified_chunk = UnifiedChunkResponse {
            id: "chatcmpl-123".to_string(),
            model: Some("gpt-4".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "Hello".to_string(),
                    }],
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(12345),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        };

        let openai_chunk: OpenAiChunkResponse = unified_chunk.into();

        assert_eq!(openai_chunk.choices.len(), 1);
        let choice = &openai_chunk.choices[0];
        assert_eq!(choice.delta.role, Some("assistant".to_string()));
        assert_eq!(choice.delta.content, Some("Hello".to_string()));
        assert!(choice.finish_reason.is_none());
    }

    #[test]
    fn test_transform_unified_chunk_to_openai_events_emits_diagnostic_for_image_delta() {
        let unified_chunk = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![
                        UnifiedContentPartDelta::ImageDelta {
                            index: 1,
                            url: Some("https://example.com/chart.png".to_string()),
                            data: None,
                        },
                        UnifiedContentPartDelta::TextDelta {
                            index: 0,
                            text: "caption".to_string(),
                        },
                    ],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };

        let mut transformer = StreamTransformer::new(LlmApiType::Gemini, LlmApiType::Openai);
        let events = transform_unified_chunk_to_openai_events(unified_chunk, &mut transformer)
            .expect("openai chunk events");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("transform_diagnostic"));
        let diagnostic: Value = serde_json::from_str(&events[0].data).unwrap();
        assert_eq!(diagnostic["semantic_unit"], json!("ImageDelta"));

        let chunk: Value = serde_json::from_str(&events[1].data).unwrap();
        assert_eq!(chunk["choices"][0]["delta"]["content"], json!("caption"));
    }

    #[test]
    fn test_determine_openai_variant_for_vertex_openai_chat_completions() {
        assert_eq!(
            determine_openai_variant(&ProviderType::VertexOpenai, "chat/completions"),
            OpenAiVariant::GeminiCompat
        );
        assert_eq!(
            determine_openai_variant(&ProviderType::Openai, "chat/completions"),
            OpenAiVariant::Standard
        );
        assert_eq!(
            determine_openai_variant(&ProviderType::VertexOpenai, "embeddings"),
            OpenAiVariant::Standard
        );
    }

    #[test]
    fn test_sanitize_openai_request_payload_for_gemini_variant() {
        let mut payload = json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "temperature": 0.2,
            "tools": [],
            "stream": true,
            "stream_options": {"include_usage": true},
            "parallel_tool_calls": true,
            "logprobs": true,
            "user": "user-123"
        });

        let report = sanitize_openai_request_payload(&mut payload, OpenAiVariant::GeminiCompat);

        assert_eq!(
            payload,
            json!({
                "model": "gemini-2.5-pro",
                "messages": [{"role": "user", "content": "hello"}],
                "temperature": 0.2,
                "tools": [],
                "stream": true
            })
        );
        assert_eq!(
            report.removed_fields,
            vec![
                "logprobs".to_string(),
                "parallel_tool_calls".to_string(),
                "stream_options".to_string(),
                "user".to_string()
            ]
        );
        assert!(report.injected_defaults.is_empty());
    }

    #[test]
    fn test_sanitize_openai_request_payload_for_standard_variant_is_noop() {
        let mut payload = json!({
            "model": "gpt-4.1",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true},
            "parallel_tool_calls": true
        });

        let original = payload.clone();
        let report = sanitize_openai_request_payload(&mut payload, OpenAiVariant::Standard);

        assert_eq!(payload, original);
        assert!(report.removed_fields.is_empty());
        assert!(report.injected_defaults.is_empty());
    }

    #[test]
    fn test_resolve_openai_variant_policy_keeps_standard_and_compat_separate() {
        let standard = resolve_openai_variant_policy(&ProviderType::Openai, "chat/completions");
        let compat = resolve_openai_variant_policy(&ProviderType::VertexOpenai, "chat/completions");

        assert_eq!(standard.variant(), OpenAiVariant::Standard);
        assert_eq!(compat.variant(), OpenAiVariant::GeminiCompat);

        let mut standard_payload = json!({
            "model": "gpt-4.1",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true},
            "parallel_tool_calls": true
        });
        let standard_report = standard.sanitize_request_payload(&mut standard_payload);
        assert!(standard_report.removed_fields.is_empty());
        assert_eq!(
            standard_payload,
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}],
                "stream_options": {"include_usage": true},
                "parallel_tool_calls": true
            })
        );

        let mut compat_payload = json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true},
            "parallel_tool_calls": true
        });
        let compat_report = compat.sanitize_request_payload(&mut compat_payload);
        assert_eq!(
            compat_report.removed_fields,
            vec![
                "parallel_tool_calls".to_string(),
                "stream_options".to_string()
            ]
        );
        assert_eq!(
            compat_payload,
            json!({
                "model": "gemini-2.5-pro",
                "messages": [{"role": "user", "content": "hello"}]
            })
        );
    }

    #[test]
    fn test_finalize_openai_compatible_request_payload_uses_variant_layer_only() {
        let mut payload = json!({
            "model": "gemini-2.5-pro",
            "messages": [{"role": "user", "content": "hello"}],
            "stream_options": {"include_usage": true},
            "user": "user-123"
        });

        let (variant, report) = finalize_openai_compatible_request_payload(
            &mut payload,
            &ProviderType::VertexOpenai,
            "chat/completions",
        );

        assert_eq!(variant, OpenAiVariant::GeminiCompat);
        assert_eq!(
            report.removed_fields,
            vec!["stream_options".to_string(), "user".to_string()]
        );
        assert_eq!(
            payload,
            json!({
                "model": "gemini-2.5-pro",
                "messages": [{"role": "user", "content": "hello"}]
            })
        );
    }

    #[test]
    fn test_build_registered_passthrough_filters_unregistered_keys() {
        let passthrough = build_registered_passthrough(
            vec![
                ("logprobs".to_string(), json!(true)),
                ("future_field".to_string(), json!("blocked")),
            ],
            "test_passthrough_registry",
        )
        .unwrap();

        assert_eq!(passthrough, json!({ "logprobs": true }));
    }

    #[test]
    fn test_unified_request_to_openai_ignores_unregistered_passthrough_keys() {
        let request = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "hello".to_string(),
                }],
            }],
            extensions: Some(UnifiedRequestExtensions {
                openai: Some(UnifiedOpenAiRequestExtension {
                    passthrough: Some(json!({
                        "parallel_tool_calls": true,
                        "future_field": "blocked"
                    })),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let openai_request = OpenAiRequestPayload::from(request);

        assert_eq!(openai_request.parallel_tool_calls, Some(true));
        assert_eq!(openai_request.logprobs, None);
        assert_eq!(openai_request.top_logprobs, None);
        assert!(openai_request.reasoning_effort.is_none());
    }
}
