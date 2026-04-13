use serde_json::Value;

use crate::schema::enum_def::LlmApiType;

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

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{UsageInfo, parse_usage_info};
    use crate::schema::enum_def::LlmApiType;

    #[test]
    fn parses_openai_usage_with_fallback_reasoning() {
        let response = json!({
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5,
                "total_tokens": 17
            }
        });

        let usage = parse_usage_info(&response, LlmApiType::GeminiOpenai).expect("usage");
        assert_eq!(
            usage,
            UsageInfo {
                input_tokens: 10,
                output_tokens: 5,
                input_image_tokens: 0,
                output_image_tokens: 0,
                cached_tokens: 0,
                reasoning_tokens: 2,
                total_tokens: 17,
            }
        );
    }
}
