use serde_json::Value;

use super::{TransformReasonCode, TransformSemanticUnit};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};

#[derive(Clone, Copy)]
struct RegisteredRequestConflictRule {
    id: &'static str,
    source: DownstreamProtocol,
    target: UpstreamProtocol,
    path: &'static [&'static str],
    path_label: &'static str,
    semantic_unit: TransformSemanticUnit,
    reason_code: TransformReasonCode,
    matches: fn(&Value) -> bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::service::transform) struct RegisteredRequestConflict {
    pub id: &'static str,
    pub path: &'static str,
    pub semantic_unit: TransformSemanticUnit,
    pub reason_code: TransformReasonCode,
}

fn is_positive_number(value: &Value) -> bool {
    value.as_f64().is_some_and(|number| number > 0.0)
}

fn is_active_value(value: &Value) -> bool {
    !value.is_null()
}

fn requests_non_text_modality(value: &Value) -> bool {
    value.as_array().is_some_and(|modalities| {
        modalities
            .iter()
            .any(|modality| modality.as_str() != Some("text"))
    })
}

const REGISTERED_REQUEST_CONFLICTS: &[RegisteredRequestConflictRule] = &[
    RegisteredRequestConflictRule {
        id: "gemini_positive_thinking_budget_to_openai",
        source: DownstreamProtocol::Gemini,
        target: UpstreamProtocol::Openai,
        path: &["generationConfig", "thinkingConfig", "thinkingBudget"],
        path_label: "/generationConfig/thinkingConfig/thinkingBudget",
        semantic_unit: TransformSemanticUnit::ReasoningContent,
        reason_code: TransformReasonCode::UnsupportedReasoning,
        matches: is_positive_number,
    },
    RegisteredRequestConflictRule {
        id: "openai_prediction_to_gemini",
        source: DownstreamProtocol::Openai,
        target: UpstreamProtocol::Gemini,
        path: &["prediction"],
        path_label: "/prediction",
        semantic_unit: TransformSemanticUnit::Metadata,
        reason_code: TransformReasonCode::UnsupportedContent,
        matches: is_active_value,
    },
    RegisteredRequestConflictRule {
        id: "openai_output_modalities_to_gemini",
        source: DownstreamProtocol::Openai,
        target: UpstreamProtocol::Gemini,
        path: &["modalities"],
        path_label: "/modalities",
        semantic_unit: TransformSemanticUnit::Metadata,
        reason_code: TransformReasonCode::UnsupportedContent,
        matches: requests_non_text_modality,
    },
    RegisteredRequestConflictRule {
        id: "openai_output_audio_to_gemini",
        source: DownstreamProtocol::Openai,
        target: UpstreamProtocol::Gemini,
        path: &["audio"],
        path_label: "/audio",
        semantic_unit: TransformSemanticUnit::AudioData,
        reason_code: TransformReasonCode::UnsupportedContent,
        matches: is_active_value,
    },
    RegisteredRequestConflictRule {
        id: "openai_web_search_options_to_gemini",
        source: DownstreamProtocol::Openai,
        target: UpstreamProtocol::Gemini,
        path: &["web_search_options"],
        path_label: "/web_search_options",
        semantic_unit: TransformSemanticUnit::ToolDefinitions,
        reason_code: TransformReasonCode::UnsupportedContent,
        matches: is_active_value,
    },
];

fn value_at_path<'a>(root: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter()
        .try_fold(root, |value, segment| value.get(*segment))
}

pub(in crate::service::transform) fn registered_request_conflict(
    source: DownstreamProtocol,
    target: UpstreamProtocol,
    request: &Value,
) -> Option<RegisteredRequestConflict> {
    REGISTERED_REQUEST_CONFLICTS.iter().find_map(|rule| {
        if rule.source != source || rule.target != target {
            return None;
        }
        let value = value_at_path(request, rule.path)?;
        (rule.matches)(value).then_some(RegisteredRequestConflict {
            id: rule.id,
            path: rule.path_label,
            semantic_unit: rule.semantic_unit,
            reason_code: rule.reason_code,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn registry_is_small_directional_and_borrows_targeted_values() {
        assert_eq!(REGISTERED_REQUEST_CONFLICTS.len(), 5);
        let request = json!({
            "generationConfig": {
                "thinkingConfig": {"thinkingBudget": 256},
                "unrelated": {"large": [1, 2, 3]}
            }
        });
        let request_address = std::ptr::from_ref(&request);
        let conflict = registered_request_conflict(
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            &request,
        )
        .expect("positive Gemini budget must match its registered conflict");

        assert_eq!(conflict.id, "gemini_positive_thinking_budget_to_openai");
        assert_eq!(
            conflict.path,
            "/generationConfig/thinkingConfig/thinkingBudget"
        );
        assert_eq!(request_address, std::ptr::from_ref(&request));
        assert!(
            registered_request_conflict(
                DownstreamProtocol::Gemini,
                UpstreamProtocol::Gemini,
                &request,
            )
            .is_none()
        );
    }

    #[test]
    fn non_positive_gemini_budgets_are_not_strong_conflicts() {
        for budget in [-1, 0] {
            let request = json!({
                "generationConfig": {"thinkingConfig": {"thinkingBudget": budget}}
            });
            assert!(
                registered_request_conflict(
                    DownstreamProtocol::Gemini,
                    UpstreamProtocol::Openai,
                    &request,
                )
                .is_none(),
                "{budget}"
            );
        }
    }
}
