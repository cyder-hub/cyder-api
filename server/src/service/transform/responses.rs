use chrono::Utc;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Value, json};

use super::{
    StreamTransformer, TransformProtocol, TransformValueKind, apply_transform_policy, openai,
    unified::*,
};
use crate::schema::enum_def::LlmApiType;
use crate::utils::sse::SseEvent;

// --- Request Payloads ---

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponsesRequestPayload {
    pub model: String,
    pub input: Input,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<TextField>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<Reasoning>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    #[serde(rename = "max_output_tokens")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Input {
    String(String),
    Items(Vec<ItemField>),
}

// --- Request Transformation ---

impl From<UnifiedRequest> for ResponsesRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let responses_extension = unified_req
            .responses_extension()
            .cloned()
            .unwrap_or_default();
        let openai_extension = unified_req.openai_extension().cloned().unwrap_or_default();

        let mut inferred_instructions = Vec::new();

        let items = if !unified_req.items.is_empty() {
            unified_req
                .items
                .into_iter()
                .flat_map(|item| match item {
                    UnifiedItem::Message(msg) if msg.role == UnifiedRole::System => {
                        let text = msg
                            .content
                            .into_iter()
                            .filter_map(|part| {
                                if matches!(
                                    part,
                                    UnifiedContentPart::Text { .. }
                                        | UnifiedContentPart::Refusal { .. }
                                        | UnifiedContentPart::Reasoning { .. }
                                ) {
                                    return render_responses_instruction_part(part);
                                }

                                let keep = apply_transform_policy(
                                    TransformProtocol::Unified,
                                    TransformProtocol::Api(LlmApiType::Responses),
                                    TransformValueKind::from(&part),
                                    "Downgrading rich system content to recoverable instruction text during Responses request conversion.",
                                );
                                keep.then(|| render_responses_instruction_part(part)).flatten()
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        if !text.trim().is_empty() {
                            inferred_instructions.push(text);
                        }
                        Vec::new()
                    }
                    UnifiedItem::Message(msg) => {
                        unified_message_to_responses_input_items(UnifiedMessage {
                            role: msg.role,
                            content: msg.content,
                        })
                    }
                    UnifiedItem::Reasoning(item) => vec![ItemField::Reasoning(ReasoningBody {
                        _type: "reasoning".to_string(),
                        id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                        content: Some(
                            item.content
                                .into_iter()
                                .map(unified_reasoning_part_to_responses_part)
                                .collect(),
                        ),
                        summary: Vec::new(),
                        encrypted_content: None,
                    })],
                    UnifiedItem::FunctionCall(call) => vec![ItemField::FunctionCall(FunctionCall {
                        _type: "function_call".to_string(),
                        id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: call.id,
                        name: call.name,
                        arguments: stringify_function_arguments(call.arguments),
                        status: MessageStatus::Completed,
                    })],
                    UnifiedItem::FunctionCallOutput(output) => vec![ItemField::FunctionCallOutput(
                        FunctionCallOutput {
                            _type: "function_call_output".to_string(),
                            id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                            call_id: output.tool_call_id,
                            output: unified_tool_result_to_function_output_payload(output.output),
                            status: MessageStatus::Completed,
                        },
                    )],
        UnifiedItem::FileReference(file) => vec![ItemField::Message(Message {
            _type: "message".to_string(),
            id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
            role: MessageRole::User,
            status: MessageStatus::Completed,
            content: vec![ItemContentPart::InputFile {
                filename: file.filename,
                file_url: file.file_url,
                file_id: file.file_id,
                file_data: None,
            }],
        })],
                })
                .collect()
        } else {
            let mut items = Vec::new();
            for message in unified_req.messages {
                if message.role == UnifiedRole::System {
                    let text = message
                        .content
                        .into_iter()
                        .filter_map(|part| {
                            if matches!(
                                part,
                                UnifiedContentPart::Text { .. }
                                    | UnifiedContentPart::Refusal { .. }
                                    | UnifiedContentPart::Reasoning { .. }
                            ) {
                                return render_responses_instruction_part(part);
                            }

                            let keep = apply_transform_policy(
                                TransformProtocol::Unified,
                                TransformProtocol::Api(LlmApiType::Responses),
                                TransformValueKind::from(&part),
                                "Downgrading rich system content to recoverable instruction text during Responses request conversion.",
                            );
                            keep.then(|| render_responses_instruction_part(part)).flatten()
                        })
                        .collect::<Vec<_>>()
                        .join("\n");

                    if !text.trim().is_empty() {
                        inferred_instructions.push(text);
                    }
                } else {
                    items.extend(unified_message_to_responses_input_items(message));
                }
            }
            items
        };

        let instructions = responses_extension.instructions.or_else(|| {
            if inferred_instructions.is_empty() {
                None
            } else {
                Some(inferred_instructions.join("\n\n"))
            }
        });

        let tools = unified_req.tools.map(|items| {
            items
                .into_iter()
                .map(|tool| {
                    Tool::Function(FunctionTool {
                        name: tool.function.name,
                        description: tool.function.description,
                        parameters: Some(tool.function.parameters),
                        strict: None,
                    })
                })
                .collect()
        });

        let tool_choice = responses_extension
            .tool_choice
            .and_then(|value| serde_json::from_value(value).ok())
            .or_else(|| {
                openai_extension
                    .tool_choice
                    .and_then(convert_openai_tool_choice_to_responses)
            });

        let text = responses_extension
            .text_format
            .and_then(|value| serde_json::from_value(value).ok())
            .or_else(|| {
                openai_extension
                    .response_format
                    .and_then(convert_openai_response_format_to_responses)
            })
            .map(|format| TextField {
                format,
                verbosity: None,
            });

        let reasoning = responses_extension
            .reasoning
            .and_then(|value| serde_json::from_value(value).ok())
            .or_else(|| {
                openai_extension
                    .passthrough
                    .as_ref()
                    .and_then(convert_openai_passthrough_to_responses_reasoning)
            });

        let parallel_tool_calls = responses_extension.parallel_tool_calls.or_else(|| {
            openai_extension
                .passthrough
                .as_ref()
                .and_then(|value| value.get("parallel_tool_calls"))
                .and_then(Value::as_bool)
        });

        ResponsesRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            input: Input::Items(items),
            instructions,
            tools,
            tool_choice,
            text,
            reasoning,
            parallel_tool_calls,
            stream: Some(unified_req.stream),
            temperature: unified_req.temperature,
            max_tokens: unified_req.max_tokens,
            top_p: unified_req.top_p,
        }
    }
}

// --- Response Payloads & Transformation ---
// --- Response Payloads & Transformation ---

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Developer,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum MessageStatus {
    InProgress,
    Completed,
    Incomplete,
}

fn default_message_id() -> String {
    format!("msg_{}", crate::utils::ID_GENERATOR.generate_id())
}

fn default_function_call_id() -> String {
    format!("fc_{}", crate::utils::ID_GENERATOR.generate_id())
}

fn default_function_call_output_id() -> String {
    format!("fco_{}", crate::utils::ID_GENERATOR.generate_id())
}

