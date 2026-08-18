use serde_json::Value;

use crate::service::transform::TransformReasonCode;
use crate::service::transform::unified::UnifiedUsage;

use super::payload::{GeminiUsageMetadata, Modality, ModalityTokenCount};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GeminiUsageError {
    InvalidShape,
    ComponentConflict,
    Overflow,
}

impl GeminiUsageError {
    pub(crate) const fn reason_code(self) -> TransformReasonCode {
        match self {
            Self::InvalidShape => TransformReasonCode::InvalidProtocolShape,
            Self::ComponentConflict => TransformReasonCode::UsageComponentConflict,
            Self::Overflow => TransformReasonCode::UsageOverflow,
        }
    }
}

fn checked_detail_total(details: &[ModalityTokenCount]) -> Result<u32, GeminiUsageError> {
    details.iter().try_fold(0_u32, |total, detail| {
        total
            .checked_add(detail.token_count)
            .ok_or(GeminiUsageError::Overflow)
    })
}

fn checked_image_total(details: &[ModalityTokenCount]) -> Result<u32, GeminiUsageError> {
    details
        .iter()
        .filter(|detail| detail.modality == Modality::Image)
        .try_fold(0_u32, |total, detail| {
            total
                .checked_add(detail.token_count)
                .ok_or(GeminiUsageError::Overflow)
        })
}

fn validate_details(details: &[ModalityTokenCount], parent: u32) -> Result<(), GeminiUsageError> {
    if checked_detail_total(details)? > parent {
        return Err(GeminiUsageError::ComponentConflict);
    }
    Ok(())
}

pub(crate) fn gemini_usage_to_unified(
    usage: &GeminiUsageMetadata,
) -> Result<UnifiedUsage, GeminiUsageError> {
    let cached_tokens = usage.cached_content_token_count.unwrap_or(0);
    let thoughts_tokens = usage.thoughts_token_count.unwrap_or(0);
    let tool_use_prompt_tokens = usage.tool_use_prompt_token_count.unwrap_or(0);

    if cached_tokens > usage.prompt_token_count {
        return Err(GeminiUsageError::ComponentConflict);
    }
    validate_details(&usage.prompt_tokens_details, usage.prompt_token_count)?;
    validate_details(&usage.cache_tokens_details, cached_tokens)?;
    validate_details(
        &usage.candidates_tokens_details,
        usage.candidates_token_count,
    )?;
    validate_details(
        &usage.tool_use_prompt_tokens_details,
        tool_use_prompt_tokens,
    )?;

    let prompt_image_tokens = checked_image_total(&usage.prompt_tokens_details)?;
    let explicit_cached_image_tokens = checked_image_total(&usage.cache_tokens_details)?;
    let cached_image_tokens = if usage.cache_tokens_details.is_empty() {
        prompt_image_tokens.min(cached_tokens)
    } else {
        if !usage.prompt_tokens_details.is_empty()
            && explicit_cached_image_tokens > prompt_image_tokens
        {
            return Err(GeminiUsageError::ComponentConflict);
        }
        explicit_cached_image_tokens
    };
    let non_cached_prompt_image_tokens = prompt_image_tokens
        .checked_sub(cached_image_tokens)
        .ok_or(GeminiUsageError::ComponentConflict)?;
    let tool_image_tokens = checked_image_total(&usage.tool_use_prompt_tokens_details)?;
    let input_image_tokens = non_cached_prompt_image_tokens
        .checked_add(tool_image_tokens)
        .ok_or(GeminiUsageError::Overflow)?;
    let output_image_tokens = checked_image_total(&usage.candidates_tokens_details)?;

    let input_tokens = usage
        .prompt_token_count
        .checked_add(tool_use_prompt_tokens)
        .ok_or(GeminiUsageError::Overflow)?;
    let output_tokens = usage
        .candidates_token_count
        .checked_add(thoughts_tokens)
        .ok_or(GeminiUsageError::Overflow)?;
    input_tokens
        .checked_add(output_tokens)
        .ok_or(GeminiUsageError::Overflow)?;

    Ok(UnifiedUsage {
        input_tokens,
        output_tokens,
        total_tokens: usage.total_token_count,
        input_image_tokens: (!usage.prompt_tokens_details.is_empty()
            || !usage.cache_tokens_details.is_empty()
            || !usage.tool_use_prompt_tokens_details.is_empty())
        .then_some(input_image_tokens),
        output_image_tokens: (!usage.candidates_tokens_details.is_empty())
            .then_some(output_image_tokens),
        cached_tokens: usage.cached_content_token_count,
        cache_write_tokens: None,
        reasoning_tokens: usage.thoughts_token_count,
    })
}

