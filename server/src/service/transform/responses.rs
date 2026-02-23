use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::unified::*;

// --- Request Payloads ---

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponsesRequestPayload {
    pub model: String,
    pub input: Input,
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
    Items(Vec<MessageItem>),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MessageItem {
    #[serde(rename = "type")]
    _type: String,
    pub role: UnifiedRole,
    pub content: Content,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    String(String),
    Parts(Vec<ContentPart>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentPart {
    InputText { text: String },
    InputImage { image_url: String },
}

// --- Request Transformation ---

impl From<ResponsesRequestPayload> for UnifiedRequest {
    fn from(responses_req: ResponsesRequestPayload) -> Self {
        let messages = match responses_req.input {
            Input::String(text) => vec![UnifiedMessage {
                role: UnifiedRole::User,
                content: vec![UnifiedContentPart::Text { text }],
            }],
            Input::Items(items) => items
                .into_iter()
                .map(|item| {
                    let content = match item.content {
                        Content::String(text) => vec![UnifiedContentPart::Text { text }],
                        Content::Parts(parts) => parts
                            .into_iter()
                            .map(|part| match part {
                                ContentPart::InputText { text } => {
                                    UnifiedContentPart::Text { text }
                                }
                                ContentPart::InputImage { image_url } => {
                                    UnifiedContentPart::ImageUrl {
                                        url: image_url,
                                        detail: None,
                                    }
                                }
                            })
                            .collect(),
                    };
                    UnifiedMessage {
                        role: item.role,
                        content,
                    }
                })
                .collect(),
        };

        UnifiedRequest {
            model: Some(responses_req.model),
            messages,
            stream: responses_req.stream.unwrap_or(false),
            temperature: responses_req.temperature,
            max_tokens: responses_req.max_tokens,
            top_p: responses_req.top_p,
            ..Default::default()
        }
    }
}

impl From<UnifiedRequest> for ResponsesRequestPayload {
    fn from(unified_req: UnifiedRequest) -> Self {
        let items = unified_req
            .messages
            .into_iter()
            .map(|msg| {
                let content_parts: Vec<ContentPart> = msg
                    .content
                    .into_iter()
                    .map(|part| match part {
                        UnifiedContentPart::Text { text } => ContentPart::InputText { text },
                        UnifiedContentPart::ImageUrl { url, .. } => {
                            ContentPart::InputImage { image_url: url }
                        }
                        // Other types are not supported by this simplified struct
                        _ => ContentPart::InputText {
                            text: "[Unsupported content type]".to_string(),
                        },
                    })
                    .collect();

                MessageItem {
                    _type: "message".to_string(),
                    role: msg.role,
                    content: Content::Parts(content_parts),
                }
            })
            .collect();

        ResponsesRequestPayload {
            model: unified_req.model.unwrap_or_default(),
            input: Input::Items(items),
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemField {
    Message(Message),
    FunctionCall(FunctionCall),
    FunctionCallOutput(FunctionCallOutput),
    Reasoning(ReasoningBody),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    pub id: String,
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
        file_url: Option<String>,
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
    pub id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    pub status: MessageStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCallOutput {
    pub id: String,
    pub call_id: String,
    pub output: Value, // Can be string or array of bits
    pub status: MessageStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReasoningBody {
    pub id: String,
    pub content: Option<Vec<ItemContentPart>>,
    pub summary: Vec<ItemContentPart>,
    pub encrypted_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub status: String,
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
    pub service_tier: String,
    pub metadata: Value,
    pub safety_identifier: Option<String>,
    pub prompt_cache_key: Option<String>,
}

impl From<ResponsesResponse> for UnifiedResponse {
    fn from(responses_res: ResponsesResponse) -> Self {
        let choices = responses_res
            .output
            .into_iter()
            .enumerate()
            .filter_map(|(idx, item)| {
                if let ItemField::Message(msg) = item {
                    let content = msg
                        .content
                        .into_iter()
                        .map(|part| match part {
                            ItemContentPart::InputText { text } => {
                                UnifiedContentPart::Text { text }
                            }
                            ItemContentPart::OutputText { text, .. } => {
                                UnifiedContentPart::Text { text }
                            }
                            ItemContentPart::Text { text } => UnifiedContentPart::Text { text },
                            ItemContentPart::SummaryText { text } => {
                                UnifiedContentPart::Text { text }
                            }
                            ItemContentPart::ReasoningText { text } => {
                                UnifiedContentPart::Text { text }
                            }
                            ItemContentPart::Refusal { refusal } => {
                                UnifiedContentPart::Text { text: refusal }
                            }
                            ItemContentPart::InputImage { image_url, detail } => {
                                UnifiedContentPart::ImageUrl {
                                    url: image_url.unwrap_or_default(),
                                    detail: Some(detail),
                                }
                            }
                            ItemContentPart::InputFile { .. } => UnifiedContentPart::Text {
                                text: "[File content]".to_string(),
                            },
                        })
                        .collect();

                    Some(UnifiedChoice {
                        index: idx as u32,
                        message: UnifiedMessage {
                            role: match msg.role {
                                MessageRole::User => UnifiedRole::User,
                                MessageRole::Assistant => UnifiedRole::Assistant,
                                MessageRole::System => UnifiedRole::System,
                                MessageRole::Developer => UnifiedRole::System,
                            },
                            content,
                        },
                        finish_reason: Some("stop".to_string()),
                        logprobs: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        UnifiedResponse {
            id: responses_res.id,
            model: responses_res.model,
            choices,
            usage: responses_res.usage.map(Into::into),
            created: Some(responses_res.created_at),
            object: Some(responses_res.object),
            system_fingerprint: None,
        }
    }
}

impl From<UnifiedResponse> for ResponsesResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let output = unified_res
            .choices
            .into_iter()
            .map(|choice| {
                let content = choice
                    .message
                    .content
                    .into_iter()
                    .map(|part| match part {
                        UnifiedContentPart::Text { text } => ItemContentPart::OutputText {
                            text,
                            annotations: Vec::new(),
                            logprobs: None,
                        },
                        UnifiedContentPart::ImageUrl { url, detail } => {
                            ItemContentPart::InputImage {
                                image_url: Some(url),
                                detail: detail.unwrap_or_else(|| "auto".to_string()),
                            }
                        }
                        _ => ItemContentPart::Text {
                            text: "[Unsupported content type]".to_string(),
                        },
                    })
                    .collect();

                ItemField::Message(Message {
                    id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
                    status: MessageStatus::Completed,
                    role: match choice.message.role {
                        UnifiedRole::User => MessageRole::User,
                        UnifiedRole::Assistant => MessageRole::Assistant,
                        UnifiedRole::System => MessageRole::System,
                        UnifiedRole::Tool => MessageRole::Assistant,
                    },
                    content,
                })
            })
            .collect();

        ResponsesResponse {
            id: unified_res.id,
            object: "response".to_string(),
            created_at: unified_res
                .created
                .unwrap_or_else(|| Utc::now().timestamp()),
            completed_at: Some(
                unified_res
                    .created
                    .unwrap_or_else(|| Utc::now().timestamp()),
            ),
            status: "completed".to_string(),
            incomplete_details: None,
            model: unified_res.model,
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
            service_tier: "default".to_string(),
            metadata: serde_json::json!({}),
            safety_identifier: None,
            prompt_cache_key: None,
        }
    }
}

// --- Chunk Response ---
// NOTE: The streaming format for `/responses` is not documented in the provided openapi.json.
// This is a plausible implementation assuming it streams items or item deltas.
#[derive(Debug, Serialize, Deserialize)]
pub struct ResponsesChunkResponse {
    pub id: String,
    pub model: String,
    pub delta: Value, // Placeholder for delta
}

impl From<ResponsesChunkResponse> for UnifiedChunkResponse {
    fn from(chunk: ResponsesChunkResponse) -> Self {
        // This is a simplified conversion. A real implementation would need to
        // parse the delta and map it to UnifiedMessageDelta.
        let text = chunk
            .delta
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let delta = if !text.is_empty() {
            UnifiedMessageDelta {
                role: Some(UnifiedRole::Assistant),
                content: vec![UnifiedContentPartDelta::TextDelta { index: 0, text }],
            }
        } else {
            UnifiedMessageDelta::default()
        };

        UnifiedChunkResponse {
            id: chunk.id,
            model: chunk.model,
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta,
                finish_reason: None,
            }],
            ..Default::default()
        }
    }
}

impl From<UnifiedChunkResponse> for ResponsesChunkResponse {
    fn from(unified_chunk: UnifiedChunkResponse) -> Self {
        let text = unified_chunk
            .choices
            .get(0)
            .and_then(|c| c.delta.content.get(0))
            .map(|p| {
                if let UnifiedContentPartDelta::TextDelta { text, .. } = p {
                    text.clone()
                } else {
                    "".to_string()
                }
            })
            .unwrap_or_default();

        ResponsesChunkResponse {
            id: unified_chunk.id,
            model: unified_chunk.model,
            delta: serde_json::json!({ "text": text }),
        }
    }
}
