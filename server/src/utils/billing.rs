use chrono::Utc;
use cyder_tools::log::debug;
use serde_json::Value;

use crate::{schema::enum_def::LlmApiType, service::cache::types::CachePriceRule};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsageInfo {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub input_image_tokens: i32,
    pub output_image_tokens: i32,
    pub cached_tokens: i32,
    pub reasoning_tokens: i32,
    pub total_tokens: i32,
}

pub fn parse_usage_info(response_body: &Value, api_type: LlmApiType) -> Option<UsageInfo> {
    match api_type {
        LlmApiType::Openai | LlmApiType::GeminiOpenai => {
            let usage_val = response_body.get("usage");
            if let Some(usage) = usage_val {
                if usage.is_null() {
                    return None;
                }

                let prompt_tokens = usage
                    .get("prompt_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let completion_tokens = usage
                    .get("completion_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let total_tokens = usage
                    .get("total_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;

                let reasoning_tokens = usage
                    .get("completion_tokens_details")
                    .and_then(|details| details.get("reasoning_tokens"))
                    .and_then(Value::as_i64)
                    .map(|rt| rt as i32)
                    .unwrap_or_else(|| {
                        let calculated_reasoning = total_tokens - prompt_tokens - completion_tokens;
                        if calculated_reasoning < 0 {
                            0
                        } else {
                            calculated_reasoning
                        }
                    });

                Some(UsageInfo {
                    input_tokens: prompt_tokens,
                    output_tokens: completion_tokens,
                    input_image_tokens: 0,
                    output_image_tokens: 0,
                    cached_tokens: 0,
                    reasoning_tokens,
                    total_tokens,
                })
            } else {
                None
            }
        }
        LlmApiType::Gemini => {
            let usage_val = response_body.get("usageMetadata");
            if let Some(usage) = usage_val {
                if usage.is_null() {
                    return None;
                }
                let prompt_tokens = usage
                    .get("promptTokenCount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let completion_tokens = usage
                    .get("candidatesTokenCount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let total_tokens = usage
                    .get("totalTokenCount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let reasoning_tokens = usage
                    .get("thoughtsTokenCount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let cached_tokens = usage
                    .get("cachedContentTokenCount")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;

                Some(UsageInfo {
                    input_tokens: prompt_tokens,
                    output_tokens: completion_tokens,
                    input_image_tokens: 0,
                    output_image_tokens: 0,
                    cached_tokens,
                    reasoning_tokens,
                    total_tokens,
                })
            } else {
                None
            }
        }
        LlmApiType::Anthropic => {
            let usage_val = response_body.get("usage");
            if let Some(usage) = usage_val {
                if usage.is_null() {
                    return None;
                }
                let prompt_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let completion_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let total_tokens = prompt_tokens + completion_tokens;

                Some(UsageInfo {
                    input_tokens: prompt_tokens,
                    output_tokens: completion_tokens,
                    input_image_tokens: 0,
                    output_image_tokens: 0,
                    cached_tokens: 0,
                    reasoning_tokens: 0,
                    total_tokens,
                })
            } else {
                None
            }
        }
        LlmApiType::Responses => {
            let usage_val = response_body
                .get("usage")
                .or_else(|| response_body.get("response").and_then(|r| r.get("usage")));
            if let Some(usage) = usage_val {
                if usage.is_null() {
                    return None;
                }
                let input_tokens = usage
                    .get("input_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let output_tokens = usage
                    .get("output_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;
                let total_tokens = usage
                    .get("total_tokens")
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;

                let cached_tokens = usage
                    .get("input_tokens_details")
                    .and_then(|details| details.get("cached_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;

                let reasoning_tokens = usage
                    .get("output_tokens_details")
                    .and_then(|details| details.get("reasoning_tokens"))
                    .and_then(Value::as_i64)
                    .unwrap_or(0) as i32;

                Some(UsageInfo {
                    input_tokens,
                    output_tokens,
                    input_image_tokens: 0,
                    output_image_tokens: 0,
                    cached_tokens,
                    reasoning_tokens,
                    total_tokens,
                })
            } else {
                None
            }
        }
        LlmApiType::Ollama => {
            let prompt_tokens = response_body
                .get("prompt_eval_count")
                .and_then(Value::as_i64)
                .map(|v| v as i32);
            let completion_tokens = response_body
                .get("eval_count")
                .and_then(Value::as_i64)
                .map(|v| v as i32);

            if prompt_tokens.is_some() || completion_tokens.is_some() {
                let p_tokens = prompt_tokens.unwrap_or(0);
                let c_tokens = completion_tokens.unwrap_or(0);
                Some(UsageInfo {
                    input_tokens: p_tokens,
                    output_tokens: c_tokens,
                    input_image_tokens: 0,
                    output_image_tokens: 0,
                    cached_tokens: 0,
                    reasoning_tokens: 0,
                    total_tokens: p_tokens + c_tokens,
                })
            } else {
                None
            }
        }
    }
}

/// Calculates the total cost of a request based on token usage and a set of price rules.
///
/// It finds the best-matching active price rule for each usage type ('PROMPT', 'COMPLETION', 'INVOCATION')
/// and calculates the cost. "Best" is the rule with the most recent `effective_from` date.
///
/// # Arguments
///
/// * `usage_info` - A reference to the `UsageInfo` struct containing token counts.
/// * `price_rules` - A slice of `PriceRule`s applicable to the request.
///
/// # Returns
///
/// The total calculated cost in micro-units.
pub fn calculate_cost(usage_info: &UsageInfo, price_rules: &[CachePriceRule]) -> i64 {
    debug!(
        "[calculate_cost] Calculating cost for usage: {:?}, with price rules: {:?}",
        usage_info, price_rules
    );
    let now = Utc::now().timestamp_millis();
    let mut total_cost: i64 = 0;

    // Helper to find the best rule for a given usage type.
    // "Best" is defined as the one that is currently active and has the latest `effective_from` date.
    let find_best_rule = |usage_type: &str| -> Option<&CachePriceRule> {
        price_rules
            .iter()
            .filter(|rule| {
                rule.usage_type == usage_type
                    && rule.effective_from <= now
                    && rule.effective_until.map_or(true, |until| now < until)
            })
            .max_by_key(|rule| rule.effective_from)
            .map(|v| v)
    };

    // Calculate cost for prompt tokens
    if let Some(rule) = find_best_rule("PROMPT") {
        if usage_info.input_tokens > 0 {
            // Price is per 1000 tokens
            let cost = usage_info.input_tokens as i64 * rule.price_in_micro_units.unwrap_or(0);
            total_cost += cost;
        }
    }

    // Calculate cost for completion tokens
    if let Some(rule) = find_best_rule("COMPLETION") {
        if usage_info.output_tokens > 0 {
            // Price is per 1000 tokens
            let cost = usage_info.output_tokens as i64 * rule.price_in_micro_units.unwrap_or(0);
            total_cost += cost;
        }
    }

    // Calculate cost for invocation (flat fee)
    if let Some(rule) = find_best_rule("INVOCATION") {
        // Invocation is a flat fee, not token-based.
        total_cost += rule.price_in_micro_units.unwrap_or(0);
    }

    debug!("[calculate_cost] Final calculated cost: {}", total_cost);
    total_cost
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{UsageInfo, calculate_cost, parse_usage_info};
    use crate::schema::enum_def::LlmApiType;
    use crate::service::cache::types::CachePriceRule;

    #[test]
    fn parse_usage_info_supports_gemini_openai_like_openai() {
        let response = json!({
            "usage": {
                "prompt_tokens": 11,
                "completion_tokens": 7,
                "total_tokens": 21,
                "completion_tokens_details": {
                    "reasoning_tokens": 3
                }
            }
        });

        let usage = parse_usage_info(&response, LlmApiType::GeminiOpenai).expect("usage");
        assert_eq!(
            usage,
            UsageInfo {
                input_tokens: 11,
                output_tokens: 7,
                input_image_tokens: 0,
                output_image_tokens: 0,
                cached_tokens: 0,
                reasoning_tokens: 3,
                total_tokens: 21,
            }
        );
    }

    #[test]
    fn calculate_cost_keeps_existing_behavior() {
        let usage = UsageInfo {
            input_tokens: 2,
            output_tokens: 3,
            input_image_tokens: 0,
            output_image_tokens: 0,
            cached_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 5,
        };

        let rules = vec![
            CachePriceRule {
                effective_from: 0,
                effective_until: None,
                period_start_seconds_utc: None,
                period_end_seconds_utc: None,
                usage_type: "PROMPT".to_string(),
                media_type: String::new(),
                condition_had_reasoning: None,
                tier_from_tokens: None,
                tier_to_tokens: None,
                price_in_micro_units: Some(10),
            },
            CachePriceRule {
                effective_from: 0,
                effective_until: None,
                period_start_seconds_utc: None,
                period_end_seconds_utc: None,
                usage_type: "COMPLETION".to_string(),
                media_type: String::new(),
                condition_had_reasoning: None,
                tier_from_tokens: None,
                tier_to_tokens: None,
                price_in_micro_units: Some(20),
            },
        ];

        assert_eq!(calculate_cost(&usage, &rules), 80);
    }
}