fn default_completed_status() -> MessageStatus {
    MessageStatus::Completed
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IncompleteDetails {
    pub reason: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Error {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Tool {
    Function(FunctionTool),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionTool {
    pub name: String,
    pub description: Option<String>,
    pub parameters: Option<Value>,
    pub strict: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum ToolChoice {
    Value(ToolChoiceValue),
    Specific(SpecificToolChoice),
    Allowed(AllowedToolChoice),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ToolChoiceValue {
    None,
    Auto,
    Required,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SpecificToolChoice {
    #[serde(rename = "type")]
    pub _type: String, // "function"
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AllowedToolChoice {
    #[serde(rename = "type")]
    pub _type: String, // "allowed_tools"
    pub tools: Vec<SpecificToolChoice>,
    pub mode: ToolChoiceValue,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Truncation {
    Auto,
    Disabled,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TextField {
    pub format: TextResponseFormat,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verbosity: Option<Verbosity>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextResponseFormat {
    Text,
    JsonObject,
    JsonSchema {
        name: String,
        description: Option<String>,
        schema: Option<Value>,
        strict: bool,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Verbosity {
    Low,
    Medium,
    High,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Reasoning {
    pub effort: Option<ReasoningEffort>,
    pub summary: Option<ReasoningSummary>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningEffort {
    None,
    Low,
    Medium,
    High,
    Xhigh,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningSummary {
    Concise,
    Detailed,
    Auto,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct InputTokensDetails {
    pub cached_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct OutputTokensDetails {
    pub reasoning_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    pub input_tokens_details: InputTokensDetails,
    pub output_tokens_details: OutputTokensDetails,
}

impl From<Usage> for UnifiedUsage {
    fn from(usage: Usage) -> Self {
        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cached_tokens: Some(usage.input_tokens_details.cached_tokens),
            reasoning_tokens: Some(usage.output_tokens_details.reasoning_tokens),
            ..Default::default()
        }
    }
}

impl From<UnifiedUsage> for Usage {
    fn from(unified_usage: UnifiedUsage) -> Self {
        Self {
            input_tokens: unified_usage.input_tokens,
            output_tokens: unified_usage.output_tokens,
            total_tokens: unified_usage.total_tokens,
            input_tokens_details: InputTokensDetails {
                cached_tokens: unified_usage.cached_tokens.unwrap_or(0),
            },
            output_tokens_details: OutputTokensDetails {
                reasoning_tokens: unified_usage.reasoning_tokens.unwrap_or(0),
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum ItemField {
    Message(Message),
    FunctionCall(FunctionCall),
    FunctionCallOutput(FunctionCallOutput),
    Reasoning(ReasoningBody),
    Unknown(Value),
}

impl Serialize for ItemField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ItemField::Message(value) => value.serialize(serializer),
            ItemField::FunctionCall(value) => value.serialize(serializer),
            ItemField::FunctionCallOutput(value) => value.serialize(serializer),
            ItemField::Reasoning(value) => value.serialize(serializer),
            ItemField::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ItemField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let type_name = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match type_name {
            "message" => serde_json::from_value(value)
                .map(ItemField::Message)
                .map_err(serde::de::Error::custom),
            "function_call" => serde_json::from_value(value)
                .map(ItemField::FunctionCall)
                .map_err(serde::de::Error::custom),
            "function_call_output" => serde_json::from_value(value)
                .map(ItemField::FunctionCallOutput)
                .map_err(serde::de::Error::custom),
            "reasoning" => serde_json::from_value(value)
                .map(ItemField::Reasoning)
                .map_err(serde::de::Error::custom),
            _ => try_deserialize_shorthand_message(&value)
                .map(ItemField::Message)
                .or_else(|| Some(ItemField::Unknown(value)))
                .ok_or_else(|| serde::de::Error::custom("failed to deserialize item")),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShorthandMessageContent {
    Text(String),
    Parts(Vec<ItemContentPart>),
}

#[derive(Debug, Deserialize)]
struct ShorthandMessage {
    role: MessageRole,
    content: ShorthandMessageContent,
}

fn try_deserialize_shorthand_message(value: &Value) -> Option<Message> {
    let shorthand: ShorthandMessage = serde_json::from_value(value.clone()).ok()?;
    let content = match shorthand.content {
        ShorthandMessageContent::Text(text) => vec![ItemContentPart::InputText { text }],
        ShorthandMessageContent::Parts(parts) => parts,
    };

    Some(Message {
        _type: "message".to_string(),
        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
        status: MessageStatus::Completed,
        role: shorthand.role,
        content,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "type")]
    pub _type: String,
    #[serde(default = "default_message_id")]
    pub id: String,
    #[serde(default = "default_completed_status")]
    pub status: MessageStatus,
    pub role: MessageRole,
    pub content: Vec<ItemContentPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemContentPart {
    InputText {
        text: String,
    },
    OutputText {
        text: String,
        annotations: Vec<Annotation>,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<LogProb>>,
    },
    Text {
        text: String,
    },
    SummaryText {
        text: String,
    },
    ReasoningText {
        text: String,
    },
    Refusal {
        refusal: String,
    },
    InputImage {
        image_url: Option<String>,
        detail: String,
    },
    InputFile {
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_data: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Annotation {
    UrlCitation {
        url: String,
        start_index: u32,
        end_index: u32,
        title: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogProb {
    pub token: String,
    pub logprob: f64,
    pub bytes: Vec<u8>,
    pub top_logprobs: Vec<TopLogProb>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopLogProb {
    pub token: String,
    pub logprob: f64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    #[serde(rename = "type")]
    pub _type: String,
    #[serde(default = "default_function_call_id")]
    pub id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    #[serde(default = "default_completed_status")]
    pub status: MessageStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCallOutput {
    #[serde(rename = "type")]
    pub _type: String,
    #[serde(default = "default_function_call_output_id")]
    pub id: String,
    pub call_id: String,
    pub output: FunctionCallOutputPayload,
    #[serde(default = "default_completed_status")]
    pub status: MessageStatus,
}

#[derive(Debug, Clone)]
pub enum FunctionCallOutputPayload {
    Text(String),
    Content(Vec<FunctionCallOutputContent>),
    Unknown(Value),
}

impl Serialize for FunctionCallOutputPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Text(value) => value.serialize(serializer),
            Self::Content(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionCallOutputPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(text) => Ok(Self::Text(text)),
            Value::Array(items) => {
                let content = items
                    .into_iter()
                    .map(FunctionCallOutputContent::from_value)
                    .collect();
                Ok(Self::Content(content))
            }
            other => Ok(Self::Unknown(other)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FunctionCallOutputContent {
    Text {
        text: String,
    },
    File {
        filename: Option<String>,
        file_url: Option<String>,
    },
    Image {
        image_url: Option<String>,
        file_url: Option<String>,
    },
    Unknown(Value),
}

impl FunctionCallOutputContent {
    fn from_value(value: Value) -> Self {
        let type_name = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match type_name {
            "text" | "output_text" => Self::Text {
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "file" => Self::File {
                filename: value
                    .get("filename")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                file_url: value
                    .get("file_url")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            },
            "image" => Self::Image {
                image_url: value
                    .get("image_url")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                file_url: value
                    .get("file_url")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            },
            _ => Self::Unknown(value),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Text { text } => json!({
                "type": "text",
                "text": text
            }),
            Self::File { filename, file_url } => json!({
                "type": "file",
                "filename": filename,
                "file_url": file_url
            }),
            Self::Image {
                image_url,
                file_url,
            } => json!({
                "type": "image",
                "image_url": image_url,
                "file_url": file_url
            }),
            Self::Unknown(value) => value.clone(),
        }
    }
}

impl Serialize for FunctionCallOutputContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FunctionCallOutputContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::from_value(Value::deserialize(deserializer)?))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReasoningBody {
    #[serde(rename = "type")]
    pub _type: String,
    pub id: String,
    pub content: Option<Vec<ItemContentPart>>,
    pub summary: Vec<ItemContentPart>,
    pub encrypted_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ResponsesReasoningMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encrypted_contents: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: ResponseObject,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub status: ResponseStatus,
    pub incomplete_details: Option<IncompleteDetails>,
    pub model: String,
    pub previous_response_id: Option<String>,
    pub instructions: Option<String>,
    pub output: Vec<ItemField>,
    pub error: Option<Error>,
    pub tools: Vec<Tool>,
    pub tool_choice: ToolChoice,
    pub truncation: Truncation,
    pub parallel_tool_calls: bool,
    pub text: TextField,
    pub top_p: f64,
    pub presence_penalty: f64,
    pub frequency_penalty: f64,
    pub top_logprobs: u32,
    pub temperature: f64,
    pub reasoning: Option<Reasoning>,
    pub usage: Option<Usage>,
    pub max_output_tokens: Option<u32>,
    pub max_tool_calls: Option<u32>,
    pub store: bool,
    pub background: bool,
    pub service_tier: ServiceTier,
    pub metadata: Value,
    pub safety_identifier: Option<String>,
    pub prompt_cache_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseObject {
    Response,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    InProgress,
    Completed,
    Incomplete,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    Default,
}

fn convert_openai_tool_choice_to_responses(value: Value) -> Option<ToolChoice> {
    match value {
        Value::String(value) => match value.as_str() {
            "none" => Some(ToolChoice::Value(ToolChoiceValue::None)),
            "auto" => Some(ToolChoice::Value(ToolChoiceValue::Auto)),
            "required" => Some(ToolChoice::Value(ToolChoiceValue::Required)),
            _ => None,
        },
        other => serde_json::from_value(other).ok(),
    }
}

fn convert_openai_response_format_to_responses(value: Value) -> Option<TextResponseFormat> {
    match value {
        Value::Object(map) => match map.get("type").and_then(Value::as_str) {
            Some("json_object") => Some(TextResponseFormat::JsonObject),
            Some("json_schema") => {
                let schema = map.get("json_schema")?;
                Some(TextResponseFormat::JsonSchema {
                    name: schema.get("name")?.as_str()?.to_string(),
                    description: schema
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    schema: schema.get("schema").cloned(),
                    strict: schema
                        .get("strict")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            }
            Some("text") => Some(TextResponseFormat::Text),
            _ => serde_json::from_value(Value::Object(map)).ok(),
        },
        other => serde_json::from_value(other).ok(),
    }
}

fn convert_openai_passthrough_to_responses_reasoning(value: &Value) -> Option<Reasoning> {
    let effort = value.get("reasoning_effort")?;
    Some(Reasoning {
        effort: serde_json::from_value(effort.clone()).ok(),
        summary: None,
    })
}

fn parse_function_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

fn stringify_function_arguments(arguments: Value) -> String {
    match arguments {
        Value::String(value) => value,
        other => serde_json::to_string(&other).unwrap_or_default(),
    }
}

fn function_output_payload_to_unified(
    output: FunctionCallOutputPayload,
) -> UnifiedToolResultOutput {
    match output {
        FunctionCallOutputPayload::Text(text) => UnifiedToolResultOutput::Text { text },
        FunctionCallOutputPayload::Content(parts) => UnifiedToolResultOutput::Content {
            parts: parts
                .into_iter()
                .map(|part| match part {
                    FunctionCallOutputContent::Text { text } => {
                        UnifiedToolResultPart::Text { text }
                    }
                    FunctionCallOutputContent::File { filename, file_url } => {
                        UnifiedToolResultPart::File { filename, file_url }
                    }
                    FunctionCallOutputContent::Image {
                        image_url,
                        file_url,
                    } => UnifiedToolResultPart::Image {
                        image_url,
                        file_url,
                    },
                    FunctionCallOutputContent::Unknown(value) => {
                        UnifiedToolResultPart::Json { value }
                    }
                })
                .collect(),
        },
        FunctionCallOutputPayload::Unknown(value) => unified_tool_result_output_from_value(value),
    }
}

fn unified_tool_result_to_function_output_payload(
    output: UnifiedToolResultOutput,
) -> FunctionCallOutputPayload {
    match output {
        UnifiedToolResultOutput::Text { text } => FunctionCallOutputPayload::Text(text),
        UnifiedToolResultOutput::Content { parts } => FunctionCallOutputPayload::Content(
            parts
                .into_iter()
                .map(|part| match part {
                    UnifiedToolResultPart::Text { text } => {
                        FunctionCallOutputContent::Text { text }
                    }
                    UnifiedToolResultPart::File { filename, file_url } => {
                        FunctionCallOutputContent::File { filename, file_url }
                    }
                    UnifiedToolResultPart::Image {
                        image_url,
                        file_url,
                    } => FunctionCallOutputContent::Image {
                        image_url,
                        file_url,
                    },
                    UnifiedToolResultPart::Json { value } => {
                        FunctionCallOutputContent::Unknown(value)
                    }
                })
                .collect(),
        ),
        UnifiedToolResultOutput::File { filename, file_url } => {
            FunctionCallOutputPayload::Content(vec![FunctionCallOutputContent::File {
                filename,
                file_url,
            }])
        }
        UnifiedToolResultOutput::Image {
            image_url,
            file_url,
        } => FunctionCallOutputPayload::Content(vec![FunctionCallOutputContent::Image {
            image_url,
            file_url,
        }]),
        UnifiedToolResultOutput::Json { value } => FunctionCallOutputPayload::Unknown(value),
    }
}

fn build_data_url(mime_type: &str, data: &str) -> String {
    format!("data:{mime_type};base64,{data}")
}

fn render_executable_code_text(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

fn render_responses_file_reference_text(
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

fn render_responses_inline_file_data_text(
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

fn parse_responses_input_file_data(
    file_data: &str,
    filename: Option<String>,
) -> UnifiedContentPart {
    if let Some(rest) = file_data.strip_prefix("data:") {
        let mut split = rest.splitn(2, ';');
        let mime_type = split.next().unwrap_or("application/octet-stream");
        if let Some(payload) = split.next().and_then(|value| value.strip_prefix("base64,")) {
            return UnifiedContentPart::FileData {
                data: payload.to_string(),
                mime_type: mime_type.to_string(),
                filename,
            };
        }
    }

    UnifiedContentPart::FileData {
        data: file_data.to_string(),
        mime_type: "application/octet-stream".to_string(),
        filename,
    }
}

fn render_responses_instruction_part(part: UnifiedContentPart) -> Option<String> {
    match part {
        UnifiedContentPart::Text { text }
        | UnifiedContentPart::Refusal { text }
        | UnifiedContentPart::Reasoning { text } => Some(text),
        UnifiedContentPart::ImageUrl { url, detail } => Some(match detail {
            Some(detail) if !detail.is_empty() => format!("image_url: {url}\ndetail: {detail}"),
            _ => format!("image_url: {url}"),
        }),
        UnifiedContentPart::ImageData { mime_type, data } => {
            Some(build_data_url(&mime_type, &data))
        }
        UnifiedContentPart::FileUrl {
            url,
            mime_type,
            filename,
        } => Some(render_responses_file_reference_text(
            &url,
            mime_type.as_deref(),
            filename.as_deref(),
        )),
        UnifiedContentPart::FileData {
            data,
            mime_type,
            filename,
        } => Some(render_responses_inline_file_data_text(
            &data,
            &mime_type,
            filename.as_deref(),
        )),
        UnifiedContentPart::ExecutableCode { language, code } => {
            Some(render_executable_code_text(&language, &code))
        }
        UnifiedContentPart::ToolCall(call) => Some(format!(
            "tool_call: {}\narguments: {}",
            call.name,
            serde_json::to_string(&call.arguments).unwrap_or_default()
        )),
        UnifiedContentPart::ToolResult(result) => Some(match result.name {
            Some(ref name) if !name.is_empty() => format!(
                "tool_result: {name}\ntool_call_id: {}\ncontent: {}",
                result.tool_call_id,
                result.legacy_content()
            ),
            _ => format!(
                "tool_result_id: {}\ncontent: {}",
                result.tool_call_id,
                result.legacy_content()
            ),
        }),
    }
}

fn unified_role_to_message(role: UnifiedRole) -> MessageRole {
    match role {
        UnifiedRole::User => MessageRole::User,
        UnifiedRole::Assistant | UnifiedRole::Tool => MessageRole::Assistant,
        UnifiedRole::System => MessageRole::System,
    }
}

fn message_role_to_unified(role: MessageRole) -> UnifiedRole {
    match role {
        MessageRole::User => UnifiedRole::User,
        MessageRole::Assistant => UnifiedRole::Assistant,
        MessageRole::System | MessageRole::Developer => UnifiedRole::System,
    }
}

fn unified_role_to_message_role(role: UnifiedRole) -> MessageRole {
    match role {
        UnifiedRole::User => MessageRole::User,
        UnifiedRole::Assistant | UnifiedRole::Tool => MessageRole::Assistant,
        UnifiedRole::System => MessageRole::System,
    }
}

fn push_message_item(
    items: &mut Vec<ItemField>,
    role: UnifiedRole,
    buffer: &mut Vec<ItemContentPart>,
) {
    if buffer.is_empty() {
        return;
    }

    items.push(ItemField::Message(Message {
        _type: "message".to_string(),
        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
        status: MessageStatus::Completed,
        role: unified_role_to_message(role),
        content: std::mem::take(buffer),
    }));
}

fn unified_message_to_responses_input_items(message: UnifiedMessage) -> Vec<ItemField> {
    let mut items = Vec::new();
    let mut message_buffer = Vec::new();

    for part in message.content {
        match part {
            UnifiedContentPart::Text { text } => {
                message_buffer.push(ItemContentPart::InputText { text });
            }
            UnifiedContentPart::Refusal { text } => {
                message_buffer.push(ItemContentPart::Refusal { refusal: text });
            }
            UnifiedContentPart::Reasoning { text } => {
                push_message_item(&mut items, message.role.clone(), &mut message_buffer);
                items.push(ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                    content: None,
                    summary: vec![ItemContentPart::SummaryText { text }],
                    encrypted_content: None,
                }));
            }
            UnifiedContentPart::ImageUrl { url, detail } => {
                message_buffer.push(ItemContentPart::InputImage {
                    image_url: Some(url),
                    detail: detail.unwrap_or_else(|| "auto".to_string()),
                });
            }
            UnifiedContentPart::ImageData { mime_type, data } => {
                message_buffer.push(ItemContentPart::InputImage {
                    image_url: Some(build_data_url(&mime_type, &data)),
                    detail: "auto".to_string(),
                });
            }
            UnifiedContentPart::FileUrl { url, filename, .. } => {
                message_buffer.push(ItemContentPart::InputFile {
                    filename,
                    file_url: Some(url),
                    file_id: None,
                    file_data: None,
                });
            }
            UnifiedContentPart::FileData {
                data,
                mime_type,
                filename,
            } => {
                message_buffer.push(ItemContentPart::InputFile {
                    filename,
                    file_url: None,
                    file_id: None,
                    file_data: Some(build_data_url(&mime_type, &data)),
                });
            }
            UnifiedContentPart::ExecutableCode { language, code } => {
                message_buffer.push(ItemContentPart::InputText {
                    text: render_executable_code_text(&language, &code),
                });
            }
            UnifiedContentPart::ToolCall(call) => {
                push_message_item(&mut items, message.role.clone(), &mut message_buffer);
                items.push(ItemField::FunctionCall(FunctionCall {
                    _type: "function_call".to_string(),
                    id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                    call_id: call.id,
                    name: call.name,
                    arguments: stringify_function_arguments(call.arguments),
                    status: MessageStatus::Completed,
                }));
            }
            UnifiedContentPart::ToolResult(result) => {
                push_message_item(&mut items, message.role.clone(), &mut message_buffer);
                items.push(ItemField::FunctionCallOutput(FunctionCallOutput {
                    _type: "function_call_output".to_string(),
                    id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                    call_id: result.tool_call_id,
                    output: unified_tool_result_to_function_output_payload(result.output),
                    status: MessageStatus::Completed,
                }));
            }
        }
    }

    push_message_item(&mut items, message.role, &mut message_buffer);
    items
}

fn unified_reasoning_part_to_responses_part(part: UnifiedContentPart) -> ItemContentPart {
    match part {
        UnifiedContentPart::Reasoning { text } => ItemContentPart::ReasoningText { text },
        UnifiedContentPart::Text { text } => ItemContentPart::Text { text },
        UnifiedContentPart::Refusal { text } => ItemContentPart::Refusal { refusal: text },
        UnifiedContentPart::ImageUrl { url, detail } => ItemContentPart::InputImage {
            image_url: Some(url),
            detail: detail.unwrap_or_else(|| "auto".to_string()),
        },
        UnifiedContentPart::ImageData { mime_type, data } => ItemContentPart::InputImage {
            image_url: Some(build_data_url(&mime_type, &data)),
            detail: "auto".to_string(),
        },
        UnifiedContentPart::FileUrl { url, filename, .. } => ItemContentPart::InputFile {
            filename,
            file_url: Some(url),
            file_id: None,
            file_data: None,
        },
        UnifiedContentPart::FileData {
            data,
            mime_type,
            filename,
        } => ItemContentPart::InputFile {
            filename,
            file_url: None,
            file_id: None,
            file_data: Some(build_data_url(&mime_type, &data)),
        },
        UnifiedContentPart::ExecutableCode { language, code } => ItemContentPart::Text {
            text: render_executable_code_text(&language, &code),
        },
        UnifiedContentPart::ToolCall(call) => ItemContentPart::Text {
            text: format!(
                "Tool call {} ({}) with arguments {}",
                call.id, call.name, call.arguments
            ),
        },
        UnifiedContentPart::ToolResult(result) => ItemContentPart::Text {
            text: format!(
                "Tool result {} {}",
                result.tool_call_id,
                result.legacy_content()
            ),
        },
    }
}

fn responses_annotations_to_unified(
    annotations: Vec<Annotation>,
    part_index: u32,
) -> Vec<UnifiedAnnotation> {
    annotations
        .into_iter()
        .map(|annotation| match annotation {
            Annotation::UrlCitation {
                url,
                start_index,
                end_index,
                title,
            } => UnifiedAnnotation::Citation(UnifiedCitation {
                part_index: Some(part_index),
                start_index: Some(start_index),
                end_index: Some(end_index),
                url: Some(url),
                title: Some(title),
                license: None,
            }),
        })
        .collect()
}

fn unified_annotations_to_responses(
    annotations: &[UnifiedAnnotation],
    part_index: u32,
) -> Vec<Annotation> {
    annotations
        .iter()
        .filter_map(|annotation| match annotation {
            UnifiedAnnotation::Citation(citation)
                if citation.part_index.is_none() || citation.part_index == Some(part_index) =>
            {
                Some(Annotation::UrlCitation {
                    url: citation.url.clone().unwrap_or_default(),
                    start_index: citation.start_index.unwrap_or_default(),
                    end_index: citation.end_index.unwrap_or_default(),
                    title: citation.title.clone().unwrap_or_default(),
                })
            }
            _ => None,
        })
        .collect()
}

fn message_content_parts_to_unified(
    parts: Vec<ItemContentPart>,
) -> (
    Vec<UnifiedContentPart>,
    Vec<UnifiedAnnotation>,
    Vec<UnifiedFileReferenceItem>,
) {
    let mut content = Vec::new();
    let mut annotations = Vec::new();
    let mut files = Vec::new();

    for part in parts {
        match part {
            ItemContentPart::InputText { text } | ItemContentPart::Text { text } => {
                content.push(UnifiedContentPart::Text { text });
            }
            ItemContentPart::OutputText {
                text,
                annotations: part_annotations,
                ..
            } => {
                let part_index = content.len() as u32;
                content.push(UnifiedContentPart::Text { text });
                annotations.extend(responses_annotations_to_unified(
                    part_annotations,
                    part_index,
                ));
            }
            ItemContentPart::ReasoningText { text } | ItemContentPart::SummaryText { text } => {
                content.push(UnifiedContentPart::Reasoning { text });
            }
            ItemContentPart::Refusal { refusal } => {
                content.push(UnifiedContentPart::Refusal { text: refusal });
            }
            ItemContentPart::InputImage { image_url, detail } => {
                content.push(UnifiedContentPart::ImageUrl {
                    url: image_url.unwrap_or_default(),
                    detail: Some(detail),
                });
            }
            ItemContentPart::InputFile {
                filename,
                file_url,
                file_id,
                file_data,
            } => {
                if let Some(file_data) = file_data {
                    content.push(parse_responses_input_file_data(&file_data, filename));
                } else {
                    files.push(UnifiedFileReferenceItem {
                        filename,
                        mime_type: None,
                        file_url,
                        file_id,
                    });
                }
            }
        }
    }

    (content, annotations, files)
}

fn reasoning_parts_to_unified(
    reasoning: ReasoningBody,
) -> (
    Vec<UnifiedContentPart>,
    Vec<UnifiedAnnotation>,
    Vec<UnifiedFileReferenceItem>,
) {
    let mut content = Vec::new();
    let mut annotations = Vec::new();
    let mut files = Vec::new();

    if let Some(parts) = reasoning.content {
        for part in parts {
            let (mut converted_content, mut converted_annotations, mut converted_files) =
                message_content_parts_to_unified(vec![part]);
            annotations.append(&mut converted_annotations);
            files.append(&mut converted_files);
            content.append(&mut converted_content);
        }
    }

    for part in reasoning.summary {
        let (mut converted_content, mut converted_annotations, mut converted_files) =
            message_content_parts_to_unified(vec![part]);
        annotations.append(&mut converted_annotations);
        files.append(&mut converted_files);
        content.append(&mut converted_content);
    }

    (content, annotations, files)
}

fn build_responses_response_metadata(
    output: &[ItemField],
    metadata: Value,
    safety_identifier: Option<String>,
    prompt_cache_key: Option<String>,
    status: ResponseStatus,
    incomplete_details: Option<IncompleteDetails>,
) -> Option<UnifiedProviderResponseMetadata> {
    let reasoning_metadata = build_reasoning_metadata(output);
    let citations = output
        .iter()
        .flat_map(|item| match item {
            ItemField::Message(message) => message.content.iter().collect::<Vec<_>>(),
            ItemField::Reasoning(reasoning) => {
                let mut parts = reasoning.content.iter().flatten().collect::<Vec<_>>();
                parts.extend(reasoning.summary.iter());
                parts
            }
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            ItemContentPart::OutputText { annotations, .. } => Some(annotations),
            _ => None,
        })
        .flat_map(|annotations| annotations.iter())
        .map(|annotation| match annotation {
            Annotation::UrlCitation {
                url,
                start_index,
                end_index,
                title,
            } => UnifiedResponsesUrlCitation {
                url: url.clone(),
                start_index: *start_index,
                end_index: *end_index,
                title: title.clone(),
            },
        })
        .collect::<Vec<_>>();

    let refusals = output
        .iter()
        .flat_map(|item| match item {
            ItemField::Message(message) => message.content.iter().collect::<Vec<_>>(),
            ItemField::Reasoning(reasoning) => {
                let mut parts = reasoning.content.iter().flatten().collect::<Vec<_>>();
                parts.extend(reasoning.summary.iter());
                parts
            }
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            ItemContentPart::Refusal { refusal } => Some(UnifiedResponsesRefusal {
                refusal: refusal.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let files = output
        .iter()
        .flat_map(|item| match item {
            ItemField::Message(message) => message.content.iter().collect::<Vec<_>>(),
            ItemField::Reasoning(reasoning) => {
                let mut parts = reasoning.content.iter().flatten().collect::<Vec<_>>();
                parts.extend(reasoning.summary.iter());
                parts
            }
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            ItemContentPart::InputFile {
                filename,
                file_url,
                file_id,
                file_data,
            } => Some(UnifiedResponsesFileReference {
                filename: filename.clone(),
                file_url: file_url.clone(),
                file_id: file_id.clone(),
                file_data: file_data.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let metadata = metadata.as_object().cloned();

    if citations.is_empty()
        && refusals.is_empty()
        && files.is_empty()
        && metadata.is_none()
        && reasoning_metadata.is_none()
        && safety_identifier.is_none()
        && prompt_cache_key.is_none()
        && matches!(status, ResponseStatus::Completed)
        && incomplete_details.is_none()
    {
        None
    } else {
        Some(UnifiedProviderResponseMetadata {
            responses: Some(UnifiedResponsesResponseMetadata {
                safety_identifier,
                prompt_cache_key,
                citations,
                refusals,
                files,
                metadata,
                reasoning: reasoning_metadata.and_then(|value| serde_json::to_value(value).ok()),
                status: Some(
                    serde_json::to_value(status)
                        .ok()
                        .and_then(|value| value.as_str().map(ToString::to_string))
                        .unwrap_or_else(|| "completed".to_string()),
                ),
                incomplete_details: incomplete_details.map(|details| {
                    UnifiedResponsesIncompleteDetails {
                        reason: details.reason,
                    }
                }),
            }),
            ..Default::default()
        })
    }
}

fn build_reasoning_metadata(output: &[ItemField]) -> Option<ResponsesReasoningMetadata> {
    let encrypted_contents = output
        .iter()
        .filter_map(|item| match item {
            ItemField::Reasoning(reasoning) => reasoning.encrypted_content.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();

    (!encrypted_contents.is_empty()).then_some(ResponsesReasoningMetadata { encrypted_contents })
}

fn unified_responses_metadata_to_payload(
    metadata: Option<UnifiedResponsesResponseMetadata>,
) -> (
    Value,
    Option<String>,
    Option<String>,
    ResponseStatus,
    Option<IncompleteDetails>,
) {
    match metadata {
        Some(metadata) => {
            let mut payload = metadata
                .metadata
                .map(Value::Object)
                .unwrap_or_else(|| json!({}));

            if let Some(reasoning) = metadata.reasoning.clone() {
                payload["responses_reasoning"] = reasoning;
            }

            (
                payload,
                metadata.safety_identifier,
                metadata.prompt_cache_key,
                metadata
                    .status
                    .and_then(|status| serde_json::from_value(json!(status)).ok())
                    .unwrap_or(ResponseStatus::Completed),
                metadata
                    .incomplete_details
                    .map(|details| IncompleteDetails {
                        reason: details.reason,
                    }),
            )
        }
        None => (json!({}), None, None, ResponseStatus::Completed, None),
    }
}

fn responses_finish_reason(response: &ResponsesResponse) -> Option<String> {
    if let Some(finish_reason) = response
        .metadata
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(ToString::to_string)
    {
        return Some(finish_reason);
    }

    match response.status {
        ResponseStatus::Incomplete => {
            response
                .incomplete_details
                .as_ref()
                .map(|details| match details.reason.as_str() {
                    "max_output_tokens" => "length".to_string(),
                    other => other.to_string(),
                })
        }
        _ => None,
    }
}

fn response_terminal_stream_events(response: ResponsesResponse) -> Vec<UnifiedStreamEvent> {
    let mut terminal = Vec::new();
    if let Some(finish_reason) = responses_finish_reason(&response) {
        terminal.push(UnifiedStreamEvent::MessageDelta {
            finish_reason: Some(finish_reason),
        });
    }
    if let Some(usage) = response.usage {
        terminal.push(UnifiedStreamEvent::Usage {
            usage: usage.into(),
        });
    }
    terminal
}

fn response_status_from_finish_reason(
    finish_reason: Option<&str>,
) -> (ResponseStatus, Option<IncompleteDetails>) {
    match finish_reason {
        Some("stop") | Some("tool_calls") | None => (ResponseStatus::Completed, None),
        Some("length") => (
            ResponseStatus::Incomplete,
            Some(IncompleteDetails {
                reason: "max_output_tokens".to_string(),
            }),
        ),
        Some(reason) => (
            ResponseStatus::Incomplete,
            Some(IncompleteDetails {
                reason: reason.to_string(),
            }),
        ),
    }
}

fn inject_refusals_into_output(output: &mut Vec<ItemField>, refusals: &[UnifiedResponsesRefusal]) {
    if refusals.is_empty()
        || output.iter().any(|item| match item {
            ItemField::Message(message) => message
                .content
                .iter()
                .any(|part| matches!(part, ItemContentPart::Refusal { .. })),
            ItemField::Reasoning(reasoning) => reasoning
                .content
                .iter()
                .flatten()
                .chain(reasoning.summary.iter())
                .any(|part| matches!(part, ItemContentPart::Refusal { .. })),
            _ => false,
        })
    {
        return;
    }

    if let Some(ItemField::Message(message)) = output
        .iter_mut()
        .find(|item| matches!(item, ItemField::Message(message) if matches!(message.role, MessageRole::Assistant)))
    {
        let mut refusal_parts = refusals
            .iter()
            .map(|refusal| ItemContentPart::Refusal {
                refusal: refusal.refusal.clone(),
            })
            .collect::<Vec<_>>();
        refusal_parts.append(&mut message.content);
        message.content = refusal_parts;
        return;
    }

    output.insert(
        0,
        ItemField::Message(Message {
            _type: "message".to_string(),
            id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: refusals
                .iter()
                .map(|refusal| ItemContentPart::Refusal {
                    refusal: refusal.refusal.clone(),
                })
                .collect(),
        }),
    );
}

fn inject_files_into_output(output: &mut Vec<ItemField>, files: &[UnifiedResponsesFileReference]) {
    if files.is_empty()
        || output.iter().any(|item| match item {
            ItemField::Message(message) => message
                .content
                .iter()
                .any(|part| matches!(part, ItemContentPart::InputFile { .. })),
            ItemField::Reasoning(reasoning) => reasoning
                .content
                .iter()
                .flatten()
                .chain(reasoning.summary.iter())
                .any(|part| matches!(part, ItemContentPart::InputFile { .. })),
            _ => false,
        })
    {
        return;
    }

    output.extend(files.iter().map(|file| {
        ItemField::Message(Message {
            _type: "message".to_string(),
            id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: vec![ItemContentPart::InputFile {
                filename: file.filename.clone(),
                file_url: file.file_url.clone(),
                file_id: file.file_id.clone(),
                file_data: file.file_data.clone(),
            }],
        })
    }));
}

fn apply_reasoning_metadata_to_output(
    output: &mut [ItemField],
    reasoning_metadata: Option<ResponsesReasoningMetadata>,
) {
    let Some(reasoning_metadata) = reasoning_metadata else {
        return;
    };

    let mut encrypted_contents = reasoning_metadata.encrypted_contents.into_iter();
    for item in output.iter_mut() {
        if let ItemField::Reasoning(reasoning) = item {
            if reasoning.encrypted_content.is_none() {
                reasoning.encrypted_content = encrypted_contents.next();
            }
        }
    }
}

fn flush_message_buffer(
    output: &mut Vec<ItemField>,
    role: UnifiedRole,
    buffer: &mut Vec<ItemContentPart>,
) {
    if buffer.is_empty() {
        return;
    }

    output.push(ItemField::Message(Message {
        _type: "message".to_string(),
        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
        status: MessageStatus::Completed,
        role: unified_role_to_message(role),
        content: std::mem::take(buffer),
    }));
}

fn push_message_buffer_part(
    buffer: &mut Vec<ItemContentPart>,
    part: UnifiedContentPart,
    annotations: &[UnifiedAnnotation],
    part_index: u32,
) {
    match part {
        UnifiedContentPart::Text { text } => {
            buffer.push(ItemContentPart::OutputText {
                text,
                annotations: unified_annotations_to_responses(annotations, part_index),
                logprobs: None,
            });
        }
        UnifiedContentPart::Refusal { text } => {
            buffer.push(ItemContentPart::Refusal { refusal: text });
        }
        UnifiedContentPart::ImageData { mime_type, data } => {
            buffer.push(ItemContentPart::InputImage {
                image_url: Some(build_data_url(&mime_type, &data)),
                detail: "auto".to_string(),
            });
        }
        UnifiedContentPart::ImageUrl { url, detail } => {
            buffer.push(ItemContentPart::InputImage {
                image_url: Some(url),
                detail: detail.unwrap_or_else(|| "auto".to_string()),
            });
        }
        UnifiedContentPart::FileUrl { url, filename, .. } => {
            buffer.push(ItemContentPart::InputFile {
                filename,
                file_url: Some(url),
                file_id: None,
                file_data: None,
            });
        }
        UnifiedContentPart::FileData {
            data,
            mime_type,
            filename,
        } => {
            buffer.push(ItemContentPart::InputFile {
                filename,
                file_url: None,
                file_id: None,
                file_data: Some(build_data_url(&mime_type, &data)),
            });
        }
        UnifiedContentPart::ExecutableCode { language, code } => {
            buffer.push(ItemContentPart::OutputText {
                text: render_executable_code_text(&language, &code),
                annotations: Vec::new(),
                logprobs: None,
            });
        }
        UnifiedContentPart::Reasoning { .. }
        | UnifiedContentPart::ToolCall(_)
        | UnifiedContentPart::ToolResult(_) => {}
    }
}

fn unified_choice_to_responses_items(choice: UnifiedChoice) -> Vec<ItemField> {
    let mut output = Vec::new();
    let mut message_buffer = Vec::new();

    if !choice.items.is_empty() {
        for item in choice.items {
            match item {
                UnifiedItem::Message(message) => {
                    for (part_index, part) in message.content.into_iter().enumerate() {
                        push_message_buffer_part(
                            &mut message_buffer,
                            part,
                            &message.annotations,
                            part_index as u32,
                        );
                    }
                }
                UnifiedItem::Reasoning(item) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::Reasoning(ReasoningBody {
                        _type: "reasoning".to_string(),
                        id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                        content: Some(
                            item.content
                                .into_iter()
                                .map(unified_reasoning_part_to_responses_part)
                                .collect(),
                        ),
                        summary: Vec::new(),
                        encrypted_content: None,
                    }));
                }
                UnifiedItem::FunctionCall(call) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCall(FunctionCall {
                        _type: "function_call".to_string(),
                        id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: call.id,
                        name: call.name,
                        arguments: stringify_function_arguments(call.arguments),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedItem::FunctionCallOutput(result) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCallOutput(FunctionCallOutput {
                        _type: "function_call_output".to_string(),
                        id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: result.tool_call_id,
                        output: unified_tool_result_to_function_output_payload(result.output),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedItem::FileReference(file) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::Message(Message {
                        _type: "message".to_string(),
                        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
                        status: MessageStatus::Completed,
                        role: MessageRole::Assistant,
                        content: vec![ItemContentPart::InputFile {
                            filename: file.filename,
                            file_url: file.file_url,
                            file_id: file.file_id,
                            file_data: None,
                        }],
                    }));
                }
            }
        }
    } else {
        for part in choice.message.content {
            match part {
                UnifiedContentPart::Text { text } => {
                    message_buffer.push(ItemContentPart::OutputText {
                        text,
                        annotations: Vec::new(),
                        logprobs: None,
                    });
                }
                UnifiedContentPart::Refusal { text } => {
                    message_buffer.push(ItemContentPart::Refusal { refusal: text });
                }
                UnifiedContentPart::ImageData { mime_type, data } => {
                    message_buffer.push(ItemContentPart::InputImage {
                        image_url: Some(build_data_url(&mime_type, &data)),
                        detail: "auto".to_string(),
                    });
                }
                UnifiedContentPart::ImageUrl { url, detail } => {
                    message_buffer.push(ItemContentPart::InputImage {
                        image_url: Some(url),
                        detail: detail.unwrap_or_else(|| "auto".to_string()),
                    });
                }
                UnifiedContentPart::FileUrl { url, filename, .. } => {
                    message_buffer.push(ItemContentPart::InputFile {
                        filename,
                        file_url: Some(url),
                        file_id: None,
                        file_data: None,
                    });
                }
                UnifiedContentPart::FileData {
                    data,
                    mime_type,
                    filename,
                } => {
                    message_buffer.push(ItemContentPart::InputFile {
                        filename,
                        file_url: None,
                        file_id: None,
                        file_data: Some(build_data_url(&mime_type, &data)),
                    });
                }
                UnifiedContentPart::Reasoning { text } => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::Reasoning(ReasoningBody {
                        _type: "reasoning".to_string(),
                        id: format!("rs_{}", crate::utils::ID_GENERATOR.generate_id()),
                        content: None,
                        summary: vec![ItemContentPart::SummaryText { text }],
                        encrypted_content: None,
                    }));
                }
                UnifiedContentPart::ToolCall(call) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCall(FunctionCall {
                        _type: "function_call".to_string(),
                        id: format!("fc_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: call.id,
                        name: call.name,
                        arguments: stringify_function_arguments(call.arguments),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedContentPart::ToolResult(result) => {
                    flush_message_buffer(
                        &mut output,
                        choice.message.role.clone(),
                        &mut message_buffer,
                    );
                    output.push(ItemField::FunctionCallOutput(FunctionCallOutput {
                        _type: "function_call_output".to_string(),
                        id: format!("fco_{}", crate::utils::ID_GENERATOR.generate_id()),
                        call_id: result.tool_call_id,
                        output: unified_tool_result_to_function_output_payload(result.output),
                        status: MessageStatus::Completed,
                    }));
                }
                UnifiedContentPart::ExecutableCode { language, code } => {
                    message_buffer.push(ItemContentPart::OutputText {
                        text: render_executable_code_text(&language, &code),
                        annotations: Vec::new(),
                        logprobs: None,
                    });
                }
            }
        }
    }

    flush_message_buffer(&mut output, choice.message.role, &mut message_buffer);
    output
}

impl From<ResponsesResponse> for UnifiedResponse {
    fn from(responses_res: ResponsesResponse) -> Self {
        let provider_response_metadata = build_responses_response_metadata(
            &responses_res.output,
            responses_res.metadata.clone(),
            responses_res.safety_identifier.clone(),
            responses_res.prompt_cache_key.clone(),
            responses_res.status.clone(),
            responses_res.incomplete_details.clone(),
        );
        let mut content = Vec::new();
        let mut response_items = Vec::new();

        for item in responses_res.output {
            match item {
                ItemField::Message(msg) => {
                    let (unified_content, annotations, files) =
                        message_content_parts_to_unified(msg.content);
                    content.extend(unified_content.clone());
                    if !unified_content.is_empty() || !annotations.is_empty() {
                        response_items.push(UnifiedItem::Message(UnifiedMessageItem {
                            role: message_role_to_unified(msg.role),
                            content: unified_content,
                            annotations,
                        }));
                    }
                    response_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                }
                ItemField::FunctionCall(call) => {
                    response_items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id: call.call_id.clone(),
                        name: call.name.clone(),
                        arguments: parse_function_arguments(&call.arguments),
                    }));
                    content.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: call.call_id,
                        name: call.name,
                        arguments: parse_function_arguments(&call.arguments),
                    }));
                }
                ItemField::FunctionCallOutput(output) => {
                    response_items.push(UnifiedItem::FunctionCallOutput(
                        UnifiedFunctionCallOutputItem {
                            tool_call_id: output.call_id.clone(),
                            name: None,
                            output: function_output_payload_to_unified(output.output.clone()),
                        },
                    ));
                    content.push(UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: output.call_id,
                        name: None,
                        output: function_output_payload_to_unified(output.output),
                    }));
                }
                ItemField::Reasoning(reasoning) => {
                    let (reasoning_content, annotations, files) =
                        reasoning_parts_to_unified(reasoning);
                    content.extend(reasoning_content.clone());
                    response_items.push(UnifiedItem::Reasoning(UnifiedReasoningItem {
                        content: reasoning_content,
                        annotations,
                    }));
                    response_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                }
                ItemField::Unknown(_) => {
                    apply_transform_policy(
                        TransformProtocol::Api(LlmApiType::Responses),
                        TransformProtocol::Unified,
                        TransformValueKind::ResponsesUnknownItem,
                        "Dropping unknown Responses item from Responses response conversion.",
                    );
                }
            }
        }

        let choices = if content.is_empty() && response_items.is_empty() {
            Vec::new()
        } else {
            vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content,
                },
                items: response_items,
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }]
        };

        UnifiedResponse {
            id: responses_res.id,
            model: Some(responses_res.model),
            choices,
            usage: responses_res.usage.map(Into::into),
            created: Some(responses_res.created_at),
            object: Some(
                serde_json::to_value(responses_res.object)
                    .ok()
                    .and_then(|value| value.as_str().map(ToString::to_string))
                    .unwrap_or_else(|| "response".to_string()),
            ),
            system_fingerprint: None,
            provider_response_metadata,
            synthetic_metadata: None,
        }
    }
}

impl From<ResponsesRequestPayload> for UnifiedRequest {
    fn from(responses_req: ResponsesRequestPayload) -> Self {
        let ResponsesRequestPayload {
            model,
            input,
            instructions,
            tools,
            tool_choice,
            text,
            reasoning,
            parallel_tool_calls,
            stream,
            max_tokens,
            temperature,
            top_p,
        } = responses_req;

        let mut messages = Vec::new();
        if let Some(instructions) = instructions
            .clone()
            .filter(|value| !value.trim().is_empty())
        {
            messages.push(UnifiedMessage {
                role: UnifiedRole::System,
                content: vec![UnifiedContentPart::Text { text: instructions }],
                ..Default::default()
            });
        }

        let mut request_items = Vec::new();
        messages.extend(match input {
            Input::String(text) => vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text { text }],
                ..Default::default()
            }],
            Input::Items(items) => items
                .into_iter()
                .filter_map(|item| match item {
                    ItemField::Message(item) => {
                        let (content, annotations, files) =
                            message_content_parts_to_unified(item.content);
                        if !content.is_empty() || !annotations.is_empty() {
                            request_items.push(UnifiedItem::Message(UnifiedMessageItem {
                                role: message_role_to_unified(item.role.clone()),
                                content: content.clone(),
                                annotations,
                            }));
                        }
                        request_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                        (!content.is_empty()).then_some(UnifiedMessage {
                            role: message_role_to_unified(item.role),
                            content,
                            ..Default::default()
                        })
                    }
                    ItemField::FunctionCall(call) => {
                        let arguments = parse_function_arguments(&call.arguments);
                        request_items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                            id: call.call_id.clone(),
                            name: call.name.clone(),
                            arguments: arguments.clone(),
                        }));
                        Some(UnifiedMessage {
                            role: UnifiedRole::Assistant,
                            content: vec![UnifiedContentPart::ToolCall(UnifiedToolCall {
                                id: call.call_id,
                                name: call.name,
                                arguments,
                            })],
                            ..Default::default()
                        })
                    }
                    ItemField::FunctionCallOutput(output) => {
                        let typed_output = function_output_payload_to_unified(output.output);
                        request_items.push(UnifiedItem::FunctionCallOutput(
                            UnifiedFunctionCallOutputItem {
                                tool_call_id: output.call_id.clone(),
                                name: None,
                                output: typed_output.clone(),
                            },
                        ));
                        Some(UnifiedMessage {
                            role: UnifiedRole::Tool,
                            content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                                tool_call_id: output.call_id,
                                name: None,
                                output: typed_output,
                            })],
                            ..Default::default()
                        })
                    }
                    ItemField::Reasoning(reasoning) => {
                        let (content, annotations, files) = reasoning_parts_to_unified(reasoning);
                        if !content.is_empty() || !annotations.is_empty() {
                            request_items.push(UnifiedItem::Reasoning(UnifiedReasoningItem {
                                content: content.clone(),
                                annotations,
                            }));
                        }
                        request_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                        (!content.is_empty()).then_some(UnifiedMessage {
                            role: UnifiedRole::Assistant,
                            content,
                            ..Default::default()
                        })
                    }
                    ItemField::Unknown(_) => None,
                })
                .collect(),
        });

        if request_items.is_empty() {
            request_items = messages
                .iter()
                .flat_map(|message| {
                    legacy_content_to_unified_items(message.role.clone(), message.content.clone())
                })
                .collect();
        }

        let tools = tools.map(|items| {
            items
                .into_iter()
                .map(|tool| match tool {
                    Tool::Function(function) => UnifiedTool {
                        type_: "function".to_string(),
                        function: UnifiedFunctionDefinition {
                            name: function.name,
                            description: function.description,
                            parameters: function.parameters.unwrap_or_else(|| json!({})),
                        },
                    },
                })
                .collect()
        });

        let responses_extension = UnifiedResponsesRequestExtension {
            instructions,
            tool_choice: tool_choice.and_then(|value| serde_json::to_value(value).ok()),
            text_format: text.and_then(|value| serde_json::to_value(value.format).ok()),
            reasoning: reasoning.and_then(|value| serde_json::to_value(value).ok()),
            parallel_tool_calls,
        };

        UnifiedRequest {
            model: Some(model),
            messages,
            items: request_items,
            tools,
            stream: stream.unwrap_or(false),
            temperature,
            max_tokens,
            top_p,
            extensions: (!responses_extension.is_empty()).then_some(UnifiedRequestExtensions {
                responses: Some(responses_extension),
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

impl From<UnifiedResponse> for ResponsesResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let responses_metadata = unified_res
            .provider_response_metadata
            .clone()
            .and_then(|metadata| metadata.responses);
        let reasoning_metadata = responses_metadata
            .as_ref()
            .and_then(|metadata| metadata.reasoning.clone())
            .and_then(|value| serde_json::from_value(value).ok());
        let refusals = responses_metadata
            .as_ref()
            .map(|metadata| metadata.refusals.clone())
            .unwrap_or_default();
        let files = responses_metadata
            .as_ref()
            .map(|metadata| metadata.files.clone())
            .unwrap_or_default();
        let (metadata, safety_identifier, prompt_cache_key, status, incomplete_details) =
            unified_responses_metadata_to_payload(responses_metadata);
        let mut output = Vec::new();

        for choice in unified_res.choices {
            output.extend(unified_choice_to_responses_items(choice));
        }

        inject_refusals_into_output(&mut output, &refusals);
        inject_files_into_output(&mut output, &files);
        apply_reasoning_metadata_to_output(&mut output, reasoning_metadata);

        ResponsesResponse {
            id: unified_res.id,
            object: ResponseObject::Response,
            created_at: unified_res
                .created
                .unwrap_or_else(|| Utc::now().timestamp()),
            completed_at: matches!(status, ResponseStatus::Completed).then_some(
                unified_res
                    .created
                    .unwrap_or_else(|| Utc::now().timestamp()),
            ),
            status,
            incomplete_details,
            model: unified_res.model.unwrap_or_default(),
            previous_response_id: None,
            instructions: None,
            output,
            error: None,
            tools: Vec::new(),
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: unified_res.usage.map(Into::into),
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata,
            safety_identifier,
            prompt_cache_key,
        }
    }
}

// --- Chunk Response ---

#[derive(Debug, Clone)]
pub enum ResponsesStreamEvent {
    ResponseCreated {
        response: ResponsesResponse,
    },
    ResponseCompleted {
        response: ResponsesResponse,
    },
    ResponseIncomplete {
        response: ResponsesResponse,
    },
    OutputItemAdded {
        output_index: u32,
        item: ItemField,
    },
    OutputItemDone {
        output_index: u32,
        item: ItemField,
    },
    ContentPartAdded {
        item_id: String,
        content_index: u32,
    },
    ContentPartDone {
        item_id: String,
        content_index: u32,
    },
    ReasoningSummaryPartAdded {
        item_id: String,
        summary_index: u32,
    },
    ReasoningSummaryPartDone {
        item_id: String,
        summary_index: u32,
    },
    Ignored,
    MessageStart {
        id: Option<String>,
        role: UnifiedRole,
    },
    MessageDelta {
        finish_reason: Option<String>,
    },
    MessageStop,
    ContentBlockStart {
        index: u32,
        kind: UnifiedBlockKind,
    },
    ContentBlockDelta {
        index: u32,
        item_index: Option<u32>,
        item_id: Option<String>,
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
        item_index: Option<u32>,
        item_id: Option<String>,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    ToolCallArgumentsDone {
        index: u32,
        item_index: Option<u32>,
        item_id: Option<String>,
        id: Option<String>,
        arguments: String,
    },
    ToolCallStop {
        index: u32,
        id: Option<String>,
    },
    ReasoningStart {
        index: u32,
    },
    ReasoningDelta {
        index: u32,
        item_index: Option<u32>,
        item_id: Option<String>,
        part_index: Option<u32>,
        text: String,
    },
    ReasoningStop {
        index: u32,
    },
    Usage {
        usage: UnifiedUsage,
    },
    Blob {
        index: Option<u32>,
        data: Value,
    },
    Error {
        error: Value,
    },
    Item(ItemField),
    Unknown(Value),
}

fn responses_item_id(item: &ItemField) -> Option<String> {
    match item {
        ItemField::Message(item) => Some(item.id.clone()),
        ItemField::FunctionCall(item) => Some(item.id.clone()),
        ItemField::FunctionCallOutput(item) => Some(item.id.clone()),
        ItemField::Reasoning(item) => Some(item.id.clone()),
        ItemField::Unknown(_) => None,
    }
}

fn responses_item_to_unified_item(item: &ItemField) -> Option<UnifiedItem> {
    match item {
        ItemField::Message(message) => {
            let (content, annotations, _) =
                message_content_parts_to_unified(message.content.clone());
            Some(UnifiedItem::Message(UnifiedMessageItem {
                role: message_role_to_unified(message.role.clone()),
                content,
                annotations,
            }))
        }
        ItemField::FunctionCall(call) => Some(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
            id: call.call_id.clone(),
            name: call.name.clone(),
            arguments: parse_function_arguments(&call.arguments),
        })),
        ItemField::FunctionCallOutput(output) => Some(UnifiedItem::FunctionCallOutput(
            UnifiedFunctionCallOutputItem {
                tool_call_id: output.call_id.clone(),
                name: None,
                output: function_output_payload_to_unified(output.output.clone()),
            },
        )),
        ItemField::Reasoning(reasoning) => {
            let (content, annotations, _) = reasoning_parts_to_unified(reasoning.clone());
            Some(UnifiedItem::Reasoning(UnifiedReasoningItem {
                content,
                annotations,
            }))
        }
        ItemField::Unknown(_) => None,
    }
}

fn responses_message_blob_events(
    parts: &[ItemContentPart],
    output_index: u32,
) -> Vec<UnifiedStreamEvent> {
    parts
        .iter()
        .filter_map(|part| match part {
            ItemContentPart::InputImage { .. } | ItemContentPart::InputFile { .. } => {
                Some(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: serde_json::to_value(part).unwrap_or(Value::Null),
                })
            }
            _ => None,
        })
        .collect()
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum TypedResponsesStreamEvent {
    #[serde(rename = "response.created")]
    ResponseCreated { response: ResponsesResponse },
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: ResponsesResponse },
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete { response: ResponsesResponse },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded { output_index: u32, item: ItemField },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone { output_index: u32, item: ItemField },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        item_id: String,
        output_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        item_id: String,
        output_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        arguments: String,
    },
    #[serde(rename = "response.reasoning_summary_part.added")]
    ReasoningSummaryPartAdded { item_id: String, summary_index: u32 },
    #[serde(rename = "response.reasoning_summary_part.done")]
    ReasoningSummaryPartDone { item_id: String, summary_index: u32 },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        item_id: String,
        summary_index: u32,
        delta: String,
    },
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded { item_id: String, content_index: u32 },
    #[serde(rename = "response.content_part.done")]
    ContentPartDone { item_id: String, content_index: u32 },
    #[serde(rename = "response.message.start")]
    MessageStart {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        role: UnifiedRole,
    },
    #[serde(rename = "response.message.delta")]
    MessageDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    #[serde(rename = "response.message.stop")]
    MessageStop,
    #[serde(rename = "response.content_block.start")]
    ContentBlockStart { index: u32, kind: UnifiedBlockKind },
    #[serde(rename = "response.content_block.delta")]
    ContentBlockDelta { index: u32, text: String },
    #[serde(rename = "response.content_block.stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "response.tool_call.start")]
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    #[serde(rename = "response.tool_call.arguments.delta")]
    LegacyToolCallArgumentsDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments: String,
    },
    #[serde(rename = "response.tool_call.stop")]
    ToolCallStop {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "response.reasoning.start")]
    ReasoningStart { index: u32 },
    #[serde(rename = "response.reasoning.delta")]
    LegacyReasoningDelta { index: u32, text: String },
    #[serde(rename = "response.reasoning.stop")]
    ReasoningStop { index: u32 },
    #[serde(rename = "response.usage")]
    Usage { usage: UnifiedUsage },
    #[serde(rename = "response.blob")]
    Blob {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        data: Value,
    },
    #[serde(rename = "response.error")]
    Error { error: Value },
}

impl ResponsesStreamEvent {
    fn to_public_value(&self) -> Value {
        match self {
            Self::ContentBlockDelta {
                item_index: Some(output_index),
                item_id: Some(item_id),
                part_index: Some(content_index),
                text,
                ..
            } => json!({
                "type": "response.output_text.delta",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": content_index,
                "delta": text
            }),
            Self::ToolCallArgumentsDelta {
                item_index: Some(output_index),
                item_id: Some(item_id),
                name,
                arguments,
                ..
            } => json!({
                "type": "response.function_call_arguments.delta",
                "item_id": item_id,
                "output_index": output_index,
                "name": name,
                "delta": arguments
            }),
            Self::ToolCallArgumentsDone {
                item_index: Some(output_index),
                item_id: Some(item_id),
                id,
                arguments,
                ..
            } => json!({
                "type": "response.function_call_arguments.done",
                "item_id": item_id,
                "output_index": output_index,
                "call_id": id,
                "arguments": arguments
            }),
            Self::ReasoningDelta {
                item_id: Some(item_id),
                part_index: Some(summary_index),
                text,
                ..
            } => json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": item_id,
                "summary_index": summary_index,
                "delta": text
            }),
            _ => self.to_value(),
        }
    }

    fn from_value(value: Value) -> Self {
        if let Ok(event) = serde_json::from_value::<TypedResponsesStreamEvent>(value.clone()) {
            return match event {
                TypedResponsesStreamEvent::ResponseCreated { response } => {
                    Self::ResponseCreated { response }
                }
                TypedResponsesStreamEvent::ResponseCompleted { response } => {
                    Self::ResponseCompleted { response }
                }
                TypedResponsesStreamEvent::ResponseIncomplete { response } => {
                    Self::ResponseIncomplete { response }
                }
                TypedResponsesStreamEvent::OutputItemAdded { output_index, item } => {
                    Self::OutputItemAdded { output_index, item }
                }
                TypedResponsesStreamEvent::OutputItemDone { output_index, item } => {
                    Self::OutputItemDone { output_index, item }
                }
                TypedResponsesStreamEvent::OutputTextDelta {
                    item_id,
                    output_index,
                    content_index,
                    delta,
                } => Self::ContentBlockDelta {
                    index: content_index,
                    item_index: Some(output_index),
                    item_id: Some(item_id),
                    part_index: Some(content_index),
                    text: delta,
                },
                TypedResponsesStreamEvent::FunctionCallArgumentsDelta {
                    item_id,
                    output_index,
                    name,
                    delta,
                } => Self::ToolCallArgumentsDelta {
                    index: output_index,
                    item_index: Some(output_index),
                    item_id: Some(item_id.clone()),
                    id: Some(item_id),
                    name,
                    arguments: delta,
                },
                TypedResponsesStreamEvent::FunctionCallArgumentsDone {
                    item_id,
                    output_index,
                    call_id,
                    arguments,
                } => Self::ToolCallArgumentsDone {
                    index: output_index,
                    item_index: Some(output_index),
                    item_id: Some(item_id),
                    id: call_id,
                    arguments,
                },
                TypedResponsesStreamEvent::ReasoningSummaryPartAdded {
                    item_id,
                    summary_index,
                } => Self::ReasoningSummaryPartAdded {
                    item_id,
                    summary_index,
                },
                TypedResponsesStreamEvent::ReasoningSummaryPartDone {
                    item_id,
                    summary_index,
                } => Self::ReasoningSummaryPartDone {
                    item_id,
                    summary_index,
                },
                TypedResponsesStreamEvent::ContentPartAdded {
                    item_id,
                    content_index,
                } => Self::ContentPartAdded {
                    item_id,
                    content_index,
                },
                TypedResponsesStreamEvent::ContentPartDone {
                    item_id,
                    content_index,
                } => Self::ContentPartDone {
                    item_id,
                    content_index,
                },
                TypedResponsesStreamEvent::ReasoningSummaryTextDelta {
                    item_id,
                    summary_index,
                    delta,
                } => Self::ReasoningDelta {
                    index: summary_index,
                    item_index: None,
                    item_id: Some(item_id),
                    part_index: Some(summary_index),
                    text: delta,
                },
                TypedResponsesStreamEvent::MessageStart { id, role } => {
                    Self::MessageStart { id, role }
                }
                TypedResponsesStreamEvent::MessageDelta { finish_reason } => {
                    Self::MessageDelta { finish_reason }
                }
                TypedResponsesStreamEvent::MessageStop => Self::MessageStop,
                TypedResponsesStreamEvent::ContentBlockStart { index, kind } => {
                    Self::ContentBlockStart { index, kind }
                }
                TypedResponsesStreamEvent::ContentBlockDelta { index, text } => {
                    Self::ContentBlockDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    }
                }
                TypedResponsesStreamEvent::ContentBlockStop { index } => {
                    Self::ContentBlockStop { index }
                }
                TypedResponsesStreamEvent::ToolCallStart { index, id, name } => {
                    Self::ToolCallStart { index, id, name }
                }
                TypedResponsesStreamEvent::LegacyToolCallArgumentsDelta {
                    index,
                    id,
                    name,
                    arguments,
                } => Self::ToolCallArgumentsDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    id,
                    name,
                    arguments,
                },
                TypedResponsesStreamEvent::ToolCallStop { index, id } => {
                    Self::ToolCallStop { index, id }
                }
                TypedResponsesStreamEvent::ReasoningStart { index } => {
                    Self::ReasoningStart { index }
                }
                TypedResponsesStreamEvent::LegacyReasoningDelta { index, text } => {
                    Self::ReasoningDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    }
                }
                TypedResponsesStreamEvent::ReasoningStop { index } => Self::ReasoningStop { index },
                TypedResponsesStreamEvent::Usage { usage } => Self::Usage { usage },
                TypedResponsesStreamEvent::Blob { index, data } => Self::Blob { index, data },
                TypedResponsesStreamEvent::Error { error } => Self::Error { error },
            };
        }

        let Some(_event_type) = value.get("type").and_then(Value::as_str) else {
            return serde_json::from_value::<ItemField>(value.clone())
                .map(Self::Item)
                .unwrap_or(Self::Unknown(value));
        };

        serde_json::from_value::<ItemField>(value.clone())
            .map(Self::Item)
            .unwrap_or(Self::Unknown(value))
    }

    fn to_value(&self) -> Value {
        let typed = match self {
            Self::ResponseCreated { response } => TypedResponsesStreamEvent::ResponseCreated {
                response: response.clone(),
            },
            Self::ResponseCompleted { response } => TypedResponsesStreamEvent::ResponseCompleted {
                response: response.clone(),
            },
            Self::ResponseIncomplete { response } => {
                TypedResponsesStreamEvent::ResponseIncomplete {
                    response: response.clone(),
                }
            }
            Self::OutputItemAdded { output_index, item } => {
                TypedResponsesStreamEvent::OutputItemAdded {
                    output_index: *output_index,
                    item: item.clone(),
                }
            }
            Self::OutputItemDone { output_index, item } => {
                TypedResponsesStreamEvent::OutputItemDone {
                    output_index: *output_index,
                    item: item.clone(),
                }
            }
            Self::ContentPartAdded {
                item_id,
                content_index,
            } => TypedResponsesStreamEvent::ContentPartAdded {
                item_id: item_id.clone(),
                content_index: *content_index,
            },
            Self::ContentPartDone {
                item_id,
                content_index,
            } => TypedResponsesStreamEvent::ContentPartDone {
                item_id: item_id.clone(),
                content_index: *content_index,
            },
            Self::ReasoningSummaryPartAdded {
                item_id,
                summary_index,
            } => TypedResponsesStreamEvent::ReasoningSummaryPartAdded {
                item_id: item_id.clone(),
                summary_index: *summary_index,
            },
            Self::ReasoningSummaryPartDone {
                item_id,
                summary_index,
            } => TypedResponsesStreamEvent::ReasoningSummaryPartDone {
                item_id: item_id.clone(),
                summary_index: *summary_index,
            },
            Self::Ignored => return Value::Null,
            Self::MessageStart { id, role } => TypedResponsesStreamEvent::MessageStart {
                id: id.clone(),
                role: role.clone(),
            },
            Self::MessageDelta { finish_reason } => TypedResponsesStreamEvent::MessageDelta {
                finish_reason: finish_reason.clone(),
            },
            Self::MessageStop => TypedResponsesStreamEvent::MessageStop,
            Self::ContentBlockStart { index, kind } => {
                TypedResponsesStreamEvent::ContentBlockStart {
                    index: *index,
                    kind: kind.clone(),
                }
            }
            Self::ContentBlockDelta {
                index,
                item_index: _,
                item_id: _,
                part_index: _,
                text,
            } => TypedResponsesStreamEvent::ContentBlockDelta {
                index: *index,
                text: text.clone(),
            },
            Self::ContentBlockStop { index } => {
                TypedResponsesStreamEvent::ContentBlockStop { index: *index }
            }
            Self::ToolCallStart { index, id, name } => TypedResponsesStreamEvent::ToolCallStart {
                index: *index,
                id: id.clone(),
                name: name.clone(),
            },
            Self::ToolCallArgumentsDelta {
                index,
                item_index: _,
                item_id: _,
                id,
                name,
                arguments,
            } => TypedResponsesStreamEvent::LegacyToolCallArgumentsDelta {
                index: *index,
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
            Self::ToolCallArgumentsDone {
                index,
                item_index,
                item_id,
                id,
                arguments,
            } => TypedResponsesStreamEvent::FunctionCallArgumentsDone {
                item_id: item_id
                    .clone()
                    .or_else(|| id.clone())
                    .unwrap_or_default(),
                output_index: item_index.unwrap_or(*index),
                call_id: id.clone(),
                arguments: arguments.clone(),
            },
            Self::ToolCallStop { index, id } => TypedResponsesStreamEvent::ToolCallStop {
                index: *index,
                id: id.clone(),
            },
            Self::ReasoningStart { index } => {
                TypedResponsesStreamEvent::ReasoningStart { index: *index }
            }
            Self::ReasoningDelta {
                index,
                item_index: _,
                item_id: _,
                part_index: _,
                text,
            } => TypedResponsesStreamEvent::LegacyReasoningDelta {
                index: *index,
                text: text.clone(),
            },
            Self::ReasoningStop { index } => {
                TypedResponsesStreamEvent::ReasoningStop { index: *index }
            }
            Self::Usage { usage } => TypedResponsesStreamEvent::Usage {
                usage: usage.clone(),
            },
            Self::Blob { index, data } => TypedResponsesStreamEvent::Blob {
                index: *index,
                data: data.clone(),
            },
            Self::Error { error } => TypedResponsesStreamEvent::Error {
                error: error.clone(),
            },
            Self::Item(item) => return serde_json::to_value(item).unwrap_or(Value::Null),
            Self::Unknown(value) => return value.clone(),
        };

        serde_json::to_value(typed).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone)]
pub struct ResponsesChunkResponse {
    pub id: String,
    pub model: String,
    pub event: ResponsesStreamEvent,
}

impl Serialize for ResponsesChunkResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.event.to_public_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResponsesChunkResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        struct LegacyWrappedResponsesChunkResponse {
            id: String,
            model: String,
            delta: Value,
        }

        if let Ok(raw) =
            serde_json::from_value::<LegacyWrappedResponsesChunkResponse>(value.clone())
        {
            return Ok(Self {
                id: raw.id,
                model: raw.model,
                event: ResponsesStreamEvent::from_value(raw.delta),
            });
        }

        let id = value
            .get("response")
            .and_then(|response| response.get("id"))
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let model = value
            .get("response")
            .and_then(|response| response.get("model"))
            .or_else(|| value.get("model"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        Ok(Self {
            id,
            model,
            event: ResponsesStreamEvent::from_value(value),
        })
    }
}

pub fn responses_chunk_to_unified_stream_events(
    chunk: ResponsesChunkResponse,
) -> Vec<UnifiedStreamEvent> {
    let ResponsesChunkResponse { id, model, event } = chunk;

    let mut events = Vec::new();

    match event {
        ResponsesStreamEvent::Ignored => return Vec::new(),
        ResponsesStreamEvent::ResponseCreated { response } => {
            return vec![UnifiedStreamEvent::MessageStart {
                id: Some(response.id),
                model: Some(response.model),
                role: UnifiedRole::Assistant,
            }];
        }
        ResponsesStreamEvent::ResponseCompleted { response }
        | ResponsesStreamEvent::ResponseIncomplete { response } => {
            return response_terminal_stream_events(response);
        }
        ResponsesStreamEvent::OutputItemAdded { output_index, item } => match item {
            ItemField::Message(message) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Message(message.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: responses_item_id(&ItemField::Message(message.clone())),
                        item,
                    });
                }
                events.extend(responses_message_blob_events(
                    &message.content,
                    output_index,
                ));
                return events;
            }
            ItemField::FunctionCall(call) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCall(call.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: Some(call.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::ToolCallStart {
                    index: output_index,
                    id: call.call_id,
                    name: call.name,
                });
                return events;
            }
            ItemField::Reasoning(reasoning) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Reasoning(reasoning.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: Some(reasoning.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::ReasoningStart {
                    index: output_index,
                });
                return events;
            }
            ItemField::FunctionCallOutput(output) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCallOutput(output.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(output_index),
                        item_id: Some(output.id.clone()),
                        item: item.clone(),
                    });
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(output.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
                return events;
            }
            ItemField::Unknown(value) => {
                return vec![UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: value,
                }];
            }
        },
        ResponsesStreamEvent::OutputItemDone { output_index, item } => match item {
            ItemField::FunctionCall(call) => {
                let mut events = vec![UnifiedStreamEvent::ToolCallStop {
                    index: output_index,
                    id: Some(call.call_id.clone()),
                }];
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCall(call.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(call.id),
                        item,
                    });
                }
                return events;
            }
            ItemField::Reasoning(reasoning) => {
                let mut events = vec![UnifiedStreamEvent::ReasoningStop {
                    index: output_index,
                }];
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Reasoning(reasoning.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(reasoning.id),
                        item,
                    });
                }
                return events;
            }
            ItemField::Message(message) => {
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::Message(message.clone()))
                {
                    return vec![UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(message.id),
                        item,
                    }];
                }
                return Vec::new();
            }
            ItemField::FunctionCallOutput(output) => {
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCallOutput(output.clone()))
                {
                    return vec![UnifiedStreamEvent::ItemDone {
                        item_index: Some(output_index),
                        item_id: Some(output.id),
                        item,
                    }];
                }
                return Vec::new();
            }
            ItemField::Unknown(_) => return Vec::new(),
        },
        ResponsesStreamEvent::ContentPartAdded {
            item_id,
            content_index,
        } => {
            return vec![UnifiedStreamEvent::ContentPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
                part: None,
            }];
        }
        ResponsesStreamEvent::ContentPartDone {
            item_id,
            content_index,
        } => {
            return vec![UnifiedStreamEvent::ContentPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
            }];
        }
        ResponsesStreamEvent::ReasoningSummaryPartAdded {
            item_id,
            summary_index,
        } => {
            return vec![UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
                part: None,
            }];
        }
        ResponsesStreamEvent::ReasoningSummaryPartDone {
            item_id,
            summary_index,
        } => {
            return vec![UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
            }];
        }
        ResponsesStreamEvent::MessageStart { id: event_id, role } => {
            return vec![UnifiedStreamEvent::MessageStart {
                id: event_id.or(Some(id)),
                model: Some(model),
                role,
            }];
        }
        ResponsesStreamEvent::MessageDelta { finish_reason } => {
            return vec![UnifiedStreamEvent::MessageDelta { finish_reason }];
        }
        ResponsesStreamEvent::MessageStop => return vec![UnifiedStreamEvent::MessageStop],
        ResponsesStreamEvent::ContentBlockStart { index, kind } => {
            return vec![UnifiedStreamEvent::ContentBlockStart { index, kind }];
        }
        ResponsesStreamEvent::ContentBlockDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            return vec![UnifiedStreamEvent::ContentBlockDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            }];
        }
        ResponsesStreamEvent::ContentBlockStop { index } => {
            return vec![UnifiedStreamEvent::ContentBlockStop { index }];
        }
        ResponsesStreamEvent::ToolCallStart { index, id, name } => {
            return vec![UnifiedStreamEvent::ToolCallStart { index, id, name }];
        }
        ResponsesStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index,
            item_id,
            id,
            name,
            arguments,
        } => {
            return vec![UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                item_index,
                item_id,
                id,
                name,
                arguments,
            }];
        }
        ResponsesStreamEvent::ToolCallArgumentsDone { .. } => {
            return Vec::new();
        }
        ResponsesStreamEvent::ToolCallStop { index, id } => {
            return vec![UnifiedStreamEvent::ToolCallStop { index, id }];
        }
        ResponsesStreamEvent::ReasoningStart { index } => {
            return vec![UnifiedStreamEvent::ReasoningStart { index }];
        }
        ResponsesStreamEvent::ReasoningDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            return vec![UnifiedStreamEvent::ReasoningDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            }];
        }
        ResponsesStreamEvent::ReasoningStop { index } => {
            return vec![UnifiedStreamEvent::ReasoningStop { index }];
        }
        ResponsesStreamEvent::Usage { usage } => return vec![UnifiedStreamEvent::Usage { usage }],
        ResponsesStreamEvent::Blob { index, data } => {
            return vec![UnifiedStreamEvent::BlobDelta { index, data }];
        }
        ResponsesStreamEvent::Error { error } => {
            return vec![UnifiedStreamEvent::Error { error }];
        }
        ResponsesStreamEvent::Item(item) => match item {
            ItemField::Message(message) => {
                let message_item =
                    responses_item_to_unified_item(&ItemField::Message(message.clone()));
                if let Some(item) = message_item.clone() {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(message.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });

                for (index, part) in message.content.clone().into_iter().enumerate() {
                    let index = index as u32;
                    match part {
                        ItemContentPart::InputText { text }
                        | ItemContentPart::OutputText { text, .. }
                        | ItemContentPart::Text { text }
                        | ItemContentPart::SummaryText { text } => {
                            events.push(UnifiedStreamEvent::ContentPartAdded {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                                part: None,
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStart {
                                index,
                                kind: UnifiedBlockKind::Text,
                            });
                            events.push(UnifiedStreamEvent::ContentBlockDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                            events.push(UnifiedStreamEvent::ContentBlockStop { index });
                            events.push(UnifiedStreamEvent::ContentPartDone {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                            });
                        }
                        ItemContentPart::ReasoningText { text } => {
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                                part: None,
                            });
                            events.push(UnifiedStreamEvent::ReasoningStart { index });
                            events.push(UnifiedStreamEvent::ReasoningDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                            events.push(UnifiedStreamEvent::ReasoningStop { index });
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartDone {
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: index,
                            });
                        }
                        other => {
                            events.push(UnifiedStreamEvent::BlobDelta {
                                index: Some(index),
                                data: serde_json::to_value(other).unwrap_or(Value::Null),
                            });
                        }
                    }
                }
                if let Some(item) = message_item {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(0),
                        item_id: Some(message.id.clone()),
                        item,
                    });
                }
                return events;
            }
            ItemField::FunctionCall(call) => {
                let function_call_item =
                    responses_item_to_unified_item(&ItemField::FunctionCall(call.clone()));
                if let Some(item) = function_call_item.clone() {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(call.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                events.push(UnifiedStreamEvent::ContentBlockStart {
                    index: 0,
                    kind: UnifiedBlockKind::ToolCall,
                });
                events.push(UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: call.call_id.clone(),
                    name: call.name.clone(),
                });
                if !call.arguments.is_empty() {
                    events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index: 0,
                        item_index: Some(0),
                        item_id: Some(call.id.clone()),
                        id: Some(call.call_id),
                        name: Some(call.name),
                        arguments: call.arguments,
                    });
                }
                if matches!(call.status, MessageStatus::Completed) {
                    events.push(UnifiedStreamEvent::ToolCallStop { index: 0, id: None });
                    events.push(UnifiedStreamEvent::ContentBlockStop { index: 0 });
                    if let Some(item) = function_call_item {
                        events.push(UnifiedStreamEvent::ItemDone {
                            item_index: Some(0),
                            item_id: Some(call.id.clone()),
                            item,
                        });
                    }
                }
                return events;
            }
            ItemField::FunctionCallOutput(output) => {
                let mut events = Vec::new();
                if let Some(item) =
                    responses_item_to_unified_item(&ItemField::FunctionCallOutput(output.clone()))
                {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(output.id.clone()),
                        item: item.clone(),
                    });
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(0),
                        item_id: Some(output.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::BlobDelta {
                    index: Some(0),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
                return events;
            }
            ItemField::Reasoning(reasoning) => {
                let reasoning_item =
                    responses_item_to_unified_item(&ItemField::Reasoning(reasoning.clone()));
                if let Some(item) = reasoning_item.clone() {
                    events.push(UnifiedStreamEvent::ItemAdded {
                        item_index: Some(0),
                        item_id: Some(reasoning.id.clone()),
                        item,
                    });
                }
                events.push(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                events.push(UnifiedStreamEvent::ReasoningStart { index: 0 });
                let content_len = reasoning
                    .content
                    .as_ref()
                    .map(|parts| parts.len())
                    .unwrap_or(0);
                if let Some(content) = reasoning.content.clone() {
                    for (part_index, part) in content.into_iter().enumerate() {
                        if let ItemContentPart::ReasoningText { text }
                        | ItemContentPart::SummaryText { text }
                        | ItemContentPart::Text { text } = part
                        {
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: part_index as u32,
                                part: None,
                            });
                            events.push(UnifiedStreamEvent::ReasoningDelta {
                                index: 0,
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: Some(part_index as u32),
                                text,
                            });
                            events.push(UnifiedStreamEvent::ReasoningSummaryPartDone {
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: part_index as u32,
                            });
                        }
                    }
                }
                let base_index = content_len as u32;
                for (offset, part) in reasoning.summary.clone().into_iter().enumerate() {
                    if let ItemContentPart::ReasoningText { text }
                    | ItemContentPart::SummaryText { text }
                    | ItemContentPart::Text { text } = part
                    {
                        let part_index = base_index + offset as u32;
                        events.push(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index,
                            part: None,
                        });
                        events.push(UnifiedStreamEvent::ReasoningDelta {
                            index: 0,
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index: Some(part_index),
                            text,
                        });
                        events.push(UnifiedStreamEvent::ReasoningSummaryPartDone {
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index,
                        });
                    }
                }
                events.push(UnifiedStreamEvent::ReasoningStop { index: 0 });
                if let Some(item) = reasoning_item {
                    events.push(UnifiedStreamEvent::ItemDone {
                        item_index: Some(0),
                        item_id: Some(reasoning.id.clone()),
                        item,
                    });
                }
                return events;
            }
            ItemField::Unknown(value) => {
                return vec![UnifiedStreamEvent::BlobDelta {
                    index: None,
                    data: value,
                }];
            }
        },
        ResponsesStreamEvent::Unknown(value) => {
            if let Some(type_name) = value.get("type").and_then(Value::as_str) {
                match type_name {
                    "response.content_part.added" => {
                        return vec![UnifiedStreamEvent::ContentPartAdded {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("content_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                            part: None,
                        }];
                    }
                    "response.content_part.done" => {
                        return vec![UnifiedStreamEvent::ContentPartDone {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("content_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                        }];
                    }
                    "response.reasoning_summary_part.added" => {
                        return vec![UnifiedStreamEvent::ReasoningSummaryPartAdded {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("summary_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                            part: None,
                        }];
                    }
                    "response.reasoning_summary_part.done" => {
                        return vec![UnifiedStreamEvent::ReasoningSummaryPartDone {
                            item_index: None,
                            item_id: value
                                .get("item_id")
                                .and_then(Value::as_str)
                                .map(ToString::to_string),
                            part_index: value
                                .get("summary_index")
                                .and_then(Value::as_u64)
                                .unwrap_or_default() as u32,
                        }];
                    }
                    _ => {}
                }
            }
            return vec![UnifiedStreamEvent::BlobDelta {
                index: None,
                data: value,
            }];
        }
    }
}

pub(super) fn transform_responses_chunk_to_openai_events(
    chunk: ResponsesChunkResponse,
    transformer: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let ResponsesChunkResponse { id, model, event } = chunk;
    let mut events = Vec::new();

    let estimated_events = match &event {
        ResponsesStreamEvent::Ignored
        | ResponsesStreamEvent::ResponseCreated { .. }
        | ResponsesStreamEvent::ResponseCompleted { .. }
        | ResponsesStreamEvent::ResponseIncomplete { .. }
        | ResponsesStreamEvent::OutputItemAdded { .. }
        | ResponsesStreamEvent::OutputItemDone { .. }
        | ResponsesStreamEvent::ContentPartAdded { .. }
        | ResponsesStreamEvent::ContentPartDone { .. }
        | ResponsesStreamEvent::ReasoningSummaryPartAdded { .. }
        | ResponsesStreamEvent::ReasoningSummaryPartDone { .. }
        | ResponsesStreamEvent::MessageStart { .. }
        | ResponsesStreamEvent::MessageDelta { .. }
        | ResponsesStreamEvent::ContentBlockDelta { .. }
        | ResponsesStreamEvent::ToolCallStart { .. }
        | ResponsesStreamEvent::ToolCallArgumentsDelta { .. }
        | ResponsesStreamEvent::ToolCallArgumentsDone { .. }
        | ResponsesStreamEvent::ReasoningStart { .. }
        | ResponsesStreamEvent::ReasoningDelta { .. }
        | ResponsesStreamEvent::ReasoningStop { .. }
        | ResponsesStreamEvent::Usage { .. }
        | ResponsesStreamEvent::Blob { .. }
        | ResponsesStreamEvent::Error { .. }
        | ResponsesStreamEvent::Unknown(_) => 1,
        ResponsesStreamEvent::MessageStop
        | ResponsesStreamEvent::ContentBlockStart { .. }
        | ResponsesStreamEvent::ContentBlockStop { .. }
        | ResponsesStreamEvent::ToolCallStop { .. } => 0,
        ResponsesStreamEvent::Item(ItemField::Message(message)) => {
            1 + message
                .content
                .iter()
                .map(|part| match part {
                    ItemContentPart::ReasoningText { .. } => 3,
                    _ => 1,
                })
                .sum::<usize>()
        }
        ResponsesStreamEvent::Item(ItemField::FunctionCall(call)) => {
            2 + usize::from(!call.arguments.is_empty())
        }
        ResponsesStreamEvent::Item(ItemField::FunctionCallOutput(_))
        | ResponsesStreamEvent::Item(ItemField::Unknown(_)) => 1,
        ResponsesStreamEvent::Item(ItemField::Reasoning(reasoning)) => {
            let content_len = reasoning
                .content
                .as_ref()
                .map(|parts| parts.len())
                .unwrap_or(0);
            3 + content_len + reasoning.summary.len()
        }
    };
    events.reserve(estimated_events);

    let mut push_event = |event: UnifiedStreamEvent| {
        transformer.update_session_from_stream_event(&event);
        if let Some(encoded) =
            openai::transform_unified_stream_event_to_openai_event(event, transformer)
        {
            events.push(encoded);
        }
    };

    match event {
        ResponsesStreamEvent::Ignored => {}
        ResponsesStreamEvent::ResponseCreated { .. } => {}
        ResponsesStreamEvent::ResponseCompleted { response }
        | ResponsesStreamEvent::ResponseIncomplete { response } => {
            for event in response_terminal_stream_events(response) {
                push_event(event);
            }
        }
        ResponsesStreamEvent::OutputItemAdded { output_index, item } => match item {
            ItemField::Message(_) => {}
            ItemField::FunctionCall(call) => {
                push_event(UnifiedStreamEvent::ToolCallStart {
                    index: output_index,
                    id: call.call_id,
                    name: call.name,
                });
            }
            ItemField::Reasoning(_) => {
                push_event(UnifiedStreamEvent::ReasoningStart {
                    index: output_index,
                });
            }
            ItemField::FunctionCallOutput(output) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
            }
            ItemField::Unknown(value) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: Some(output_index),
                    data: value,
                });
            }
        },
        ResponsesStreamEvent::OutputItemDone { output_index, item } => match item {
            ItemField::FunctionCall(call) => {
                push_event(UnifiedStreamEvent::ToolCallStop {
                    index: output_index,
                    id: Some(call.call_id),
                });
            }
            ItemField::Reasoning(_) => {
                push_event(UnifiedStreamEvent::ReasoningStop {
                    index: output_index,
                });
            }
            _ => {}
        },
        ResponsesStreamEvent::ContentPartAdded {
            item_id,
            content_index,
        } => {
            push_event(UnifiedStreamEvent::ContentPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
                part: None,
            });
        }
        ResponsesStreamEvent::ContentPartDone {
            item_id,
            content_index,
        } => {
            push_event(UnifiedStreamEvent::ContentPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: content_index,
            });
        }
        ResponsesStreamEvent::ReasoningSummaryPartAdded {
            item_id,
            summary_index,
        } => {
            push_event(UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
                part: None,
            });
        }
        ResponsesStreamEvent::ReasoningSummaryPartDone {
            item_id,
            summary_index,
        } => {
            push_event(UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: None,
                item_id: Some(item_id),
                part_index: summary_index,
            });
        }
        ResponsesStreamEvent::MessageStart { id: event_id, role } => {
            push_event(UnifiedStreamEvent::MessageStart {
                id: event_id.or(Some(id)),
                model: Some(model),
                role,
            });
        }
        ResponsesStreamEvent::MessageDelta { finish_reason } => {
            push_event(UnifiedStreamEvent::MessageDelta { finish_reason });
        }
        ResponsesStreamEvent::MessageStop => {}
        ResponsesStreamEvent::ContentBlockStart { .. } => {}
        ResponsesStreamEvent::ContentBlockDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            push_event(UnifiedStreamEvent::ContentBlockDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            });
        }
        ResponsesStreamEvent::ContentBlockStop { .. } => {}
        ResponsesStreamEvent::ToolCallStart { index, id, name } => {
            push_event(UnifiedStreamEvent::ToolCallStart { index, id, name });
        }
        ResponsesStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index,
            item_id,
            id,
            name,
            arguments,
        } => {
            push_event(UnifiedStreamEvent::ToolCallArgumentsDelta {
                index,
                item_index,
                item_id,
                id,
                name,
                arguments,
            });
        }
        ResponsesStreamEvent::ToolCallArgumentsDone { .. } => {}
        ResponsesStreamEvent::ToolCallStop { .. } => {}
        ResponsesStreamEvent::ReasoningStart { index } => {
            push_event(UnifiedStreamEvent::ReasoningStart { index });
        }
        ResponsesStreamEvent::ReasoningDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            push_event(UnifiedStreamEvent::ReasoningDelta {
                index,
                item_index,
                item_id,
                part_index,
                text,
            });
        }
        ResponsesStreamEvent::ReasoningStop { index } => {
            push_event(UnifiedStreamEvent::ReasoningStop { index });
        }
        ResponsesStreamEvent::Usage { usage } => {
            push_event(UnifiedStreamEvent::Usage { usage });
        }
        ResponsesStreamEvent::Blob { index, data } => {
            push_event(UnifiedStreamEvent::BlobDelta { index, data });
        }
        ResponsesStreamEvent::Error { error } => {
            push_event(UnifiedStreamEvent::Error { error });
        }
        ResponsesStreamEvent::Item(item) => match item {
            ItemField::Message(message) => {
                push_event(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });

                for (index, part) in message.content.into_iter().enumerate() {
                    let index = index as u32;
                    match part {
                        ItemContentPart::InputText { text }
                        | ItemContentPart::OutputText { text, .. }
                        | ItemContentPart::Text { text }
                        | ItemContentPart::SummaryText { text } => {
                            push_event(UnifiedStreamEvent::ContentBlockDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                        }
                        ItemContentPart::ReasoningText { text } => {
                            push_event(UnifiedStreamEvent::ReasoningStart { index });
                            push_event(UnifiedStreamEvent::ReasoningDelta {
                                index,
                                item_index: Some(0),
                                item_id: Some(message.id.clone()),
                                part_index: Some(index),
                                text,
                            });
                            push_event(UnifiedStreamEvent::ReasoningStop { index });
                        }
                        other => {
                            push_event(UnifiedStreamEvent::BlobDelta {
                                index: Some(index),
                                data: serde_json::to_value(other).unwrap_or(Value::Null),
                            });
                        }
                    }
                }
            }
            ItemField::FunctionCall(call) => {
                push_event(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                push_event(UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: call.call_id.clone(),
                    name: call.name.clone(),
                });
                if !call.arguments.is_empty() {
                    push_event(UnifiedStreamEvent::ToolCallArgumentsDelta {
                        index: 0,
                        item_index: Some(0),
                        item_id: Some(call.id.clone()),
                        id: Some(call.call_id),
                        name: Some(call.name),
                        arguments: call.arguments,
                    });
                }
            }
            ItemField::FunctionCallOutput(output) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: Some(0),
                    data: serde_json::to_value(output).unwrap_or(Value::Null),
                });
            }
            ItemField::Reasoning(reasoning) => {
                push_event(UnifiedStreamEvent::MessageStart {
                    id: Some(id),
                    model: Some(model),
                    role: UnifiedRole::Assistant,
                });
                push_event(UnifiedStreamEvent::ReasoningStart { index: 0 });
                if let Some(content) = reasoning.content {
                    for (part_index, part) in content.into_iter().enumerate() {
                        if let ItemContentPart::ReasoningText { text }
                        | ItemContentPart::SummaryText { text }
                        | ItemContentPart::Text { text } = part
                        {
                            push_event(UnifiedStreamEvent::ReasoningDelta {
                                index: 0,
                                item_index: Some(0),
                                item_id: Some(reasoning.id.clone()),
                                part_index: Some(part_index as u32),
                                text,
                            });
                        }
                    }
                }
                for (offset, part) in reasoning.summary.into_iter().enumerate() {
                    if let ItemContentPart::ReasoningText { text }
                    | ItemContentPart::SummaryText { text }
                    | ItemContentPart::Text { text } = part
                    {
                        push_event(UnifiedStreamEvent::ReasoningDelta {
                            index: 0,
                            item_index: Some(0),
                            item_id: Some(reasoning.id.clone()),
                            part_index: Some(offset as u32),
                            text,
                        });
                    }
                }
                push_event(UnifiedStreamEvent::ReasoningStop { index: 0 });
            }
            ItemField::Unknown(value) => {
                push_event(UnifiedStreamEvent::BlobDelta {
                    index: None,
                    data: value,
                });
            }
        },
        ResponsesStreamEvent::Unknown(value) => {
            push_event(UnifiedStreamEvent::BlobDelta {
                index: None,
                data: value,
            });
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

fn build_formal_responses_message_item(
    item_id: &str,
    role: UnifiedRole,
    text: &str,
    status: MessageStatus,
) -> ItemField {
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(ItemContentPart::OutputText {
            text: text.to_string(),
            annotations: Vec::new(),
            logprobs: None,
        });
    }

    ItemField::Message(Message {
        _type: "message".to_string(),
        id: item_id.to_string(),
        status,
        role: unified_role_to_message(role),
        content,
    })
}

