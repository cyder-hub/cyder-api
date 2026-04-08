use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    StreamTransformer, TransformProtocol, TransformValueKind, apply_transform_policy,
    build_stream_diagnostic_sse, unified::*,
};
use crate::schema::enum_def::LlmApiType;
use crate::utils::ID_GENERATOR;
use crate::utils::sse::SseEvent;

pub(super) fn build_gemini_tool_call_key(
    provider_order: u32,
    message_index: u32,
    part_index: u32,
    function_name: &str,
) -> String {
    format!(
        "provider_order={provider_order}:message_index={message_index}:part_index={part_index}:function_name={function_name}"
    )
}

pub(super) fn build_gemini_synthetic_tool_call_id(
    provider_order: u32,
    message_index: u32,
    part_index: u32,
    function_name: &str,
) -> String {
    let normalized_name: String = function_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();

    format!("gemini-call-{provider_order}-{message_index}-{part_index}-{normalized_name}")
}

fn build_gemini_synthetic_response_id(kind: &str) -> String {
    format!("gemini-{kind}-{}", ID_GENERATOR.generate_id())
}

fn build_gemini_fallback_tool_name(tool_call_id: &str) -> String {
    let normalized: String = tool_call_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("gemini-tool-result-{normalized}")
}

fn build_gemini_stream_diagnostic(
    transformer: &mut StreamTransformer,
    kind: TransformValueKind,
    context: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        transformer,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Gemini),
        kind,
        "gemini_stream_encoding",
        context,
        None,
        Some(
            "Use a Responses or Anthropic target when structured reasoning/blob stream events must remain recoverable.".to_string(),
        ),
    )
}

fn gemini_inline_data_from_blob(value: &Value) -> Option<GeminiInlineData> {
    let object = value.as_object()?;
    let mime_type = object.get("mime_type")?.as_str()?;
    let data = object.get("data")?.as_str()?;
    Some(GeminiInlineData {
        mime_type: mime_type.to_string(),
        data: data.to_string(),
    })
}

fn render_gemini_image_reference_text(url: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("image_url: {url}\ndetail: {detail}"),
        _ => format!("image_url: {url}"),
    }
}

fn render_gemini_inline_file_data_text(
    data: &str,
    mime_type: &str,
    filename: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("file_data: {data}"),
        format!("mime_type: {mime_type}"),
    ];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    lines.join("\n")
}

fn render_gemini_tool_call_text(call: &UnifiedToolCall) -> String {
    format!(
        "tool_call: {}\narguments: {}",
        call.name,
        serde_json::to_string(&call.arguments).unwrap_or_default()
    )
}

fn render_gemini_tool_result_text(result: &UnifiedToolResult) -> String {
    match result.name.as_deref() {
        Some(name) if !name.is_empty() => format!(
            "tool_result: {name}\ntool_call_id: {}\ncontent: {}",
            result.tool_call_id,
            result.legacy_content()
        ),
        _ => format!(
            "tool_result_id: {}\ncontent: {}",
            result.tool_call_id,
            result.legacy_content()
        ),
    }
}

fn gemini_function_response_to_unified_output(response: Value) -> UnifiedToolResultOutput {
    match response {
        Value::Object(object) => {
            if let Some(result) = object.get("result") {
                unified_tool_result_output_from_value(result.clone())
            } else {
                UnifiedToolResultOutput::Json {
                    value: Value::Object(object),
                }
            }
        }
        other => unified_tool_result_output_from_value(other),
    }
}

fn unified_tool_result_to_gemini_response(output: &UnifiedToolResultOutput) -> Value {
    match output {
        UnifiedToolResultOutput::Text { text } => json!({ "result": text }),
        other => unified_tool_result_output_to_value(other),
    }
}

fn build_gemini_synthetic_metadata(
    id: bool,
    model: bool,
    gemini_safety_ratings: bool,
) -> Option<UnifiedSyntheticMetadata> {
    let metadata = UnifiedSyntheticMetadata {
        id,
        model,
        gemini_safety_ratings,
    };

    (metadata.id || metadata.model || metadata.gemini_safety_ratings).then_some(metadata)
}

fn merge_gemini_synthetic_metadata(
    existing: Option<UnifiedSyntheticMetadata>,
    generated: Option<UnifiedSyntheticMetadata>,
) -> Option<UnifiedSyntheticMetadata> {
    match (existing, generated) {
        (Some(existing), Some(generated)) => Some(UnifiedSyntheticMetadata {
            id: existing.id || generated.id,
            model: existing.model || generated.model,
            gemini_safety_ratings: existing.gemini_safety_ratings
                || generated.gemini_safety_ratings,
        }),
        (Some(existing), None) => Some(existing),
        (None, Some(generated)) => Some(generated),
        (None, None) => None,
    }
}

