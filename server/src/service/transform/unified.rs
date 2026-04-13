use crate::utils::usage::UsageInfo;
use cyder_tools::log::warn;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const REGISTERED_PASSTHROUGH_KEYS: &[&str] = &[
    "logprobs",
    "top_logprobs",
    "parallel_tool_calls",
    "reasoning_effort",
];

pub fn is_registered_passthrough_key(key: &str) -> bool {
    REGISTERED_PASSTHROUGH_KEYS.contains(&key)
}

pub fn build_registered_passthrough(
    entries: impl IntoIterator<Item = (String, Value)>,
    context: &str,
) -> Option<Value> {
    let mut passthrough = Map::new();

    for (key, value) in entries {
        if is_registered_passthrough_key(&key) {
            passthrough.insert(key, value);
        } else {
            warn!(
                "[transform][passthrough] rejected_unregistered_key key={} context={} registered_keys={:?}",
                key, context, REGISTERED_PASSTHROUGH_KEYS
            );
        }
    }

    if passthrough.is_empty() {
        None
    } else {
        Some(Value::Object(passthrough))
    }
}

pub fn audit_passthrough_keys(passthrough: &Value, context: &str) {
    let Some(object) = passthrough.as_object() else {
        warn!(
            "[transform][passthrough] non_object_passthrough context={} value_type={}",
            context,
            match passthrough {
                Value::Null => "null",
                Value::Bool(_) => "bool",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            }
        );
        return;
    };

    for key in object.keys() {
        if !is_registered_passthrough_key(key) {
            warn!(
                "[transform][passthrough] encountered_unregistered_key key={} context={} registered_keys={:?}",
                key, context, REGISTERED_PASSTHROUGH_KEYS
            );
        }
    }
}

