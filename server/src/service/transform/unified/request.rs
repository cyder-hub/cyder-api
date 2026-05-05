use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    pub name: Option<String>,
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
        detail: Option<String>,
    },
    ImageData {
        mime_type: String,
        data: String,
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
    pub fn filter_empty_content(mut self) -> Self {
        self.content.retain(|part| !part.is_empty());
        self
    }

    pub fn is_empty(&self) -> bool {
        self.content.is_empty() || self.content.iter().all(|p| p.is_empty())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedFunctionDefinition {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedTool {
    #[serde(rename = "type")]
    pub type_: String,
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
    pub fn filter_empty(mut self) -> Self {
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