fn build_unified_tool_name_lookup(
    request: &UnifiedRequest,
) -> std::collections::HashMap<String, String> {
    let mut tool_name_by_id = std::collections::HashMap::new();
    for item in request.content_items() {
        match item {
            UnifiedItem::FunctionCall(call) => {
                tool_name_by_id.insert(call.id, call.name);
            }
            UnifiedItem::Message(message) => {
                for part in &message.content {
                    if let UnifiedContentPart::ToolCall(call) = part {
                        tool_name_by_id.insert(call.id.clone(), call.name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    tool_name_by_id
}

fn gemini_inline_data_to_unified_content(inline_data: GeminiInlineData) -> UnifiedContentPart {
    if inline_data.mime_type.starts_with("image/") {
        UnifiedContentPart::ImageData {
            mime_type: inline_data.mime_type,
            data: inline_data.data,
        }
    } else {
        UnifiedContentPart::FileData {
            data: inline_data.data,
            mime_type: inline_data.mime_type,
            filename: None,
        }
    }
}

// --- Gemini to Unified ---

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiRequestPayload {
    contents: Vec<GeminiRequestContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<GeminiSystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<GeminiTools>>,
    #[serde(rename = "generationConfig")]
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GeminiGenerationConfig>,
    #[serde(rename = "safetySettings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_settings: Option<Vec<GeminiSafetySetting>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum GeminiSystemInstruction {
    String(String),
    Object { parts: Vec<GeminiPart> },
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiRequestContent {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    role: Option<String>,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiResponseContent {
    role: String,
    parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub(super) enum GeminiPart {
    Text {
        text: String,
    },
    ExecutableCode {
        #[serde(rename = "executableCode")]
        executable_code: GeminiExecutableCode,
    },
    FunctionCall {
        #[serde(rename = "functionCall")]
        function_call: GeminiFunctionCall,
    },
    FunctionResponse {
        #[serde(rename = "functionResponse")]
        function_response: GeminiFunctionResponse,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: GeminiInlineData,
    },
    FileData {
        #[serde(rename = "fileData")]
        file_data: GeminiFileData,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiExecutableCode {
    language: String,
    code: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiFunctionCall {
    name: String,
    args: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiFunctionResponse {
    name: String,
    response: Value,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiInlineData {
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiFileData {
    mime_type: String,
    file_uri: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct GeminiTools {
    #[serde(rename = "functionDeclarations")]
    function_declarations: Vec<UnifiedFunctionDefinition>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(rename = "maxOutputTokens")]
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(rename = "topP")]
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(rename = "stopSequences")]
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiSafetySetting {
    category: String,
    threshold: String,
}

impl From<GeminiRequestPayload> for UnifiedRequest {
    fn from(gemini_req: GeminiRequestPayload) -> Self {
        let mut messages = Vec::new();
        let mut items = Vec::new();
        let mut tool_call_ids: std::collections::HashMap<
            String,
            std::collections::VecDeque<String>,
        > = std::collections::HashMap::new();

        if let Some(system_instruction) = gemini_req.system_instruction {
            let content = match system_instruction {
                GeminiSystemInstruction::String(text) => text,
                GeminiSystemInstruction::Object { parts } => parts
                    .into_iter()
                    .filter_map(|p| match p {
                        GeminiPart::Text { text } => Some(text),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
            };
            if !content.is_empty() {
                let system_message = UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text {
                        text: content.clone(),
                    }],
                };
                items.extend(legacy_content_to_unified_items(
                    UnifiedRole::System,
                    system_message.content.clone(),
                ));
                messages.push(UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![UnifiedContentPart::Text { text: content }],
                });
            }
        }

        for content_item in gemini_req.contents {
            let role = content_item.role.as_deref().unwrap_or("user");
            let parts = content_item.parts;

            let has_function_call = parts
                .iter()
                .any(|p| matches!(p, GeminiPart::FunctionCall { .. }));
            let has_function_response = parts
                .iter()
                .any(|p| matches!(p, GeminiPart::FunctionResponse { .. }));

            if role == "model" && has_function_call {
                let mut content_parts = Vec::new();
                for p in parts {
                    match p {
                        GeminiPart::FunctionCall { function_call } => {
                            let tool_id = format!("call_{}", ID_GENERATOR.generate_id());
                            tool_call_ids
                                .entry(function_call.name.clone())
                                .or_default()
                                .push_back(tool_id.clone());
                            items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                id: tool_id.clone(),
                                name: function_call.name.clone(),
                                arguments: function_call.args.clone(),
                            }));
                            content_parts.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                                id: tool_id,
                                name: function_call.name,
                                arguments: function_call.args,
                            }));
                        }
                        GeminiPart::ExecutableCode { executable_code } => {
                            items.push(UnifiedItem::Message(UnifiedMessageItem {
                                role: UnifiedRole::Assistant,
                                content: vec![UnifiedContentPart::ExecutableCode {
                                    language: executable_code.language.clone(),
                                    code: executable_code.code.clone(),
                                }],
                                annotations: Vec::new(),
                            }));
                            content_parts.push(UnifiedContentPart::ExecutableCode {
                                language: executable_code.language,
                                code: executable_code.code,
                            });
                        }
                        GeminiPart::Text { text } => {
                            content_parts.push(UnifiedContentPart::Text { text });
                        }
                        _ => {}
                    }
                }
                messages.push(UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: content_parts,
                });
            } else if role == "user" && has_function_response {
                parts
                    .into_iter()
                    .filter_map(|p| match p {
                        GeminiPart::FunctionResponse { function_response } => {
                            Some(function_response)
                        }
                        _ => None,
                    })
                    .for_each(|fr| {
                        let tool_call_id = tool_call_ids
                            .get_mut(&fr.name)
                            .and_then(|ids| ids.pop_front())
                            .unwrap_or_else(|| format!("call_{}", ID_GENERATOR.generate_id()));
                        let output = gemini_function_response_to_unified_output(fr.response);
                        items.push(UnifiedItem::FunctionCallOutput(
                            UnifiedFunctionCallOutputItem {
                                tool_call_id: tool_call_id.clone(),
                                name: Some(fr.name.clone()),
                                output: output.clone(),
                            },
                        ));

                        messages.push(UnifiedMessage {
                            role: UnifiedRole::Tool,
                            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                                tool_call_id,
                                name: Some(fr.name.clone()),
                                output,
                            })],
                        });
                    });
            } else {
                let unified_role = if role == "model" {
                    UnifiedRole::Assistant
                } else {
                    UnifiedRole::User
                };

                let mut content_parts = Vec::new();
                for p in parts {
                    match p {
                        GeminiPart::Text { text } => {
                            if !text.is_empty() {
                                content_parts.push(UnifiedContentPart::Text { text });
                            }
                        }
                        GeminiPart::InlineData { inline_data } => {
                            content_parts.push(gemini_inline_data_to_unified_content(inline_data));
                        }
                        GeminiPart::FileData { file_data } => {
                            content_parts.push(UnifiedContentPart::FileUrl {
                                url: file_data.file_uri,
                                mime_type: Some(file_data.mime_type),
                                filename: None,
                            });
                        }
                        _ => {}
                    }
                }

                if !content_parts.is_empty() {
                    items.extend(legacy_content_to_unified_items(
                        unified_role.clone(),
                        content_parts.clone(),
                    ));
                    messages.push(UnifiedMessage {
                        role: unified_role,
                        content: content_parts,
                    });
                }
            }
        }

        let tools = gemini_req.tools.map(|ts| {
            ts.into_iter()
                .flat_map(|t| t.function_declarations)
                .map(|f| {
                    let mut params = f.parameters;
                    transform_gemini_tool_params_to_openai(&mut params);
                    UnifiedTool {
                        type_: "function".to_string(),
                        function: UnifiedFunctionDefinition {
                            name: f.name,
                            description: f.description,
                            parameters: params,
                        },
                    }
                })
                .collect()
        });

        let (temperature, max_tokens, top_p, stop) =
            if let Some(config) = gemini_req.generation_config {
                (
                    config.temperature,
                    config.max_output_tokens,
                    config.top_p,
                    config.stop_sequences,
                )
            } else {
                (None, None, None, None)
            };

        UnifiedRequest {
            model: None, // Not in Gemini request body
            messages,
            items,
            tools,
            stream: false, // Set by `into_unified_request`
            temperature,
            max_tokens,
            top_p,
            stop,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            ..Default::default()
        }
        .filter_empty() // Filter out empty content and messages
    }
}

impl From<UnifiedRequest> for GeminiRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let tool_name_by_id = build_unified_tool_name_lookup(&unified_req);
        let mut contents = Vec::new();
        let mut system_instruction: Option<GeminiSystemInstruction> = None;

        for msg in unified_req.messages {
            match msg.role {
                UnifiedRole::System => {
                    let system_texts: Vec<String> = msg
                        .content
                        .iter()
                        .filter_map(|part| match part {
                            UnifiedContentPart::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .collect();

                    if !system_texts.is_empty() {
                        // Use object format with parts to match expected test format
                        let parts: Vec<GeminiPart> = system_texts
                            .into_iter()
                            .map(|text| GeminiPart::Text { text })
                            .collect();
                        system_instruction = Some(GeminiSystemInstruction::Object { parts });
                    }
                }
                UnifiedRole::User => {
                    let mut parts = Vec::new();
                    for part in msg.content {
                        match part {
                            UnifiedContentPart::Text { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::Refusal { text } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                let keep = apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading remote image URL to recoverable text during Gemini request conversion.",
                                );
                                if keep {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        ),
                                    });
                                }
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri: url,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ToolCall,
                                    "Downgrading user tool call to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_tool_call_text(&call),
                                    });
                                }
                            }
                            UnifiedContentPart::ToolResult(result) => {
                                let name = result
                                    .name
                                    .clone()
                                    .or_else(|| tool_name_by_id.get(&result.tool_call_id).cloned())
                                    .unwrap_or_else(|| {
                                        build_gemini_fallback_tool_name(&result.tool_call_id)
                                    });
                                parts.push(GeminiPart::FunctionResponse {
                                    function_response: GeminiFunctionResponse {
                                        name,
                                        response: unified_tool_result_to_gemini_response(
                                            &result.output,
                                        ),
                                    },
                                });
                            }
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("user".to_string()),
                            parts,
                        });
                    }
                }
                UnifiedRole::Assistant => {
                    let mut parts = Vec::new();
                    for part in msg.content {
                        match part {
                            UnifiedContentPart::Text { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::Refusal { text } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini assistant conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::FileUrl {
                                url,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri: url,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                parts.push(GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        name: call.name,
                                        args: call.arguments,
                                    },
                                });
                            }
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading assistant image URL to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        ),
                                    });
                                }
                            }
                            UnifiedContentPart::ToolResult(result) => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ToolResult,
                                    "Downgrading assistant tool result to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_tool_result_text(&result),
                                    });
                                }
                            }
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("model".to_string()),
                            parts,
                        });
                    }
                }
                UnifiedRole::Tool => {
                    let mut parts = Vec::new();
                    for part in msg.content {
                        match part {
                            UnifiedContentPart::ToolResult(result) => {
                                let name = result
                                    .name
                                    .or_else(|| tool_name_by_id.get(&result.tool_call_id).cloned())
                                    .unwrap_or_else(|| {
                                        apply_transform_policy(
                                            TransformProtocol::Unified,
                                            TransformProtocol::Api(LlmApiType::Gemini),
                                            TransformValueKind::ToolResult,
                                            "Gemini tool result is missing tool name; using explicit synthetic fallback name derived from tool_call_id.",
                                        );
                                        build_gemini_fallback_tool_name(&result.tool_call_id)
                                    });
                                let response_content =
                                    unified_tool_result_to_gemini_response(&result.output);

                                parts.push(GeminiPart::FunctionResponse {
                                    function_response: GeminiFunctionResponse {
                                        name,
                                        response: response_content,
                                    },
                                });
                            }
                            UnifiedContentPart::Text { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::Refusal { text } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::Refusal,
                                    "Downgrading refusal content to plain text during Gemini tool conversion.",
                                ) {
                                    parts.push(GeminiPart::Text { text });
                                }
                            }
                            UnifiedContentPart::Reasoning { text } => {
                                parts.push(GeminiPart::Text { text });
                            }
                            UnifiedContentPart::ImageData { mime_type, data } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ImageUrl { url, detail } => {
                                if apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Gemini),
                                    TransformValueKind::ImageUrl,
                                    "Downgrading tool message image URL to recoverable text during Gemini request conversion.",
                                ) {
                                    parts.push(GeminiPart::Text {
                                        text: render_gemini_image_reference_text(
                                            &url,
                                            detail.as_deref(),
                                        ),
                                    });
                                }
                            }
                            UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri: url,
                                    },
                                });
                            }
                            UnifiedContentPart::FileData {
                                data,
                                mime_type,
                                filename: _,
                            } => {
                                parts.push(GeminiPart::InlineData {
                                    inline_data: GeminiInlineData { mime_type, data },
                                });
                            }
                            UnifiedContentPart::ExecutableCode { language, code } => {
                                parts.push(GeminiPart::ExecutableCode {
                                    executable_code: GeminiExecutableCode { language, code },
                                });
                            }
                            UnifiedContentPart::ToolCall(call) => {
                                parts.push(GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall {
                                        name: call.name,
                                        args: call.arguments,
                                    },
                                });
                            }
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(GeminiRequestContent {
                            role: Some("user".to_string()), // Gemini expects tool responses under 'user' role
                            parts,
                        });
                    }
                }
            }
        }

        // Gemini has a specific structure for tools.
        let tools = unified_req.tools.map(|tools| {
            let function_declarations = tools.into_iter().map(|tool| tool.function).collect();
            vec![GeminiTools {
                function_declarations,
            }]
        });

        let generation_config = if unified_req.temperature.is_some()
            || unified_req.max_tokens.is_some()
            || unified_req.top_p.is_some()
            || unified_req.stop.is_some()
        {
            Some(GeminiGenerationConfig {
                temperature: unified_req.temperature,
                max_output_tokens: unified_req.max_tokens,
                top_p: unified_req.top_p,
                stop_sequences: unified_req.stop,
            })
        } else {
            None
        };

        GeminiRequestPayload {
            contents,
            system_instruction,
            tools,
            generation_config,
            safety_settings: None,
        }
    }
}