fn collect_completed_output_items(state: &StreamTransformer) -> Vec<ItemField> {
    state
        .session
        .responses
        .completed_output
        .values()
        .cloned()
        .collect()
}

fn complete_current_message_item(state: &mut StreamTransformer) -> Option<(u32, ItemField)> {
    let output_index = state.session.responses.current_output_index;
    let item_id = state.session.responses.current_item_id.clone()?;
    let role = state.session.responses.current_item_role.clone()?;
    let item = build_formal_responses_message_item(
        &item_id,
        role,
        &state.session.responses.output_text,
        MessageStatus::Completed,
    );
    state
        .session
        .responses
        .completed_output
        .insert(output_index, item.clone());
    Some((output_index, item))
}

fn resolve_responses_stream_item_identity(
    state: &mut StreamTransformer,
    item_index: Option<u32>,
    item_id: Option<String>,
    prefix: &str,
) -> (u32, String) {
    let output_index = item_index.unwrap_or_else(|| {
        let next = state.session.responses.next_output_index;
        state.session.responses.next_output_index = next.saturating_add(1);
        next
    });
    let item_id =
        item_id.unwrap_or_else(|| format!("{prefix}_{}", crate::utils::ID_GENERATOR.generate_id()));
    state
        .session
        .responses
        .output_item_ids
        .insert(output_index, item_id.clone());
    if output_index >= state.session.responses.next_output_index {
        state.session.responses.next_output_index = output_index.saturating_add(1);
    }
    (output_index, item_id)
}

