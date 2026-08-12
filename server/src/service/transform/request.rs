use serde_json::Value;
use std::fmt;

use cyder_tools::log::debug;

use super::adapter::{downstream_adapter_for, noop_finalize_request, upstream_adapter_for};
use super::diagnostics::{
    capture_transform_diagnostics, merge_transform_summaries, record_captured_transform_fact,
    transform_failure, transform_success,
};
use super::request_conflict::registered_request_conflict;
use super::{
    TransformAction, TransformFailure, TransformFailureOrigin, TransformOutcomeKind,
    TransformOutcomeSummary, TransformPhase, TransformReasonCode, TransformResult,
    TransformSafeSummary, TransformSemanticUnit,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FinalRequestValidationError {
    pub path: &'static str,
    pub reason: &'static str,
}

impl fmt::Display for FinalRequestValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path, self.reason)
    }
}

fn require_non_empty_string(
    data: &Value,
    path: &'static str,
    field: &str,
) -> Result<(), FinalRequestValidationError> {
    if data
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
    {
        Ok(())
    } else {
        Err(FinalRequestValidationError {
            path,
            reason: "must be a non-empty string",
        })
    }
}

fn require_array(
    data: &Value,
    path: &'static str,
    field: &str,
) -> Result<(), FinalRequestValidationError> {
    if data.get(field).is_some_and(Value::is_array) {
        Ok(())
    } else {
        Err(FinalRequestValidationError {
            path,
            reason: "must be an array",
        })
    }
}

/// Validates the invariant-bearing core of the already materialized request.
///
/// This intentionally borrows the existing JSON tree instead of cloning and
/// deserializing the whole payload again. Profile-specific extension policy is
/// layered onto this boundary without turning unknown vendor fields into a
/// global rejection policy.
pub(in crate::service::transform) fn validate_final_generation_request(
    data: &Value,
    upstream_protocol: UpstreamProtocol,
    profile_type: &UpstreamProfileType,
) -> Result<(), FinalRequestValidationError> {
    if !data.is_object() {
        return Err(FinalRequestValidationError {
            path: "$",
            reason: "must be a JSON object",
        });
    }

    match upstream_protocol {
        UpstreamProtocol::Openai => {
            super::providers::openai::validate_openai_target_request(data, profile_type).map_err(
                |error| FinalRequestValidationError {
                    path: error.path,
                    reason: error.reason,
                },
            )
        }
        UpstreamProtocol::Responses => {
            require_non_empty_string(data, "/model", "model")?;
            if !data
                .get("input")
                .is_some_and(|value| value.is_string() || value.is_array())
            {
                return Err(FinalRequestValidationError {
                    path: "/input",
                    reason: "must be a string or array",
                });
            }
            Ok(())
        }
        UpstreamProtocol::Anthropic => {
            require_non_empty_string(data, "/model", "model")?;
            require_array(data, "/messages", "messages")?;
            if !data.get("max_tokens").is_some_and(Value::is_u64) {
                return Err(FinalRequestValidationError {
                    path: "/max_tokens",
                    reason: "must be a non-negative integer",
                });
            }
            Ok(())
        }
        UpstreamProtocol::Gemini => require_array(data, "/contents", "contents"),
        UpstreamProtocol::Ollama => {
            require_non_empty_string(data, "/model", "model")?;
            require_array(data, "/messages", "messages")
        }
    }
}

fn protocols_share_wire_format(
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
) -> bool {
    matches!(
        (downstream_protocol, upstream_protocol),
        (DownstreamProtocol::Openai, UpstreamProtocol::Openai)
            | (DownstreamProtocol::Responses, UpstreamProtocol::Responses)
            | (DownstreamProtocol::Anthropic, UpstreamProtocol::Anthropic)
            | (DownstreamProtocol::Gemini, UpstreamProtocol::Gemini)
    )
}