// --- Gemini Response Chunk ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiChunkResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiChunkUsageMetadata>,
    #[serde(skip)]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCandidate {
    #[serde(skip_serializing_if = "Option::is_none")]
    index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<GeminiResponseContent>,
    #[serde(rename = "finishReason")]
    #[serde(skip_serializing_if = "Option::is_none")]
    finish_reason: Option<String>,
    #[serde(rename = "safetyRatings")]
    #[serde(skip_serializing_if = "Option::is_none")]
    safety_ratings: Option<Vec<GeminiSafetyRating>>,
    #[serde(rename = "tokenCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    token_count: Option<u32>,
    #[serde(rename = "citationMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    citation_metadata: Option<GeminiCitationMetadata>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(super) enum Modality {
    ModalityUnspecified,
    Text,
    Image,
    Video,
    Audio,
    Document,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct ModalityTokenCount {
    modality: Modality,
    token_count: u32,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GeminiUsageMetadata {
    prompt_token_count: u32,
    candidates_token_count: u32,
    total_token_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    thoughts_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cached_content_token_count: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_use_prompt_token_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    prompt_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    cache_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    candidates_tokens_details: Vec<ModalityTokenCount>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    tool_use_prompt_tokens_details: Vec<ModalityTokenCount>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiChunkUsageMetadata {
    #[serde(rename = "promptTokenCount")]
    prompt_token_count: u32,
    #[serde(rename = "candidatesTokenCount")]
    #[serde(skip_serializing_if = "Option::is_none")]
    candidates_token_count: Option<u32>,
    #[serde(rename = "totalTokenCount")]
    total_token_count: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiSafetyRating {
    category: String,
    probability: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCitationMetadata {
    citation_sources: Vec<GeminiCitationSource>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiCitationSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    end_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    license: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiPromptFeedback {
    #[serde(skip_serializing_if = "Option::is_none")]
    block_reason: Option<String>,
    safety_ratings: Vec<GeminiSafetyRating>,
}

fn gemini_safety_ratings_to_unified(
    safety_ratings: Option<Vec<GeminiSafetyRating>>,
) -> Vec<UnifiedGeminiSafetyRating> {
    safety_ratings
        .unwrap_or_default()
        .into_iter()
        .map(|rating| UnifiedGeminiSafetyRating {
            category: rating.category,
            probability: rating.probability,
        })
        .collect()
}

fn gemini_citation_metadata_to_unified(
    citation_metadata: Option<GeminiCitationMetadata>,
) -> Option<UnifiedGeminiCitationMetadata> {
    citation_metadata.map(|metadata| UnifiedGeminiCitationMetadata {
        citation_sources: metadata
            .citation_sources
            .into_iter()
            .map(|source| UnifiedGeminiCitationSource {
                start_index: source.start_index,
                end_index: source.end_index,
                uri: source.uri,
                license: source.license,
            })
            .collect(),
    })
}

fn gemini_citation_metadata_to_annotations(
    citation_metadata: Option<GeminiCitationMetadata>,
) -> Vec<UnifiedAnnotation> {
    citation_metadata
        .map(|metadata| {
            metadata
                .citation_sources
                .into_iter()
                .map(|source| {
                    UnifiedAnnotation::Citation(UnifiedCitation {
                        part_index: None,
                        start_index: source.start_index,
                        end_index: source.end_index,
                        url: source.uri,
                        title: None,
                        license: source.license,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn gemini_prompt_feedback_to_unified(
    prompt_feedback: Option<GeminiPromptFeedback>,
) -> Option<UnifiedGeminiPromptFeedback> {
    prompt_feedback.map(|feedback| UnifiedGeminiPromptFeedback {
        block_reason: feedback.block_reason,
        safety_ratings: feedback
            .safety_ratings
            .into_iter()
            .map(|rating| UnifiedGeminiSafetyRating {
                category: rating.category,
                probability: rating.probability,
            })
            .collect(),
    })
}

fn build_gemini_response_metadata(
    prompt_feedback: Option<GeminiPromptFeedback>,
    candidates: &[GeminiCandidate],
) -> Option<UnifiedProviderResponseMetadata> {
    let candidates = candidates
        .iter()
        .map(|candidate| UnifiedGeminiCandidateMetadata {
            index: candidate.index.unwrap_or(0),
            safety_ratings: gemini_safety_ratings_to_unified(candidate.safety_ratings.clone()),
            citation_metadata: gemini_citation_metadata_to_unified(
                candidate.citation_metadata.clone(),
            ),
            token_count: candidate.token_count,
        })
        .filter(|candidate| {
            !candidate.safety_ratings.is_empty()
                || candidate.citation_metadata.is_some()
                || candidate.token_count.is_some()
        })
        .collect::<Vec<_>>();

    let prompt_feedback = gemini_prompt_feedback_to_unified(prompt_feedback);

    if prompt_feedback.is_none() && candidates.is_empty() {
        None
    } else {
        Some(UnifiedProviderResponseMetadata {
            gemini: Some(UnifiedGeminiResponseMetadata {
                prompt_feedback,
                candidates,
            }),
            ..Default::default()
        })
    }
}

fn build_gemini_session_metadata(
    prompt_feedback: Option<GeminiPromptFeedback>,
    candidates: &[GeminiCandidate],
) -> Option<UnifiedProviderSessionMetadata> {
    build_gemini_response_metadata(prompt_feedback, candidates).map(|metadata| {
        UnifiedProviderSessionMetadata {
            gemini: metadata.gemini,
            anthropic: None,
            responses: None,
        }
    })
}

fn unified_safety_ratings_to_gemini(
    safety_ratings: Vec<UnifiedGeminiSafetyRating>,
) -> Option<Vec<GeminiSafetyRating>> {
    let ratings = safety_ratings
        .into_iter()
        .map(|rating| GeminiSafetyRating {
            category: rating.category,
            probability: rating.probability,
        })
        .collect::<Vec<_>>();

    (!ratings.is_empty()).then_some(ratings)
}

fn unified_citation_metadata_to_gemini(
    citation_metadata: Option<UnifiedGeminiCitationMetadata>,
) -> Option<GeminiCitationMetadata> {
    citation_metadata.map(|metadata| GeminiCitationMetadata {
        citation_sources: metadata
            .citation_sources
            .into_iter()
            .map(|source| GeminiCitationSource {
                start_index: source.start_index,
                end_index: source.end_index,
                uri: source.uri,
                license: source.license,
            })
            .collect(),
    })
}

fn unified_annotations_to_gemini_citation_metadata(
    annotations: &[UnifiedAnnotation],
) -> Option<GeminiCitationMetadata> {
    let citation_sources = annotations
        .iter()
        .filter_map(|annotation| match annotation {
            UnifiedAnnotation::Citation(citation) => Some(GeminiCitationSource {
                start_index: citation.start_index,
                end_index: citation.end_index,
                uri: citation.url.clone(),
                license: citation.license.clone(),
            }),
        })
        .collect::<Vec<_>>();

    (!citation_sources.is_empty()).then_some(GeminiCitationMetadata { citation_sources })
}

fn unified_prompt_feedback_to_gemini(
    prompt_feedback: Option<UnifiedGeminiPromptFeedback>,
) -> Option<GeminiPromptFeedback> {
    prompt_feedback.map(|feedback| GeminiPromptFeedback {
        block_reason: feedback.block_reason,
        safety_ratings: feedback
            .safety_ratings
            .into_iter()
            .map(|rating| GeminiSafetyRating {
                category: rating.category,
                probability: rating.probability,
            })
            .collect(),
    })
}

// --- Gemini Response to Unified ---

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct GeminiResponse {
    candidates: Vec<GeminiCandidate>,
    #[serde(rename = "promptFeedback")]
    #[serde(skip_serializing_if = "Option::is_none")]
    prompt_feedback: Option<GeminiPromptFeedback>,
    #[serde(rename = "usageMetadata")]
    #[serde(skip_serializing_if = "Option::is_none")]
    usage_metadata: Option<GeminiUsageMetadata>,
    #[serde(skip)]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl From<GeminiResponse> for UnifiedResponse {
    fn from(gemini_res: GeminiResponse) -> Self {
        let GeminiResponse {
            candidates,
            prompt_feedback,
            usage_metadata,
            synthetic_metadata,
        } = gemini_res;

        let provider_response_metadata =
            build_gemini_response_metadata(prompt_feedback, &candidates);

        let choices = candidates
            .into_iter()
            .map(|candidate| {
                let mut content_parts = Vec::new();
                let mut items = Vec::new();
                let mut role = UnifiedRole::Assistant;
                let mut has_function_call = false;

                if let Some(content) = candidate.content {
                    role = match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User, // Should not happen in a response choice
                        _ => UnifiedRole::Assistant,
                    };

                    let candidate_index = candidate.index.unwrap_or(0);
                    for (part_index, p) in content.parts.into_iter().enumerate() {
                        match p {
                            GeminiPart::Text { text } => {
                                if !text.is_empty() {
                                    content_parts
                                        .push(UnifiedContentPart::Text { text: text.clone() });
                                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                                        role: role.clone(),
                                        content: vec![UnifiedContentPart::Text { text }],
                                        annotations: Vec::new(),
                                    }));
                                }
                            }
                            GeminiPart::InlineData { inline_data } => {
                                let part = gemini_inline_data_to_unified_content(inline_data);
                                content_parts.push(part.clone());
                                items.push(UnifiedItem::Message(UnifiedMessageItem {
                                    role: role.clone(),
                                    content: vec![part],
                                    annotations: Vec::new(),
                                }));
                            }
                            GeminiPart::FileData { file_data } => {
                                let file_part = UnifiedContentPart::FileUrl {
                                    url: file_data.file_uri.clone(),
                                    mime_type: Some(file_data.mime_type.clone()),
                                    filename: None,
                                };
                                content_parts.push(file_part);
                                items.push(UnifiedItem::FileReference(UnifiedFileReferenceItem {
                                    filename: None,
                                    mime_type: Some(file_data.mime_type),
                                    file_url: Some(file_data.file_uri),
                                    file_id: None,
                                }));
                            }
                            GeminiPart::ExecutableCode { executable_code } => {
                                let code_part = UnifiedContentPart::ExecutableCode {
                                    language: executable_code.language,
                                    code: executable_code.code,
                                };
                                content_parts.push(code_part.clone());
                                items.push(UnifiedItem::Message(UnifiedMessageItem {
                                    role: role.clone(),
                                    content: vec![code_part],
                                    annotations: Vec::new(),
                                }));
                            }
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                let id = build_gemini_synthetic_tool_call_id(
                                    candidate_index,
                                    0,
                                    part_index as u32,
                                    &function_call.name,
                                );
                                let tool_call = UnifiedToolCall {
                                    id: id.clone(),
                                    name: function_call.name.clone(),
                                    arguments: function_call.args.clone(),
                                };
                                content_parts.push(UnifiedContentPart::ToolCall(tool_call.clone()));
                                items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                    id,
                                    name: function_call.name,
                                    arguments: function_call.args,
                                }));
                            }
                            GeminiPart::FunctionResponse { function_response } => {
                                let tool_call_id = build_gemini_synthetic_tool_call_id(
                                    candidate_index,
                                    0,
                                    part_index as u32,
                                    &function_response.name,
                                );
                                let output = gemini_function_response_to_unified_output(
                                    function_response.response,
                                );
                                content_parts.push(UnifiedContentPart::ToolResult(
                                    UnifiedToolResult {
                                        tool_call_id: tool_call_id.clone(),
                                        name: Some(function_response.name.clone()),
                                        output: output.clone(),
                                    },
                                ));
                                items.push(UnifiedItem::FunctionCallOutput(
                                    UnifiedFunctionCallOutputItem {
                                        tool_call_id,
                                        name: Some(function_response.name),
                                        output,
                                    },
                                ));
                            }
                        }
                    }
                }

                let message = UnifiedMessage {
                    role,
                    content: content_parts,
                    ..Default::default()
                };

                let items = if message.content.is_empty() {
                    items
                } else {
                    let annotations = candidate
                        .citation_metadata
                        .clone()
                        .map(|metadata| gemini_citation_metadata_to_annotations(Some(metadata)))
                        .unwrap_or_default();
                    if !annotations.is_empty() || items.is_empty() {
                        items.insert(
                            0,
                            UnifiedItem::Message(UnifiedMessageItem {
                                role: message.role.clone(),
                                content: message.content.clone(),
                                annotations,
                            }),
                        );
                    }
                    items
                };

                let finish_reason = candidate.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_gemini_finish_reason_to_openai(
                        &fr,
                        has_function_call,
                    )
                });

                UnifiedChoice {
                    index: candidate.index.unwrap_or(0),
                    message,
                    items,
                    finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        let usage = usage_metadata.map(|u| {
            let mut usage = UnifiedUsage {
                input_tokens: u.prompt_token_count,
                output_tokens: u.candidates_token_count,
                total_tokens: u.total_token_count,
                reasoning_tokens: u.thoughts_token_count,
                cached_tokens: u.cached_content_token_count,
                ..Default::default()
            };

            // Handle image tokens from details
            let input_image_tokens = u
                .prompt_tokens_details
                .iter()
                .find(|d| d.modality == Modality::Image)
                .map(|d| d.token_count);
            if input_image_tokens.is_some() {
                usage.input_image_tokens = input_image_tokens;
            }

            let output_image_tokens = u
                .candidates_tokens_details
                .iter()
                .find(|d| d.modality == Modality::Image)
                .map(|d| d.token_count);
            if output_image_tokens.is_some() {
                usage.output_image_tokens = output_image_tokens;
            }

            usage
        });

        let synthetic_id = true;
        let synthetic_model = false;

        UnifiedResponse {
            id: build_gemini_synthetic_response_id("response"),
            model: None,
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata,
            synthetic_metadata: merge_gemini_synthetic_metadata(
                synthetic_metadata,
                build_gemini_synthetic_metadata(synthetic_id, synthetic_model, false),
            ),
        }
    }
}

impl From<UnifiedResponse> for GeminiResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let gemini_metadata = unified_res
            .provider_response_metadata
            .clone()
            .and_then(|metadata| metadata.gemini);
        let candidates = unified_res
            .choices
            .into_iter()
            .filter_map(|choice| {
                let choice_items = choice.content_items();
                let candidate_metadata = gemini_metadata
                    .as_ref()
                    .and_then(|metadata| {
                        metadata
                            .candidates
                            .iter()
                            .find(|candidate| candidate.index == choice.index)
                    })
                    .cloned()
                    .or_else(|| {
                        choice_items.iter().find_map(|item| match item {
                            UnifiedItem::Message(message) if !message.annotations.is_empty() => {
                                Some(UnifiedGeminiCandidateMetadata {
                                    index: choice.index,
                                    safety_ratings: Vec::new(),
                                    citation_metadata: gemini_citation_metadata_to_unified(
                                        unified_annotations_to_gemini_citation_metadata(
                                            &message.annotations,
                                        ),
                                    ),
                                    token_count: None,
                                })
                            }
                            _ => None,
                        })
                    });
                let response_role = choice_items
                    .iter()
                    .find_map(|item| match item {
                        UnifiedItem::Message(message) => Some(message.role.clone()),
                        _ => None,
                    })
                    .unwrap_or(choice.message.role.clone());
                let role = match response_role {
                    UnifiedRole::Assistant => "model",
                    _ => "user",
                }
                .to_string();

                let mut parts = Vec::new();
                for item in choice_items {
                    match item {
                        UnifiedItem::Message(message) => {
                            for part in message.content {
                                match part {
                                    UnifiedContentPart::Text { text }
                                    | UnifiedContentPart::Reasoning { text }
                                    | UnifiedContentPart::Refusal { text } => {
                                        parts.push(GeminiPart::Text { text });
                                    }
                                    UnifiedContentPart::ImageData { mime_type, data } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData { mime_type, data },
                                        });
                                    }
                                    UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                        parts.push(GeminiPart::FileData {
                                            file_data: GeminiFileData {
                                                mime_type: mime_type.unwrap_or_else(|| {
                                                    "application/octet-stream".to_string()
                                                }),
                                                file_uri: url,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::FileData {
                                        data, mime_type, ..
                                    } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData { mime_type, data },
                                        });
                                    }
                                    UnifiedContentPart::ExecutableCode { language, code } => {
                                        parts.push(GeminiPart::ExecutableCode {
                                            executable_code: GeminiExecutableCode {
                                                language,
                                                code,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::ToolCall(call) => {
                                        parts.push(GeminiPart::FunctionCall {
                                            function_call: GeminiFunctionCall {
                                                name: call.name,
                                                args: call.arguments,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::ToolResult(result) => {
                                        let name = result.name.unwrap_or_else(|| {
                                            build_gemini_fallback_tool_name(&result.tool_call_id)
                                        });
                                        parts.push(GeminiPart::FunctionResponse {
                                            function_response: GeminiFunctionResponse {
                                                name,
                                                response: unified_tool_result_to_gemini_response(
                                                    &result.output,
                                                ),
                                            },
                                        });
                                    }
                                    UnifiedContentPart::ImageUrl { url, detail } => {
                                        parts.push(GeminiPart::Text {
                                            text: render_gemini_image_reference_text(
                                                &url,
                                                detail.as_deref(),
                                            ),
                                        });
                                    }
                                }
                            }
                        }
                        UnifiedItem::Reasoning(reasoning) => {
                            for part in reasoning.content {
                                match part {
                                    UnifiedContentPart::Reasoning { text }
                                    | UnifiedContentPart::Text { text }
                                    | UnifiedContentPart::Refusal { text } => {
                                        parts.push(GeminiPart::Text { text });
                                    }
                                    UnifiedContentPart::ExecutableCode { language, code } => {
                                        parts.push(GeminiPart::ExecutableCode {
                                            executable_code: GeminiExecutableCode {
                                                language,
                                                code,
                                            },
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        UnifiedItem::FunctionCall(call) => {
                            parts.push(GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    name: call.name,
                                    args: call.arguments,
                                },
                            });
                        }
                        UnifiedItem::FunctionCallOutput(output) => {
                            let name = output.name.unwrap_or_else(|| {
                                build_gemini_fallback_tool_name(&output.tool_call_id)
                            });
                            parts.push(GeminiPart::FunctionResponse {
                                function_response: GeminiFunctionResponse {
                                    name,
                                    response: unified_tool_result_to_gemini_response(
                                        &output.output,
                                    ),
                                },
                            });
                        }
                        UnifiedItem::FileReference(file) => {
                            if let Some(file_uri) = file.file_url {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: file.mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri,
                                    },
                                });
                            }
                        }
                    }
                }

                let content = if parts.is_empty() {
                    None
                } else {
                    Some(GeminiResponseContent { role, parts })
                };

                let finish_reason = choice.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_openai_finish_reason_to_gemini(&fr)
                });

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        index: Some(choice.index),
                        content,
                        finish_reason,
                        safety_ratings: candidate_metadata.as_ref().and_then(|metadata| {
                            unified_safety_ratings_to_gemini(metadata.safety_ratings.clone())
                        }),
                        token_count: candidate_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.token_count),
                        citation_metadata: candidate_metadata.and_then(|metadata| {
                            unified_citation_metadata_to_gemini(metadata.citation_metadata)
                        }),
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_res.usage.map(|u| {
            let mut prompt_tokens_details = vec![];
            let text_prompt_tokens = u
                .input_tokens
                .saturating_sub(u.input_image_tokens.unwrap_or(0));
            if text_prompt_tokens > 0 {
                prompt_tokens_details.push(ModalityTokenCount {
                    modality: Modality::Text,
                    token_count: text_prompt_tokens,
                });
            }
            if let Some(token_count) = u.input_image_tokens {
                if token_count > 0 {
                    prompt_tokens_details.push(ModalityTokenCount {
                        modality: Modality::Image,
                        token_count,
                    });
                }
            }

            let mut candidates_tokens_details = vec![];
            let text_candidates_tokens = u
                .output_tokens
                .saturating_sub(u.output_image_tokens.unwrap_or(0));
            if text_candidates_tokens > 0 {
                candidates_tokens_details.push(ModalityTokenCount {
                    modality: Modality::Text,
                    token_count: text_candidates_tokens,
                });
            }
            if let Some(token_count) = u.output_image_tokens {
                if token_count > 0 {
                    candidates_tokens_details.push(ModalityTokenCount {
                        modality: Modality::Image,
                        token_count,
                    });
                }
            }

            GeminiUsageMetadata {
                prompt_token_count: u.input_tokens,
                candidates_token_count: u.output_tokens,
                total_token_count: u.total_tokens,
                thoughts_token_count: u.reasoning_tokens,
                cached_content_token_count: u.cached_tokens,
                tool_use_prompt_token_count: None,
                prompt_tokens_details,
                candidates_tokens_details,
                cache_tokens_details: vec![],
                tool_use_prompt_tokens_details: vec![],
            }
        });

        GeminiResponse {
            candidates,
            prompt_feedback: gemini_metadata
                .and_then(|metadata| unified_prompt_feedback_to_gemini(metadata.prompt_feedback)),
            usage_metadata,
            synthetic_metadata: unified_res.synthetic_metadata,
        }
    }
}

impl From<UnifiedChunkResponse> for GeminiChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let synthetic_metadata = unified_chunk.synthetic_metadata.clone();
        let gemini_metadata = unified_chunk
            .provider_session_metadata
            .clone()
            .and_then(|metadata| metadata.gemini);
        let candidates = unified_chunk
            .choices
            .into_iter()
            .filter_map(|choice| {
                let candidate_metadata = gemini_metadata
                    .as_ref()
                    .and_then(|metadata| {
                        metadata
                            .candidates
                            .iter()
                            .find(|candidate| candidate.index == choice.index)
                    })
                    .cloned();
                let mut parts = Vec::new();
                let mut role = "model".to_string(); // Default role for Gemini assistant messages

                if let Some(r) = choice.delta.role {
                    role = match r {
                        UnifiedRole::Assistant => "model".to_string(),
                        UnifiedRole::User => "user".to_string(),
                        // System and Tool roles don't map directly to Gemini chunk roles,
                        // so we'll default to model.
                        _ => "model".to_string(),
                    };
                }

                for part in choice.delta.content {
                    match part {
                        UnifiedContentPartDelta::TextDelta { text, .. } => {
                            parts.push(GeminiPart::Text { text });
                        }
                        UnifiedContentPartDelta::ImageDelta { .. } => {
                            apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Gemini),
                                TransformValueKind::ImageDelta,
                                "Dropping unsupported image delta from Gemini stream conversion.",
                            );
                        }
                        UnifiedContentPartDelta::ToolCallDelta(tc) => {
                            // Gemini doesn't stream partial tool calls in the same way,
                            // but we can try to construct a FunctionCall if we have enough info.
                            // For now, we might need to accumulate or simplify.
                            // Assuming we get a complete call or handle it simplified:
                            if let (Some(name), Some(args_str)) = (tc.name, tc.arguments) {
                                if let Ok(args) = serde_json::from_str(&args_str) {
                                    parts.push(GeminiPart::FunctionCall {
                                        function_call: GeminiFunctionCall { name, args },
                                    });
                                }
                            }
                        }
                    }
                }

                let content = if !parts.is_empty() {
                    Some(GeminiResponseContent { role, parts })
                } else {
                    None
                };

                let finish_reason = choice.finish_reason.as_ref().map(|fr| {
                    // Note: Gemini doesn't have a direct "tool_calls" finish reason,
                    // so we map it to "STOP" which is semantically closest
                    crate::service::transform::unified::map_openai_finish_reason_to_gemini(fr)
                });

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        index: Some(choice.index),
                        content,
                        finish_reason,
                        safety_ratings: candidate_metadata.as_ref().and_then(|metadata| {
                            unified_safety_ratings_to_gemini(metadata.safety_ratings.clone())
                        }),
                        token_count: candidate_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.token_count),
                        citation_metadata: candidate_metadata.and_then(|metadata| {
                            unified_citation_metadata_to_gemini(metadata.citation_metadata)
                        }),
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_chunk.usage.map(|u| GeminiChunkUsageMetadata {
            prompt_token_count: u.input_tokens,
            candidates_token_count: Some(u.output_tokens),
            total_token_count: u.total_tokens,
        });

        GeminiChunkResponse {
            candidates,
            prompt_feedback: gemini_metadata
                .and_then(|metadata| unified_prompt_feedback_to_gemini(metadata.prompt_feedback)),
            usage_metadata,
            synthetic_metadata,
        }
    }
}

pub(super) fn transform_unified_stream_events_to_gemini_events(
    stream_events: Vec<UnifiedStreamEvent>,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut transformed = Vec::new();

    for event in stream_events {
        let maybe_event = match event {
            UnifiedStreamEvent::ContentBlockDelta { text, .. } => {
                serde_json::to_string(&GeminiChunkResponse {
                    candidates: vec![GeminiCandidate {
                        index: Some(0),
                        content: Some(GeminiResponseContent {
                            role: "model".to_string(),
                            parts: vec![GeminiPart::Text { text }],
                        }),
                        finish_reason: None,
                        safety_ratings: None,
                        token_count: None,
                        citation_metadata: None,
                    }],
                    prompt_feedback: None,
                    usage_metadata: None,
                    synthetic_metadata: None,
                })
                .ok()
                .map(|data| SseEvent {
                    data,
                    ..Default::default()
                })
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                name: Some(name),
                arguments,
                ..
            } => serde_json::from_str::<Value>(&arguments)
                .ok()
                .and_then(|args| {
                    serde_json::to_string(&GeminiChunkResponse {
                        candidates: vec![GeminiCandidate {
                            index: Some(0),
                            content: Some(GeminiResponseContent {
                                role: "model".to_string(),
                                parts: vec![GeminiPart::FunctionCall {
                                    function_call: GeminiFunctionCall { name, args },
                                }],
                            }),
                            finish_reason: None,
                            safety_ratings: None,
                            token_count: None,
                            citation_metadata: None,
                        }],
                        prompt_feedback: None,
                        usage_metadata: None,
                        synthetic_metadata: None,
                    })
                    .ok()
                    .map(|data| SseEvent {
                        data,
                        ..Default::default()
                    })
                }),
            UnifiedStreamEvent::MessageDelta { finish_reason } => finish_reason
                .map(|reason| {
                    let finish_reason = map_openai_finish_reason_to_gemini(&reason);
                    GeminiChunkResponse {
                        candidates: vec![GeminiCandidate {
                            index: Some(0),
                            content: None,
                            finish_reason: Some(finish_reason),
                            safety_ratings: None,
                            token_count: None,
                            citation_metadata: None,
                        }],
                        prompt_feedback: None,
                        usage_metadata: None,
                        synthetic_metadata: None,
                    }
                })
                .and_then(|chunk| {
                    serde_json::to_string(&chunk).ok().map(|data| SseEvent {
                        data,
                        ..Default::default()
                    })
                }),
            UnifiedStreamEvent::Usage { usage } => serde_json::to_string(&GeminiChunkResponse {
                candidates: vec![GeminiCandidate {
                    index: Some(0),
                    content: None,
                    finish_reason: None,
                    safety_ratings: None,
                    token_count: None,
                    citation_metadata: None,
                }],
                prompt_feedback: None,
                usage_metadata: Some(GeminiChunkUsageMetadata {
                    prompt_token_count: usage.input_tokens,
                    candidates_token_count: Some(usage.output_tokens),
                    total_token_count: usage.total_tokens,
                }),
                synthetic_metadata: None,
            })
            .ok()
            .map(|data| SseEvent {
                data,
                ..Default::default()
            }),
            UnifiedStreamEvent::ReasoningStart { index } => Some(build_gemini_stream_diagnostic(
                transformer,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Gemini chunk candidates do not expose a native reasoning_start event; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::ReasoningDelta { index, text, .. } => {
                Some(build_gemini_stream_diagnostic(
                    transformer,
                    TransformValueKind::ReasoningDelta,
                    format!(
                        "Gemini chunk candidates do not expose a native reasoning delta; index={index}, chars={} was downgraded to a structured transform diagnostic.",
                        text.chars().count()
                    ),
                ))
            }
            UnifiedStreamEvent::ReasoningStop { index } => Some(build_gemini_stream_diagnostic(
                transformer,
                TransformValueKind::ReasoningDelta,
                format!(
                    "Gemini chunk candidates do not expose a native reasoning_stop event; index={index} was downgraded to a structured transform diagnostic."
                ),
            )),
            UnifiedStreamEvent::BlobDelta { index, data } => {
                if let Some(inline_data) = gemini_inline_data_from_blob(&data) {
                    serde_json::to_string(&GeminiChunkResponse {
                        candidates: vec![GeminiCandidate {
                            index: index.or(Some(0)),
                            content: Some(GeminiResponseContent {
                                role: "model".to_string(),
                                parts: vec![GeminiPart::InlineData { inline_data }],
                            }),
                            finish_reason: None,
                            safety_ratings: None,
                            token_count: None,
                            citation_metadata: None,
                        }],
                        prompt_feedback: None,
                        usage_metadata: None,
                        synthetic_metadata: None,
                    })
                    .ok()
                    .map(|data| SseEvent {
                        data,
                        ..Default::default()
                    })
                } else {
                    Some(build_gemini_stream_diagnostic(
                        transformer,
                        TransformValueKind::BlobDelta,
                        format!(
                            "Gemini stream encoding only preserves blob deltas that carry inline data fields; index={index:?} was downgraded to a structured transform diagnostic."
                        ),
                    ))
                }
            }
            UnifiedStreamEvent::ItemAdded { .. }
            | UnifiedStreamEvent::ItemDone { .. }
            | UnifiedStreamEvent::MessageStart { .. }
            | UnifiedStreamEvent::MessageStop
            | UnifiedStreamEvent::ContentPartAdded { .. }
            | UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ToolCallStart { .. }
            | UnifiedStreamEvent::ToolCallArgumentsDelta { name: None, .. }
            | UnifiedStreamEvent::ToolCallStop { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. }
            | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
            | UnifiedStreamEvent::Error { .. } => None,
        };

        if let Some(event) = maybe_event {
            transformed.push(event);
        }
    }

    if transformed.is_empty() {
        None
    } else {
        Some(transformed)
    }
}

pub(super) fn transform_unified_chunk_to_gemini_events(
    mut unified_chunk: UnifiedChunkResponse,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for choice in &mut unified_chunk.choices {
        let mut filtered = Vec::new();
        for part in std::mem::take(&mut choice.delta.content) {
            match part {
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    events.push(build_gemini_stream_diagnostic(
                        transformer,
                        TransformValueKind::ImageDelta,
                        format!(
                            "Gemini chunk candidates cannot faithfully encode legacy image deltas without inline mime metadata; index={index}, has_url={}, has_data={} was downgraded to a structured transform diagnostic.",
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
        let value = serde_json::to_value(GeminiChunkResponse::from(unified_chunk)).ok()?;
        if value
            .get("candidates")
            .and_then(|c| c.as_array())
            .is_some_and(|candidates| !candidates.is_empty())
        {
            events.push(SseEvent {
                data: serde_json::to_string(&value).unwrap_or_default(),
                ..Default::default()
            });
        }
    }

    (!events.is_empty()).then_some(events)
}

impl From<GeminiChunkResponse> for UnifiedChunkResponse {
    fn from(gemini_chunk: GeminiChunkResponse) -> Self {
        let GeminiChunkResponse {
            candidates,
            prompt_feedback,
            usage_metadata,
            synthetic_metadata,
        } = gemini_chunk;

        let provider_session_metadata = build_gemini_session_metadata(prompt_feedback, &candidates);

        let choices = candidates
            .into_iter()
            .map(|candidate| {
                let mut delta = UnifiedMessageDelta::default();
                let mut has_function_call = false;

                if let Some(content) = candidate.content {
                    delta.role = Some(match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User,
                        _ => UnifiedRole::User,
                    });

                    // Track indices separately for different content types
                    let mut text_index = 0;
                    let mut tool_call_index = 0;
                    let mut image_index = 0;

                    for part in content.parts {
                        match part {
                            GeminiPart::Text { text } => {
                                delta.content.push(UnifiedContentPartDelta::TextDelta {
                                    index: text_index,
                                    text,
                                });
                                text_index += 1;
                            }
                            GeminiPart::InlineData { inline_data } => {
                                delta.content.push(UnifiedContentPartDelta::ImageDelta {
                                    index: image_index,
                                    url: None,
                                    data: Some(inline_data.data),
                                });
                                image_index += 1;
                            }
                            GeminiPart::FileData { .. } => {
                                // File data doesn't map well to delta, skip for now
                            }
                            GeminiPart::ExecutableCode { executable_code } => {
                                has_function_call = true;
                                delta.content.push(UnifiedContentPartDelta::ToolCallDelta(
                                    UnifiedToolCallDelta {
                                        index: tool_call_index,
                                        id: None,
                                        name: Some("code_interpreter".to_string()),
                                        arguments: Some(
                                            json!({
                                                "language": executable_code.language,
                                                "code": executable_code.code,
                                            })
                                            .to_string(),
                                        ),
                                    },
                                ));
                                tool_call_index += 1;
                            }
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                delta.content.push(UnifiedContentPartDelta::ToolCallDelta(
                                    UnifiedToolCallDelta {
                                        index: tool_call_index,
                                        id: None,
                                        name: Some(function_call.name),
                                        arguments: Some(function_call.args.to_string()),
                                    },
                                ));
                                tool_call_index += 1;
                            }
                            _ => {}
                        }
                    }
                }

                let finish_reason = candidate.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_gemini_finish_reason_to_openai(
                        &fr,
                        has_function_call,
                    )
                });

                UnifiedChunkChoice {
                    index: candidate.index.unwrap_or(0),
                    delta,
                    finish_reason,
                }
            })
            .collect();

        let usage = usage_metadata.map(|u| UnifiedUsage {
            input_tokens: u.prompt_token_count,
            output_tokens: u.candidates_token_count.unwrap_or(0),
            total_tokens: u.total_token_count,
            ..Default::default()
        });

        let synthetic_id = true;
        let synthetic_model = false;

        UnifiedChunkResponse {
            // Gemini chunks don't carry top-level id/model fields.
            id: build_gemini_synthetic_response_id("chunk"),
            model: None,
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata,
            synthetic_metadata: merge_gemini_synthetic_metadata(
                synthetic_metadata,
                build_gemini_synthetic_metadata(synthetic_id, synthetic_model, false),
            ),
        }
    }
}

// Helper to recursively transform Gemini tool parameter types to lowercase for OpenAI.
pub(super) fn transform_gemini_tool_params_to_openai(params: &mut Value) {
    if let Some(obj) = params.as_object_mut() {
        // Transform "type" field
        if let Some(type_val) = obj.get_mut("type") {
            if let Some(type_str) = type_val.as_str() {
                *type_val = json!(type_str.to_lowercase());
            }
        }
        // Recurse into "properties"
        if let Some(properties) = obj.get_mut("properties") {
            if let Some(props_obj) = properties.as_object_mut() {
                for (_, prop_val) in props_obj.iter_mut() {
                    transform_gemini_tool_params_to_openai(prop_val);
                }
            }
        }
        // Recurse into "items" for arrays
        if let Some(items) = obj.get_mut("items") {
            transform_gemini_tool_params_to_openai(items);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gemini_request_to_unified() {
        let gemini_req = GeminiRequestPayload {
            contents: vec![GeminiRequestContent {
                role: Some("user".to_string()),
                parts: vec![GeminiPart::Text {
                    text: "Hello".to_string(),
                }],
            }],
            system_instruction: Some(GeminiSystemInstruction::String(
                "You are a helpful assistant.".to_string(),
            )),
            tools: None,
            generation_config: Some(GeminiGenerationConfig {
                temperature: Some(0.8),
                max_output_tokens: Some(100),
                top_p: Some(0.9),
                stop_sequences: Some(vec!["stop".to_string()]),
            }),
            safety_settings: None,
        };

        let unified_req: UnifiedRequest = gemini_req.into();

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
    }

    #[test]
    fn test_unified_request_to_gemini() {
        let unified_req = UnifiedRequest {
            model: Some("test-model".to_string()),
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

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert!(gemini_req.system_instruction.is_some());
        let system_instruction = gemini_req.system_instruction.unwrap();
        match system_instruction {
            GeminiSystemInstruction::String(text) => {
                assert_eq!(text, "You are a helpful assistant.");
            }
            GeminiSystemInstruction::Object { parts } => {
                assert_eq!(parts.len(), 1);
                if let GeminiPart::Text { text } = &parts[0] {
                    assert_eq!(text, "You are a helpful assistant.");
                } else {
                    panic!("Expected text part in system instruction");
                }
            }
        }

        assert_eq!(gemini_req.contents.len(), 1);
        assert_eq!(gemini_req.contents[0].role, Some("user".to_string()));
        assert_eq!(gemini_req.contents[0].parts.len(), 1);
        if let GeminiPart::Text { text } = &gemini_req.contents[0].parts[0] {
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected text part in user content");
        }

        assert!(gemini_req.generation_config.is_some());
        let config = gemini_req.generation_config.unwrap();
        assert_eq!(config.temperature, Some(0.8));
        assert_eq!(config.max_output_tokens, Some(100));
        assert_eq!(config.top_p, Some(0.9));
        assert_eq!(config.stop_sequences, Some(vec!["stop".to_string()]));
    }

    #[test]
    fn test_unified_request_to_gemini_preserves_image_url_as_recoverable_text() {
        let unified_req = UnifiedRequest {
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "Describe this".to_string(),
                    },
                    UnifiedContentPart::ImageUrl {
                        url: "https://example.com/cat.png".to_string(),
                        detail: None,
                    },
                ],
            }],
            ..Default::default()
        };

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert_eq!(gemini_req.contents.len(), 1);
        assert_eq!(gemini_req.contents[0].parts.len(), 2);
        assert!(matches!(
            &gemini_req.contents[0].parts[0],
            GeminiPart::Text { text } if text == "Describe this"
        ));
        assert!(matches!(
            &gemini_req.contents[0].parts[1],
            GeminiPart::Text { text } if text == "image_url: https://example.com/cat.png"
        ));
    }

    #[test]
    fn test_unified_request_to_gemini_recovers_tool_result_name_from_tool_call_id() {
        let unified_req = UnifiedRequest {
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "call_123".to_string(),
                        name: "get_current_weather".to_string(),
                        arguments: json!({ "location": "Boston" }),
                    })],
                },
                UnifiedMessage {
                    role: UnifiedRole::Tool,
                    content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: "call_123".to_string(),
                        name: None,
                        output: UnifiedToolResultOutput::Json {
                            value: json!({"temperature": 22}),
                        },
                    })],
                },
            ],
            ..Default::default()
        };

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert_eq!(gemini_req.contents.len(), 2);
        match &gemini_req.contents[1].parts[0] {
            GeminiPart::FunctionResponse { function_response } => {
                assert_eq!(function_response.name, "get_current_weather");
                assert_eq!(function_response.response, json!({ "temperature": 22 }));
            }
            other => panic!("Expected function response part, got {:?}", other),
        }
    }

    #[test]
    fn test_unified_request_to_gemini_preserves_tool_result_with_synthetic_fallback_name() {
        let unified_req = UnifiedRequest {
            messages: vec![UnifiedMessage {
                role: UnifiedRole::Tool,
                content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                    tool_call_id: "call:123".to_string(),
                    name: None,
                    output: UnifiedToolResultOutput::Json {
                        value: json!({"temperature": 22}),
                    },
                })],
            }],
            ..Default::default()
        };

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert_eq!(gemini_req.contents.len(), 1);
        match &gemini_req.contents[0].parts[0] {
            GeminiPart::FunctionResponse { function_response } => {
                assert_eq!(function_response.name, "gemini-tool-result-call_123");
                assert_eq!(function_response.response, json!({ "temperature": 22 }));
            }
            other => panic!("Expected function response part, got {:?}", other),
        }
    }

    #[test]
    fn test_gemini_request_to_unified_preserves_structured_tool_result_output() {
        let gemini_req = GeminiRequestPayload {
            contents: vec![GeminiRequestContent {
                role: Some("user".to_string()),
                parts: vec![GeminiPart::FunctionResponse {
                    function_response: GeminiFunctionResponse {
                        name: "lookup_weather".to_string(),
                        response: json!({
                            "result": [
                                {"type": "text", "text": "hello"},
                                {
                                    "type": "file",
                                    "filename": "report.pdf",
                                    "file_url": "https://files.example.com/report.pdf"
                                }
                            ]
                        }),
                    },
                }],
            }],
            system_instruction: None,
            tools: None,
            generation_config: None,
            safety_settings: None,
        };

        let unified_req: UnifiedRequest = gemini_req.into();

        assert_eq!(unified_req.messages.len(), 1);
        match &unified_req.messages[0].content[0] {
            UnifiedContentPart::ToolResult(result) => {
                assert_eq!(result.name.as_deref(), Some("lookup_weather"));
                assert!(matches!(
                    &result.output,
                    UnifiedToolResultOutput::Content { parts }
                    if matches!(&parts[0], UnifiedToolResultPart::Text { text } if text == "hello")
                        && matches!(
                            &parts[1],
                            UnifiedToolResultPart::File { filename, file_url }
                            if filename.as_deref() == Some("report.pdf")
                                && file_url.as_deref()
                                    == Some("https://files.example.com/report.pdf")
                        )
                ));
            }
            other => panic!("Expected tool result, got {:?}", other),
        }
    }

    #[test]
    fn test_unified_request_to_gemini_preserves_reasoning_and_executable_code() {
        let unified_req = UnifiedRequest {
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![
                        UnifiedContentPart::Reasoning {
                            text: "step by step".to_string(),
                        },
                        UnifiedContentPart::ExecutableCode {
                            language: "python".to_string(),
                            code: "print('hi')".to_string(),
                        },
                    ],
                },
                UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Reasoning {
                        text: "internal summary".to_string(),
                    }],
                },
            ],
            ..Default::default()
        };

        let gemini_req: GeminiRequestPayload = unified_req.into();
        assert_eq!(gemini_req.contents.len(), 2);
        assert!(matches!(
            &gemini_req.contents[0].parts[0],
            GeminiPart::Text { text } if text == "step by step"
        ));
        assert!(matches!(
            &gemini_req.contents[0].parts[1],
            GeminiPart::ExecutableCode { executable_code }
            if executable_code.language == "python" && executable_code.code == "print('hi')"
        ));
        assert!(matches!(
            &gemini_req.contents[1].parts[0],
            GeminiPart::Text { text } if text == "internal summary"
        ));
    }

    #[test]
    fn test_unified_request_to_gemini_preserves_user_assistant_and_tool_fallback_content() {
        let unified_req = UnifiedRequest {
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![
                        UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: "call_user".to_string(),
                            name: "lookup_weather".to_string(),
                            arguments: json!({ "city": "Boston" }),
                        }),
                        UnifiedContentPart::ToolResult(UnifiedToolResult {
                            tool_call_id: "call_user".to_string(),
                            name: None,
                            output: UnifiedToolResultOutput::Json {
                                value: json!({"ok": true}),
                            },
                        }),
                    ],
                },
                UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![
                        UnifiedContentPart::ImageUrl {
                            url: "https://example.com/chart.png".to_string(),
                            detail: Some("high".to_string()),
                        },
                        UnifiedContentPart::ToolResult(UnifiedToolResult {
                            tool_call_id: "call_assistant".to_string(),
                            name: Some("summarize".to_string()),
                            output: UnifiedToolResultOutput::Json {
                                value: json!({"summary": "done"}),
                            },
                        }),
                    ],
                },
                UnifiedMessage {
                    role: UnifiedRole::Tool,
                    content: vec![
                        UnifiedContentPart::Text {
                            text: "tool text".to_string(),
                        },
                        UnifiedContentPart::ImageData {
                            mime_type: "image/png".to_string(),
                            data: "ZmFrZQ==".to_string(),
                        },
                        UnifiedContentPart::FileUrl {
                            url: "https://files.example.com/report.pdf".to_string(),
                            mime_type: Some("application/pdf".to_string()),
                            filename: None,
                        },
                    ],
                },
            ],
            ..Default::default()
        };

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert!(matches!(
            &gemini_req.contents[0].parts[0],
            GeminiPart::Text { text }
            if text == "tool_call: lookup_weather\narguments: {\"city\":\"Boston\"}"
        ));
        assert!(matches!(
            &gemini_req.contents[0].parts[1],
            GeminiPart::FunctionResponse { function_response }
            if function_response.name == "lookup_weather"
                && function_response.response == json!({"ok": true})
        ));
        assert!(matches!(
            &gemini_req.contents[1].parts[0],
            GeminiPart::Text { text }
            if text == "image_url: https://example.com/chart.png\ndetail: high"
        ));
        assert!(matches!(
            &gemini_req.contents[1].parts[1],
            GeminiPart::Text { text }
            if text == "tool_result: summarize\ntool_call_id: call_assistant\ncontent: {\"summary\":\"done\"}"
        ));
        assert!(matches!(
            &gemini_req.contents[2].parts[0],
            GeminiPart::Text { text } if text == "tool text"
        ));
        assert!(matches!(
            &gemini_req.contents[2].parts[1],
            GeminiPart::InlineData { inline_data }
            if inline_data.mime_type == "image/png" && inline_data.data == "ZmFrZQ=="
        ));
        assert!(matches!(
            &gemini_req.contents[2].parts[2],
            GeminiPart::FileData { file_data }
            if file_data.file_uri == "https://files.example.com/report.pdf"
                && file_data.mime_type == "application/pdf"
        ));
    }

    #[test]
    fn test_unified_request_to_gemini_preserves_inline_file_data_as_inline_data() {
        let unified_req = UnifiedRequest {
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::FileData {
                    data: "dGVzdA==".to_string(),
                    mime_type: "application/pdf".to_string(),
                    filename: Some("report.pdf".to_string()),
                }],
            }],
            ..Default::default()
        };

        let gemini_req: GeminiRequestPayload = unified_req.into();

        assert!(matches!(
            &gemini_req.contents[0].parts[0],
            GeminiPart::InlineData { inline_data }
            if inline_data.mime_type == "application/pdf" && inline_data.data == "dGVzdA=="
        ));
    }

    #[test]
    fn test_gemini_response_to_unified() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hi there!".to_string(),
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
                token_count: Some(20),
                citation_metadata: Some(GeminiCitationMetadata {
                    citation_sources: vec![GeminiCitationSource {
                        start_index: Some(0),
                        end_index: Some(4),
                        uri: Some("https://example.com".to_string()),
                        license: None,
                    }],
                }),
            }],
            prompt_feedback: None,
            usage_metadata: Some(GeminiUsageMetadata {
                prompt_token_count: 10,
                candidates_token_count: 20,
                total_token_count: 30,
                thoughts_token_count: None,
                cached_content_token_count: None,
                tool_use_prompt_token_count: None,
                prompt_tokens_details: vec![],
                cache_tokens_details: vec![],
                candidates_tokens_details: vec![],
                tool_use_prompt_tokens_details: vec![],
            }),
            synthetic_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();

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
        let usage = unified_res.usage.as_ref().unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 20);
        assert_eq!(usage.total_tokens, 30);
        assert!(unified_res.id.starts_with("gemini-response-"));
        assert_eq!(unified_res.model, None);
        let synthetic = unified_res.synthetic_metadata().unwrap();
        assert!(synthetic.id);
        assert!(!synthetic.model);
        assert!(!synthetic.gemini_safety_ratings);
        let metadata = unified_res.provider_response_metadata().unwrap();
        let gemini_metadata = metadata.gemini.as_ref().unwrap();
        assert_eq!(gemini_metadata.candidates.len(), 1);
        assert_eq!(gemini_metadata.candidates[0].token_count, Some(20));
        assert!(matches!(
            &unified_res.choices[0].items[0],
            UnifiedItem::Message(UnifiedMessageItem { annotations, .. })
            if matches!(
                &annotations[..],
                [UnifiedAnnotation::Citation(UnifiedCitation { url, start_index, end_index, .. })]
                if url.as_deref() == Some("https://example.com")
                && *start_index == Some(0)
                && *end_index == Some(4)
            )
        ));
    }

    #[test]
    fn test_gemini_response_to_unified_preserves_inline_file_data_as_typed_file() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::InlineData {
                        inline_data: GeminiInlineData {
                            mime_type: "application/pdf".to_string(),
                            data: "dGVzdA==".to_string(),
                        },
                    }],
                }),
                finish_reason: Some("STOP".to_string()),
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
            synthetic_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();
        assert!(matches!(
            &unified_res.choices[0].message.content[0],
            UnifiedContentPart::FileData { mime_type, data, .. }
            if mime_type == "application/pdf" && data == "dGVzdA=="
        ));
    }

    #[test]
    fn test_unified_response_to_gemini_prefers_typed_items_for_file_and_tool_output() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: Some("gpt-4".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![],
                    ..Default::default()
                },
                items: vec![
                    UnifiedItem::FileReference(UnifiedFileReferenceItem {
                        filename: Some("report.pdf".to_string()),
                        mime_type: Some("application/pdf".to_string()),
                        file_url: Some("https://files.example.com/report.pdf".to_string()),
                        file_id: None,
                    }),
                    UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
                        tool_call_id: "call_123".to_string(),
                        name: Some("lookup_weather".to_string()),
                        output: UnifiedToolResultOutput::Json {
                            value: json!({"temperature": 22}),
                        },
                    }),
                ],
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: None,
            object: None,
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let gemini_res: GeminiResponse = unified_res.into();
        let parts = &gemini_res.candidates[0].content.as_ref().unwrap().parts;
        assert!(matches!(
            &parts[0],
            GeminiPart::FileData { file_data }
            if file_data.file_uri == "https://files.example.com/report.pdf"
                && file_data.mime_type == "application/pdf"
        ));
        assert!(matches!(
            &parts[1],
            GeminiPart::FunctionResponse { function_response }
            if function_response.name == "lookup_weather"
                && function_response.response == json!({"temperature": 22})
        ));
    }

    #[test]
    fn test_unified_response_to_gemini() {
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
            created: Some(1234567890),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let gemini_res: GeminiResponse = unified_res.into();

        assert_eq!(gemini_res.candidates.len(), 1);
        let candidate = &gemini_res.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 1);
        if let GeminiPart::Text { text } = &content.parts[0] {
            assert_eq!(text, "Hi there!");
        } else {
            panic!("Expected text part");
        }
        assert_eq!(candidate.finish_reason, Some("STOP".to_string()));

        assert!(gemini_res.usage_metadata.is_some());
        let usage = gemini_res.usage_metadata.unwrap();
        assert_eq!(usage.prompt_token_count, 10);
        assert_eq!(usage.candidates_token_count, 20);
        assert_eq!(usage.total_token_count, 30);
        assert!(gemini_res.synthetic_metadata.is_none());
    }

    #[test]
    fn test_unified_response_to_gemini_restores_citation_metadata_from_structured_annotations() {
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
                items: vec![UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "Hi there!".to_string(),
                    }],
                    annotations: vec![UnifiedAnnotation::Citation(UnifiedCitation {
                        part_index: None,
                        start_index: Some(0),
                        end_index: Some(4),
                        url: Some("https://example.com".to_string()),
                        title: None,
                        license: Some("CC-BY".to_string()),
                    })],
                })],
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1234567890),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let gemini_res: GeminiResponse = unified_res.into();

        let citation_metadata = gemini_res.candidates[0].citation_metadata.as_ref().unwrap();
        assert_eq!(citation_metadata.citation_sources.len(), 1);
        assert_eq!(
            citation_metadata.citation_sources[0].uri.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(citation_metadata.citation_sources[0].start_index, Some(0));
        assert_eq!(citation_metadata.citation_sources[0].end_index, Some(4));
        assert_eq!(
            citation_metadata.citation_sources[0].license.as_deref(),
            Some("CC-BY")
        );
    }

    #[test]
    fn test_unified_response_to_gemini_preserves_synthetic_metadata() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: None,
            choices: vec![],
            usage: None,
            created: None,
            object: None,
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: Some(UnifiedSyntheticMetadata {
                id: true,
                model: false,
                gemini_safety_ratings: false,
            }),
        };

        let gemini_res: GeminiResponse = unified_res.into();

        assert!(gemini_res.synthetic_metadata.is_some());
        assert!(gemini_res.synthetic_metadata.as_ref().unwrap().id);
    }

    #[test]
    fn test_unified_response_to_gemini_preserves_provider_metadata() {
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
            usage: None,
            created: None,
            object: None,
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                gemini: Some(UnifiedGeminiResponseMetadata {
                    prompt_feedback: Some(UnifiedGeminiPromptFeedback {
                        block_reason: Some("SAFETY".to_string()),
                        safety_ratings: vec![UnifiedGeminiSafetyRating {
                            category: "HARM_CATEGORY_HATE_SPEECH".to_string(),
                            probability: "LOW".to_string(),
                        }],
                    }),
                    candidates: vec![UnifiedGeminiCandidateMetadata {
                        index: 0,
                        safety_ratings: vec![UnifiedGeminiSafetyRating {
                            category: "HARM_CATEGORY_HARASSMENT".to_string(),
                            probability: "NEGLIGIBLE".to_string(),
                        }],
                        citation_metadata: Some(UnifiedGeminiCitationMetadata {
                            citation_sources: vec![UnifiedGeminiCitationSource {
                                start_index: Some(0),
                                end_index: Some(5),
                                uri: Some("https://example.com".to_string()),
                                license: Some("CC-BY".to_string()),
                            }],
                        }),
                        token_count: Some(7),
                    }],
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        };

        let gemini_res: GeminiResponse = unified_res.into();
        assert_eq!(
            gemini_res
                .prompt_feedback
                .as_ref()
                .and_then(|f| f.block_reason.as_deref()),
            Some("SAFETY")
        );
        assert_eq!(
            gemini_res.candidates[0].safety_ratings.as_ref().unwrap()[0].category,
            "HARM_CATEGORY_HARASSMENT"
        );
        assert_eq!(
            gemini_res.candidates[0]
                .citation_metadata
                .as_ref()
                .unwrap()
                .citation_sources[0]
                .uri
                .as_deref(),
            Some("https://example.com")
        );
        assert_eq!(gemini_res.candidates[0].token_count, Some(7));
    }

    #[test]
    fn test_gemini_chunk_to_unified() {
        let gemini_chunk = GeminiChunkResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::Text {
                        text: "Hello".to_string(),
                    }],
                }),
                finish_reason: None,
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
            synthetic_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

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
        assert!(unified_chunk.id.starts_with("gemini-chunk-"));
        assert_eq!(unified_chunk.model, None);
        let synthetic = unified_chunk.synthetic_metadata().unwrap();
        assert!(synthetic.id);
        assert!(!synthetic.model);
        assert!(!synthetic.gemini_safety_ratings);
    }

    #[test]
    fn test_unified_chunk_to_gemini() {
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
            created: Some(1234567890),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        };

        let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

        assert_eq!(gemini_chunk.candidates.len(), 1);
        let candidate = &gemini_chunk.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 1);
        if let GeminiPart::Text { text } = &content.parts[0] {
            assert_eq!(text, "Hello");
        } else {
            panic!("Expected text part");
        }
        assert!(candidate.finish_reason.is_none());
        assert!(gemini_chunk.synthetic_metadata.is_none());
    }

    #[test]
    fn test_unified_chunk_to_gemini_preserves_synthetic_metadata() {
        let unified_chunk = UnifiedChunkResponse {
            id: "chatcmpl-123".to_string(),
            model: None,
            choices: vec![],
            usage: None,
            created: None,
            object: None,
            provider_session_metadata: None,
            synthetic_metadata: Some(UnifiedSyntheticMetadata {
                id: true,
                model: false,
                gemini_safety_ratings: false,
            }),
        };

        let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

        assert!(gemini_chunk.synthetic_metadata.is_some());
        assert!(gemini_chunk.synthetic_metadata.as_ref().unwrap().id);
    }

    #[test]
    fn test_gemini_response_to_unified_with_thinking() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![
                        GeminiPart::Text {
                            text: "I should call a tool".to_string(),
                        },
                        GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "get_weather".to_string(),
                                args: json!({"location": "Boston"}),
                            },
                        },
                    ],
                }),
                finish_reason: Some("TOOL_USE".to_string()),
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
            synthetic_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));

        match &choice.message.content[0] {
            UnifiedContentPart::Text { text } => assert_eq!(text, "I should call a tool"),
            _ => panic!("Expected text content"),
        }
        match &choice.message.content[1] {
            UnifiedContentPart::ToolCall(tc) => {
                assert_eq!(tc.name, "get_weather");
            }
            _ => panic!("Expected tool call content"),
        }
    }

    #[test]
    fn test_unified_response_to_gemini_with_thinking() {
        let unified_res = UnifiedResponse {
            id: "chatcmpl-123".to_string(),
            model: Some("gpt-4".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![
                        UnifiedContentPart::Text {
                            text: "I will call a tool".to_string(),
                        },
                        UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: "call_123".to_string(),
                            name: "get_weather".to_string(),
                            arguments: json!({"location": "Boston"}),
                        }),
                    ],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("tool_calls".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: None,
            object: None,
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let gemini_res: GeminiResponse = unified_res.into();

        assert_eq!(gemini_res.candidates.len(), 1);
        let candidate = &gemini_res.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 2);
        assert!(
            matches!(&content.parts[0], GeminiPart::Text { text } if text == "I will call a tool")
        );
        assert!(
            matches!(&content.parts[1], GeminiPart::FunctionCall { function_call } if function_call.name == "get_weather")
        );
        assert_eq!(candidate.finish_reason, Some("TOOL_USE".to_string()));
    }

    #[test]
    fn test_transform_unified_chunk_to_gemini_events_emits_diagnostic_for_image_delta() {
        let unified_chunk = UnifiedChunkResponse {
            id: "cmpl-123".to_string(),
            model: Some("gemini-2.0-flash".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![
                        UnifiedContentPartDelta::ImageDelta {
                            index: 2,
                            url: None,
                            data: Some("ZmFrZQ==".to_string()),
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

        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Gemini);
        let events = transform_unified_chunk_to_gemini_events(unified_chunk, &mut transformer)
            .expect("gemini chunk events");

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event.as_deref(), Some("transform_diagnostic"));
        let diagnostic: Value = serde_json::from_str(&events[0].data).unwrap();
        assert_eq!(diagnostic["semantic_unit"], json!("ImageDelta"));

        let chunk: Value = serde_json::from_str(&events[1].data).unwrap();
        assert_eq!(
            chunk["candidates"][0]["content"]["parts"][0]["text"],
            json!("caption")
        );
    }

    #[test]
    fn test_gemini_chunk_to_unified_with_thinking() {
        let gemini_chunk = GeminiChunkResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![
                        GeminiPart::Text {
                            text: "Thinking...".to_string(),
                        },
                        GeminiPart::FunctionCall {
                            function_call: GeminiFunctionCall {
                                name: "search".to_string(),
                                args: json!({"query": "stuff"}),
                            },
                        },
                    ],
                }),
                finish_reason: None,
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
            synthetic_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));

        match &choice.delta.content[0] {
            UnifiedContentPartDelta::TextDelta { text, .. } => assert_eq!(text, "Thinking..."),
            _ => panic!("Expected text delta"),
        }

        match &choice.delta.content[1] {
            UnifiedContentPartDelta::ToolCallDelta(tc) => {
                assert_eq!(tc.name, Some("search".to_string()));
            }
            _ => panic!("Expected tool call delta"),
        }
    }

    #[test]
    fn test_unified_chunk_to_gemini_with_thinking() {
        let unified_chunk = UnifiedChunkResponse {
            id: "chatcmpl-123".to_string(),
            model: Some("gpt-4".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![
                        UnifiedContentPartDelta::TextDelta {
                            index: 0,
                            text: "Thinking...".to_string(),
                        },
                        UnifiedContentPartDelta::ToolCallDelta(UnifiedToolCallDelta {
                            index: 0,
                            id: Some("call_123".to_string()),
                            name: Some("search".to_string()),
                            arguments: Some(json!({"query": "stuff"}).to_string()),
                        }),
                    ],
                },
                finish_reason: None,
            }],
            usage: None,
            created: Some(1234567890),
            object: Some("chat.completion.chunk".to_string()),
            provider_session_metadata: None,
            synthetic_metadata: None,
        };

        let gemini_chunk: GeminiChunkResponse = unified_chunk.into();

        assert_eq!(gemini_chunk.candidates.len(), 1);
        let candidate = &gemini_chunk.candidates[0];
        assert!(candidate.content.is_some());
        let content = candidate.content.as_ref().unwrap();
        assert_eq!(content.role, "model");
        assert_eq!(content.parts.len(), 2);
        assert!(matches!(&content.parts[0], GeminiPart::Text { text } if text == "Thinking..."));
        assert!(
            matches!(&content.parts[1], GeminiPart::FunctionCall { function_call } if function_call.name == "search")
        );
    }

    #[test]
    fn test_gemini_response_to_unified_with_executable_code() {
        let gemini_res = GeminiResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::ExecutableCode {
                        executable_code: GeminiExecutableCode {
                            language: "PYTHON".to_string(),
                            code: "print('Hello World')".to_string(),
                        },
                    }],
                }),
                finish_reason: Some("TOOL_USE".to_string()),
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
            synthetic_metadata: None,
        };

        let unified_res: UnifiedResponse = gemini_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        let choice = &unified_res.choices[0];
        assert_eq!(choice.message.role, UnifiedRole::Assistant);
        assert_eq!(choice.finish_reason, Some("tool_calls".to_string()));

        match &choice.message.content[0] {
            UnifiedContentPart::ExecutableCode { language, code } => {
                assert_eq!(language, "PYTHON");
                assert_eq!(code, "print('Hello World')");
            }
            _ => panic!("Expected executable code content"),
        }
        assert!(matches!(
            &choice.items[0],
            UnifiedItem::Message(UnifiedMessageItem { content, .. })
            if matches!(
                &content[0],
                UnifiedContentPart::ExecutableCode { language, code }
                if language == "PYTHON" && code == "print('Hello World')"
            )
        ));
    }

    #[test]
    fn test_gemini_chunk_to_unified_with_executable_code() {
        let gemini_chunk = GeminiChunkResponse {
            candidates: vec![GeminiCandidate {
                index: Some(0),
                content: Some(GeminiResponseContent {
                    role: "model".to_string(),
                    parts: vec![GeminiPart::ExecutableCode {
                        executable_code: GeminiExecutableCode {
                            language: "PYTHON".to_string(),
                            code: "print('Hello')".to_string(),
                        },
                    }],
                }),
                finish_reason: None,
                safety_ratings: None,
                token_count: None,
                citation_metadata: None,
            }],
            prompt_feedback: None,
            usage_metadata: None,
            synthetic_metadata: None,
        };

        let unified_chunk: UnifiedChunkResponse = gemini_chunk.into();

        assert_eq!(unified_chunk.choices.len(), 1);
        let choice = &unified_chunk.choices[0];
        assert_eq!(choice.delta.role, Some(UnifiedRole::Assistant));

        match &choice.delta.content[0] {
            UnifiedContentPartDelta::ToolCallDelta(tc) => {
                assert_eq!(tc.name, Some("code_interpreter".to_string()));
                assert_eq!(
                    tc.arguments,
                    Some(json!({"language": "PYTHON", "code": "print('Hello')"}).to_string())
                );
            }
            _ => panic!("Expected tool call delta"),
        }
    }
}