fn item_field_to_formal_responses_item(item: &UnifiedItem, item_id: &str) -> Option<ItemField> {
    match item {
        UnifiedItem::Message(message) => Some(ItemField::Message(Message {
            _type: "message".to_string(),
            id: item_id.to_string(),
            status: MessageStatus::InProgress,
            role: unified_role_to_message_role(message.role.clone()),
            content: Vec::new(),
        })),
        UnifiedItem::FunctionCall(call) => Some(ItemField::FunctionCall(FunctionCall {
            _type: "function_call".to_string(),
            id: item_id.to_string(),
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
            status: MessageStatus::InProgress,
        })),
        UnifiedItem::FunctionCallOutput(output) => {
            Some(ItemField::FunctionCallOutput(FunctionCallOutput {
                _type: "function_call_output".to_string(),
                id: item_id.to_string(),
                call_id: output.tool_call_id.clone(),
                output: unified_tool_result_to_function_output_payload(output.output.clone()),
                status: MessageStatus::Completed,
            }))
        }
        UnifiedItem::Reasoning(_) => Some(ItemField::Reasoning(ReasoningBody {
            _type: "reasoning".to_string(),
            id: item_id.to_string(),
            content: None,
            summary: Vec::new(),
            encrypted_content: None,
        })),
        UnifiedItem::FileReference(_) => None,
    }
}

