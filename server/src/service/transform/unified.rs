use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum UnifiedRole {
    System,
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
    pub name: String, // The name of the tool that was called
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedContentPart {
    Text { text: String },
    ImageUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        detail: Option<String>, // e.g., "low", "high", "auto"
    },
    ImageData {
        mime_type: String,
        data: String, // Base64 encoded
    },
    FileData {
        file_uri: String,
        mime_type: String,
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
            UnifiedContentPart::ImageUrl { url, .. } => url.is_empty(),
            UnifiedContentPart::ImageData { data, .. } => data.is_empty(),
            UnifiedContentPart::FileData { file_uri, .. } => file_uri.is_empty(),
            UnifiedContentPart::ExecutableCode { code, .. } => code.is_empty(),
            // Tool calls and results are never considered empty as they have structural meaning
            UnifiedContentPart::ToolCall(_) | UnifiedContentPart::ToolResult(_) => false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
pub struct UnifiedRequest {
    pub model: Option<String>,
    pub messages: Vec<UnifiedMessage>,
    pub tools: Option<Vec<UnifiedTool>>,
    pub stream: bool,

    // Common generation configs
    pub temperature: Option<f64>,
    pub max_tokens: Option<u32>,
    pub top_p: Option<f64>,
    /// Top-K sampling parameter
    /// 
    /// Note: This is primarily supported by Anthropic. Other providers may ignore this field.
    /// When converting from Anthropic requests, this field will be populated.
    /// When converting to non-Anthropic providers, this field will be silently dropped.
    pub top_k: Option<u32>,
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,

    // OpenAI-specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<Value>, // Controls tool calling behavior
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<u32>, // Number of completions to generate
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_format: Option<Value>, // e.g., {"type": "json_object"}
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logit_bias: Option<Value>, // Token bias map
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>, // User identifier for moderation

    // Anthropic-specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>, // Request metadata

    // Ollama-specific fields
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>, // Output format (e.g., "json")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>, // Model keep-alive duration
    
    // Passthrough fields for provider-specific features not mapped to unified structure
    // This allows preserving fields like OpenAI's logprobs, parallel_tool_calls, etc.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub passthrough: Option<Value>, // JSON object containing provider-specific fields
}

impl UnifiedRequest {
    /// Filters out empty content parts and empty messages
    pub fn filter_empty(mut self) -> Self {
        // Filter empty content from each message
        self.messages = self.messages
            .into_iter()
            .map(|msg| msg.filter_empty_content())
            .filter(|msg| !msg.is_empty())
            .collect();
        self
    }
}

// --- Unified Response ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChoice {
    pub index: u32,
    pub message: UnifiedMessage,
    pub finish_reason: Option<String>,
    // OpenAI-specific: log probabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logprobs: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<UnifiedChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>, // e.g. "chat.completion"
    // OpenAI-specific: system fingerprint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_fingerprint: Option<String>,
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
    TextDelta { index: u32, text: String },
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
    pub model: String,
    pub choices: Vec<UnifiedChunkChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>, // e.g. "chat.completion.chunk"
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