fn invalid_openai_stream_usage(data: &Value) -> TransformFailure {
    transform_failure(
        TransformFailureOrigin::DownstreamInput,
        TransformPhase::RequestDecode,
        TransformSemanticUnit::Usage,
        TransformReasonCode::InvalidProtocolShape,
        Some(TransformSafeSummary::from_json(data)),
    )
}

fn enforce_openai_stream_usage(data: &mut Value, is_stream: bool) -> Result<(), TransformFailure> {
    if !is_stream {
        return Ok(());
    }

    enum UsageMutation {
        InsertOptions,
        InsertUsage,
        OverrideFalse,
        None,
    }

    let Some(object) = data.as_object() else {
        return Err(invalid_openai_stream_usage(data));
    };
    let mutation = match object.get("stream_options") {
        None => UsageMutation::InsertOptions,
        Some(Value::Object(stream_options)) => match stream_options.get("include_usage") {
            None => UsageMutation::InsertUsage,
            Some(Value::Bool(false)) => UsageMutation::OverrideFalse,
            Some(Value::Bool(true)) => UsageMutation::None,
            Some(_) => return Err(invalid_openai_stream_usage(data)),
        },
        Some(_) => return Err(invalid_openai_stream_usage(data)),
    };

    let object = data
        .as_object_mut()
        .expect("stream usage shape was validated as an object");
    match mutation {
        UsageMutation::InsertOptions => {
            object.insert(
                "stream_options".to_string(),
                serde_json::json!({ "include_usage": true }),
            );
        }
        UsageMutation::InsertUsage => {
            object
                .get_mut("stream_options")
                .and_then(Value::as_object_mut)
                .expect("stream_options shape was validated as an object")
                .insert("include_usage".to_string(), Value::Bool(true));
        }
        UsageMutation::OverrideFalse => {
            *object
                .get_mut("stream_options")
                .and_then(Value::as_object_mut)
                .and_then(|stream_options| stream_options.get_mut("include_usage"))
                .expect("include_usage shape was validated as a boolean") = Value::Bool(true);
            record_captured_transform_fact(super::TransformDiagnosticFact {
                sequence: 0,
                phase: TransformPhase::RequestEncode,
                semantic_unit: TransformSemanticUnit::Usage,
                outcome: TransformOutcomeKind::ControlledLossMinor,
                action: TransformAction::Synthesize,
                reason_code: TransformReasonCode::PolicyOverride,
                safe_summary: None,
            });
        }
        UsageMutation::None => {}
    }

    Ok(())
}

pub(in crate::service::transform) fn finalize_request_data(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    profile_type: &UpstreamProfileType,
    downstream_path: &str,
) -> Value {
    let adapter = upstream_adapter_for(upstream_protocol);
    let finalize = adapter.request.finalize.unwrap_or(noop_finalize_request);
    finalize(data, profile_type, downstream_path)
}