fn build_formal_responses_response(
    state: &mut StreamTransformer,
    status: ResponseStatus,
    incomplete_details: Option<IncompleteDetails>,
    output: Vec<ItemField>,
) -> ResponsesResponse {
    let stream_id = state.get_or_generate_stream_id();
    let stream_model = state.get_or_default_stream_model();
    let usage = state.session.usage_cache.clone().map(|usage| Usage {
        input_tokens: usage.input_tokens as u32,
        output_tokens: usage.output_tokens as u32,
        total_tokens: usage.total_tokens as u32,
        input_tokens_details: InputTokensDetails {
            cached_tokens: usage.cached_tokens as u32,
        },
        output_tokens_details: OutputTokensDetails {
            reasoning_tokens: usage.reasoning_tokens as u32,
        },
    });

    let mut metadata = json!({});
    if let Some(finish_reason) = state.session.finish_reason_cache.clone() {
        metadata["finish_reason"] = Value::String(finish_reason);
    }

    ResponsesResponse {
        id: stream_id,
        object: ResponseObject::Response,
        created_at: Utc::now().timestamp(),
        completed_at: matches!(status, ResponseStatus::Completed).then_some(Utc::now().timestamp()),
        status,
        incomplete_details,
        model: stream_model,
        previous_response_id: None,
        instructions: None,
        output,
        error: None,
        tools: Vec::new(),
        tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
        truncation: Truncation::Disabled,
        parallel_tool_calls: true,
        text: TextField {
            format: TextResponseFormat::Text,
            verbosity: None,
        },
        top_p: 1.0,
        presence_penalty: 0.0,
        frequency_penalty: 0.0,
        top_logprobs: 0,
        temperature: 1.0,
        reasoning: None,
        usage,
        max_output_tokens: None,
        max_tool_calls: None,
        store: false,
        background: false,
        service_tier: ServiceTier::Default,
        metadata,
        safety_identifier: None,
        prompt_cache_key: None,
    }
}

fn encode_formal_responses_stream_event(
    event: UnifiedStreamEvent,
    state: &mut StreamTransformer,
) -> Vec<Value> {
    let mut frames = Vec::new();

    match event {
        UnifiedStreamEvent::ItemAdded {
            item_index,
            item_id,
            item,
        } => {
            if !state.session.responses.created_sent {
                state.session.responses.created_sent = true;
                frames.push(json!({
                    "type": "response.created",
                    "response": build_formal_responses_response(
                        state,
                        ResponseStatus::InProgress,
                        None,
                        Vec::new()
                    )
                }));
            }

            let prefix = match &item {
                UnifiedItem::Message(_) => "msg",
                UnifiedItem::Reasoning(_) => "rs",
                UnifiedItem::FunctionCall(_) => "fc",
                UnifiedItem::FunctionCallOutput(_) => "fco",
                UnifiedItem::FileReference(_) => "file",
            };
            let (output_index, item_id) =
                resolve_responses_stream_item_identity(state, item_index, item_id, prefix);

            if let Some(item) = item_field_to_formal_responses_item(&item, &item_id) {
                frames.push(json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": item
                }));
            }
        }
        UnifiedStreamEvent::ItemDone {
            item_index,
            item_id,
            item,
        } => {
            let output_index = item_index.unwrap_or(state.session.responses.current_output_index);
            let item_id = item_id
                .or_else(|| {
                    state
                        .session
                        .responses
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .unwrap_or_else(|| format!("item_{}", crate::utils::ID_GENERATOR.generate_id()));

            let item = match item {
                UnifiedItem::Message(message) => build_formal_responses_message_item(
                    &item_id,
                    message.role,
                    &state.session.responses.output_text,
                    MessageStatus::Completed,
                ),
                UnifiedItem::Reasoning(_) => ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: item_id.clone(),
                    content: None,
                    summary: Vec::new(),
                    encrypted_content: None,
                }),
                UnifiedItem::FunctionCall(call) => ItemField::FunctionCall(FunctionCall {
                    _type: "function_call".to_string(),
                    id: item_id.clone(),
                    call_id: call.id,
                    name: call.name,
                    arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
                    status: MessageStatus::Completed,
                }),
                UnifiedItem::FunctionCallOutput(output) => {
                    ItemField::FunctionCallOutput(FunctionCallOutput {
                        _type: "function_call_output".to_string(),
                        id: item_id.clone(),
                        call_id: output.tool_call_id,
                        output: unified_tool_result_to_function_output_payload(output.output),
                        status: MessageStatus::Completed,
                    })
                }
                UnifiedItem::FileReference(_) => return frames,
            };
            state
                .session
                .responses
                .completed_output
                .insert(output_index, item.clone());
            frames.push(json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item
            }));
        }
        UnifiedStreamEvent::MessageStart { id, model, role } => {
            if let Some(id) = id {
                state.session.stream_id = Some(id);
            }
            if let Some(model) = model {
                state.session.stream_model = Some(model);
            }

            if !state.session.responses.created_sent {
                state.session.responses.created_sent = true;
                frames.push(json!({
                    "type": "response.created",
                    "response": build_formal_responses_response(
                        state,
                        ResponseStatus::InProgress,
                        None,
                        Vec::new()
                    )
                }));
            }

            let item_id = format!("msg_{}", crate::utils::ID_GENERATOR.generate_id());
            let output_index = state.session.responses.next_output_index;
            state.session.responses.current_item_id = Some(item_id.clone());
            state.session.responses.current_item_role = Some(role.clone());
            state.session.responses.current_output_index = output_index;
            state.session.responses.next_output_index = output_index.saturating_add(1);
            state
                .session
                .responses
                .output_item_ids
                .insert(output_index, item_id.clone());
            state.session.responses.output_text.clear();
            state.session.responses.completion_pending = false;

            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": build_formal_responses_message_item(
                    &item_id,
                    role,
                    "",
                    MessageStatus::InProgress
                )
            }));
        }
        UnifiedStreamEvent::ContentBlockDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            let output_index = item_index.unwrap_or(state.session.responses.current_output_index);
            let response_item_id = item_id
                .or_else(|| {
                    state
                        .session
                        .responses
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .or_else(|| state.session.responses.current_item_id.clone());
            if let Some(item_id) = response_item_id {
                state.session.responses.output_text.push_str(&text);
                let content_index = part_index
                    .or(state.session.current_content_part_index)
                    .unwrap_or(index);
                frames.push(json!({
                    "type": "response.output_text.delta",
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": content_index,
                    "delta": text
                }));
            }
        }
        UnifiedStreamEvent::ContentPartAdded {
            item_index,
            item_id,
            part_index,
            ..
        } => {
            let output_index = item_index.unwrap_or(state.session.responses.current_output_index);
            let item_id = item_id
                .or_else(|| {
                    state
                        .session
                        .responses
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .or_else(|| state.session.responses.current_item_id.clone());
            if let Some(item_id) = item_id {
                frames.push(json!({
                    "type": "response.content_part.added",
                    "item_id": item_id,
                    "content_index": part_index
                }));
            }
        }
        UnifiedStreamEvent::ContentPartDone {
            item_index,
            item_id,
            part_index,
        } => {
            let output_index = item_index.unwrap_or(state.session.responses.current_output_index);
            let item_id = item_id
                .or_else(|| {
                    state
                        .session
                        .responses
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .or_else(|| state.session.responses.current_item_id.clone());
            if let Some(item_id) = item_id {
                frames.push(json!({
                    "type": "response.content_part.done",
                    "item_id": item_id,
                    "content_index": part_index
                }));
            }
        }
        UnifiedStreamEvent::MessageDelta { finish_reason } => {
            state.session.finish_reason_cache = finish_reason;
            state.session.responses.completion_pending = true;
        }
        UnifiedStreamEvent::Usage { usage } => {
            state
                .session
                .merge_usage(usage.clone().into(), state.usage_merge_strategy());

            if state.session.responses.completion_pending {
                let finish_reason = state.session.finish_reason_cache.as_deref();
                let (status, incomplete_details) =
                    response_status_from_finish_reason(finish_reason);
                let response_event_type = match status {
                    ResponseStatus::Incomplete => "response.incomplete",
                    _ => "response.completed",
                };
                if let Some((output_index, item)) = complete_current_message_item(state) {
                    frames.push(json!({
                        "type": "response.output_item.done",
                        "output_index": output_index,
                        "item": item.clone()
                    }));
                    frames.push(json!({
                        "type": response_event_type,
                        "response": build_formal_responses_response(
                            state,
                            status.clone(),
                            incomplete_details.clone(),
                            collect_completed_output_items(state)
                        )
                    }));
                } else {
                    frames.push(json!({
                        "type": response_event_type,
                        "response": build_formal_responses_response(
                            state,
                            status,
                            incomplete_details,
                            collect_completed_output_items(state)
                        )
                    }));
                }
                state.session.responses.completion_pending = false;
            }
        }
        UnifiedStreamEvent::ToolCallStart { index, id, name } => {
            let function_call = FunctionCall {
                _type: "function_call".to_string(),
                id: id.clone(),
                call_id: id,
                name,
                arguments: String::new(),
                status: MessageStatus::InProgress,
            };
            state
                .session
                .responses
                .active_tool_calls
                .insert(index, function_call.clone());
            let item = ItemField::FunctionCall(function_call);
            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": item
            }));
        }
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index,
            item_id,
            id,
            name,
            arguments,
        } => {
            let mut response_item_id = item_id
                .clone()
                .or_else(|| {
                    state
                        .session
                        .responses
                        .active_tool_calls
                        .get(&index)
                        .map(|call| call.id.clone())
                })
                .or_else(|| {
                    state
                        .session
                        .responses
                        .output_item_ids
                        .get(&item_index.unwrap_or(index))
                        .cloned()
                });
            if let Some(active_call) = state.session.responses.active_tool_calls.get_mut(&index) {
                if let Some(explicit_item_id) = item_id.clone() {
                    active_call.id = explicit_item_id.clone();
                    response_item_id = Some(explicit_item_id);
                }
                active_call.arguments.push_str(&arguments);
                if let Some(name) = name.clone() {
                    active_call.name = name;
                }
                if let Some(id) = id.clone() {
                    active_call.call_id = id;
                }
            }
            if let Some(item_id) = response_item_id {
                frames.push(json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": item_id,
                    "output_index": item_index.unwrap_or(index),
                    "name": name,
                    "delta": arguments
                }));
            }
        }
        UnifiedStreamEvent::ReasoningStart { index } => {
            let item_id = format!("rs_{}", crate::utils::ID_GENERATOR.generate_id());
            state
                .session
                .responses
                .reasoning_item_ids
                .insert(index, item_id.clone());
            state
                .session
                .responses
                .reasoning_summaries
                .insert(index, String::new());
            let item = ItemField::Reasoning(ReasoningBody {
                _type: "reasoning".to_string(),
                id: item_id.clone(),
                content: None,
                summary: Vec::new(),
                encrypted_content: None,
            });
            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": item
            }));
        }
        UnifiedStreamEvent::ReasoningSummaryPartAdded {
            item_index,
            item_id,
            part_index,
            ..
        } => {
            let output_index = item_index.unwrap_or_default();
            let item_id = item_id.or_else(|| {
                state
                    .session
                    .responses
                    .reasoning_item_ids
                    .get(&output_index)
                    .cloned()
            });
            if let Some(item_id) = item_id {
                frames.push(json!({
                    "type": "response.reasoning_summary_part.added",
                    "item_id": item_id,
                    "summary_index": part_index
                }));
            }
        }
        UnifiedStreamEvent::ReasoningDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            let output_index = item_index.unwrap_or(index);
            state
                .session
                .responses
                .reasoning_summaries
                .entry(output_index)
                .and_modify(|summary| summary.push_str(&text))
                .or_insert_with(|| text.clone());
            let summary_index = part_index
                .or(state.session.current_reasoning_part_index)
                .unwrap_or(index);
            let response_item_id = item_id.or_else(|| {
                state
                    .session
                    .responses
                    .reasoning_item_ids
                    .get(&output_index)
                    .cloned()
            });
            frames.push(json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": response_item_id.unwrap_or_default(),
                "summary_index": summary_index,
                "delta": text
            }));
        }
        UnifiedStreamEvent::ReasoningSummaryPartDone {
            item_index,
            item_id,
            part_index,
        } => {
            let output_index = item_index.unwrap_or_default();
            let item_id = item_id.or_else(|| {
                state
                    .session
                    .responses
                    .reasoning_item_ids
                    .get(&output_index)
                    .cloned()
            });
            if let Some(item_id) = item_id {
                frames.push(json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": item_id,
                    "summary_index": part_index
                }));
            }
        }
        UnifiedStreamEvent::ReasoningStop { index } => {
            if let Some(item_id) = state.session.responses.reasoning_item_ids.remove(&index) {
                let summary = state
                    .session
                    .responses
                    .reasoning_summaries
                    .remove(&index)
                    .unwrap_or_default();
                let item = ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: item_id.clone(),
                    content: None,
                    summary: (!summary.is_empty())
                        .then_some(vec![ItemContentPart::SummaryText { text: summary }])
                        .unwrap_or_default(),
                    encrypted_content: None,
                });
                state
                    .session
                    .responses
                    .completed_output
                    .insert(index, item.clone());
                frames.push(json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": item_id,
                    "summary_index": 0
                }));
                frames.push(json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item
                }));
            }
        }
        UnifiedStreamEvent::ToolCallStop { index, .. } => {
            if let Some(mut function_call) =
                state.session.responses.active_tool_calls.remove(&index)
            {
                function_call.status = MessageStatus::Completed;
                let item = ItemField::FunctionCall(function_call);
                state
                    .session
                    .responses
                    .completed_output
                    .insert(index, item.clone());
                if let ItemField::FunctionCall(function_call) = &item {
                    frames.push(json!({
                        "type": "response.function_call_arguments.done",
                        "item_id": function_call.id,
                        "output_index": index,
                        "call_id": function_call.call_id,
                        "arguments": function_call.arguments
                    }));
                }
                frames.push(json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item
                }));
            }
        }
        UnifiedStreamEvent::MessageStop => {
            if state.session.responses.completion_pending {
                let finish_reason = state.session.finish_reason_cache.as_deref();
                let (status, incomplete_details) =
                    response_status_from_finish_reason(finish_reason);
                let response_event_type = match status {
                    ResponseStatus::Incomplete => "response.incomplete",
                    _ => "response.completed",
                };
                if let Some((output_index, item)) = complete_current_message_item(state) {
                    frames.push(json!({
                        "type": "response.output_item.done",
                        "output_index": output_index,
                        "item": item
                    }));
                }
                frames.push(json!({
                    "type": response_event_type,
                    "response": build_formal_responses_response(
                        state,
                        status,
                        incomplete_details,
                        collect_completed_output_items(state)
                    )
                }));
                state.session.responses.completion_pending = false;
            }
        }
        UnifiedStreamEvent::ContentBlockStart { .. }
        | UnifiedStreamEvent::ContentBlockStop { .. }
        | UnifiedStreamEvent::Error { .. } => {}
        UnifiedStreamEvent::BlobDelta { index, data } => {
            if !state.session.responses.created_sent {
                state.session.responses.created_sent = true;
                frames.push(json!({
                    "type": "response.created",
                    "response": build_formal_responses_response(
                        state,
                        ResponseStatus::InProgress,
                        None,
                        Vec::new()
                    )
                }));
            }

            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": index.unwrap_or_default(),
                "item": data
            }));
        }
    }

    frames
}

