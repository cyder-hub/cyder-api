use serde::{Deserialize, Serialize};

use crate::utils::usage::UsageInfo;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_image_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_image_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
}

impl From<UnifiedUsage> for UsageInfo {
    fn from(unified_usage: UnifiedUsage) -> Self {
        Self {
            input_tokens: unified_usage.input_tokens as i32,
            output_tokens: unified_usage.output_tokens as i32,
            total_tokens: unified_usage.total_tokens as i32,
            input_image_tokens: unified_usage.input_image_tokens.unwrap_or(0) as i32,
            output_image_tokens: unified_usage.output_image_tokens.unwrap_or(0) as i32,
            cached_tokens: unified_usage.cached_tokens.unwrap_or(0) as i32,
            reasoning_tokens: unified_usage.reasoning_tokens.unwrap_or(0) as i32,
        }
    }
}