impl From<UnifiedUsage> for UsageInfo {
    fn from(unified_usage: UnifiedUsage) -> Self {
        Self {
            input_tokens: unified_usage.input_tokens as i32,
            output_tokens: unified_usage.output_tokens as i32,
            total_tokens: unified_usage.total_tokens as i32,
            input_image_tokens: unified_usage.input_image_tokens.unwrap_or(0) as i32,
            output_image_tokens: unified_usage.output_image_tokens.unwrap_or(0) as i32,
            cached_tokens: unified_usage.cached_tokens.unwrap_or(0) as i32,
            reasoning_tokens: unified_usage.reasoning_tokens.unwrap_or(0) as i32,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum UnifiedRole {
    System,
    #[default]
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedToolResult {
    pub tool_call_id: String,
    pub name: Option<String>, // The name of the tool that was called, when known
    pub output: UnifiedToolResultOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedToolResultOutput {
    Text {
        text: String,
    },
    Content {
        parts: Vec<UnifiedToolResultPart>,
    },
    File {
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
    },
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        image_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
    },
    Json {
        value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedToolResultPart {
    Text {
        text: String,
    },
    File {
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
    },
    Image {
        #[serde(skip_serializing_if = "Option::is_none")]
        image_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
    },
    Json {
        value: Value,
    },
}

impl UnifiedToolResult {
    pub fn from_legacy_content(
        tool_call_id: String,
        name: Option<String>,
        content: String,
    ) -> Self {
        Self {
            tool_call_id,
            name,
            output: parse_unified_tool_result_content(&content),
        }
    }

    pub fn legacy_content(&self) -> String {
        stringify_unified_tool_result_output(&self.output)
    }

    pub fn output_value(&self) -> Value {
        unified_tool_result_output_to_value(&self.output)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UnifiedCitation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedAnnotation {
    Citation(UnifiedCitation),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UnifiedMessageItem {
    pub role: UnifiedRole,
    pub content: Vec<UnifiedContentPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<UnifiedAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UnifiedReasoningItem {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content: Vec<UnifiedContentPart>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<UnifiedAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedFunctionCallItem {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UnifiedFunctionCallOutputItem {
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub output: UnifiedToolResultOutput,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct UnifiedFileReferenceItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedItem {
    Message(UnifiedMessageItem),
    Reasoning(UnifiedReasoningItem),
    FunctionCall(UnifiedFunctionCallItem),
    FunctionCallOutput(UnifiedFunctionCallOutputItem),
    FileReference(UnifiedFileReferenceItem),
}

impl UnifiedItem {
    pub fn is_empty(&self) -> bool {
        match self {
            UnifiedItem::Message(item) => {
                (item.content.is_empty() || item.content.iter().all(UnifiedContentPart::is_empty))
                    && item.annotations.is_empty()
            }
            UnifiedItem::Reasoning(item) => {
                (item.content.is_empty() || item.content.iter().all(UnifiedContentPart::is_empty))
                    && item.annotations.is_empty()
            }
            UnifiedItem::FunctionCall(_) | UnifiedItem::FunctionCallOutput(_) => false,
            UnifiedItem::FileReference(item) => {
                item.filename.is_none()
                    && item.mime_type.is_none()
                    && item.file_url.is_none()
                    && item.file_id.is_none()
            }
        }
    }

    pub fn legacy_content_parts(&self) -> Vec<UnifiedContentPart> {
        match self {
            UnifiedItem::Message(item) => item.content.clone(),
            UnifiedItem::Reasoning(item) => item.content.clone(),
            UnifiedItem::FunctionCall(item) => {
                vec![UnifiedContentPart::ToolCall(UnifiedToolCall {
                    id: item.id.clone(),
                    name: item.name.clone(),
                    arguments: item.arguments.clone(),
                })]
            }
            UnifiedItem::FunctionCallOutput(item) => {
                vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                    tool_call_id: item.tool_call_id.clone(),
                    name: item.name.clone(),
                    output: item.output.clone(),
                })]
            }
            UnifiedItem::FileReference(item) => item
                .file_url
                .clone()
                .map(|url| {
                    vec![UnifiedContentPart::FileUrl {
                        url,
                        mime_type: item.mime_type.clone(),
                        filename: item.filename.clone(),
                    }]
                })
                .unwrap_or_default(),
        }
    }
}

fn unified_tool_result_part_from_value(value: Value) -> UnifiedToolResultPart {
    let type_name = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();

    match type_name {
        "text" | "output_text" => UnifiedToolResultPart::Text {
            text: value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
        },
        "file" => UnifiedToolResultPart::File {
            filename: value
                .get("filename")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            file_url: value
                .get("file_url")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        },
        "image" => UnifiedToolResultPart::Image {
            image_url: value
                .get("image_url")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            file_url: value
                .get("file_url")
                .and_then(Value::as_str)
                .map(ToString::to_string),
        },
        _ => UnifiedToolResultPart::Json { value },
    }
}

fn unified_tool_result_part_to_value(part: &UnifiedToolResultPart) -> Value {
    match part {
        UnifiedToolResultPart::Text { text } => {
            serde_json::json!({ "type": "text", "text": text })
        }
        UnifiedToolResultPart::File { filename, file_url } => {
            serde_json::json!({ "type": "file", "filename": filename, "file_url": file_url })
        }
        UnifiedToolResultPart::Image {
            image_url,
            file_url,
        } => {
            serde_json::json!({ "type": "image", "image_url": image_url, "file_url": file_url })
        }
        UnifiedToolResultPart::Json { value } => value.clone(),
    }
}

pub fn unified_tool_result_output_from_value(value: Value) -> UnifiedToolResultOutput {
    match value {
        Value::String(text) => UnifiedToolResultOutput::Text { text },
        Value::Array(items) => UnifiedToolResultOutput::Content {
            parts: items
                .into_iter()
                .map(unified_tool_result_part_from_value)
                .collect(),
        },
        Value::Object(_) => {
            let type_name = value
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or_default();

            match type_name {
                "text" | "output_text" => UnifiedToolResultOutput::Text {
                    text: value
                        .get("text")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string(),
                },
                "file" => UnifiedToolResultOutput::File {
                    filename: value
                        .get("filename")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    file_url: value
                        .get("file_url")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                },
                "image" => UnifiedToolResultOutput::Image {
                    image_url: value
                        .get("image_url")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    file_url: value
                        .get("file_url")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                },
                _ => UnifiedToolResultOutput::Json { value },
            }
        }
        other => UnifiedToolResultOutput::Json { value: other },
    }
}

pub fn unified_tool_result_output_to_value(output: &UnifiedToolResultOutput) -> Value {
    match output {
        UnifiedToolResultOutput::Text { text } => Value::String(text.clone()),
        UnifiedToolResultOutput::Content { parts } => Value::Array(
            parts
                .iter()
                .map(unified_tool_result_part_to_value)
                .collect(),
        ),
        UnifiedToolResultOutput::File { filename, file_url } => {
            serde_json::json!({ "type": "file", "filename": filename, "file_url": file_url })
        }
        UnifiedToolResultOutput::Image {
            image_url,
            file_url,
        } => {
            serde_json::json!({ "type": "image", "image_url": image_url, "file_url": file_url })
        }
        UnifiedToolResultOutput::Json { value } => value.clone(),
    }
}

pub fn stringify_unified_tool_result_output(output: &UnifiedToolResultOutput) -> String {
    match output {
        UnifiedToolResultOutput::Text { text } => text.clone(),
        other => serde_json::to_string(&unified_tool_result_output_to_value(other))
            .unwrap_or_else(|_| unified_tool_result_output_to_value(other).to_string()),
    }
}

fn parse_unified_tool_result_content(content: &str) -> UnifiedToolResultOutput {
    serde_json::from_str(content)
        .map(unified_tool_result_output_from_value)
        .unwrap_or_else(|_| UnifiedToolResultOutput::Text {
            text: content.to_string(),
        })
}

pub fn legacy_content_to_unified_items(
    role: UnifiedRole,
    content: Vec<UnifiedContentPart>,
) -> Vec<UnifiedItem> {
    let mut items = Vec::new();
    let mut message_parts = Vec::new();

    for part in content {
        match part {
            UnifiedContentPart::ToolCall(call) => {
                if !message_parts.is_empty() {
                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                        role: role.clone(),
                        content: std::mem::take(&mut message_parts),
                        annotations: Vec::new(),
                    }));
                }
                items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                    id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                }));
            }
            UnifiedContentPart::ToolResult(result) => {
                if !message_parts.is_empty() {
                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                        role: role.clone(),
                        content: std::mem::take(&mut message_parts),
                        annotations: Vec::new(),
                    }));
                }
                items.push(UnifiedItem::FunctionCallOutput(
                    UnifiedFunctionCallOutputItem {
                        tool_call_id: result.tool_call_id,
                        name: result.name,
                        output: result.output,
                    },
                ));
            }
            UnifiedContentPart::Refusal { text } => {
                message_parts.push(UnifiedContentPart::Refusal { text });
            }
            UnifiedContentPart::Reasoning { text } => {
                if !message_parts.is_empty() {
                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                        role: role.clone(),
                        content: std::mem::take(&mut message_parts),
                        annotations: Vec::new(),
                    }));
                }
                items.push(UnifiedItem::Reasoning(UnifiedReasoningItem {
                    content: vec![UnifiedContentPart::Reasoning { text }],
                    annotations: Vec::new(),
                }));
            }
            UnifiedContentPart::FileUrl {
                url,
                mime_type,
                filename,
            } => {
                if !message_parts.is_empty() {
                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                        role: role.clone(),
                        content: std::mem::take(&mut message_parts),
                        annotations: Vec::new(),
                    }));
                }
                items.push(UnifiedItem::FileReference(UnifiedFileReferenceItem {
                    filename,
                    mime_type,
                    file_url: Some(url),
                    file_id: None,
                }));
            }
            other => message_parts.push(other),
        }
    }

    if !message_parts.is_empty() {
        items.push(UnifiedItem::Message(UnifiedMessageItem {
            role,
            content: message_parts,
            annotations: Vec::new(),
        }));
    }

    items
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedContentPart {
    Text {
        text: String,
    },
    Refusal {
        text: String,
    },
    Reasoning {
        text: String,
    },
    ImageUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>, // e.g., "low", "high", "auto"
    },
    ImageData {
        mime_type: String,
        data: String, // Base64 encoded
    },
    FileUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        mime_type: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    FileData {
        data: String,
        mime_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    ExecutableCode {
        language: String,
        code: String,
    },
    ToolCall(UnifiedToolCall),
    ToolResult(UnifiedToolResult),
}

