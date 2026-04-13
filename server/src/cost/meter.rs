use serde::{Deserialize, Serialize};
use strum_macros::{AsRefStr, Display, EnumString};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, AsRefStr, EnumString,
)]
pub enum MeterKey {
    #[serde(rename = "llm.input_text_tokens")]
    #[strum(serialize = "llm.input_text_tokens")]
    LlmInputTextTokens,
    #[serde(rename = "llm.output_text_tokens")]
    #[strum(serialize = "llm.output_text_tokens")]
    LlmOutputTextTokens,
    #[serde(rename = "llm.input_image_tokens")]
    #[strum(serialize = "llm.input_image_tokens")]
    LlmInputImageTokens,
    #[serde(rename = "llm.output_image_tokens")]
    #[strum(serialize = "llm.output_image_tokens")]
    LlmOutputImageTokens,
    #[serde(rename = "llm.cache_read_tokens")]
    #[strum(serialize = "llm.cache_read_tokens")]
    LlmCacheReadTokens,
    #[serde(rename = "llm.cache_write_tokens")]
    #[strum(serialize = "llm.cache_write_tokens")]
    LlmCacheWriteTokens,
    #[serde(rename = "llm.reasoning_tokens")]
    #[strum(serialize = "llm.reasoning_tokens")]
    LlmReasoningTokens,
    #[serde(rename = "invoke.request_calls")]
    #[strum(serialize = "invoke.request_calls")]
    InvokeRequestCalls,
}

impl MeterKey {
    pub const fn unit(self) -> CostUnit {
        match self {
            Self::InvokeRequestCalls => CostUnit::Call,
            Self::LlmInputTextTokens
            | Self::LlmOutputTextTokens
            | Self::LlmInputImageTokens
            | Self::LlmOutputImageTokens
            | Self::LlmCacheReadTokens
            | Self::LlmCacheWriteTokens
            | Self::LlmReasoningTokens => CostUnit::Token,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, AsRefStr, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CostUnit {
    Token,
    Call,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Display, AsRefStr, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ChargeKind {
    PerUnit,
    Flat,
    TieredPerUnit,
}
