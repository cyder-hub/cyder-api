use serde::{Deserialize, Serialize};

use crate::service::transform::unified::UnifiedUsage;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageNormalization {
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub input_text_tokens: i64,
    pub output_text_tokens: i64,
    pub input_image_tokens: i64,
    pub output_image_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    #[serde(default = "output_tokens_are_applicable")]
    pub output_tokens_applicable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

const fn output_tokens_are_applicable() -> bool {
    true
}

impl Default for UsageNormalization {
    fn default() -> Self {
        Self {
            total_input_tokens: 0,
            total_output_tokens: 0,
            input_text_tokens: 0,
            output_text_tokens: 0,
            input_image_tokens: 0,
            output_image_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            output_tokens_applicable: true,
            warnings: Vec::new(),
        }
    }
}

impl UsageNormalization {
    pub fn input_only(total_input_tokens: i64, warning: impl Into<String>) -> Self {
        Self {
            total_input_tokens,
            input_text_tokens: total_input_tokens,
            output_tokens_applicable: false,
            warnings: vec![warning.into()],
            ..Self::default()
        }
    }

    pub const fn normalized_total_tokens(&self) -> i64 {
        if self.output_tokens_applicable {
            self.total_input_tokens + self.total_output_tokens
        } else {
            self.total_input_tokens
        }
    }

    pub fn from_unified_usage(usage: &UnifiedUsage) -> Self {
        let total_input_tokens = i64::from(usage.input_tokens);
        let total_output_tokens = i64::from(usage.output_tokens);
        let input_image_tokens = i64::from(usage.input_image_tokens.unwrap_or(0));
        let output_image_tokens = i64::from(usage.output_image_tokens.unwrap_or(0));
        let cache_read_tokens = i64::from(usage.cached_tokens.unwrap_or(0));
        let cache_write_tokens = i64::from(usage.cache_write_tokens.unwrap_or(0));
        let reasoning_tokens = i64::from(usage.reasoning_tokens.unwrap_or(0));

        let mut warnings = Vec::new();

        let input_text_tokens = subtract_components(
            total_input_tokens,
            &[
                ("input_image_tokens", input_image_tokens),
                ("cache_read_tokens", cache_read_tokens),
                ("cache_write_tokens", cache_write_tokens),
            ],
            "input",
            &mut warnings,
        );
        let output_text_tokens = subtract_components(
            total_output_tokens,
            &[
                ("output_image_tokens", output_image_tokens),
                ("reasoning_tokens", reasoning_tokens),
            ],
            "output",
            &mut warnings,
        );

        let total_tokens = i64::from(usage.total_tokens);
        let derived_total_tokens = total_input_tokens + total_output_tokens;
        if total_tokens > 0 && total_tokens != derived_total_tokens {
            warnings.push(format!(
                "reported total_tokens {} did not match input/output sum {}; normalization used input/output totals",
                total_tokens, derived_total_tokens
            ));
        }

        Self {
            total_input_tokens,
            total_output_tokens,
            input_text_tokens,
            output_text_tokens,
            input_image_tokens,
            output_image_tokens,
            cache_read_tokens,
            cache_write_tokens,
            reasoning_tokens,
            output_tokens_applicable: true,
            warnings,
        }
    }
}

impl From<&UnifiedUsage> for UsageNormalization {
    fn from(usage: &UnifiedUsage) -> Self {
        Self::from_unified_usage(usage)
    }
}

impl From<UnifiedUsage> for UsageNormalization {
    fn from(usage: UnifiedUsage) -> Self {
        Self::from_unified_usage(&usage)
    }
}

fn subtract_components(
    total: i64,
    components: &[(&str, i64)],
    scope: &str,
    warnings: &mut Vec<String>,
) -> i64 {
    let sum = components
        .iter()
        .filter_map(|(_, value)| (*value > 0).then_some(*value))
        .sum::<i64>();

    if sum > total {
        warnings.push(format!(
            "{} subcomponents exceeded total tokens: total={}, subcomponents={}",
            scope, total, sum
        ));
    }

    (total - sum).max(0)
}

#[cfg(test)]
mod tests {
    use super::UsageNormalization;
    use crate::service::transform::unified::UnifiedUsage;

    #[test]
    fn normalizes_openai_usage_with_cache_and_reasoning() {
        let usage = UnifiedUsage {
            input_tokens: 120,
            output_tokens: 80,
            total_tokens: 200,
            cached_tokens: Some(20),
            reasoning_tokens: Some(15),
            ..Default::default()
        };

        let normalized = UsageNormalization::from(&usage);

        assert_eq!(normalized.total_input_tokens, 120);
        assert_eq!(normalized.total_output_tokens, 80);
        assert_eq!(normalized.input_text_tokens, 100);
        assert_eq!(normalized.output_text_tokens, 65);
        assert_eq!(normalized.cache_read_tokens, 20);
        assert_eq!(normalized.reasoning_tokens, 15);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn normalizes_responses_usage_with_cache_and_reasoning() {
        let usage = UnifiedUsage {
            input_tokens: 64,
            output_tokens: 32,
            total_tokens: 96,
            cached_tokens: Some(8),
            reasoning_tokens: Some(4),
            ..Default::default()
        };

        let normalized = UsageNormalization::from(usage);

        assert_eq!(normalized.input_text_tokens, 56);
        assert_eq!(normalized.output_text_tokens, 28);
        assert_eq!(normalized.cache_read_tokens, 8);
        assert_eq!(normalized.reasoning_tokens, 4);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn normalizes_gemini_usage_without_double_counting_image_tokens() {
        let usage = UnifiedUsage {
            input_tokens: 100,
            output_tokens: 70,
            total_tokens: 170,
            input_image_tokens: Some(30),
            output_image_tokens: Some(25),
            cached_tokens: Some(10),
            cache_write_tokens: None,
            reasoning_tokens: Some(5),
        };

        let normalized = UsageNormalization::from(&usage);

        assert_eq!(normalized.total_input_tokens, 100);
        assert_eq!(normalized.input_image_tokens, 30);
        assert_eq!(normalized.cache_read_tokens, 10);
        assert_eq!(normalized.input_text_tokens, 60);
        assert_eq!(normalized.total_output_tokens, 70);
        assert_eq!(normalized.output_image_tokens, 25);
        assert_eq!(normalized.reasoning_tokens, 5);
        assert_eq!(normalized.output_text_tokens, 40);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn normalizes_anthropic_cache_read_and_write_without_double_counting_input() {
        let usage = UnifiedUsage {
            input_tokens: 16,
            output_tokens: 7,
            total_tokens: 23,
            cached_tokens: Some(3),
            cache_write_tokens: Some(2),
            ..Default::default()
        };

        let normalized = UsageNormalization::from(&usage);

        assert_eq!(normalized.total_input_tokens, 16);
        assert_eq!(normalized.input_text_tokens, 11);
        assert_eq!(normalized.output_text_tokens, 7);
        assert_eq!(normalized.cache_read_tokens, 3);
        assert_eq!(normalized.cache_write_tokens, 2);
        assert_eq!(normalized.reasoning_tokens, 0);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn normalizes_ollama_usage_as_text_only() {
        let usage = UnifiedUsage {
            input_tokens: 9,
            output_tokens: 4,
            total_tokens: 13,
            ..Default::default()
        };

        let normalized = UsageNormalization::from(&usage);

        assert_eq!(normalized.input_text_tokens, 9);
        assert_eq!(normalized.output_text_tokens, 4);
        assert!(normalized.warnings.is_empty());
    }

    #[test]
    fn clamps_negative_text_tokens_and_records_warning() {
        let usage = UnifiedUsage {
            input_tokens: 20,
            output_tokens: 10,
            total_tokens: 30,
            input_image_tokens: Some(18),
            cached_tokens: Some(9),
            reasoning_tokens: Some(12),
            ..Default::default()
        };

        let normalized = UsageNormalization::from(&usage);

        assert_eq!(normalized.input_text_tokens, 0);
        assert_eq!(normalized.output_text_tokens, 0);
        assert_eq!(normalized.warnings.len(), 2);
        assert!(normalized.warnings[0].contains("input subcomponents exceeded total tokens"));
        assert!(normalized.warnings[1].contains("output subcomponents exceeded total tokens"));
    }

    #[test]
    fn warns_when_reported_total_tokens_do_not_match_input_and_output_sum() {
        let usage = UnifiedUsage {
            input_tokens: 10,
            output_tokens: 7,
            total_tokens: 999,
            ..Default::default()
        };

        let normalized = UsageNormalization::from(&usage);

        assert_eq!(normalized.total_input_tokens, 10);
        assert_eq!(normalized.total_output_tokens, 7);
        assert_eq!(normalized.warnings.len(), 1);
        assert!(normalized.warnings[0].contains("reported total_tokens 999"));
    }

    #[test]
    fn input_only_normalization_marks_output_as_not_applicable() {
        let normalized = UsageNormalization::input_only(17, "embedding input usage");

        assert_eq!(normalized.total_input_tokens, 17);
        assert_eq!(normalized.input_text_tokens, 17);
        assert!(!normalized.output_tokens_applicable);
        assert_eq!(normalized.normalized_total_tokens(), 17);
        assert_eq!(normalized.warnings, vec!["embedding input usage"]);
    }
}
