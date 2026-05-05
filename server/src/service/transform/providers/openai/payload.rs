use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::service::transform::unified::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiRequestPayload {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub(crate) model: String,
    pub(crate) messages: Vec<OpenAiMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tools: Option<Vec<UnifiedTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_choice: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stream: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop: Option<OpenAiStop>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) n: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) seed: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) presence_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) frequency_penalty: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) logit_bias: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) logprobs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_logprobs: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) response_format: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) user: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) parallel_tool_calls: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) enum ReasoningEffort {
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

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum OpenAiStop {
    String(String),
    Array(Vec<String>),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiMessage {
    pub(crate) role: String,
    pub(crate) content: Option<OpenAiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_calls: Option<Vec<OpenAiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refusal: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub(crate) enum OpenAiContent {
    Text(String),
    Parts(Vec<OpenAiContentPart>),
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OpenAiContentPart {
    Text { text: String },
    ImageUrl { image_url: OpenAiImageUrl },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiImageUrl {
    pub(crate) url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) detail: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiToolCall {
    pub(crate) id: String,
    #[serde(rename = "type")]
    pub(crate) type_: String, // "function"
    pub(crate) function: OpenAiFunction,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiFunction {
    pub(crate) name: String,
    pub(crate) arguments: String,
}

pub(crate) fn build_data_url(mime_type: &str, data: &str) -> String {
    format!("data:{mime_type};base64,{data}")
}

pub(crate) fn render_file_reference_text(
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

pub(crate) fn render_inline_file_data_text(
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

pub(crate) fn render_executable_code_text(language: &str, code: &str) -> String {
    format!("```{language}\n{code}\n```")
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiLogProbs {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<Vec<OpenAiLogProb>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiLogProb {
    pub(crate) token: String,
    pub(crate) logprob: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bytes: Option<Vec<u8>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) top_logprobs: Option<Vec<OpenAiTopLogProb>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiTopLogProb {
    pub(crate) token: String,
    pub(crate) logprob: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bytes: Option<Vec<u8>>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiCompletionTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct OpenAiPromptTokenDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) audio_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cached_tokens: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiUsage {
    pub(crate) completion_tokens: u32,
    pub(crate) prompt_tokens: u32,
    pub(crate) total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) completion_tokens_details: Option<OpenAiCompletionTokenDetails>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) prompt_tokens_details: Option<OpenAiPromptTokenDetails>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiResponse {
    pub(crate) id: String,
    pub(crate) object: String, // Usually "chat.completion"
    pub(crate) created: i64,
    pub(crate) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system_fingerprint: Option<String>,
    pub(crate) choices: Vec<OpenAiChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) usage: Option<OpenAiUsage>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiChoice {
    pub(crate) index: u32,
    pub(crate) message: OpenAiMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) logprobs: Option<OpenAiLogProbs>,
    pub(crate) finish_reason: Option<String>, // Can be null in some cases (e.g., content filtering)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiChunkResponse {
    pub(crate) id: String,
    pub(crate) object: String, // Usually "chat.completion.chunk"
    pub(crate) created: i64,
    pub(crate) model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) system_fingerprint: Option<String>,
    pub(crate) choices: Vec<OpenAiChunkChoice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) usage: Option<OpenAiUsage>, // Usually only present in the last chunk
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiChunkChoice {
    pub(crate) index: u32,
    pub(crate) delta: OpenAiChunkDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) finish_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) logprobs: Option<OpenAiLogProbs>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct OpenAiChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) tool_calls: Option<Vec<OpenAiChunkToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) refusal: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>, // For tool messages
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OpenAiChunkToolCall {
    pub(crate) index: u32,         // OpenAI includes index in chunk tool calls
    pub(crate) id: Option<String>, // ID is optional in chunks
    #[serde(rename = "type")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) type_: Option<String>,
    pub(crate) function: OpenAiChunkFunction,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct OpenAiChunkFunction {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) arguments: Option<String>,
}
