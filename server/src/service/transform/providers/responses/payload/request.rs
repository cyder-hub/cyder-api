use super::*;

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

pub(super) fn default_message_id() -> String {
    format!("msg_{}", crate::utils::ID_GENERATOR.generate_id())
}

pub(super) fn default_function_call_id() -> String {
    format!("fc_{}", crate::utils::ID_GENERATOR.generate_id())
}

pub(super) fn default_function_call_output_id() -> String {
    format!("fco_{}", crate::utils::ID_GENERATOR.generate_id())
}

pub(super) fn default_completed_status() -> MessageStatus {
    MessageStatus::Completed
}
