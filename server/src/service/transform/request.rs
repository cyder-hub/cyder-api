use serde_json::Value;
use std::fmt;

use cyder_tools::log::debug;

use super::adapter::{downstream_adapter_for, noop_finalize_request, upstream_adapter_for};
use super::audit::normalize_same_wire_responses_tool_call_ids;
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

fn responses_state_control_is_active(data: &Value, field: &str) -> bool {
    match data.get(field) {
        None | Some(Value::Null) => false,
        Some(Value::Bool(false)) if matches!(field, "store" | "background") => false,
        Some(Value::String(value)) if matches!(field, "previous_response_id" | "conversation") => {
            !value.is_empty()
        }
        Some(_) => true,
    }
}

fn reject_responses_stateful_controls(data: &Value) -> Result<(), TransformFailure> {
    if [
        "store",
        "previous_response_id",
        "conversation",
        "background",
    ]
    .into_iter()
    .any(|field| responses_state_control_is_active(data, field))
    {
        return Err(transform_failure(
            TransformFailureOrigin::DownstreamInput,
            TransformPhase::RequestDecode,
            TransformSemanticUnit::RequestEnvelope,
            TransformReasonCode::PolicyOverride,
            None,
        ));
    }
    Ok(())
}

fn enforce_responses_stateless_target(data: &mut Value) {
    if let Value::Object(object) = data {
        object.insert("store".to_string(), Value::Bool(false));
    }
}

fn validate_final_responses_stateless_controls(
    data: &Value,
) -> Result<(), FinalRequestValidationError> {
    if data.get("store") != Some(&Value::Bool(false)) {
        return Err(FinalRequestValidationError {
            path: "/store",
            reason: "must be false",
        });
    }
    for (field, path) in [
        ("previous_response_id", "/previous_response_id"),
        ("conversation", "/conversation"),
        ("background", "/background"),
    ] {
        if responses_state_control_is_active(data, field) {
            return Err(FinalRequestValidationError {
                path,
                reason: "must be absent, null, empty, or false",
            });
        }
    }
    Ok(())
}

fn invalid_responses_media(
    path: &'static str,
    reason: &'static str,
) -> FinalRequestValidationError {
    FinalRequestValidationError { path, reason }
}

