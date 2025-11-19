use chrono::Utc;
use cyder_tools::log::debug;
use serde_json::Value;

use crate::{
    controller::llm_types::LlmApiType,
    database::{price::PriceRule, request_log::UpdateRequestLogData},
};

#[derive(Debug)]
pub struct UsageInfo {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub reasoning_tokens: i32,
    pub total_tokens: i32,
}

pub fn parse_usage_info(response_body: &Value, api_type: LlmApiType) -> Option<UsageInfo> {
    match api_type {
        LlmApiType::OpenAI => {
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
                        let calculated_reasoning =
                            total_tokens - prompt_tokens - completion_tokens;
                        if calculated_reasoning < 0 {
                            0
                        } else {
                            calculated_reasoning
                        }
                    });

                Some(UsageInfo {
                    prompt_tokens,
                    completion_tokens,
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
                let prompt_tokens =
                    usage.get("promptTokenCount").and_then(Value::as_i64).unwrap_or(0) as i32;
                let completion_tokens =
                    usage.get("candidatesTokenCount").and_then(Value::as_i64).unwrap_or(0) as i32;
                let total_tokens =
                    usage.get("totalTokenCount").and_then(Value::as_i64).unwrap_or(0) as i32;
                let reasoning_tokens =
                    usage.get("thoughtsTokenCount").and_then(Value::as_i64).unwrap_or(0) as i32;

                Some(UsageInfo {
                    prompt_tokens,
                    completion_tokens,
                    reasoning_tokens,
                    total_tokens,
                })
            } else {
                None
            }
        }
        _ => return None
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
pub fn calculate_cost(usage_info: &UsageInfo, price_rules: &[PriceRule]) -> i64 {
    debug!("[calculate_cost] Calculating cost for usage: {:?}, with price rules: {:?}", usage_info, price_rules);
    let now = Utc::now().timestamp_millis();
    let mut total_cost: i64 = 0;

    // Helper to find the best rule for a given usage type.
    // "Best" is defined as the one that is currently active and has the latest `effective_from` date.
    let find_best_rule = |usage_type: &str| -> Option<&PriceRule> {
        price_rules
            .iter()
            .filter(|rule| {
                rule.usage_type == usage_type
                    && rule.is_enabled
                    && rule.effective_from <= now
                    && rule.effective_until.map_or(true, |until| now < until)
            })
            .max_by_key(|rule| rule.effective_from)
    };

    // Calculate cost for prompt tokens
    if let Some(rule) = find_best_rule("PROMPT") {
        if usage_info.prompt_tokens > 0 {
            // Price is per 1000 tokens
            let cost = usage_info.prompt_tokens as i64 * rule.price_in_micro_units;
            total_cost += cost;
        }
    }

    // Calculate cost for completion tokens
    if let Some(rule) = find_best_rule("COMPLETION") {
        if usage_info.completion_tokens > 0 {
            // Price is per 1000 tokens
            let cost = usage_info.completion_tokens as i64 * rule.price_in_micro_units;
            total_cost += cost;
        }
    }

    // Calculate cost for invocation (flat fee)
    if let Some(rule) = find_best_rule("INVOCATION") {
        // Invocation is a flat fee, not token-based.
        total_cost += rule.price_in_micro_units;
    }

    debug!("[calculate_cost] Final calculated cost: {}", total_cost);
    total_cost
}

// Helper function to populate token and cost fields in UpdateRequestLogData
pub fn populate_token_cost_fields(
    update_data: &mut UpdateRequestLogData,
    usage_info: Option<&UsageInfo>,
    price_rules: &[PriceRule],
    currency: Option<&str>,
) {
    debug!("[populate_token_cost_fields] Populating with usage_info: {:?}, price_rules: {:?}", usage_info, price_rules);
    if let Some(u) = usage_info {
        update_data.prompt_tokens = Some(u.prompt_tokens);
        update_data.completion_tokens = Some(u.completion_tokens);
        update_data.reasoning_tokens = Some(u.reasoning_tokens);
        update_data.total_tokens = Some(u.total_tokens);

        if !price_rules.is_empty() {
            let cost = calculate_cost(u, price_rules);
            update_data.calculated_cost = Some(cost);
            if cost > 0 {
                update_data.cost_currency = currency.map(|c| Some(c.to_string()));
            }
        }
    }
    debug!("[populate_token_cost_fields] Resulting update_data: {:?}", update_data);
}
