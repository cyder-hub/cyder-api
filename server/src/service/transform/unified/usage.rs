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
    pub cache_write_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u32>,
}

impl TryFrom<&UnifiedUsage> for UsageInfo {
    type Error = std::num::TryFromIntError;

    fn try_from(unified_usage: &UnifiedUsage) -> Result<Self, Self::Error> {
        Ok(Self {
            input_tokens: i32::try_from(unified_usage.input_tokens)?,
            output_tokens: i32::try_from(unified_usage.output_tokens)?,
            total_tokens: i32::try_from(unified_usage.total_tokens)?,
            input_image_tokens: i32::try_from(unified_usage.input_image_tokens.unwrap_or(0))?,
            output_image_tokens: i32::try_from(unified_usage.output_image_tokens.unwrap_or(0))?,
            cached_tokens: i32::try_from(unified_usage.cached_tokens.unwrap_or(0))?,
            cache_write_tokens: i32::try_from(unified_usage.cache_write_tokens.unwrap_or(0))?,
            reasoning_tokens: i32::try_from(unified_usage.reasoning_tokens.unwrap_or(0))?,
        })
    }
}

impl TryFrom<UnifiedUsage> for UsageInfo {
    type Error = std::num::TryFromIntError;

    fn try_from(unified_usage: UnifiedUsage) -> Result<Self, Self::Error> {
        Self::try_from(&unified_usage)
    }
}

#[cfg(test)]
mod tests {
    use super::UnifiedUsage;
    use crate::utils::usage::UsageInfo;

    #[test]
    fn usage_info_conversion_checks_i32_boundaries_and_cache_write() {
        let usage = UnifiedUsage {
            input_tokens: 16,
            output_tokens: 7,
            total_tokens: 23,
            cached_tokens: Some(3),
            cache_write_tokens: Some(2),
            ..Default::default()
        };
        let info = UsageInfo::try_from(&usage).expect("usage fits persistence boundary");
        assert_eq!(info.input_tokens, 16);
        assert_eq!(info.cached_tokens, 3);
        assert_eq!(info.cache_write_tokens, 2);

        let overflow = UnifiedUsage {
            input_tokens: i32::MAX as u32 + 1,
            total_tokens: i32::MAX as u32 + 1,
            ..Default::default()
        };
        assert!(UsageInfo::try_from(&overflow).is_err());
    }
}
