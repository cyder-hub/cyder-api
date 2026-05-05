use super::*;

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