fn validate_final_responses_media(data: &Value) -> Result<(), FinalRequestValidationError> {
    let Some(items) = data.get("input").and_then(Value::as_array) else {
        return Ok(());
    };

    for item in items {
        let role = item.get("role").and_then(Value::as_str);
        let Some(parts) = item.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in parts {
            let part_type = part.get("type").and_then(Value::as_str);
            let is_media = matches!(
                part_type,
                Some("input_image" | "input_audio" | "input_file")
            );
            if is_media && role != Some("user") {
                return Err(invalid_responses_media(
                    "/input/*/role",
                    "media input is only allowed on user messages",
                ));
            }

            match part_type {
                Some("input_image") => {
                    let image_url = part.get("image_url").filter(|value| !value.is_null());
                    let file_id = part.get("file_id").filter(|value| !value.is_null());
                    if image_url.is_some() == file_id.is_some()
                        || image_url.is_some_and(|value| {
                            !value
                                .as_str()
                                .is_some_and(super::media::is_valid_image_reference)
                        })
                        || file_id.is_some_and(|value| {
                            !value
                                .as_str()
                                .is_some_and(|file_id| !file_id.trim().is_empty())
                        })
                        || part.get("detail").is_some_and(|value| {
                            !value.is_null()
                                && !matches!(value.as_str(), Some("auto" | "low" | "high"))
                        })
                    {
                        return Err(invalid_responses_media(
                            "/input/*/content/*/input_image",
                            "must contain exactly one valid image_url or file_id and an optional supported detail",
                        ));
                    }
                }
                Some("input_audio") => {
                    let audio = part.get("input_audio").and_then(Value::as_object);
                    if !audio
                        .and_then(|value| value.get("data"))
                        .and_then(Value::as_str)
                        .is_some_and(super::media::is_valid_base64)
                        || !matches!(
                            audio
                                .and_then(|value| value.get("format"))
                                .and_then(Value::as_str),
                            Some("wav" | "mp3")
                        )
                    {
                        return Err(invalid_responses_media(
                            "/input/*/content/*/input_audio",
                            "must contain valid base64 wav or mp3 input",
                        ));
                    }
                }
                Some("input_file") => {
                    let filename = part.get("filename").filter(|value| !value.is_null());
                    if filename.is_some_and(|value| {
                        !value
                            .as_str()
                            .is_some_and(|filename| !filename.trim().is_empty())
                    }) {
                        return Err(invalid_responses_media(
                            "/input/*/content/*/filename",
                            "must be a non-empty string when present",
                        ));
                    }
                    let present = ["file_url", "file_id", "file_data"]
                        .into_iter()
                        .filter(|field| part.get(*field).is_some_and(|value| !value.is_null()))
                        .collect::<Vec<_>>();
                    if present.len() != 1 {
                        return Err(invalid_responses_media(
                            "/input/*/content/*/input_file",
                            "must contain exactly one portable file source",
                        ));
                    }
                    match present[0] {
                        "file_url" => {
                            return Err(invalid_responses_media(
                                "/input/*/content/*/file_url",
                                "external file URLs are not portable",
                            ));
                        }
                        "file_id"
                            if !part
                                .get("file_id")
                                .and_then(Value::as_str)
                                .is_some_and(|file_id| !file_id.trim().is_empty()) =>
                        {
                            return Err(invalid_responses_media(
                                "/input/*/content/*/file_id",
                                "must be a non-empty same-target file id",
                            ));
                        }
                        "file_data" => {
                            let Some(filename) = filename.and_then(Value::as_str) else {
                                return Err(invalid_responses_media(
                                    "/input/*/content/*/filename",
                                    "inline files require a non-empty filename",
                                ));
                            };
                            let Some(file_data) = part.get("file_data").and_then(Value::as_str)
                            else {
                                return Err(invalid_responses_media(
                                    "/input/*/content/*/file_data",
                                    "must be valid portable inline file data",
                                ));
                            };
                            let valid = if let Some(data_url) =
                                super::media::parse_base64_data_url(file_data)
                            {
                                super::media::classify_inline_mime(data_url.mime_type)
                                    == Some(super::media::InlineMediaKind::File)
                            } else {
                                super::media::mime_type_from_filename(filename).is_some_and(
                                    |mime_type| {
                                        super::media::classify_inline_mime(mime_type)
                                            == Some(super::media::InlineMediaKind::File)
                                    },
                                ) && super::media::is_valid_base64(file_data)
                            };
                            if !valid {
                                return Err(invalid_responses_media(
                                    "/input/*/content/*/file_data",
                                    "must be valid PDF, text, Markdown, CSV, or JSON inline data",
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn validate_final_responses_structured_output(
    data: &Value,
) -> Result<(), FinalRequestValidationError> {
    if data
        .get("response_format")
        .is_some_and(|value| !value.is_null())
    {
        return Err(FinalRequestValidationError {
            path: "/response_format",
            reason: "must use Responses text.format instead",
        });
    }
    let Some(text) = data.get("text").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(text) = text.as_object() else {
        return Err(FinalRequestValidationError {
            path: "/text",
            reason: "must be an object when present",
        });
    };
    let Some(format) = text.get("format").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(format) = format.as_object() else {
        return Err(FinalRequestValidationError {
            path: "/text/format",
            reason: "must be an object when present",
        });
    };
    match format.get("type").and_then(Value::as_str) {
        Some("text") => {
            if ["name", "description", "schema", "strict"]
                .into_iter()
                .any(|field| format.get(field).is_some_and(|value| !value.is_null()))
            {
                return Err(FinalRequestValidationError {
                    path: "/text/format",
                    reason: "plain text format cannot contain JSON schema controls",
                });
            }
        }
        Some("json_object") => {
            if ["name", "description", "schema", "strict"]
                .into_iter()
                .any(|field| format.get(field).is_some_and(|value| !value.is_null()))
            {
                return Err(FinalRequestValidationError {
                    path: "/text/format",
                    reason: "json_object cannot contain JSON schema controls",
                });
            }
        }
        Some("json_schema") => {
            if !format
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(super::structured::is_valid_schema_name)
            {
                return Err(FinalRequestValidationError {
                    path: "/text/format/name",
                    reason: "must be a valid non-empty schema name",
                });
            }
            if format
                .get("description")
                .is_some_and(|value| !value.is_null() && !value.is_string())
            {
                return Err(FinalRequestValidationError {
                    path: "/text/format/description",
                    reason: "must be a string when present",
                });
            }
            if !format.get("schema").is_some_and(Value::is_object) {
                return Err(FinalRequestValidationError {
                    path: "/text/format/schema",
                    reason: "must be a JSON object",
                });
            }
            if format
                .get("strict")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
            {
                return Err(FinalRequestValidationError {
                    path: "/text/format/strict",
                    reason: "must be a boolean when present",
                });
            }
        }
        _ => {
            return Err(FinalRequestValidationError {
                path: "/text/format/type",
                reason: "must be text, json_object, or json_schema",
            });
        }
    }
    Ok(())
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
            if data
                .get("parallel_tool_calls")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
            {
                return Err(FinalRequestValidationError {
                    path: "/parallel_tool_calls",
                    reason: "must be a boolean when present",
                });
            }
            if let Some(reasoning) = data.get("reasoning").filter(|value| !value.is_null()) {
                let Some(reasoning) = reasoning.as_object() else {
                    return Err(FinalRequestValidationError {
                        path: "/reasoning",
                        reason: "must be an object when present",
                    });
                };
                if reasoning.get("effort").is_some_and(|effort| {
                    !effort.is_null()
                        && !matches!(
                            effort.as_str(),
                            Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh")
                        )
                }) {
                    return Err(FinalRequestValidationError {
                        path: "/reasoning/effort",
                        reason: "must be a supported qualitative effort",
                    });
                }
                if reasoning.get("summary").is_some_and(|summary| {
                    !summary.is_null()
                        && !matches!(summary.as_str(), Some("concise" | "detailed" | "auto"))
                }) {
                    return Err(FinalRequestValidationError {
                        path: "/reasoning/summary",
                        reason: "must be concise, detailed, or auto",
                    });
                }
            }
            for tool in data
                .get("tools")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if tool.get("type").and_then(Value::as_str) != Some("function") {
                    continue;
                }
                let valid_name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.trim().is_empty());
                let valid_description = tool
                    .get("description")
                    .is_none_or(|value| value.is_null() || value.is_string());
                let valid_parameters = tool.get("parameters").is_some_and(Value::is_object);
                let valid_strict = tool
                    .get("strict")
                    .is_none_or(|value| value.is_null() || value.is_boolean());
                if !(valid_name && valid_description && valid_parameters && valid_strict) {
                    return Err(FinalRequestValidationError {
                        path: "/tools",
                        reason: "portable function definitions require a non-empty name, object parameters, optional string description, and optional boolean strict",
                    });
                }
            }
            validate_final_responses_media(data)?;
            validate_final_responses_structured_output(data)?;
            validate_final_responses_stateless_controls(data)?;
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
        let mut data = data;
        if upstream_protocol == UpstreamProtocol::Responses {
            reject_responses_stateful_controls(&data)?;
        }
        if downstream_protocol == DownstreamProtocol::Responses
            && upstream_protocol == UpstreamProtocol::Responses
        {
            normalize_same_wire_responses_tool_call_ids(&mut data);
        }
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
        if upstream_protocol == UpstreamProtocol::Responses {
            if let Ok(success) = &mut result {
                enforce_responses_stateless_target(&mut success.value);
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
                json!({"model": "gpt", "input": "hi", "store": false}),
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

    #[test]
    fn final_responses_validation_rejects_stateful_or_unfinalized_targets() {
        for (field, value, expected_path) in [
            ("store", json!(true), "/store"),
            (
                "previous_response_id",
                json!("response-private-marker"),
                "/previous_response_id",
            ),
            (
                "conversation",
                json!({"id": "conversation-private-marker"}),
                "/conversation",
            ),
            ("background", json!(true), "/background"),
        ] {
            let mut payload = json!({"model": "gpt", "input": "hi", "store": false});
            payload[field] = value;
            let error = validate_final_generation_request(
                &payload,
                UpstreamProtocol::Responses,
                &UpstreamProfileType::Responses,
            )
            .expect_err("stateful Responses target must be rejected");
            assert_eq!(error.path, expected_path);
            assert!(!error.to_string().contains("private-marker"));
        }

        let missing_store = json!({"model": "gpt", "input": "hi"});
        let error = validate_final_generation_request(
            &missing_store,
            UpstreamProtocol::Responses,
            &UpstreamProfileType::Responses,
        )
        .expect_err("unfinalized Responses target must be rejected");
        assert_eq!(error.path, "/store");
    }

    #[test]
    fn responses_finalizer_reasserts_store_false_without_cloning_extensions() {
        let finalized = finalize_request_data(
            json!({
                "model": "gpt",
                "input": "hi",
                "store": true,
                "vendor_extension": {"preserved": true}
            }),
            UpstreamProtocol::Responses,
            &UpstreamProfileType::Responses,
            "responses",
        );
        assert_eq!(finalized["store"], false);
        assert_eq!(finalized["vendor_extension"]["preserved"], true);
    }
}