pub fn transform_unified_stream_events_to_responses_events(
    stream_events: Vec<UnifiedStreamEvent>,
    state: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut events = Vec::new();

    for event in stream_events {
        state.update_session_from_stream_event(&event);
        for frame in encode_formal_responses_stream_event(event, state) {
            events.push(SseEvent {
                data: serde_json::to_string(&frame).unwrap_or_default(),
                ..Default::default()
            });
        }
    }

    if events.is_empty() {
        None
    } else {
        Some(events)
    }
}

pub fn transform_unified_chunk_to_responses_events(
    unified_chunk: UnifiedChunkResponse,
    state: &mut StreamTransformer,
) -> Option<Vec<SseEvent>> {
    let mut stream_events = Vec::new();

    for choice in unified_chunk.choices {
        if let Some(role) = choice.delta.role {
            stream_events.push(UnifiedStreamEvent::MessageStart {
                id: Some(unified_chunk.id.clone()),
                model: unified_chunk.model.clone(),
                role,
            });
        }

        for part in choice.delta.content {
            match part {
                UnifiedContentPartDelta::TextDelta { index, text } => {
                    stream_events.push(UnifiedStreamEvent::ContentBlockDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    });
                }
                UnifiedContentPartDelta::ImageDelta { index, url, data } => {
                    stream_events.push(UnifiedStreamEvent::BlobDelta {
                        index: Some(index),
                        data: json!({
                            "type": "image_delta",
                            "url": url,
                            "data": data
                        }),
                    });
                }
                UnifiedContentPartDelta::ToolCallDelta(tool_call) => {
                    if let (Some(id), Some(name)) = (tool_call.id.clone(), tool_call.name.clone()) {
                        stream_events.push(UnifiedStreamEvent::ToolCallStart {
                            index: tool_call.index,
                            id,
                            name,
                        });
                    }
                    if let Some(arguments) = tool_call.arguments {
                        stream_events.push(UnifiedStreamEvent::ToolCallArgumentsDelta {
                            index: tool_call.index,
                            item_index: None,
                            item_id: None,
                            id: tool_call.id,
                            name: tool_call.name,
                            arguments,
                        });
                    }
                }
            }
        }

        if choice.finish_reason.is_some() {
            stream_events.push(UnifiedStreamEvent::MessageDelta {
                finish_reason: choice.finish_reason,
            });
        }
    }

    if let Some(usage) = unified_chunk.usage {
        stream_events.push(UnifiedStreamEvent::Usage { usage });
    }

    transform_unified_stream_events_to_responses_events(stream_events, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_unified_request_to_responses_preserves_structured_input_items() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::Text {
                        text: "hello".to_string(),
                    },
                    UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: "call_1".to_string(),
                        name: "lookup".to_string(),
                        arguments: serde_json::json!({"city": "Boston"}),
                    }),
                    UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: "call_1".to_string(),
                        name: Some("lookup".to_string()),
                        output: UnifiedToolResultOutput::Json {
                            value: json!({"ok": true}),
                        },
                    }),
                    UnifiedContentPart::ImageData {
                        mime_type: "image/png".to_string(),
                        data: "ZmFrZQ==".to_string(),
                    },
                    UnifiedContentPart::FileUrl {
                        url: "https://files.example.com/report.pdf".to_string(),
                        mime_type: Some("application/pdf".to_string()),
                        filename: None,
                    },
                    UnifiedContentPart::Reasoning {
                        text: "internal reasoning".to_string(),
                    },
                    UnifiedContentPart::ExecutableCode {
                        language: "python".to_string(),
                        code: "print(1)".to_string(),
                    },
                ],
            }],
            extensions: Some(UnifiedRequestExtensions {
                responses: Some(UnifiedResponsesRequestExtension {
                    instructions: Some("Be concise".to_string()),
                    tool_choice: Some(json!("required")),
                    text_format: Some(json!({"type":"json_object"})),
                    reasoning: Some(json!({"effort":"medium"})),
                    parallel_tool_calls: Some(false),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let responses_req: ResponsesRequestPayload = unified_req.into();

        assert_eq!(responses_req.instructions.as_deref(), Some("Be concise"));
        assert!(matches!(
            responses_req.tool_choice,
            Some(ToolChoice::Value(ToolChoiceValue::Required))
        ));
        assert!(matches!(
            responses_req.text.as_ref().map(|text| &text.format),
            Some(TextResponseFormat::JsonObject)
        ));
        assert!(matches!(
            responses_req
                .reasoning
                .as_ref()
                .and_then(|r| r.effort.as_ref()),
            Some(ReasoningEffort::Medium)
        ));
        assert_eq!(responses_req.parallel_tool_calls, Some(false));

        let Input::Items(items) = responses_req.input else {
            panic!("Expected item-based responses input");
        };
        assert!(matches!(
            &items[0],
            ItemField::Message(Message { content, .. })
            if matches!(&content[0], ItemContentPart::InputText { text } if text == "hello")
        ));
        assert!(matches!(
            &items[1],
            ItemField::FunctionCall(FunctionCall { call_id, name, arguments, .. })
            if call_id == "call_1" && name == "lookup" && arguments == "{\"city\":\"Boston\"}"
        ));
        assert!(matches!(
            &items[2],
            ItemField::FunctionCallOutput(FunctionCallOutput { call_id, output, .. })
            if call_id == "call_1" && matches!(output, FunctionCallOutputPayload::Unknown(value) if value == &json!({"ok": true}))
        ));
        assert!(matches!(
            &items[3],
            ItemField::Message(Message { content, .. })
            if matches!(&content[0], ItemContentPart::InputImage { image_url: Some(url), .. } if url == "data:image/png;base64,ZmFrZQ==")
                && matches!(&content[1], ItemContentPart::InputFile { file_url: Some(url), .. } if url == "https://files.example.com/report.pdf")
        ));
        assert!(matches!(
            &items[4],
            ItemField::Reasoning(ReasoningBody { summary, .. })
            if matches!(&summary[0], ItemContentPart::SummaryText { text } if text == "internal reasoning")
        ));
        assert!(matches!(
            &items[5],
            ItemField::Message(Message { content, .. })
            if matches!(&content[0], ItemContentPart::InputText { text } if text == "```python\nprint(1)\n```")
        ));
    }

    #[test]
    fn test_unified_request_to_responses_derives_rich_system_instructions() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: vec![
                UnifiedMessage {
                    role: UnifiedRole::System,
                    content: vec![
                        UnifiedContentPart::Text {
                            text: "Follow policy".to_string(),
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
                        UnifiedContentPart::ExecutableCode {
                            language: "python".to_string(),
                            code: "print(1)".to_string(),
                        },
                        UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: "call_1".to_string(),
                            name: "lookup".to_string(),
                            arguments: json!({"city":"Boston"}),
                        }),
                    ],
                },
                UnifiedMessage {
                    role: UnifiedRole::User,
                    content: vec![UnifiedContentPart::Text {
                        text: "hello".to_string(),
                    }],
                },
            ],
            ..Default::default()
        };

        let responses_req: ResponsesRequestPayload = unified_req.into();

        assert_eq!(
            responses_req.instructions.as_deref(),
            Some(
                "Follow policy\ndata:image/png;base64,ZmFrZQ==\nfile_url: https://files.example.com/report.pdf\nmime_type: application/pdf\n```python\nprint(1)\n```\ntool_call: lookup\narguments: {\"city\":\"Boston\"}"
            )
        );
    }

    #[test]
    fn test_responses_request_from_shorthand_input_message_preserves_text() {
        let payload = json!({
            "model": "gemini/gemini-2.5-flash-lite",
            "input": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "input_text",
                            "text": "你好"
                        }
                    ]
                }
            ]
        });

        let unified_req: UnifiedRequest =
            serde_json::from_value::<ResponsesRequestPayload>(payload)
                .expect("valid responses request")
                .into();

        assert_eq!(unified_req.messages.len(), 1);
        assert_eq!(unified_req.messages[0].role, UnifiedRole::User);
        assert_eq!(
            unified_req.messages[0].content,
            vec![UnifiedContentPart::Text {
                text: "你好".to_string()
            }]
        );
    }

    #[test]
    fn test_unified_response_to_responses_preserves_structured_items() {
        let unified_res = UnifiedResponse {
            id: "resp_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![
                        UnifiedContentPart::Text {
                            text: "done".to_string(),
                        },
                        UnifiedContentPart::ImageData {
                            mime_type: "image/png".to_string(),
                            data: "ZmFrZQ==".to_string(),
                        },
                        UnifiedContentPart::Reasoning {
                            text: "checked the tool output".to_string(),
                        },
                        UnifiedContentPart::ToolCall(UnifiedToolCall {
                            id: "call_1".to_string(),
                            name: "lookup".to_string(),
                            arguments: json!({"city":"Boston"}),
                        }),
                        UnifiedContentPart::ToolResult(UnifiedToolResult {
                            tool_call_id: "call_1".to_string(),
                            name: Some("lookup".to_string()),
                            output: UnifiedToolResultOutput::Json {
                                value: json!({"ok": true}),
                            },
                        }),
                        UnifiedContentPart::ExecutableCode {
                            language: "python".to_string(),
                            code: "print(1)".to_string(),
                        },
                    ],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();

        match &responses_res.output[0] {
            ItemField::Message(message) => {
                assert_eq!(message.content.len(), 2);
                match &message.content[0] {
                    ItemContentPart::OutputText { text, .. } => assert_eq!(text, "done"),
                    _ => panic!("Expected output_text content"),
                }
                match &message.content[1] {
                    ItemContentPart::InputImage { image_url, .. } => {
                        assert_eq!(image_url.as_deref(), Some("data:image/png;base64,ZmFrZQ=="));
                    }
                    other => panic!("Expected input_image content, got {:?}", other),
                }
            }
            _ => panic!("Expected message output"),
        }
        assert!(matches!(
            &responses_res.output[1],
            ItemField::Reasoning(ReasoningBody { summary, .. })
            if matches!(&summary[0], ItemContentPart::SummaryText { text } if text == "checked the tool output")
        ));
        assert!(matches!(
            &responses_res.output[2],
            ItemField::FunctionCall(FunctionCall { call_id, name, arguments, .. })
            if call_id == "call_1" && name == "lookup" && arguments == "{\"city\":\"Boston\"}"
        ));
        assert!(matches!(
            &responses_res.output[3],
            ItemField::FunctionCallOutput(FunctionCallOutput { call_id, output, .. })
            if call_id == "call_1" && matches!(output, FunctionCallOutputPayload::Unknown(value) if value == &json!({"ok": true}))
        ));
        assert!(matches!(
            &responses_res.output[4],
            ItemField::Message(Message { content, .. })
            if matches!(&content[0], ItemContentPart::OutputText { text, .. } if text == "```python\nprint(1)\n```")
        ));
    }

    #[test]
    fn test_responses_response_to_unified_preserves_structured_items() {
        let responses_res = ResponsesResponse {
            id: "resp_1".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![
                ItemField::FunctionCall(FunctionCall {
                    _type: "function_call".to_string(),
                    id: "fc_1".to_string(),
                    call_id: "call_1".to_string(),
                    name: "lookup_weather".to_string(),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                    status: MessageStatus::Completed,
                }),
                ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: "rs_1".to_string(),
                    content: None,
                    summary: vec![ItemContentPart::SummaryText {
                        text: "internal reasoning".to_string(),
                    }],
                    encrypted_content: None,
                }),
                ItemField::Message(Message {
                    _type: "message".to_string(),
                    id: "msg_1".to_string(),
                    status: MessageStatus::Completed,
                    role: MessageRole::Assistant,
                    content: vec![ItemContentPart::OutputText {
                        text: "final answer".to_string(),
                        annotations: vec![],
                        logprobs: None,
                    }],
                }),
            ],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        assert_eq!(unified_res.choices[0].message.content.len(), 3);
        assert!(matches!(
            &unified_res.choices[0].message.content[0],
            UnifiedContentPart::ToolCall(UnifiedToolCall { id, name, arguments })
            if id == "call_1" && name == "lookup_weather" && arguments == &json!({"city":"Boston"})
        ));
        assert!(matches!(
            &unified_res.choices[0].message.content[1],
            UnifiedContentPart::Reasoning { text } if text == "internal reasoning"
        ));
        assert!(matches!(
            &unified_res.choices[0].message.content[2],
            UnifiedContentPart::Text { text } if text == "final answer"
        ));
    }

    #[test]
    fn test_responses_response_to_unified_preserves_provider_metadata() {
        let responses_res = ResponsesResponse {
            id: "resp_1".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![
                    ItemContentPart::Refusal {
                        refusal: "refused".to_string(),
                    },
                    ItemContentPart::OutputText {
                        text: "final answer".to_string(),
                        annotations: vec![Annotation::UrlCitation {
                            url: "https://example.com".to_string(),
                            start_index: 0,
                            end_index: 5,
                            title: "Example".to_string(),
                        }],
                        logprobs: None,
                    },
                ],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({"trace_id":"abc"}),
            safety_identifier: Some("safe-1".to_string()),
            prompt_cache_key: Some("cache-1".to_string()),
        };

        let unified_res: UnifiedResponse = responses_res.into();
        let metadata = unified_res.provider_response_metadata().unwrap();
        let responses_metadata = metadata.responses.as_ref().unwrap();
        assert_eq!(
            responses_metadata.safety_identifier.as_deref(),
            Some("safe-1")
        );
        assert_eq!(
            responses_metadata.prompt_cache_key.as_deref(),
            Some("cache-1")
        );
        assert_eq!(responses_metadata.citations.len(), 1);
        assert_eq!(responses_metadata.refusals.len(), 1);
        assert_eq!(
            responses_metadata
                .metadata
                .as_ref()
                .unwrap()
                .get("trace_id")
                .and_then(Value::as_str),
            Some("abc")
        );
        assert!(matches!(
            &unified_res.choices[0].items[0],
            UnifiedItem::Message(UnifiedMessageItem { content, annotations, .. })
            if matches!(
                &content[..],
                [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
                if text == "refused" && answer == "final answer"
            ) && matches!(
                &annotations[..],
                [UnifiedAnnotation::Citation(UnifiedCitation { url, title, start_index, end_index, .. })]
                if url.as_deref() == Some("https://example.com")
                && title.as_deref() == Some("Example")
                && *start_index == Some(0)
                && *end_index == Some(5)
            )
        ));
    }

    #[test]
    fn test_responses_response_to_unified_preserves_incomplete_status_metadata() {
        let responses_res = ResponsesResponse {
            id: "resp_incomplete".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: None,
            status: ResponseStatus::Incomplete,
            incomplete_details: Some(IncompleteDetails {
                reason: "max_output_tokens".to_string(),
            }),
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Incomplete,
                role: MessageRole::Assistant,
                content: vec![ItemContentPart::OutputText {
                    text: "partial answer".to_string(),
                    annotations: vec![],
                    logprobs: None,
                }],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();
        let responses_metadata = unified_res
            .provider_response_metadata()
            .and_then(|metadata| metadata.responses.as_ref())
            .unwrap();

        assert_eq!(responses_metadata.status.as_deref(), Some("incomplete"));
        assert_eq!(
            responses_metadata
                .incomplete_details
                .as_ref()
                .map(|details| details.reason.as_str()),
            Some("max_output_tokens")
        );
    }

    #[test]
    fn test_unified_response_to_responses_preserves_provider_metadata() {
        let unified_res = UnifiedResponse {
            id: "resp_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![],
            usage: None,
            created: Some(1),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                responses: Some(UnifiedResponsesResponseMetadata {
                    safety_identifier: Some("safe-1".to_string()),
                    prompt_cache_key: Some("cache-1".to_string()),
                    citations: vec![],
                    refusals: vec![],
                    files: vec![],
                    metadata: Some(serde_json::Map::from_iter([(
                        "trace_id".to_string(),
                        json!("abc"),
                    )])),
                    reasoning: None,
                    status: None,
                    incomplete_details: None,
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();
        assert_eq!(responses_res.safety_identifier.as_deref(), Some("safe-1"));
        assert_eq!(responses_res.prompt_cache_key.as_deref(), Some("cache-1"));
        assert_eq!(responses_res.metadata["trace_id"], json!("abc"));
    }

    #[test]
    fn test_unified_response_to_responses_restores_incomplete_status_metadata() {
        let unified_res = UnifiedResponse {
            id: "resp_incomplete".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "partial answer".to_string(),
                    }],
                    ..Default::default()
                },
                items: vec![],
                finish_reason: Some("length".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                responses: Some(UnifiedResponsesResponseMetadata {
                    status: Some("incomplete".to_string()),
                    incomplete_details: Some(UnifiedResponsesIncompleteDetails {
                        reason: "max_output_tokens".to_string(),
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();

        assert!(matches!(responses_res.status, ResponseStatus::Incomplete));
        assert_eq!(responses_res.completed_at, None);
        assert_eq!(
            responses_res
                .incomplete_details
                .as_ref()
                .map(|details| details.reason.as_str()),
            Some("max_output_tokens")
        );
    }

    #[test]
    fn test_unified_response_to_responses_preserves_file_url_as_input_file() {
        let unified_res = UnifiedResponse {
            id: "resp_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::FileUrl {
                        url: "https://files.example.com/report.pdf".to_string(),
                        mime_type: Some("application/pdf".to_string()),
                        filename: None,
                    }],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();
        match &responses_res.output[0] {
            ItemField::Message(message) => match &message.content[0] {
                ItemContentPart::InputFile {
                    filename,
                    file_url,
                    file_id,
                    file_data,
                } => {
                    assert!(filename.is_none());
                    assert_eq!(
                        file_url.as_deref(),
                        Some("https://files.example.com/report.pdf")
                    );
                    assert!(file_id.is_none());
                    assert!(file_data.is_none());
                }
                other => panic!("Expected input_file item, got {:?}", other),
            },
            other => panic!("Expected message output, got {:?}", other),
        }
    }

    #[test]
    fn test_unified_request_to_responses_splits_file_url_and_inline_file_data_paths() {
        let unified_req = UnifiedRequest {
            messages: vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![
                    UnifiedContentPart::FileUrl {
                        url: "https://files.example.com/report.pdf".to_string(),
                        mime_type: Some("application/pdf".to_string()),
                        filename: Some("report.pdf".to_string()),
                    },
                    UnifiedContentPart::FileData {
                        data: "ZmFrZV9maWxl".to_string(),
                        mime_type: "application/pdf".to_string(),
                        filename: Some("inline.pdf".to_string()),
                    },
                ],
            }],
            ..Default::default()
        };

        let responses_req: ResponsesRequestPayload = unified_req.into();
        let Input::Items(items) = responses_req.input else {
            panic!("Expected item-based responses input");
        };

        assert!(matches!(
            &items[0],
            ItemField::Message(Message { content, .. })
            if matches!(
                &content[0],
                ItemContentPart::InputFile { filename, file_url, file_id, file_data }
                if filename.as_deref() == Some("report.pdf")
                    && file_url.as_deref() == Some("https://files.example.com/report.pdf")
                    && file_id.is_none()
                    && file_data.is_none()
            ) && matches!(
                &content[1],
                ItemContentPart::InputFile { filename, file_url, file_id, file_data }
                if filename.as_deref() == Some("inline.pdf")
                    && file_url.is_none()
                    && file_id.is_none()
                    && file_data.as_deref()
                        == Some("data:application/pdf;base64,ZmFrZV9maWxl")
            )
        ));
    }

    #[test]
    fn test_unified_request_to_responses_preserves_file_reference_id() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: Vec::new(),
            items: vec![UnifiedItem::FileReference(UnifiedFileReferenceItem {
                filename: Some("report.pdf".to_string()),
                mime_type: None,
                file_url: None,
                file_id: Some("file_123".to_string()),
            })],
            ..Default::default()
        };

        let responses_req: ResponsesRequestPayload = unified_req.into();
        let Input::Items(items) = responses_req.input else {
            panic!("Expected item-based responses input");
        };

        assert!(matches!(
            &items[0],
            ItemField::Message(Message { content, .. })
            if matches!(
                &content[0],
                ItemContentPart::InputFile { filename, file_url, file_id, file_data }
                if filename.as_deref() == Some("report.pdf")
                    && file_url.is_none()
                    && file_id.as_deref() == Some("file_123")
                    && file_data.is_none()
            )
        ));
    }

    #[test]
    fn test_responses_request_to_unified_preserves_input_file_id_and_data() {
        let request = ResponsesRequestPayload {
            model: "gpt-4.1".to_string(),
            input: Input::Items(vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::User,
                content: vec![
                    ItemContentPart::InputFile {
                        filename: Some("report.pdf".to_string()),
                        file_url: None,
                        file_id: Some("file_123".to_string()),
                        file_data: None,
                    },
                    ItemContentPart::InputFile {
                        filename: Some("inline.pdf".to_string()),
                        file_url: None,
                        file_id: None,
                        file_data: Some("data:application/pdf;base64,ZmFrZV9maWxl".to_string()),
                    },
                ],
            })]),
            instructions: None,
            tools: None,
            tool_choice: None,
            text: None,
            reasoning: None,
            parallel_tool_calls: None,
            stream: Some(false),
            max_tokens: None,
            temperature: None,
            top_p: None,
        };

        let unified_req: UnifiedRequest = request.into();

        assert!(matches!(
            &unified_req.items[..],
            [
                UnifiedItem::Message(UnifiedMessageItem { content, .. }),
                UnifiedItem::FileReference(UnifiedFileReferenceItem { filename, file_id, file_url, .. })
            ]
            if matches!(
                &content[..],
                [UnifiedContentPart::FileData { data, mime_type, filename }]
                if data == "ZmFrZV9maWxl"
                    && mime_type == "application/pdf"
                    && filename.as_deref() == Some("inline.pdf")
            )
            && filename.as_deref() == Some("report.pdf")
            && file_id.as_deref() == Some("file_123")
            && file_url.is_none()
        ));
    }

    #[test]
    fn test_responses_response_to_unified_preserves_file_references_in_metadata() {
        let responses_res = ResponsesResponse {
            id: "resp_file".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![ItemContentPart::InputFile {
                    filename: Some("report.pdf".to_string()),
                    file_url: Some("https://files.example.com/report.pdf".to_string()),
                    file_id: None,
                    file_data: None,
                }],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();
        let responses_metadata = unified_res
            .provider_response_metadata()
            .and_then(|metadata| metadata.responses.as_ref())
            .unwrap();
        assert_eq!(responses_metadata.files.len(), 1);
        assert_eq!(
            responses_metadata.files[0].filename.as_deref(),
            Some("report.pdf")
        );
        assert_eq!(
            responses_metadata.files[0].file_url.as_deref(),
            Some("https://files.example.com/report.pdf")
        );
        assert!(matches!(
            &unified_res.choices[0].items[..],
            [UnifiedItem::FileReference(UnifiedFileReferenceItem { filename, file_url, .. })]
            if filename.as_deref() == Some("report.pdf")
            && file_url.as_deref() == Some("https://files.example.com/report.pdf")
        ));
    }

    #[test]
    fn test_responses_response_to_unified_preserves_input_file_id_and_data() {
        let responses_res = ResponsesResponse {
            id: "resp_file".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![
                    ItemContentPart::InputFile {
                        filename: Some("report.pdf".to_string()),
                        file_url: None,
                        file_id: Some("file_123".to_string()),
                        file_data: None,
                    },
                    ItemContentPart::InputFile {
                        filename: Some("inline.pdf".to_string()),
                        file_url: None,
                        file_id: None,
                        file_data: Some("data:application/pdf;base64,ZmFrZV9maWxl".to_string()),
                    },
                ],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();
        let responses_metadata = unified_res
            .provider_response_metadata()
            .and_then(|metadata| metadata.responses.as_ref())
            .unwrap();

        assert!(matches!(
            &responses_metadata.files[..],
            [
                UnifiedResponsesFileReference { filename, file_id, file_url, file_data },
                UnifiedResponsesFileReference { filename: inline_name, file_id: inline_id, file_url: inline_url, file_data: inline_data }
            ]
            if filename.as_deref() == Some("report.pdf")
                && file_id.as_deref() == Some("file_123")
                && file_url.is_none()
                && file_data.is_none()
                && inline_name.as_deref() == Some("inline.pdf")
                && inline_id.is_none()
                && inline_url.is_none()
                && inline_data.as_deref() == Some("data:application/pdf;base64,ZmFrZV9maWxl")
        ));
        assert!(matches!(
            &unified_res.choices[0].items[..],
            [
                UnifiedItem::Message(UnifiedMessageItem { content, .. }),
                UnifiedItem::FileReference(UnifiedFileReferenceItem { filename, file_id, file_url, .. })
            ]
            if matches!(
                &content[..],
                [UnifiedContentPart::FileData { data, mime_type, filename }]
                if data == "ZmFrZV9maWxl"
                    && mime_type == "application/pdf"
                    && filename.as_deref() == Some("inline.pdf")
            )
            && filename.as_deref() == Some("report.pdf")
            && file_id.as_deref() == Some("file_123")
            && file_url.is_none()
        ));
    }

    #[test]
    fn test_responses_response_to_unified_drops_input_file_instead_of_placeholder_text() {
        let responses_res = ResponsesResponse {
            id: "resp_file".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![
                    ItemContentPart::OutputText {
                        text: "usable text".to_string(),
                        annotations: vec![],
                        logprobs: None,
                    },
                    ItemContentPart::InputFile {
                        filename: Some("doc.txt".to_string()),
                        file_url: Some("https://example.com/doc.txt".to_string()),
                        file_id: None,
                        file_data: None,
                    },
                ],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        assert_eq!(unified_res.choices[0].message.content.len(), 1);
        match &unified_res.choices[0].message.content[0] {
            UnifiedContentPart::Text { text } => assert_eq!(text, "usable text"),
            _ => panic!("Expected plain text output"),
        }
    }

    #[test]
    fn test_responses_response_deserializes_unknown_item_type_without_failing() {
        let raw = json!({
            "id": "resp_1",
            "object": "response",
            "created_at": 1,
            "completed_at": 1,
            "status": "completed",
            "incomplete_details": null,
            "model": "gpt-4.1",
            "previous_response_id": null,
            "instructions": null,
            "output": [
                {
                    "type": "custom_unknown_item",
                    "id": "x_1",
                    "payload": {"foo": "bar"}
                },
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "ok",
                            "annotations": []
                        }
                    ]
                }
            ],
            "error": null,
            "tools": [],
            "tool_choice": "auto",
            "truncation": "disabled",
            "parallel_tool_calls": true,
            "text": {"format": {"type": "text"}},
            "top_p": 1.0,
            "presence_penalty": 0.0,
            "frequency_penalty": 0.0,
            "top_logprobs": 0,
            "temperature": 1.0,
            "reasoning": null,
            "usage": null,
            "max_output_tokens": null,
            "max_tool_calls": null,
            "store": true,
            "background": false,
            "service_tier": "default",
            "metadata": {},
            "safety_identifier": null,
            "prompt_cache_key": null
        });

        let responses_res: ResponsesResponse = serde_json::from_value(raw).unwrap();
        let unified_res: UnifiedResponse = responses_res.into();

        assert_eq!(unified_res.choices.len(), 1);
        match &unified_res.choices[0].message.content[0] {
            UnifiedContentPart::Text { text } => assert_eq!(text, "ok"),
            _ => panic!("Expected text output"),
        }
    }

    #[test]
    fn test_unified_chunk_to_responses_uses_formal_stream_events() {
        let unified_chunk = UnifiedChunkResponse {
            id: "chunk_1".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(UnifiedRole::Assistant),
                    content: vec![UnifiedContentPartDelta::TextDelta {
                        index: 0,
                        text: "hello".to_string(),
                    }],
                },
                finish_reason: Some("stop".to_string()),
            }],
            usage: Some(UnifiedUsage {
                input_tokens: 3,
                output_tokens: 5,
                total_tokens: 8,
                ..Default::default()
            }),
            ..Default::default()
        };

        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_chunk_to_responses_events(unified_chunk, &mut state).unwrap();
        let chunks: Vec<Value> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();

        assert_eq!(chunks[0]["type"], json!("response.created"));
        assert_eq!(chunks[1]["type"], json!("response.output_item.added"));
        assert_eq!(chunks[1]["item"]["role"], json!("assistant"));
        assert_eq!(chunks[2]["type"], json!("response.output_text.delta"));
        assert_eq!(chunks[2]["delta"], json!("hello"));
        assert_eq!(chunks[3]["type"], json!("response.output_item.done"));
        assert_eq!(chunks[4]["type"], json!("response.completed"));
        assert_eq!(
            chunks[4]["response"]["usage"],
            json!({
                "input_tokens": 3,
                "output_tokens": 5,
                "total_tokens": 8,
                "input_tokens_details": {
                    "cached_tokens": 0
                },
                "output_tokens_details": {
                    "reasoning_tokens": 0
                }
            })
        );
    }

    #[test]
    fn test_responses_chunk_to_unified_stream_events_preserves_response_incomplete() {
        let chunk = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ResponseIncomplete {
                response: ResponsesResponse {
                    id: "resp_1".to_string(),
                    object: ResponseObject::Response,
                    created_at: 1,
                    completed_at: None,
                    status: ResponseStatus::Incomplete,
                    incomplete_details: Some(IncompleteDetails {
                        reason: "max_output_tokens".to_string(),
                    }),
                    model: "gpt-4.1".to_string(),
                    previous_response_id: None,
                    instructions: None,
                    output: vec![],
                    error: None,
                    tools: vec![],
                    tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
                    truncation: Truncation::Disabled,
                    parallel_tool_calls: true,
                    text: TextField {
                        format: TextResponseFormat::Text,
                        verbosity: None,
                    },
                    top_p: 1.0,
                    presence_penalty: 0.0,
                    frequency_penalty: 0.0,
                    top_logprobs: 0,
                    temperature: 1.0,
                    reasoning: None,
                    usage: Some(Usage {
                        input_tokens: 3,
                        output_tokens: 5,
                        total_tokens: 8,
                        input_tokens_details: InputTokensDetails { cached_tokens: 0 },
                        output_tokens_details: OutputTokensDetails {
                            reasoning_tokens: 0,
                        },
                    }),
                    max_output_tokens: None,
                    max_tool_calls: None,
                    store: false,
                    background: false,
                    service_tier: ServiceTier::Default,
                    metadata: json!({}),
                    safety_identifier: None,
                    prompt_cache_key: None,
                },
            },
        };

        let events = responses_chunk_to_unified_stream_events(chunk);

        assert_eq!(
            events,
            vec![
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("length".to_string()),
                },
                UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: 3,
                        output_tokens: 5,
                        total_tokens: 8,
                        cached_tokens: Some(0),
                        reasoning_tokens: Some(0),
                        ..Default::default()
                    },
                },
            ]
        );
    }

    #[test]
    fn test_responses_chunk_to_unified_stream_events_maps_function_call_item() {
        let chunk = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::Item(ItemField::FunctionCall(FunctionCall {
                _type: "function_call".to_string(),
                id: "fc_1".to_string(),
                call_id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
                arguments: "{\"city\":\"Boston\"}".to_string(),
                status: MessageStatus::Completed,
            })),
        };

        let events = responses_chunk_to_unified_stream_events(chunk);

        assert_eq!(
            events,
            vec![
                UnifiedStreamEvent::ItemAdded {
                    item_index: Some(0),
                    item_id: Some("fc_1".to_string()),
                    item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id: "call_1".to_string(),
                        name: "lookup_weather".to_string(),
                        arguments: json!({"city":"Boston"}),
                    }),
                },
                UnifiedStreamEvent::MessageStart {
                    id: Some("resp_1".to_string()),
                    model: Some("gpt-4.1".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ContentBlockStart {
                    index: 0,
                    kind: UnifiedBlockKind::ToolCall,
                },
                UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_1".to_string(),
                    name: "lookup_weather".to_string(),
                },
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: 0,
                    item_index: Some(0),
                    item_id: Some("fc_1".to_string()),
                    id: Some("call_1".to_string()),
                    name: Some("lookup_weather".to_string()),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                },
                UnifiedStreamEvent::ToolCallStop { index: 0, id: None },
                UnifiedStreamEvent::ContentBlockStop { index: 0 },
                UnifiedStreamEvent::ItemDone {
                    item_index: Some(0),
                    item_id: Some("fc_1".to_string()),
                    item: UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id: "call_1".to_string(),
                        name: "lookup_weather".to_string(),
                        arguments: json!({"city":"Boston"}),
                    }),
                },
            ]
        );
    }

    #[test]
    fn test_responses_chunk_to_unified_stream_events_maps_content_part_lifecycle() {
        let added = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ContentPartAdded {
                item_id: "msg_1".to_string(),
                content_index: 2,
            },
        };
        let done = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ContentPartDone {
                item_id: "msg_1".to_string(),
                content_index: 2,
            },
        };

        assert_eq!(
            responses_chunk_to_unified_stream_events(added),
            vec![UnifiedStreamEvent::ContentPartAdded {
                item_index: None,
                item_id: Some("msg_1".to_string()),
                part_index: 2,
                part: None,
            }]
        );
        assert_eq!(
            responses_chunk_to_unified_stream_events(done),
            vec![UnifiedStreamEvent::ContentPartDone {
                item_index: None,
                item_id: Some("msg_1".to_string()),
                part_index: 2,
            }]
        );
    }

    #[test]
    fn test_responses_chunk_to_unified_stream_events_maps_reasoning_summary_lifecycle() {
        let added = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ReasoningSummaryPartAdded {
                item_id: "rs_1".to_string(),
                summary_index: 0,
            },
        };
        let done = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ReasoningSummaryPartDone {
                item_id: "rs_1".to_string(),
                summary_index: 0,
            },
        };

        assert_eq!(
            responses_chunk_to_unified_stream_events(added),
            vec![UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: None,
                item_id: Some("rs_1".to_string()),
                part_index: 0,
                part: None,
            }]
        );
        assert_eq!(
            responses_chunk_to_unified_stream_events(done),
            vec![UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: None,
                item_id: Some("rs_1".to_string()),
                part_index: 0,
            }]
        );
    }

    #[test]
    fn test_responses_chunk_response_serializes_as_standard_event() {
        let chunk = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: Some(0),
                item_id: Some("msg_1".to_string()),
                part_index: Some(0),
                text: "hello".to_string(),
            },
        };

        let value = serde_json::to_value(chunk).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "hello"
            })
        );
    }

    #[test]
    fn test_responses_chunk_response_serializes_tool_arguments_as_standard_event() {
        let chunk = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                id: Some("call_1".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
        };

        let value = serde_json::to_value(chunk).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_1",
                "output_index": 0,
                "name": "lookup_weather",
                "delta": "{\"city\":\"Boston\"}"
            })
        );
    }

    #[test]
    fn test_responses_chunk_response_serializes_tool_arguments_done_as_standard_event() {
        let chunk = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ToolCallArgumentsDone {
                index: 0,
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                id: Some("call_1".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
        };

        let value = serde_json::to_value(chunk).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "response.function_call_arguments.done",
                "item_id": "fc_1",
                "output_index": 0,
                "call_id": "call_1",
                "arguments": "{\"city\":\"Boston\"}"
            })
        );
    }

    #[test]
    fn test_responses_chunk_response_serializes_reasoning_delta_as_standard_event() {
        let chunk = ResponsesChunkResponse {
            id: "resp_1".to_string(),
            model: "gpt-4.1".to_string(),
            event: ResponsesStreamEvent::ReasoningDelta {
                index: 1,
                item_index: Some(1),
                item_id: Some("rs_1".to_string()),
                part_index: Some(2),
                text: "step".to_string(),
            },
        };

        let value = serde_json::to_value(chunk).unwrap();

        assert_eq!(
            value,
            json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": "rs_1",
                "summary_index": 2,
                "delta": "step"
            })
        );
    }

    #[test]
    fn test_responses_chunk_response_deserializes_legacy_wrapped_delta() {
        let chunk: ResponsesChunkResponse = serde_json::from_value(json!({
            "id": "resp_legacy",
            "model": "gpt-4.1",
            "delta": {
                "type": "response.output_text.delta",
                "item_id": "msg_1",
                "output_index": 0,
                "content_index": 0,
                "delta": "hello"
            }
        }))
        .unwrap();

        assert_eq!(chunk.id, "resp_legacy");
        assert_eq!(chunk.model, "gpt-4.1");
        assert!(matches!(
            chunk.event,
            ResponsesStreamEvent::ContentBlockDelta {
                index: 0,
                ref text,
                ..
            } if text == "hello"
        ));
    }

    #[test]
    fn test_responses_response_serializes_typed_enums_to_schema_strings() {
        let response = ResponsesResponse {
            id: "resp_1".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(2),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: Vec::new(),
            error: None,
            tools: Vec::new(),
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: false,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let value = serde_json::to_value(response).unwrap();

        assert_eq!(value["object"], json!("response"));
        assert_eq!(value["status"], json!("completed"));
        assert_eq!(value["service_tier"], json!("default"));
    }

    #[test]
    fn test_function_call_output_payload_deserializes_content_array() {
        let payload: FunctionCallOutputPayload = serde_json::from_value(json!([
            {"type": "text", "text": "hello"},
            {"type": "file", "filename": "report.pdf", "file_url": "https://files.example.com/report.pdf"},
            {"type": "image", "image_url": "https://images.example.com/1.png"}
        ]))
        .unwrap();

        match payload {
            FunctionCallOutputPayload::Content(parts) => {
                assert!(
                    matches!(&parts[0], FunctionCallOutputContent::Text { text } if text == "hello")
                );
                assert!(matches!(
                    &parts[1],
                    FunctionCallOutputContent::File { filename, file_url }
                    if filename.as_deref() == Some("report.pdf")
                    && file_url.as_deref() == Some("https://files.example.com/report.pdf")
                ));
                assert!(matches!(
                    &parts[2],
                    FunctionCallOutputContent::Image { image_url, file_url }
                    if image_url.as_deref() == Some("https://images.example.com/1.png")
                    && file_url.is_none()
                ));
            }
            other => panic!("Expected content payload, got {:?}", other),
        }
    }

    #[test]
    fn test_unified_request_to_responses_preserves_multimodal_tool_result_output() {
        let unified_req = UnifiedRequest {
            messages: vec![UnifiedMessage {
                role: UnifiedRole::Tool,
                content: vec![UnifiedContentPart::ToolResult(UnifiedToolResult {
                    tool_call_id: "call_1".to_string(),
                    name: Some("lookup".to_string()),
                    output: UnifiedToolResultOutput::Content {
                        parts: vec![
                            UnifiedToolResultPart::Text {
                                text: "hello".to_string(),
                            },
                            UnifiedToolResultPart::File {
                                filename: Some("report.pdf".to_string()),
                                file_url: Some("https://files.example.com/report.pdf".to_string()),
                            },
                            UnifiedToolResultPart::Image {
                                image_url: Some("https://images.example.com/1.png".to_string()),
                                file_url: None,
                            },
                        ],
                    },
                })],
            }],
            ..Default::default()
        };

        let responses_req: ResponsesRequestPayload = unified_req.into();
        let Input::Items(items) = responses_req.input else {
            panic!("Expected item-based responses input");
        };

        match &items[0] {
            ItemField::FunctionCallOutput(FunctionCallOutput {
                call_id, output, ..
            }) => {
                assert_eq!(call_id, "call_1");
                match output {
                    FunctionCallOutputPayload::Content(parts) => {
                        assert!(matches!(
                            &parts[0],
                            FunctionCallOutputContent::Text { text } if text == "hello"
                        ));
                        assert!(matches!(
                            &parts[1],
                            FunctionCallOutputContent::File { filename, file_url }
                            if filename.as_deref() == Some("report.pdf")
                                && file_url.as_deref()
                                    == Some("https://files.example.com/report.pdf")
                        ));
                        assert!(matches!(
                            &parts[2],
                            FunctionCallOutputContent::Image { image_url, file_url }
                            if image_url.as_deref()
                                == Some("https://images.example.com/1.png")
                                && file_url.is_none()
                        ));
                    }
                    other => panic!("Expected content payload, got {:?}", other),
                }
            }
            other => panic!("Expected function_call_output item, got {:?}", other),
        }
    }

    #[test]
    fn test_responses_response_to_unified_preserves_typed_function_call_output_item() {
        let responses_res = ResponsesResponse {
            id: "resp_function_output".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::FunctionCallOutput(FunctionCallOutput {
                _type: "function_call_output".to_string(),
                id: "fco_1".to_string(),
                call_id: "call_1".to_string(),
                output: FunctionCallOutputPayload::Content(vec![
                    FunctionCallOutputContent::Text {
                        text: "hello".to_string(),
                    },
                    FunctionCallOutputContent::File {
                        filename: Some("report.pdf".to_string()),
                        file_url: Some("https://files.example.com/report.pdf".to_string()),
                    },
                    FunctionCallOutputContent::Image {
                        image_url: Some("https://images.example.com/1.png".to_string()),
                        file_url: None,
                    },
                ]),
                status: MessageStatus::Completed,
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();

        assert!(matches!(
            &unified_res.choices[0].items[0],
            UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
                tool_call_id,
                output: UnifiedToolResultOutput::Content { parts },
                ..
            }) if tool_call_id == "call_1"
                && matches!(&parts[0], UnifiedToolResultPart::Text { text } if text == "hello")
                && matches!(
                    &parts[1],
                    UnifiedToolResultPart::File { filename, file_url }
                    if filename.as_deref() == Some("report.pdf")
                        && file_url.as_deref() == Some("https://files.example.com/report.pdf")
                )
                && matches!(
                    &parts[2],
                    UnifiedToolResultPart::Image { image_url, file_url }
                    if image_url.as_deref() == Some("https://images.example.com/1.png")
                        && file_url.is_none()
                )
        ));
        assert!(matches!(
            &unified_res.choices[0].message.content[0],
            UnifiedContentPart::ToolResult(UnifiedToolResult {
                tool_call_id,
                output: UnifiedToolResultOutput::Content { parts },
                ..
            }) if tool_call_id == "call_1"
                && matches!(&parts[0], UnifiedToolResultPart::Text { text } if text == "hello")
        ));
    }

    #[test]
    fn test_unified_response_to_responses_preserves_multimodal_function_call_output_item() {
        let unified_res = UnifiedResponse {
            id: "resp_function_output_roundtrip".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: Vec::new(),
                    ..Default::default()
                },
                items: vec![UnifiedItem::FunctionCallOutput(
                    UnifiedFunctionCallOutputItem {
                        tool_call_id: "call_1".to_string(),
                        name: Some("lookup".to_string()),
                        output: UnifiedToolResultOutput::Content {
                            parts: vec![
                                UnifiedToolResultPart::Text {
                                    text: "hello".to_string(),
                                },
                                UnifiedToolResultPart::File {
                                    filename: Some("report.pdf".to_string()),
                                    file_url: Some(
                                        "https://files.example.com/report.pdf".to_string(),
                                    ),
                                },
                                UnifiedToolResultPart::Image {
                                    image_url: Some("https://images.example.com/1.png".to_string()),
                                    file_url: None,
                                },
                            ],
                        },
                    },
                )],
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1),
            object: Some("response".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();

        assert!(matches!(
            &responses_res.output[0],
            ItemField::FunctionCallOutput(FunctionCallOutput {
                call_id,
                output: FunctionCallOutputPayload::Content(parts),
                ..
            }) if call_id == "call_1"
                && matches!(&parts[0], FunctionCallOutputContent::Text { text } if text == "hello")
                && matches!(
                    &parts[1],
                    FunctionCallOutputContent::File { filename, file_url }
                    if filename.as_deref() == Some("report.pdf")
                        && file_url.as_deref() == Some("https://files.example.com/report.pdf")
                )
                && matches!(
                    &parts[2],
                    FunctionCallOutputContent::Image { image_url, file_url }
                    if image_url.as_deref() == Some("https://images.example.com/1.png")
                        && file_url.is_none()
                )
        ));
    }

    #[test]
    fn test_responses_response_to_unified_promotes_refusal_to_content_and_metadata() {
        let responses_res = ResponsesResponse {
            id: "resp_refusal".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![
                    ItemContentPart::Refusal {
                        refusal: "cannot comply".to_string(),
                    },
                    ItemContentPart::OutputText {
                        text: "safe answer".to_string(),
                        annotations: vec![],
                        logprobs: None,
                    },
                ],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();

        assert!(matches!(
            &unified_res.choices[0].message.content[..],
            [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
            if text == "cannot comply" && answer == "safe answer"
        ));
        let responses_metadata = unified_res
            .provider_response_metadata()
            .and_then(|metadata| metadata.responses.as_ref())
            .unwrap();
        assert_eq!(responses_metadata.refusals.len(), 1);
        assert_eq!(responses_metadata.refusals[0].refusal, "cannot comply");
    }

    #[test]
    fn test_responses_refusal_survives_cross_provider_unified_conversion() {
        let responses_res = ResponsesResponse {
            id: "resp_refusal_cross_provider".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(1),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::Assistant,
                content: vec![
                    ItemContentPart::Refusal {
                        refusal: "cannot comply".to_string(),
                    },
                    ItemContentPart::OutputText {
                        text: "safe answer".to_string(),
                        annotations: vec![],
                        logprobs: None,
                    },
                ],
            })],
            error: None,
            tools: vec![],
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: true,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata: json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        };

        let unified_res: UnifiedResponse = responses_res.into();
        assert!(matches!(
            &unified_res.choices[0].items[0],
            UnifiedItem::Message(UnifiedMessageItem { content, .. })
            if matches!(
                &content[..],
                [UnifiedContentPart::Refusal { text }, UnifiedContentPart::Text { text: answer }]
                if text == "cannot comply" && answer == "safe answer"
            )
        ));

        let openai_res: openai::OpenAiResponse = unified_res.into();
        let openai_json = serde_json::to_value(openai_res).unwrap();
        assert_eq!(
            openai_json["choices"][0]["message"]["refusal"],
            json!("cannot comply")
        );
        assert_eq!(
            openai_json["choices"][0]["message"]["content"],
            json!("safe answer")
        );
    }

    #[test]
    fn test_unified_response_to_responses_preserves_structured_annotations_and_file_reference_items()
     {
        let unified_res = UnifiedResponse {
            id: "resp_structured".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "legacy".to_string(),
                    }],
                    ..Default::default()
                },
                items: vec![
                    UnifiedItem::Message(UnifiedMessageItem {
                        role: UnifiedRole::Assistant,
                        content: vec![UnifiedContentPart::Text {
                            text: "final answer".to_string(),
                        }],
                        annotations: vec![UnifiedAnnotation::Citation(UnifiedCitation {
                            part_index: Some(0),
                            start_index: Some(0),
                            end_index: Some(5),
                            url: Some("https://example.com".to_string()),
                            title: Some("Example".to_string()),
                            license: None,
                        })],
                    }),
                    UnifiedItem::FileReference(UnifiedFileReferenceItem {
                        filename: Some("report.pdf".to_string()),
                        mime_type: None,
                        file_url: Some("https://files.example.com/report.pdf".to_string()),
                        file_id: None,
                    }),
                ],
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1),
            object: Some("response".to_string()),
            system_fingerprint: None,
            provider_response_metadata: None,
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();

        assert!(matches!(
            &responses_res.output[0],
            ItemField::Message(Message { content, .. })
            if matches!(
                &content[0],
                ItemContentPart::OutputText { text, annotations, .. }
                if text == "final answer"
                && matches!(
                    &annotations[..],
                    [Annotation::UrlCitation { url, title, start_index, end_index }]
                    if url == "https://example.com"
                    && title == "Example"
                    && *start_index == 0
                    && *end_index == 5
                )
            )
        ));
        assert!(matches!(
            &responses_res.output[1],
            ItemField::Message(Message { content, .. })
            if matches!(
                &content[0],
                ItemContentPart::InputFile { filename, file_url, file_id, file_data }
                if filename.as_deref() == Some("report.pdf")
                && file_url.as_deref() == Some("https://files.example.com/report.pdf")
                && file_id.is_none()
                && file_data.is_none()
            )
        ));
    }

    #[test]
    fn test_unified_response_to_responses_restores_refusal_and_reasoning_metadata() {
        let unified_res = UnifiedResponse {
            id: "resp_restore".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![
                        UnifiedContentPart::Reasoning {
                            text: "checked policy".to_string(),
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
            created: Some(1),
            object: Some("response".to_string()),
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                responses: Some(UnifiedResponsesResponseMetadata {
                    safety_identifier: None,
                    prompt_cache_key: None,
                    citations: vec![],
                    refusals: vec![UnifiedResponsesRefusal {
                        refusal: "cannot comply".to_string(),
                    }],
                    files: vec![],
                    metadata: None,
                    reasoning: Some(json!({
                        "encrypted_contents": ["enc_1"]
                    })),
                    status: None,
                    incomplete_details: None,
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();

        assert!(matches!(
            &responses_res.output[1],
            ItemField::Message(Message { content, .. })
            if matches!(&content[0], ItemContentPart::Refusal { refusal } if refusal == "cannot comply")
            && matches!(&content[1], ItemContentPart::OutputText { text, .. } if text == "safe answer")
        ));
        assert!(matches!(
            &responses_res.output[0],
            ItemField::Reasoning(ReasoningBody { encrypted_content, .. })
            if encrypted_content.as_deref() == Some("enc_1")
        ));
    }

    #[test]
    fn test_unified_response_to_responses_restores_file_input_metadata() {
        let unified_res = UnifiedResponse {
            id: "resp_restore_files".to_string(),
            model: Some("gpt-4.1".to_string()),
            choices: vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: vec![UnifiedContentPart::Text {
                        text: "safe answer".to_string(),
                    }],
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }],
            usage: None,
            created: Some(1),
            object: Some("response".to_string()),
            system_fingerprint: None,
            provider_response_metadata: Some(UnifiedProviderResponseMetadata {
                responses: Some(UnifiedResponsesResponseMetadata {
                    safety_identifier: None,
                    prompt_cache_key: None,
                    citations: vec![],
                    refusals: vec![],
                    files: vec![
                        UnifiedResponsesFileReference {
                            filename: Some("report.pdf".to_string()),
                            file_url: None,
                            file_id: Some("file_123".to_string()),
                            file_data: None,
                        },
                        UnifiedResponsesFileReference {
                            filename: Some("inline.pdf".to_string()),
                            file_url: None,
                            file_id: None,
                            file_data: Some("data:application/pdf;base64,ZmFrZV9maWxl".to_string()),
                        },
                    ],
                    metadata: None,
                    reasoning: None,
                    status: None,
                    incomplete_details: None,
                }),
                ..Default::default()
            }),
            synthetic_metadata: None,
        };

        let responses_res: ResponsesResponse = unified_res.into();

        assert!(matches!(
            &responses_res.output[1],
            ItemField::Message(Message { content, .. })
            if matches!(
                &content[0],
                ItemContentPart::InputFile { filename, file_url, file_id, file_data }
                if filename.as_deref() == Some("report.pdf")
                    && file_url.is_none()
                    && file_id.as_deref() == Some("file_123")
                    && file_data.is_none()
            )
        ));
        assert!(matches!(
            &responses_res.output[2],
            ItemField::Message(Message { content, .. })
            if matches!(
                &content[0],
                ItemContentPart::InputFile { filename, file_url, file_id, file_data }
                if filename.as_deref() == Some("inline.pdf")
                    && file_url.is_none()
                    && file_id.is_none()
                    && file_data.as_deref() == Some("data:application/pdf;base64,ZmFrZV9maWxl")
            )
        ));
    }

    #[test]
    fn test_responses_request_to_unified_preserves_responses_extensions() {
        let request = ResponsesRequestPayload {
            model: "gpt-4.1".to_string(),
            input: Input::Items(vec![ItemField::Message(Message {
                _type: "message".to_string(),
                id: "msg_1".to_string(),
                status: MessageStatus::Completed,
                role: MessageRole::User,
                content: vec![ItemContentPart::InputText {
                    text: "hello".to_string(),
                }],
            })]),
            instructions: Some("Follow the house style".to_string()),
            tools: Some(vec![Tool::Function(FunctionTool {
                name: "lookup_weather".to_string(),
                description: Some("Weather lookup".to_string()),
                parameters: Some(json!({"type":"object"})),
                strict: Some(true),
            })]),
            tool_choice: Some(ToolChoice::Value(ToolChoiceValue::Required)),
            text: Some(TextField {
                format: TextResponseFormat::JsonObject,
                verbosity: None,
            }),
            reasoning: Some(Reasoning {
                effort: Some(ReasoningEffort::High),
                summary: Some(ReasoningSummary::Detailed),
            }),
            parallel_tool_calls: Some(false),
            stream: Some(true),
            max_tokens: Some(128),
            temperature: Some(0.2),
            top_p: Some(0.9),
        };

        let unified: UnifiedRequest = request.into();

        assert!(matches!(
            unified.messages.first(),
            Some(UnifiedMessage {
                role: UnifiedRole::System,
                ..
            })
        ));
        assert!(unified.tools.as_ref().is_some_and(|tools| tools.len() == 1));
        let ext = unified.responses_extension().expect("responses extension");
        assert_eq!(ext.instructions.as_deref(), Some("Follow the house style"));
        assert_eq!(ext.parallel_tool_calls, Some(false));
        assert_eq!(ext.tool_choice.as_ref(), Some(&json!("required")));
        assert_eq!(
            ext.text_format.as_ref(),
            Some(&json!({"type":"json_object"}))
        );
        assert_eq!(
            ext.reasoning.as_ref(),
            Some(&json!({"effort":"high","summary":"detailed"}))
        );
    }

    #[test]
    fn test_unified_stream_events_to_responses_events_are_not_stubbed() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let events = vec![
            UnifiedStreamEvent::MessageStart {
                id: Some("resp_1".to_string()),
                model: Some("gpt-4.1".to_string()),
                role: UnifiedRole::Assistant,
            },
            UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "call_1".to_string(),
                name: "lookup_weather".to_string(),
            },
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: Some(0),
                item_id: Some("fc_1".to_string()),
                id: Some("call_1".to_string()),
                name: Some("lookup_weather".to_string()),
                arguments: "{\"city\":\"Boston\"}".to_string(),
            },
            UnifiedStreamEvent::ReasoningStart { index: 1 },
            UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: Some(1),
                item_id: Some("rs_1".to_string()),
                part_index: 2,
                part: None,
            },
            UnifiedStreamEvent::ReasoningDelta {
                index: 1,
                item_index: Some(1),
                item_id: Some("rs_1".to_string()),
                part_index: Some(2),
                text: "thinking".to_string(),
            },
            UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: Some(1),
                item_id: Some("rs_1".to_string()),
                part_index: 2,
            },
            UnifiedStreamEvent::ReasoningStop { index: 1 },
        ];

        let sse = transform_unified_stream_events_to_responses_events(events.clone(), &mut state)
            .unwrap();
        let chunks: Vec<ResponsesChunkResponse> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();

        assert!(
            chunks
                .iter()
                .all(|chunk| !matches!(chunk.event, ResponsesStreamEvent::Unknown(_)))
        );

        let rebuilt: Vec<UnifiedStreamEvent> = chunks
            .into_iter()
            .flat_map(responses_chunk_to_unified_stream_events)
            .collect();

        assert!(rebuilt.iter().any(|event| matches!(
            event,
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: Some(_),
                item_id: Some(_),
                id: Some(_),
                name: Some(name),
                arguments,
            } if name == "lookup_weather"
                && arguments == "{\"city\":\"Boston\"}"
        )));
        assert!(rebuilt.iter().any(|event| matches!(
            event,
            UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index: _,
                item_id: Some(item_id),
                part_index: 2,
                ..
            } if item_id == "rs_1"
        )));
        assert!(rebuilt.iter().any(|event| matches!(
            event,
            UnifiedStreamEvent::ReasoningDelta {
                item_index: _,
                item_id: Some(item_id),
                part_index: Some(2),
                text,
                ..
            } if item_id == "rs_1" && text == "thinking"
        )));
        assert!(rebuilt.iter().any(|event| matches!(
            event,
            UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index: _,
                item_id: Some(item_id),
                part_index: 2,
            } if item_id == "rs_1"
        )));
    }

    #[test]
    fn test_unified_stream_events_to_responses_completed_includes_all_output_items() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_stream_events_to_responses_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("resp_multi".to_string()),
                    model: Some("gpt-4.1".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "final answer".to_string(),
                },
                UnifiedStreamEvent::ToolCallStart {
                    index: 1,
                    id: "call_1".to_string(),
                    name: "lookup_weather".to_string(),
                },
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: 1,
                    item_index: None,
                    item_id: None,
                    id: Some("call_1".to_string()),
                    name: Some("lookup_weather".to_string()),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                },
                UnifiedStreamEvent::ToolCallStop {
                    index: 1,
                    id: Some("call_1".to_string()),
                },
                UnifiedStreamEvent::ReasoningStart { index: 2 },
                UnifiedStreamEvent::ReasoningDelta {
                    index: 2,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "checked policy".to_string(),
                },
                UnifiedStreamEvent::ReasoningStop { index: 2 },
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("stop".to_string()),
                },
                UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: 3,
                        output_tokens: 5,
                        total_tokens: 8,
                        ..Default::default()
                    },
                },
            ],
            &mut state,
        )
        .unwrap();

        let frames: Vec<Value> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();
        let completed = frames
            .iter()
            .find(|frame| frame["type"] == json!("response.completed"))
            .unwrap();
        let output = completed["response"]["output"].as_array().unwrap();

        assert_eq!(output.len(), 3);
        assert!(matches!(
            &output[0],
            value if value["type"] == json!("message")
                && value["content"][0]["type"] == json!("output_text")
                && value["content"][0]["text"] == json!("final answer")
        ));
        assert!(matches!(
            &output[1],
            value if value["type"] == json!("function_call")
                && value["call_id"] == json!("call_1")
                && value["arguments"] == json!("{\"city\":\"Boston\"}")
                && value["status"] == json!("completed")
        ));
        assert!(matches!(
            &output[2],
            value if value["type"] == json!("reasoning")
                && value["summary"][0]["type"] == json!("summary_text")
                && value["summary"][0]["text"] == json!("checked policy")
        ));
    }

    #[test]
    fn test_unified_stream_events_to_responses_emit_function_call_arguments_done() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_stream_events_to_responses_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("resp_tool".to_string()),
                    model: Some("gpt-4.1".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ToolCallStart {
                    index: 0,
                    id: "call_1".to_string(),
                    name: "lookup_weather".to_string(),
                },
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: 0,
                    item_index: Some(0),
                    item_id: Some("fc_1".to_string()),
                    id: Some("call_1".to_string()),
                    name: Some("lookup_weather".to_string()),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                },
                UnifiedStreamEvent::ToolCallStop {
                    index: 0,
                    id: Some("call_1".to_string()),
                },
            ],
            &mut state,
        )
        .unwrap();

        let frames: Vec<Value> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();

        assert!(frames.iter().any(|frame| {
            frame["type"] == json!("response.function_call_arguments.done")
                && frame["item_id"] == json!("fc_1")
                && frame["output_index"] == json!(0)
                && frame["call_id"] == json!("call_1")
                && frame["arguments"] == json!("{\"city\":\"Boston\"}")
        }));
    }

    #[test]
    fn test_unified_stream_events_to_responses_uses_explicit_content_part_lifecycle_for_text_delta()
    {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_stream_events_to_responses_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("resp_parts".to_string()),
                    model: Some("gpt-4.1".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ContentPartAdded {
                    item_index: Some(0),
                    item_id: Some("msg_part".to_string()),
                    part_index: 3,
                    part: None,
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: Some(3),
                    text: "hello".to_string(),
                },
                UnifiedStreamEvent::ContentPartDone {
                    item_index: Some(0),
                    item_id: Some("msg_part".to_string()),
                    part_index: 3,
                },
            ],
            &mut state,
        )
        .unwrap();

        let frames: Vec<Value> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();

        assert!(frames.iter().any(|frame| {
            frame["type"] == json!("response.content_part.added")
                && frame["content_index"] == json!(3)
        }));
        assert!(frames.iter().any(|frame| {
            frame["type"] == json!("response.output_text.delta")
                && frame["content_index"] == json!(3)
                && frame["delta"] == json!("hello")
        }));
        assert!(frames.iter().any(|frame| {
            frame["type"] == json!("response.content_part.done")
                && frame["content_index"] == json!(3)
        }));
    }

    #[test]
    fn test_unified_stream_events_to_responses_uses_explicit_reasoning_part_lifecycle_without_synthetic_added()
     {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_stream_events_to_responses_events(
            vec![
                UnifiedStreamEvent::ReasoningStart { index: 2 },
                UnifiedStreamEvent::ReasoningSummaryPartAdded {
                    item_index: Some(2),
                    item_id: None,
                    part_index: 4,
                    part: None,
                },
                UnifiedStreamEvent::ReasoningDelta {
                    index: 2,
                    item_index: Some(2),
                    item_id: None,
                    part_index: Some(4),
                    text: "step".to_string(),
                },
                UnifiedStreamEvent::ReasoningSummaryPartDone {
                    item_index: Some(2),
                    item_id: None,
                    part_index: 4,
                },
                UnifiedStreamEvent::ReasoningStop { index: 2 },
            ],
            &mut state,
        )
        .unwrap();

        let frames: Vec<Value> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();

        let added_frames: Vec<&Value> = frames
            .iter()
            .filter(|frame| frame["type"] == json!("response.reasoning_summary_part.added"))
            .collect();
        assert_eq!(added_frames.len(), 1);
        assert_eq!(added_frames[0]["summary_index"], json!(4));

        let delta = frames
            .iter()
            .find(|frame| frame["type"] == json!("response.reasoning_summary_text.delta"))
            .unwrap();
        assert_eq!(delta["summary_index"], json!(4));

        let done = frames
            .iter()
            .find(|frame| frame["type"] == json!("response.reasoning_summary_part.done"))
            .unwrap();
        assert_eq!(done["summary_index"], json!(4));
    }

    #[test]
    fn test_unified_stream_events_to_responses_emits_response_incomplete_for_length_finish_reason()
    {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_stream_events_to_responses_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("resp_incomplete".to_string()),
                    model: Some("gpt-4.1".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "partial answer".to_string(),
                },
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("length".to_string()),
                },
                UnifiedStreamEvent::Usage {
                    usage: UnifiedUsage {
                        input_tokens: 3,
                        output_tokens: 5,
                        total_tokens: 8,
                        ..Default::default()
                    },
                },
            ],
            &mut state,
        )
        .unwrap();

        let frames: Vec<Value> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();
        let incomplete = frames
            .iter()
            .find(|frame| frame["type"] == json!("response.incomplete"))
            .unwrap();

        assert_eq!(incomplete["response"]["status"], json!("incomplete"));
        assert_eq!(
            incomplete["response"]["incomplete_details"]["reason"],
            json!("max_output_tokens")
        );
        assert_eq!(incomplete["response"]["completed_at"], Value::Null);
    }

    #[test]
    fn test_unified_stream_events_to_responses_preserve_explicit_stream_id_and_model() {
        let mut state = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Responses);
        let sse = transform_unified_stream_events_to_responses_events(
            vec![
                UnifiedStreamEvent::MessageStart {
                    id: Some("resp_explicit".to_string()),
                    model: Some("gpt-4.1-mini".to_string()),
                    role: UnifiedRole::Assistant,
                },
                UnifiedStreamEvent::ContentBlockDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "hello".to_string(),
                },
                UnifiedStreamEvent::MessageDelta {
                    finish_reason: Some("stop".to_string()),
                },
            ],
            &mut state,
        )
        .unwrap();

        let chunks: Vec<ResponsesChunkResponse> = sse
            .iter()
            .map(|event| serde_json::from_str(&event.data).unwrap())
            .collect();

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].id, "resp_explicit");
        assert_eq!(chunks[0].model, "gpt-4.1-mini");
        assert!(matches!(
            chunks[0].event,
            ResponsesStreamEvent::ResponseCreated { .. }
        ));
        assert!(matches!(
            chunks[1].event,
            ResponsesStreamEvent::OutputItemAdded {
                output_index: 0,
                ..
            }
        ));
        assert!(matches!(
            chunks[2].event,
            ResponsesStreamEvent::ContentBlockDelta {
                index: 0,
                ref text,
                ..
            } if text == "hello"
        ));
    }

    #[test]
    fn test_responses_response_to_unified_preserves_item_family() {
        let responses_res = ResponsesResponse {
            id: "resp_123".to_string(),
            object: ResponseObject::Response,
            created_at: 1,
            completed_at: Some(2),
            status: ResponseStatus::Completed,
            incomplete_details: None,
            model: "gpt-4.1".to_string(),
            previous_response_id: None,
            instructions: None,
            output: vec![
                ItemField::Message(Message {
                    _type: "message".to_string(),
                    id: "msg_1".to_string(),
                    status: MessageStatus::Completed,
                    role: MessageRole::Assistant,
                    content: vec![ItemContentPart::OutputText {
                        text: "done".to_string(),
                        annotations: Vec::new(),
                        logprobs: None,
                    }],
                }),
                ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: "rs_1".to_string(),
                    content: Some(vec![ItemContentPart::ReasoningText {
                        text: "checked".to_string(),
                    }]),
                    summary: Vec::new(),
                    encrypted_content: None,
                }),
                ItemField::FunctionCall(FunctionCall {
                    _type: "function_call".to_string(),
                    id: "fc_1".to_string(),
                    call_id: "call_1".to_string(),
                    name: "lookup".to_string(),
                    arguments: "{\"city\":\"Boston\"}".to_string(),
                    status: MessageStatus::Completed,
                }),
                ItemField::FunctionCallOutput(FunctionCallOutput {
                    _type: "function_call_output".to_string(),
                    id: "fco_1".to_string(),
                    call_id: "call_1".to_string(),
                    output: FunctionCallOutputPayload::Text("ok".to_string()),
                    status: MessageStatus::Completed,
                }),
            ],
            error: None,
            tools: Vec::new(),
            tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
            truncation: Truncation::Disabled,
            parallel_tool_calls: false,
            text: TextField {
                format: TextResponseFormat::Text,
                verbosity: None,
            },
            top_p: 1.0,
            presence_penalty: 0.0,
            frequency_penalty: 0.0,
            top_logprobs: 0,
            temperature: 1.0,
            reasoning: None,
            usage: None,
            max_output_tokens: None,
            max_tool_calls: None,
            store: false,
            background: false,
            service_tier: ServiceTier::Default,
            prompt_cache_key: None,
            safety_identifier: None,
            metadata: json!({}),
        };

        let unified_res: UnifiedResponse = responses_res.into();
        let items = &unified_res.choices[0].items;

        assert_eq!(items.len(), 4);
        assert!(matches!(&items[0], UnifiedItem::Message(_)));
        assert!(matches!(&items[1], UnifiedItem::Reasoning(_)));
        assert!(matches!(&items[2], UnifiedItem::FunctionCall(_)));
        assert!(matches!(
            &items[3],
            UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
                tool_call_id,
                output: UnifiedToolResultOutput::Text { text },
                ..
            }) if tool_call_id == "call_1" && text == "ok"
        ));
    }

    #[test]
    fn test_unified_request_items_to_responses_input() {
        let unified_req = UnifiedRequest {
            model: Some("gpt-4.1".to_string()),
            messages: Vec::new(),
            items: vec![
                UnifiedItem::Message(UnifiedMessageItem {
                    role: UnifiedRole::User,
                    content: vec![UnifiedContentPart::Text {
                        text: "hello".to_string(),
                    }],
                    annotations: Vec::new(),
                }),
                UnifiedItem::Reasoning(UnifiedReasoningItem {
                    content: vec![UnifiedContentPart::Reasoning {
                        text: "checked".to_string(),
                    }],
                    annotations: Vec::new(),
                }),
                UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                    id: "call_1".to_string(),
                    name: "lookup".to_string(),
                    arguments: json!({"city":"Boston"}),
                }),
                UnifiedItem::FunctionCallOutput(UnifiedFunctionCallOutputItem {
                    tool_call_id: "call_1".to_string(),
                    name: None,
                    output: UnifiedToolResultOutput::Text {
                        text: "ok".to_string(),
                    },
                }),
            ],
            tools: None,
            stream: false,
            temperature: None,
            max_tokens: None,
            top_p: None,
            stop: None,
            seed: None,
            presence_penalty: None,
            frequency_penalty: None,
            extensions: None,
        };

        let payload: ResponsesRequestPayload = unified_req.into();
        let Input::Items(items) = payload.input else {
            panic!("expected item input");
        };

        assert_eq!(items.len(), 4);
        assert!(matches!(&items[0], ItemField::Message(_)));
        assert!(matches!(&items[1], ItemField::Reasoning(_)));
        assert!(matches!(&items[2], ItemField::FunctionCall(_)));
        assert!(matches!(
            &items[3],
            ItemField::FunctionCallOutput(FunctionCallOutput {
                call_id,
                output: FunctionCallOutputPayload::Text(text),
                ..
            }) if call_id == "call_1" && text == "ok"
        ));
    }
}