impl UnifiedContentPart {
    /// Returns true if this content part is considered empty and should be filtered out
    pub fn is_empty(&self) -> bool {
        match self {
            UnifiedContentPart::Text { text } => text.trim().is_empty(),
            UnifiedContentPart::Refusal { text } => text.trim().is_empty(),
            UnifiedContentPart::Reasoning { text } => text.trim().is_empty(),
            UnifiedContentPart::ImageUrl { url, .. } => url.is_empty(),
            UnifiedContentPart::ImageData { data, .. } => data.is_empty(),
            UnifiedContentPart::FileUrl { url, .. } => url.is_empty(),
            UnifiedContentPart::FileData { data, .. } => data.is_empty(),
            UnifiedContentPart::ExecutableCode { code, .. } => code.is_empty(),
            // Tool calls and results are never considered empty as they have structural meaning
            UnifiedContentPart::ToolCall(_) | UnifiedContentPart::ToolResult(_) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedMessage {
    pub role: UnifiedRole,
    pub content: Vec<UnifiedContentPart>,
}

impl UnifiedMessage {
    /// Filters out empty content parts from this message
    pub fn filter_empty_content(mut self) -> Self {
        self.content.retain(|part| !part.is_empty());
        self
    }

    /// Returns true if this message has no meaningful content
    pub fn is_empty(&self) -> bool {
        self.content.is_empty() || self.content.iter().all(|p| p.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value, // JSON Schema
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTool {
    #[serde(rename = "type")]
    pub type_: String, // e.g. "function"
    pub function: UnifiedFunctionDefinition,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedOpenAiRequestExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passthrough: Option<Value>,
}

impl UnifiedOpenAiRequestExtension {
    pub fn is_empty(&self) -> bool {
        self.tool_choice.is_none()
            && self.n.is_none()
            && self.response_format.is_none()
            && self.logit_bias.is_none()
            && self.user.is_none()
            && self.passthrough.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedAnthropicRequestExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

impl UnifiedAnthropicRequestExtension {
    pub fn is_empty(&self) -> bool {
        self.metadata.is_none() && self.top_k.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedOllamaRequestExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

impl UnifiedOllamaRequestExtension {
    pub fn is_empty(&self) -> bool {
        self.format.is_none() && self.keep_alive.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponsesRequestExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
}

impl UnifiedResponsesRequestExtension {
    pub fn is_empty(&self) -> bool {
        self.instructions.is_none()
            && self.tool_choice.is_none()
            && self.text_format.is_none()
            && self.reasoning.is_none()
            && self.parallel_tool_calls.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedRequestExtensions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<UnifiedOpenAiRequestExtension>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<UnifiedAnthropicRequestExtension>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ollama: Option<UnifiedOllamaRequestExtension>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<UnifiedResponsesRequestExtension>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedRequest {
    pub model: Option<String>,
    pub messages: Vec<UnifiedMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<UnifiedItem>,
    pub tools: Option<Vec<UnifiedTool>>,
    pub stream: bool,

    // Common generation configs
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<UnifiedRequestExtensions>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedRequestCore {
    pub model: Option<String>,
    pub messages: Vec<UnifiedMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<UnifiedItem>,
    pub tools: Option<Vec<UnifiedTool>>,
    pub stream: bool,
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
}

impl UnifiedRequest {
    /// Filters out empty content parts and empty messages
    pub fn filter_empty(mut self) -> Self {
        // Filter empty content from each message
        self.messages = self
            .messages
            .into_iter()
            .map(|msg| msg.filter_empty_content())
            .filter(|msg| !msg.is_empty())
            .collect();
        self.items.retain(|item| !item.is_empty());
        self
    }

    pub fn content_items(&self) -> Vec<UnifiedItem> {
        if !self.items.is_empty() {
            return self.items.clone();
        }

        self.messages
            .iter()
            .flat_map(|message| {
                legacy_content_to_unified_items(message.role.clone(), message.content.clone())
            })
            .collect()
    }

    pub fn openai_extension(&self) -> Option<&UnifiedOpenAiRequestExtension> {
        self.extensions.as_ref().and_then(|ext| ext.openai.as_ref())
    }

    pub fn anthropic_extension(&self) -> Option<&UnifiedAnthropicRequestExtension> {
        self.extensions
            .as_ref()
            .and_then(|ext| ext.anthropic.as_ref())
    }

    pub fn ollama_extension(&self) -> Option<&UnifiedOllamaRequestExtension> {
        self.extensions.as_ref().and_then(|ext| ext.ollama.as_ref())
    }

    pub fn responses_extension(&self) -> Option<&UnifiedResponsesRequestExtension> {
        self.extensions
            .as_ref()
            .and_then(|ext| ext.responses.as_ref())
    }

    pub fn core(&self) -> UnifiedRequestCore {
        UnifiedRequestCore {
            model: self.model.clone(),
            messages: self.messages.clone(),
            items: self.items.clone(),
            tools: self.tools.clone(),
            stream: self.stream,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            top_p: self.top_p,
            stop: self.stop.clone(),
            seed: self.seed,
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
        }
    }

    pub fn into_core_and_extensions(
        self,
    ) -> (UnifiedRequestCore, Option<UnifiedRequestExtensions>) {
        (
            UnifiedRequestCore {
                model: self.model,
                messages: self.messages,
                items: self.items,
                tools: self.tools,
                stream: self.stream,
                temperature: self.temperature,
                max_tokens: self.max_tokens,
                top_p: self.top_p,
                stop: self.stop,
                seed: self.seed,
                presence_penalty: self.presence_penalty,
                frequency_penalty: self.frequency_penalty,
            },
            self.extensions,
        )
    }

    pub fn from_core_and_extensions(
        core: UnifiedRequestCore,
        extensions: Option<UnifiedRequestExtensions>,
    ) -> Self {
        Self {
            model: core.model,
            messages: core.messages,
            items: core.items,
            tools: core.tools,
            stream: core.stream,
            temperature: core.temperature,
            max_tokens: core.max_tokens,
            top_p: core.top_p,
            stop: core.stop,
            seed: core.seed,
            presence_penalty: core.presence_penalty,
            frequency_penalty: core.frequency_penalty,
            extensions,
        }
    }

    pub fn top_k(&self) -> Option<u32> {
        self.anthropic_extension().and_then(|ext| ext.top_k)
    }
}

// --- Unified Response ---

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_image_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_image_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedSyntheticMetadata {
    #[serde(default)]
    pub id: bool,
    #[serde(default)]
    pub model: bool,
    #[serde(default)]
    pub gemini_safety_ratings: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedOpenAiResponseExtension {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
}

impl UnifiedOpenAiResponseExtension {
    pub fn is_empty(&self) -> bool {
        self.system_fingerprint.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiSafetyRating {
    pub category: String,
    pub probability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiCitationSource {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_index: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiCitationMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citation_sources: Vec<UnifiedGeminiCitationSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiPromptFeedback {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety_ratings: Vec<UnifiedGeminiSafetyRating>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiCandidateMetadata {
    pub index: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub safety_ratings: Vec<UnifiedGeminiSafetyRating>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citation_metadata: Option<UnifiedGeminiCitationMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedGeminiResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_feedback: Option<UnifiedGeminiPromptFeedback>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<UnifiedGeminiCandidateMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedAnthropicResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesUrlCitation {
    pub url: String,
    pub start_index: u32,
    pub end_index: u32,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesRefusal {
    pub refusal: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesFileReference {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct UnifiedResponsesIncompleteDetails {
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedResponsesResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_cache_key: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub citations: Vec<UnifiedResponsesUrlCitation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refusals: Vec<UnifiedResponsesRefusal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<UnifiedResponsesFileReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<UnifiedResponsesIncompleteDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedProviderResponseMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<UnifiedGeminiResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<UnifiedAnthropicResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<UnifiedResponsesResponseMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedProviderSessionMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gemini: Option<UnifiedGeminiResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic: Option<UnifiedAnthropicResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<UnifiedResponsesResponseMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponseExtensions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai: Option<UnifiedOpenAiResponseExtension>,
}

impl UnifiedResponseExtensions {
    pub fn is_empty(&self) -> bool {
        self.openai
            .as_ref()
            .is_none_or(UnifiedOpenAiResponseExtension::is_empty)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponseCore {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedResponseContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<UnifiedResponseExtensions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_metadata: Option<UnifiedProviderResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl UnifiedResponseContext {
    pub fn is_empty(&self) -> bool {
        self.extensions
            .as_ref()
            .is_none_or(UnifiedResponseExtensions::is_empty)
            && self.provider_metadata.is_none()
            && self.synthetic_metadata.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedTransformDiagnosticKind {
    FatalTransformError,
    LossyTransform,
    CapabilityDowngrade,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedTransformDiagnosticAction {
    Send,
    Drop,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedTransformDiagnosticLossLevel {
    Lossless,
    LossyMinor,
    LossyMajor,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnifiedTransformDiagnostic {
    #[serde(rename = "type")]
    pub type_: String,
    pub diagnostic_kind: UnifiedTransformDiagnosticKind,
    pub provider: String,
    pub target_provider: String,
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub loss_level: UnifiedTransformDiagnosticLossLevel,
    pub action: UnifiedTransformDiagnosticAction,
    pub semantic_unit: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_data_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChoice {
    pub index: u32,
    pub message: UnifiedMessage,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<UnifiedItem>,
    pub finish_reason: Option<String>,
    // OpenAI-specific: log probabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<Value>,
}

impl UnifiedChoice {
    pub fn content_items(&self) -> Vec<UnifiedItem> {
        if !self.items.is_empty() {
            return self.items.clone();
        }

        legacy_content_to_unified_items(self.message.role.clone(), self.message.content.clone())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>, // e.g. "chat.completion"
    // OpenAI-specific: system fingerprint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_response_metadata: Option<UnifiedProviderResponseMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl UnifiedResponse {
    pub fn core(&self) -> UnifiedResponseCore {
        UnifiedResponseCore {
            id: self.id.clone(),
            model: self.model.clone(),
            choices: self.choices.clone(),
            usage: self.usage.clone(),
            created: self.created,
            object: self.object.clone(),
        }
    }

    pub fn extensions(&self) -> Option<UnifiedResponseExtensions> {
        let openai = (!UnifiedOpenAiResponseExtension {
            system_fingerprint: self.system_fingerprint.clone(),
        }
        .is_empty())
        .then(|| UnifiedOpenAiResponseExtension {
            system_fingerprint: self.system_fingerprint.clone(),
        });

        let extensions = UnifiedResponseExtensions { openai };
        (!extensions.is_empty()).then_some(extensions)
    }

    pub fn context(&self) -> UnifiedResponseContext {
        UnifiedResponseContext {
            extensions: self.extensions(),
            provider_metadata: self.provider_response_metadata.clone(),
            synthetic_metadata: self.synthetic_metadata.clone(),
        }
    }

    pub fn from_core_and_context(
        core: UnifiedResponseCore,
        context: UnifiedResponseContext,
    ) -> Self {
        Self {
            id: core.id,
            model: core.model,
            choices: core.choices,
            usage: core.usage,
            created: core.created,
            object: core.object,
            system_fingerprint: context
                .extensions
                .and_then(|ext| ext.openai)
                .and_then(|openai| openai.system_fingerprint),
            provider_response_metadata: context.provider_metadata,
            synthetic_metadata: context.synthetic_metadata,
        }
    }

    pub fn into_core_and_context(self) -> (UnifiedResponseCore, UnifiedResponseContext) {
        let system_fingerprint = self.system_fingerprint;
        (
            UnifiedResponseCore {
                id: self.id,
                model: self.model,
                choices: self.choices,
                usage: self.usage,
                created: self.created,
                object: self.object,
            },
            UnifiedResponseContext {
                extensions: (!UnifiedOpenAiResponseExtension {
                    system_fingerprint: system_fingerprint.clone(),
                }
                .is_empty())
                .then(|| UnifiedResponseExtensions {
                    openai: Some(UnifiedOpenAiResponseExtension { system_fingerprint }),
                }),
                provider_metadata: self.provider_response_metadata,
                synthetic_metadata: self.synthetic_metadata,
            },
        )
    }

    pub fn system_fingerprint(&self) -> Option<&str> {
        self.system_fingerprint.as_deref()
    }

    pub fn synthetic_metadata(&self) -> Option<&UnifiedSyntheticMetadata> {
        self.synthetic_metadata.as_ref()
    }

    pub fn provider_response_metadata(&self) -> Option<&UnifiedProviderResponseMetadata> {
        self.provider_response_metadata.as_ref()
    }
}

// --- Unified Chunk Response ---

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>, // Arguments will be streamed as a partial JSON string
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedContentPartDelta {
    TextDelta {
        index: u32,
        text: String,
    },
    ImageDelta {
        index: u32,
        url: Option<String>,
        data: Option<String>, // Base64 encoded
    },
    ToolCallDelta(UnifiedToolCallDelta),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedMessageDelta {
    pub role: Option<UnifiedRole>,
    pub content: Vec<UnifiedContentPartDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChunkChoice {
    pub index: u32,
    pub delta: UnifiedMessageDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChunkResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChunkChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>, // e.g. "chat.completion.chunk"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_metadata: Option<UnifiedProviderSessionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChunkResponseCore {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChunkChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChunkResponseContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_metadata: Option<UnifiedProviderSessionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl UnifiedChunkResponseContext {
    pub fn is_empty(&self) -> bool {
        self.provider_session_metadata.is_none() && self.synthetic_metadata.is_none()
    }
}

impl UnifiedChunkResponse {
    pub fn core(&self) -> UnifiedChunkResponseCore {
        UnifiedChunkResponseCore {
            id: self.id.clone(),
            model: self.model.clone(),
            choices: self.choices.clone(),
            usage: self.usage.clone(),
            created: self.created,
            object: self.object.clone(),
        }
    }

    pub fn context(&self) -> UnifiedChunkResponseContext {
        UnifiedChunkResponseContext {
            provider_session_metadata: self.provider_session_metadata.clone(),
            synthetic_metadata: self.synthetic_metadata.clone(),
        }
    }

    pub fn from_core_and_context(
        core: UnifiedChunkResponseCore,
        context: UnifiedChunkResponseContext,
    ) -> Self {
        Self {
            id: core.id,
            model: core.model,
            choices: core.choices,
            usage: core.usage,
            created: core.created,
            object: core.object,
            provider_session_metadata: context.provider_session_metadata,
            synthetic_metadata: context.synthetic_metadata,
        }
    }

    pub fn into_core_and_context(self) -> (UnifiedChunkResponseCore, UnifiedChunkResponseContext) {
        (
            UnifiedChunkResponseCore {
                id: self.id,
                model: self.model,
                choices: self.choices,
                usage: self.usage,
                created: self.created,
                object: self.object,
            },
            UnifiedChunkResponseContext {
                provider_session_metadata: self.provider_session_metadata,
                synthetic_metadata: self.synthetic_metadata,
            },
        )
    }

    pub fn synthetic_metadata(&self) -> Option<&UnifiedSyntheticMetadata> {
        self.synthetic_metadata.as_ref()
    }

    pub fn provider_session_metadata(&self) -> Option<&UnifiedProviderSessionMetadata> {
        self.provider_session_metadata.as_ref()
    }
}

// --- Unified Stream Event IR ---

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedBlockKind {
    Text,
    ToolCall,
    Reasoning,
    Blob,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedStreamEvent {
    ItemAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        item: UnifiedItem,
    },
    ItemDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        item: UnifiedItem,
    },
    MessageStart {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        role: UnifiedRole,
    },
    ContentPartAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<UnifiedContentPart>,
    },
    ContentPartDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
    },
    MessageDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    MessageStop,
    ContentBlockStart {
        index: u32,
        kind: UnifiedBlockKind,
    },
    ContentBlockDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        part_index: Option<u32>,
        text: String,
    },
    ContentBlockStop {
        index: u32,
    },
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments: String,
    },
    ToolCallStop {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    ReasoningStart {
        index: u32,
    },
    ReasoningSummaryPartAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<UnifiedContentPart>,
    },
    ReasoningSummaryPartDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
    },
    ReasoningDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        part_index: Option<u32>,
        text: String,
    },
    ReasoningStop {
        index: u32,
    },
    BlobDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        data: Value,
    },
    Usage {
        usage: UnifiedUsage,
    },
    Error {
        error: Value,
    },
}

// --- Finish Reason Mapping Utilities ---

/// Maps a finish reason from Gemini format to OpenAI-compatible format
///
/// Gemini finish reasons:
/// - "STOP": Natural completion → "stop"
/// - "TOOL_USE": Tool was called → "tool_calls"
/// - "MAX_TOKENS": Hit token limit → "length"
/// - "SAFETY" / "RECITATION": Content filtered → "content_filter"
/// - Other: Unknown reason → "stop" (default)
pub fn map_gemini_finish_reason_to_openai(reason: &str, has_tool_call: bool) -> String {
    match reason {
        "STOP" => {
            if has_tool_call {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }
        }
        "TOOL_USE" => "tool_calls".to_string(),
        "MAX_TOKENS" => "length".to_string(),
        "SAFETY" | "RECITATION" => "content_filter".to_string(),
        _ => "stop".to_string(),
    }
}

/// Maps a finish reason from OpenAI-compatible format to Gemini format
///
/// OpenAI finish reasons:
/// - "stop": Natural completion → "STOP"
/// - "length": Hit token limit → "MAX_TOKENS"
/// - "content_filter": Content filtered → "SAFETY"
/// - "tool_calls": Tool was called → "TOOL_USE"
/// - Other: Unknown reason → "FINISH_REASON_UNSPECIFIED"
pub fn map_openai_finish_reason_to_gemini(reason: &str) -> String {
    match reason {
        "stop" => "STOP".to_string(),
        "length" => "MAX_TOKENS".to_string(),
        "content_filter" => "SAFETY".to_string(),
        "tool_calls" => "TOOL_USE".to_string(),
        _ => "FINISH_REASON_UNSPECIFIED".to_string(),
    }
}

/// Maps a finish reason from Anthropic format to OpenAI-compatible format
///
/// Anthropic finish reasons:
/// - "end_turn" / "stop_sequence": Natural completion → "stop"
/// - "tool_use": Tool was called → "tool_calls"
/// - "max_tokens": Hit token limit → "length"
/// - Other: Unknown reason → "stop" (default)
pub fn map_anthropic_finish_reason_to_openai(reason: &str) -> String {
    match reason {
        "end_turn" | "stop_sequence" => "stop".to_string(),
        "tool_use" => "tool_calls".to_string(),
        "max_tokens" => "length".to_string(),
        _ => "stop".to_string(),
    }
}

/// Maps a finish reason from OpenAI-compatible format to Anthropic format
///
/// OpenAI finish reasons:
/// - "stop": Natural completion → "end_turn"
/// - "tool_calls": Tool was called → "tool_use"
/// - "length": Hit token limit → "max_tokens"
/// - Other: Unknown reason → "end_turn" (default)
pub fn map_openai_finish_reason_to_anthropic(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "length" => "max_tokens".to_string(),
        _ => "end_turn".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_unified_request_core_and_extensions_round_trip() {
        let request = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text {
                    text: "hello".to_string(),
                }],
            }],
            items: vec![UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                id: "call_1".to_string(),
                name: "lookup".to_string(),
                arguments: json!({"city": "Boston"}),
            })],
            tools: Some(vec![UnifiedTool {
                type_: "function".to_string(),
                function: UnifiedFunctionDefinition {
                    name: "lookup".to_string(),
                    description: Some("Finds weather".to_string()),
                    parameters: json!({"type": "object"}),
                },
            }]),
            stream: true,
            temperature: Some(0.2),
            max_tokens: Some(128),
            top_p: Some(0.9),
            stop: Some(vec!["DONE".to_string()]),
            seed: Some(7),
            presence_penalty: Some(0.1),
            frequency_penalty: Some(0.2),
            extensions: Some(UnifiedRequestExtensions {
                openai: Some(UnifiedOpenAiRequestExtension {
                    tool_choice: Some(json!("auto")),
                    ..Default::default()
                }),
                ..Default::default()
            }),
        };

        let core = request.core();
        let (owned_core, extensions) = request.clone().into_core_and_extensions();

        assert_eq!(core.model, owned_core.model);
        assert_eq!(core.messages.len(), owned_core.messages.len());
        assert_eq!(core.messages[0].role, owned_core.messages[0].role);
        assert_eq!(core.messages[0].content, owned_core.messages[0].content);
        assert_eq!(core.items, owned_core.items);
        assert_eq!(core.stream, owned_core.stream);
        assert!(
            extensions
                .as_ref()
                .and_then(|ext| ext.openai.as_ref())
                .is_some()
        );

        let rebuilt = UnifiedRequest::from_core_and_extensions(owned_core, extensions);
        assert_eq!(rebuilt.model, request.model);
        assert_eq!(rebuilt.messages.len(), request.messages.len());
        assert_eq!(rebuilt.messages[0].role, request.messages[0].role);
        assert_eq!(rebuilt.messages[0].content, request.messages[0].content);
        assert_eq!(rebuilt.items, request.items);
        assert_eq!(rebuilt.extensions.is_some(), request.extensions.is_some());
    }

    #[test]
    fn test_unified_response_and_chunk_layering_round_trip() {
        let response = UnifiedResponse {
            id: "resp_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "done".to_string(),
                    }],
                },
                items: vec![],
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            }),
            created: Some(1),
            object: Some("response".to_string()),
            system_fingerprint: Some("fp_123".to_string()),
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                responses: Some(UnifiedResponsesResponseMetadata {
                    safety_identifier: Some("safe".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            synthetic_metadata: Some(UnifiedSyntheticMetadata {
                id: true,
                model: false,
                gemini_safety_ratings: false,
            }),
        };

        let response_core = response.core();
        let response_context = response.context();
        assert_eq!(response_core.id, "resp_1");
        assert_eq!(
            response_context
                .extensions
                .as_ref()
                .and_then(|ext| ext.openai.as_ref())
                .and_then(|openai| openai.system_fingerprint.as_deref()),
            Some("fp_123")
        );
        assert!(response_context.provider_metadata.is_some());
        assert!(response_context.synthetic_metadata.is_some());

        let rebuilt_response =
            UnifiedResponse::from_core_and_context(response_core, response_context);
        assert_eq!(rebuilt_response.system_fingerprint(), Some("fp_123"));
        assert!(rebuilt_response.provider_response_metadata().is_some());
        assert!(rebuilt_response.synthetic_metadata().is_some());

        let chunk = UnifiedChunkResponse {
            id: "chunk_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "hi".to_string(),
                    }],
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
                ..Default::default()
            }),
            created: Some(2),
            object: Some("response.chunk".to_string()),
            provider_session_metadata: Some(UnifiedProviderSessionMetadata {
                anthropic: Some(UnifiedAnthropicResponseMetadata {
                    role: Some("assistant".to_string()),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            synthetic_metadata: Some(UnifiedSyntheticMetadata {
                id: false,
                model: true,
                gemini_safety_ratings: false,
            }),
        };

        let (chunk_core, chunk_context) = chunk.clone().into_core_and_context();
        assert_eq!(chunk_core.id, "chunk_1");
        assert!(chunk_context.provider_session_metadata.is_some());
        assert!(chunk_context.synthetic_metadata.is_some());

        let rebuilt_chunk = UnifiedChunkResponse::from_core_and_context(chunk_core, chunk_context);
        assert_eq!(rebuilt_chunk.model, chunk.model);
        assert!(rebuilt_chunk.provider_session_metadata().is_some());
        assert!(rebuilt_chunk.synthetic_metadata().is_some());
    }
}