pub(crate) fn decode_gemini_usage(value: &Value) -> Result<UnifiedUsage, GeminiUsageError> {
    let usage = serde_json::from_value::<GeminiUsageMetadata>(value.clone())
        .map_err(|_| GeminiUsageError::InvalidShape)?;
    gemini_usage_to_unified(&usage)
}

pub(crate) fn unified_usage_to_gemini(usage: UnifiedUsage) -> GeminiUsageMetadata {
    let reasoning_tokens = usage.reasoning_tokens.unwrap_or(0);
    let candidates_token_count = usage.output_tokens.saturating_sub(reasoning_tokens);
    let input_image_tokens = usage.input_image_tokens.unwrap_or(0);
    let output_image_tokens = usage.output_image_tokens.unwrap_or(0);
    let mut prompt_tokens_details = Vec::new();
    let input_text_tokens = usage.input_tokens.saturating_sub(input_image_tokens);
    if input_text_tokens > 0 {
        prompt_tokens_details.push(ModalityTokenCount {
            modality: Modality::Text,
            token_count: input_text_tokens,
        });
    }
    if input_image_tokens > 0 {
        prompt_tokens_details.push(ModalityTokenCount {
            modality: Modality::Image,
            token_count: input_image_tokens,
        });
    }
    let mut candidates_tokens_details = Vec::new();
    let output_text_tokens = candidates_token_count.saturating_sub(output_image_tokens);
    if output_text_tokens > 0 {
        candidates_tokens_details.push(ModalityTokenCount {
            modality: Modality::Text,
            token_count: output_text_tokens,
        });
    }
    if output_image_tokens > 0 {
        candidates_tokens_details.push(ModalityTokenCount {
            modality: Modality::Image,
            token_count: output_image_tokens,
        });
    }

    GeminiUsageMetadata {
        prompt_token_count: usage.input_tokens,
        candidates_token_count,
        total_token_count: usage.total_tokens,
        thoughts_token_count: usage.reasoning_tokens,
        cached_content_token_count: usage.cached_tokens,
        tool_use_prompt_token_count: None,
        prompt_tokens_details,
        cache_tokens_details: Vec::new(),
        candidates_tokens_details,
        tool_use_prompt_tokens_details: Vec::new(),
    }
}

pub(crate) fn gemini_usage_snapshot_regressed(
    previous: &UnifiedUsage,
    current: &UnifiedUsage,
) -> bool {
    current.input_tokens < previous.input_tokens
        || current.output_tokens < previous.output_tokens
        || current.total_tokens < previous.total_tokens
        || optional_regressed(previous.input_image_tokens, current.input_image_tokens)
        || optional_regressed(previous.output_image_tokens, current.output_image_tokens)
        || optional_regressed(previous.cached_tokens, current.cached_tokens)
        || optional_regressed(previous.reasoning_tokens, current.reasoning_tokens)
}

