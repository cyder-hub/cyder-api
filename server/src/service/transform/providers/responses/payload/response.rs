use super::*;

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
