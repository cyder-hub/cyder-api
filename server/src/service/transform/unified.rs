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
#[serde(untagged)]
pub enum UnifiedMessageContent {
    Text(String),
    ToolCalls(Vec<UnifiedToolCall>),
    ToolResult(UnifiedToolResult),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedMessage {
    pub role: UnifiedRole,
    pub content: UnifiedMessageContent,
    pub thinking_content: Option<String>,
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
    pub stop: Option<Vec<String>>,
    pub seed: Option<i64>,
    pub presence_penalty: Option<f64>,
    pub frequency_penalty: Option<f64>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedResponse {
    pub id: String,
    pub model: String,
    pub choices: Vec<UnifiedChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>, // e.g. "chat.completion"
}

// --- Unified Chunk Response ---

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedMessageDelta {
    pub role: Option<UnifiedRole>,
    pub content: Option<String>,
    // For now, we'll represent tool calls as a list of complete tool calls.
    // The transformation logic will need to handle assembling them from chunks.
    pub tool_calls: Option<Vec<UnifiedToolCall>>,
    pub thinking_content: Option<String>,
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
