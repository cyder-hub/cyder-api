use super::*;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: ResponseObject,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub completed_at: Option<i64>,
    pub status: ResponseStatus,
    #[serde(default)]
    pub incomplete_details: Option<IncompleteDetails>,
    pub model: String,
    #[serde(default)]
    pub previous_response_id: Option<String>,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default)]
    pub output: Vec<ItemField>,
    #[serde(default)]
    pub error: Option<Error>,
    #[serde(default)]
    pub tools: Vec<Tool>,
    #[serde(default)]
    pub tool_choice: ToolChoice,
    #[serde(default)]
    pub truncation: Truncation,
    #[serde(default)]
    pub parallel_tool_calls: bool,
    #[serde(default)]
    pub text: TextField,
    #[serde(default)]
    pub top_p: f64,
    #[serde(default)]
    pub presence_penalty: f64,
    #[serde(default)]
    pub frequency_penalty: f64,
    #[serde(default)]
    pub top_logprobs: u32,
    #[serde(default)]
    pub temperature: f64,
    #[serde(default)]
    pub reasoning: Option<Reasoning>,
    #[serde(default)]
    pub usage: Option<Usage>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub store: bool,
    #[serde(default)]
    pub background: bool,
    #[serde(default)]
    pub service_tier: ServiceTier,
    #[serde(default)]
    pub metadata: Value,
    #[serde(default)]
    pub safety_identifier: Option<String>,
    #[serde(default)]
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
    Queued,
    InProgress,
    Completed,
    Incomplete,
    Failed,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ServiceTier {
    #[default]
    Default,
}