pub(in crate::service::transform) fn transform_request_data(
    data: Value,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> TransformResult<Value> {
    let (result, policy_summary) = capture_transform_diagnostics(|| {
        let mut result = if protocols_share_wire_format(downstream_protocol, upstream_protocol) {
            Ok(transform_success(
                data,
                TransformPhase::RequestEncode,
                TransformSemanticUnit::RequestEnvelope,
                TransformOutcomeKind::Passthrough,
                TransformAction::PassThrough,
                TransformReasonCode::SameWirePassthrough,
            ))
        } else {
            transform_request_data_inner(data, downstream_protocol, upstream_protocol, is_stream)
        };

        if upstream_protocol == UpstreamProtocol::Openai {
            if let Ok(success) = &mut result {
                if let Err(failure) = enforce_openai_stream_usage(&mut success.value, is_stream) {
                    return Err(failure);
                }
            }
        }

        result
    });

    if let Some(rejection) = explicit_rejection(&policy_summary) {
        let result_summary = match &result {
            Ok(success) => success.summary.clone(),
            Err(failure) => failure.summary.clone(),
        };
        return Err(TransformFailure {
            origin: TransformFailureOrigin::TargetCapability,
            phase: rejection.phase,
            semantic_unit: rejection.semantic_unit,
            reason_code: rejection.reason_code,
            summary: merge_transform_summaries([policy_summary, result_summary]),
        });
    }

    match result {
        Ok(mut success) => {
            success.summary = merge_transform_summaries([policy_summary, success.summary]);
            Ok(success)
        }
        Err(mut failure) => {
            failure.summary = merge_transform_summaries([policy_summary, failure.summary]);
            Err(failure)
        }
    }
}

fn explicit_rejection(summary: &TransformOutcomeSummary) -> Option<super::TransformDiagnosticFact> {
    summary
        .control_fact()
        .filter(|fact| fact.action == TransformAction::Reject)
        .cloned()
}

fn transform_request_data_inner(
    data: Value,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> TransformResult<Value> {
    if let Some(conflict) =
        registered_request_conflict(downstream_protocol, upstream_protocol, &data)
    {
        debug!(
            "Rejecting registered request conflict '{}' at safe path '{}'",
            conflict.id, conflict.path
        );
        return Err(transform_failure(
            TransformFailureOrigin::DownstreamInput,
            TransformPhase::RequestDecode,
            conflict.semantic_unit,
            conflict.reason_code,
            Some(TransformSafeSummary::from_json(&data)),
        ));
    }

    let source_adapter = downstream_adapter_for(downstream_protocol);
    let target_adapter = upstream_adapter_for(upstream_protocol);

    let decoded = (source_adapter.request.decode)(data)?;
    let mut unified_request = decoded.value;
    unified_request.stream = is_stream;

    match (target_adapter.request.encode)(unified_request) {
        Ok(mut encoded) => {
            encoded.summary = merge_transform_summaries([decoded.summary, encoded.summary]);
            Ok(encoded)
        }
        Err(mut failure) => {
            failure.summary = merge_transform_summaries([decoded.summary, failure.summary]);
            Err(failure)
        }
    }
}

#[cfg(test)]
mod final_validation_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn final_generation_validation_accepts_each_upstream_wire_and_unknown_extensions() {
        for (protocol, payload) in [
            (
                UpstreamProtocol::Openai,
                json!({
                    "model": "gpt",
                    "messages": [{"role": "user", "content": "hi"}],
                    "vendor_extension": {"enabled": true}
                }),
            ),
            (
                UpstreamProtocol::Responses,
                json!({"model": "gpt", "input": "hi"}),
            ),
            (
                UpstreamProtocol::Anthropic,
                json!({
                    "model": "claude",
                    "messages": [{"role": "user", "content": "hi"}],
                    "max_tokens": 1
                }),
            ),
            (
                UpstreamProtocol::Gemini,
                json!({"contents": [{"parts": [{"text": "hi"}]}]}),
            ),
            (
                UpstreamProtocol::Ollama,
                json!({
                    "model": "llama",
                    "messages": [{"role": "user", "content": "hi"}]
                }),
            ),
        ] {
            assert!(
                validate_final_generation_request(
                    &payload,
                    protocol,
                    &match protocol {
                        UpstreamProtocol::Openai => UpstreamProfileType::Openai,
                        UpstreamProtocol::Responses => UpstreamProfileType::Responses,
                        UpstreamProtocol::Anthropic => UpstreamProfileType::Anthropic,
                        UpstreamProtocol::Gemini => UpstreamProfileType::Gemini,
                        UpstreamProtocol::Ollama => UpstreamProfileType::Ollama,
                    },
                )
                .is_ok(),
                "{protocol:?}"
            );
        }
    }

    #[test]
    fn final_generation_validation_reports_safe_field_paths() {
        let invalid = json!({
            "model": "gpt",
            "messages": "replaced-by-patch"
        });
        let error = validate_final_generation_request(
            &invalid,
            UpstreamProtocol::Openai,
            &UpstreamProfileType::OpenaiCompatible,
        )
        .expect_err("invalid patched messages must be rejected");
        assert_eq!(error.path, "/messages");
        assert_eq!(error.reason, "must be an array");
        assert!(!error.to_string().contains("replaced-by-patch"));
    }
}