const fn optional_regressed(previous: Option<u32>, current: Option<u32>) -> bool {
    match (previous, current) {
        (Some(previous), Some(current)) => current < previous,
        (Some(previous), None) => previous > 0,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::cost::{CostLedger, MeterKey, UsageNormalization};

    #[test]
    fn normalizes_inclusive_gemini_components_without_cache_or_reasoning_double_counting() {
        let usage = decode_gemini_usage(&json!({
            "promptTokenCount":11,
            "candidatesTokenCount":7,
            "cachedContentTokenCount":3,
            "thoughtsTokenCount":2,
            "toolUsePromptTokenCount":1,
            "totalTokenCount":21,
            "promptTokensDetails":[
                {"modality":"TEXT","tokenCount":8},
                {"modality":"IMAGE","tokenCount":3}
            ],
            "cacheTokensDetails":[{"modality":"IMAGE","tokenCount":3}],
            "candidatesTokensDetails":[
                {"modality":"TEXT","tokenCount":5},
                {"modality":"IMAGE","tokenCount":2}
            ],
            "toolUsePromptTokensDetails":[{"modality":"TEXT","tokenCount":1}]
        }))
        .expect("Gemini usage should normalize");

        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 9);
        assert_eq!(usage.total_tokens, 21);
        assert_eq!(usage.input_image_tokens, Some(0));
        assert_eq!(usage.output_image_tokens, Some(2));
        assert_eq!(usage.cached_tokens, Some(3));
        assert_eq!(usage.reasoning_tokens, Some(2));

        let normalization = UsageNormalization::from(&usage);
        assert_eq!(normalization.total_input_tokens, 12);
        assert_eq!(normalization.total_output_tokens, 9);
        assert_eq!(normalization.input_text_tokens, 9);
        assert_eq!(normalization.input_image_tokens, 0);
        assert_eq!(normalization.cache_read_tokens, 3);
        assert_eq!(normalization.output_text_tokens, 5);
        assert_eq!(normalization.output_image_tokens, 2);
        assert_eq!(normalization.reasoning_tokens, 2);
        assert!(normalization.warnings.is_empty());

        let ledger = CostLedger::from(&normalization);
        let meters = ledger
            .items
            .iter()
            .map(|item| (item.meter_key, item.quantity))
            .collect::<Vec<_>>();
        assert_eq!(
            meters,
            vec![
                (MeterKey::LlmInputTextTokens, 9),
                (MeterKey::LlmOutputTextTokens, 5),
                (MeterKey::LlmOutputImageTokens, 2),
                (MeterKey::LlmCacheReadTokens, 3),
                (MeterKey::LlmReasoningTokens, 2),
            ]
        );
    }

    #[test]
    fn folds_non_image_modalities_into_text_and_warns_only_for_reported_total_mismatch() {
        let usage = decode_gemini_usage(&json!({
            "promptTokenCount":11,
            "candidatesTokenCount":7,
            "thoughtsTokenCount":2,
            "toolUsePromptTokenCount":1,
            "totalTokenCount":999,
            "promptTokensDetails":[
                {"modality":"TEXT","tokenCount":5},
                {"modality":"AUDIO","tokenCount":4},
                {"modality":"IMAGE","tokenCount":2}
            ],
            "candidatesTokensDetails":[
                {"modality":"VIDEO","tokenCount":5},
                {"modality":"IMAGE","tokenCount":2}
            ],
            "toolUsePromptTokensDetails":[{"modality":"DOCUMENT","tokenCount":1}]
        }))
        .expect("known non-image modalities should fold into text");
        let normalization = UsageNormalization::from(&usage);

        assert_eq!(normalization.input_text_tokens, 10);
        assert_eq!(normalization.input_image_tokens, 2);
        assert_eq!(normalization.output_text_tokens, 5);
        assert_eq!(normalization.output_image_tokens, 2);
        assert_eq!(normalization.reasoning_tokens, 2);
        assert_eq!(normalization.warnings.len(), 1);
        assert!(normalization.warnings[0].contains("999"));
        assert!(normalization.warnings[0].contains("21"));
    }

    #[test]
    fn rejects_invalid_components_and_checked_overflow() {
        for value in [
            json!({"promptTokenCount":-1,"candidatesTokenCount":0,"totalTokenCount":0}),
            json!({"promptTokenCount":"1","candidatesTokenCount":0,"totalTokenCount":1}),
            json!({"promptTokenCount":1,"candidatesTokenCount":0,
                "cachedContentTokenCount":2,"totalTokenCount":1}),
            json!({"promptTokenCount":1,"candidatesTokenCount":0,"totalTokenCount":1,
                "promptTokensDetails":[{"modality":"IMAGE","tokenCount":2}]}),
        ] {
            assert!(decode_gemini_usage(&value).is_err(), "{value}");
        }

        let overflow = json!({
            "promptTokenCount":u32::MAX,
            "candidatesTokenCount":0,
            "toolUsePromptTokenCount":1,
            "totalTokenCount":u32::MAX
        });
        assert_eq!(
            decode_gemini_usage(&overflow),
            Err(GeminiUsageError::Overflow)
        );
    }

    #[test]
    fn detects_cumulative_snapshot_regression() {
        let previous = UnifiedUsage {
            input_tokens: 12,
            output_tokens: 9,
            total_tokens: 21,
            cached_tokens: Some(3),
            reasoning_tokens: Some(2),
            ..Default::default()
        };
        assert!(!gemini_usage_snapshot_regressed(&previous, &previous));
        assert!(gemini_usage_snapshot_regressed(
            &previous,
            &UnifiedUsage {
                output_tokens: 8,
                ..previous.clone()
            }
        ));
    }
}
