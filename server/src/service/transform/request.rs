use serde_json::Value;
use std::collections::BTreeSet;
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

fn invalid_anthropic_request(
    path: &'static str,
    reason: &'static str,
) -> FinalRequestValidationError {
    FinalRequestValidationError { path, reason }
}

fn validate_anthropic_system(data: &Value) -> Result<(), FinalRequestValidationError> {
    let Some(system) = data.get("system").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    if system.is_string() {
        return Ok(());
    }
    let Some(blocks) = system.as_array() else {
        return Err(invalid_anthropic_request(
            "/system",
            "must be a string or an array of text blocks",
        ));
    };
    for block in blocks {
        if block.get("type").and_then(Value::as_str) != Some("text")
            || block.get("text").and_then(Value::as_str).is_none()
        {
            return Err(invalid_anthropic_request(
                "/system/*",
                "must be an Anthropic text block",
            ));
        }
    }
    Ok(())
}

fn validate_anthropic_media_source(
    block_type: &str,
    source: &Value,
    same_wire: bool,
) -> Result<(), FinalRequestValidationError> {
    let Some(source) = source.as_object() else {
        return Err(invalid_anthropic_request(
            "/messages/*/content/*/source",
            "must be an object",
        ));
    };
    match (block_type, source.get("type").and_then(Value::as_str)) {
        ("image", Some("base64")) => {
            let valid_mime = source
                .get("media_type")
                .and_then(Value::as_str)
                .is_some_and(|mime| {
                    matches!(
                        mime,
                        "image/jpeg" | "image/png" | "image/gif" | "image/webp"
                    )
                });
            let valid_data = source
                .get("data")
                .and_then(Value::as_str)
                .is_some_and(super::media::is_valid_base64);
            if !valid_mime || !valid_data {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*/source",
                    "image base64 sources require a supported media_type and valid base64 data",
                ));
            }
        }
        ("image", Some("url")) => {
            if !source
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(super::media::is_valid_http_url)
            {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*/source/url",
                    "must be an HTTP or HTTPS URL",
                ));
            }
        }
        ("document", Some("base64")) => {
            let valid_mime = source
                .get("media_type")
                .and_then(Value::as_str)
                .is_some_and(|mime| {
                    if same_wire {
                        super::media::classify_inline_mime(mime)
                            == Some(super::media::InlineMediaKind::File)
                    } else {
                        mime == "application/pdf"
                    }
                });
            let valid_data = source
                .get("data")
                .and_then(Value::as_str)
                .is_some_and(super::media::is_valid_base64);
            if !valid_mime || !valid_data {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*/source",
                    "document base64 sources require a supported media_type and valid base64 data",
                ));
            }
        }
        ("document", Some("url")) => {
            if !source
                .get("url")
                .and_then(Value::as_str)
                .is_some_and(super::media::is_valid_http_url)
            {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*/source/url",
                    "must be an HTTP or HTTPS URL",
                ));
            }
        }
        ("document", Some("text")) => {
            let supported_mime = source
                .get("media_type")
                .and_then(Value::as_str)
                .is_some_and(|mime| {
                    if same_wire {
                        matches!(
                            mime,
                            "text/plain" | "text/markdown" | "text/csv" | "application/json"
                        )
                    } else {
                        mime == "text/plain"
                    }
                });
            if !supported_mime || source.get("data").and_then(Value::as_str).is_none() {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*/source",
                    "text documents require supported UTF-8 media_type and string data",
                ));
            }
        }
        (_, Some("file")) if same_wire => {
            if !source
                .get("file_id")
                .and_then(Value::as_str)
                .is_some_and(|id| !id.trim().is_empty())
            {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*/source/file_id",
                    "must be a non-empty string",
                ));
            }
        }
        (_, Some(_)) if same_wire => {}
        _ => {
            return Err(invalid_anthropic_request(
                "/messages/*/content/*/source/type",
                "is not a portable Anthropic media source",
            ));
        }
    }
    Ok(())
}

fn validate_final_anthropic_tools(
    data: &Value,
    same_wire: bool,
) -> Result<(BTreeSet<String>, bool), FinalRequestValidationError> {
    let Some(tools) = data.get("tools").filter(|value| !value.is_null()) else {
        return Ok((BTreeSet::new(), false));
    };
    let Some(tools) = tools.as_array() else {
        return Err(invalid_anthropic_request(
            "/tools",
            "must be an array when present",
        ));
    };
    let mut names = BTreeSet::new();
    for tool in tools {
        let Some(tool) = tool.as_object() else {
            return Err(invalid_anthropic_request("/tools/*", "must be an object"));
        };
        if tool.get("type").is_some() {
            if same_wire
                && tool
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| !kind.trim().is_empty())
            {
                if let Some(name) = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| !name.trim().is_empty())
                {
                    if !names.insert(name.to_string()) {
                        return Err(invalid_anthropic_request(
                            "/tools/*/name",
                            "tool names must be unique",
                        ));
                    }
                }
                continue;
            }
            return Err(invalid_anthropic_request(
                "/tools/*/type",
                "non-portable Anthropic tools are only allowed on same-wire requests",
            ));
        }
        let Some(name) = tool
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| !name.trim().is_empty())
        else {
            return Err(invalid_anthropic_request(
                "/tools/*/name",
                "must be a non-empty string",
            ));
        };
        if !names.insert(name.to_string()) {
            return Err(invalid_anthropic_request(
                "/tools/*/name",
                "tool names must be unique",
            ));
        }
        if !tool.get("input_schema").is_some_and(Value::is_object) {
            return Err(invalid_anthropic_request(
                "/tools/*/input_schema",
                "must be a JSON object",
            ));
        }
        if tool
            .get("description")
            .is_some_and(|value| !value.is_null() && !value.is_string())
            || tool
                .get("strict")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
        {
            return Err(invalid_anthropic_request(
                "/tools/*",
                "description must be a string and strict must be a boolean when present",
            ));
        }
    }
    Ok((names, !tools.is_empty()))
}

fn validate_final_anthropic_tool_choice(
    data: &Value,
    tool_names: &BTreeSet<String>,
    has_tools: bool,
) -> Result<(), FinalRequestValidationError> {
    let Some(choice) = data.get("tool_choice").filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let Some(choice) = choice.as_object() else {
        return Err(invalid_anthropic_request(
            "/tool_choice",
            "must be an object when present",
        ));
    };
    let choice_type = choice.get("type").and_then(Value::as_str);
    if !matches!(choice_type, Some("none" | "auto" | "any" | "tool")) {
        return Err(invalid_anthropic_request(
            "/tool_choice/type",
            "must be none, auto, any, or tool",
        ));
    }
    if choice
        .get("disable_parallel_tool_use")
        .is_some_and(|value| !value.is_null() && !value.is_boolean())
    {
        return Err(invalid_anthropic_request(
            "/tool_choice/disable_parallel_tool_use",
            "must be a boolean when present",
        ));
    }
    if choice_type == Some("tool") {
        let Some(name) = choice.get("name").and_then(Value::as_str) else {
            return Err(invalid_anthropic_request(
                "/tool_choice/name",
                "named tool choice requires a name",
            ));
        };
        if !tool_names.contains(name) {
            return Err(invalid_anthropic_request(
                "/tool_choice/name",
                "must reference a declared portable tool",
            ));
        }
    } else if choice.get("name").is_some_and(|value| !value.is_null()) {
        return Err(invalid_anthropic_request(
            "/tool_choice/name",
            "is only valid for named tool choice",
        ));
    }
    if !has_tools && choice_type != Some("none") {
        return Err(invalid_anthropic_request(
            "/tool_choice",
            "requires at least one declared portable tool",
        ));
    }
    Ok(())
}

fn validate_final_anthropic_reasoning_and_output(
    data: &Value,
    same_wire: bool,
) -> Result<(), FinalRequestValidationError> {
    let max_tokens = data
        .get("max_tokens")
        .and_then(Value::as_u64)
        .expect("Anthropic max_tokens must be validated before reasoning configuration");
    let thinking_type = match data.get("thinking").filter(|value| !value.is_null()) {
        None => None,
        Some(thinking) => {
            let Some(thinking) = thinking.as_object() else {
                return Err(invalid_anthropic_request(
                    "/thinking",
                    "must be an object when present",
                ));
            };
            let kind = thinking.get("type").and_then(Value::as_str);
            if !matches!(kind, Some("adaptive" | "enabled" | "disabled")) {
                return Err(invalid_anthropic_request(
                    "/thinking/type",
                    "must be adaptive, enabled, or disabled",
                ));
            }
            let budget = thinking
                .get("budget_tokens")
                .filter(|value| !value.is_null());
            if kind == Some("enabled") {
                if !budget
                    .and_then(Value::as_u64)
                    .is_some_and(|value| value >= 1024 && value < max_tokens)
                {
                    return Err(invalid_anthropic_request(
                        "/thinking/budget_tokens",
                        "enabled thinking requires a budget of at least 1024 tokens and less than max_tokens",
                    ));
                }
            } else if budget.is_some() {
                return Err(invalid_anthropic_request(
                    "/thinking/budget_tokens",
                    "is only valid for enabled thinking",
                ));
            }
            if thinking.get("display").is_some_and(|value| {
                !value.is_null() && !matches!(value.as_str(), Some("summarized" | "omitted"))
            }) {
                return Err(invalid_anthropic_request(
                    "/thinking/display",
                    "must be summarized or omitted when present",
                ));
            }
            kind
        }
    };

    if let Some(output) = data.get("output_config").filter(|value| !value.is_null()) {
        let Some(output) = output.as_object() else {
            return Err(invalid_anthropic_request(
                "/output_config",
                "must be an object when present",
            ));
        };
        if !same_wire
            && output
                .keys()
                .any(|key| !matches!(key.as_str(), "effort" | "format"))
        {
            return Err(invalid_anthropic_request(
                "/output_config",
                "cross-wire output_config only supports effort and format",
            ));
        }
        if output.get("effort").is_some_and(|value| {
            !value.is_null()
                && !matches!(
                    value.as_str(),
                    Some("low" | "medium" | "high" | "xhigh" | "max")
                )
        }) {
            return Err(invalid_anthropic_request(
                "/output_config/effort",
                "must be a supported qualitative effort",
            ));
        }
        if thinking_type == Some("disabled")
            && output.get("effort").is_some_and(|value| !value.is_null())
        {
            return Err(invalid_anthropic_request(
                "/output_config/effort",
                "cannot be combined with disabled thinking",
            ));
        }
        if let Some(format) = output.get("format").filter(|value| !value.is_null()) {
            let Some(format) = format.as_object() else {
                return Err(invalid_anthropic_request(
                    "/output_config/format",
                    "must be an object when present",
                ));
            };
            if !same_wire
                && format
                    .keys()
                    .any(|key| !matches!(key.as_str(), "type" | "schema"))
            {
                return Err(invalid_anthropic_request(
                    "/output_config/format",
                    "cross-wire json_schema format only supports type and schema",
                ));
            }
            if format.get("type").and_then(Value::as_str) != Some("json_schema")
                || !format.get("schema").is_some_and(Value::is_object)
            {
                return Err(invalid_anthropic_request(
                    "/output_config/format",
                    "must be json_schema with an object schema",
                ));
            }
        }
    }
    Ok(())
}

fn validate_final_anthropic_messages(
    data: &Value,
    same_wire: bool,
) -> Result<(), FinalRequestValidationError> {
    let Some(messages) = data.get("messages").and_then(Value::as_array) else {
        return Err(invalid_anthropic_request("/messages", "must be an array"));
    };
    if messages.is_empty() {
        return Err(invalid_anthropic_request(
            "/messages",
            "must contain at least one message",
        ));
    }
    let mut tool_uses = BTreeSet::new();
    let mut tool_results = BTreeSet::new();
    for message in messages {
        let Some(message) = message.as_object() else {
            return Err(invalid_anthropic_request(
                "/messages/*",
                "must be an object",
            ));
        };
        let role = message.get("role").and_then(Value::as_str);
        if !matches!(role, Some("user" | "assistant")) {
            return Err(invalid_anthropic_request(
                "/messages/*/role",
                "must be user or assistant",
            ));
        }
        let Some(content) = message.get("content") else {
            return Err(invalid_anthropic_request(
                "/messages/*/content",
                "is required",
            ));
        };
        if content.is_string() {
            continue;
        }
        let Some(blocks) = content.as_array().filter(|blocks| !blocks.is_empty()) else {
            return Err(invalid_anthropic_request(
                "/messages/*/content",
                "must be a string or non-empty block array",
            ));
        };
        let mut saw_non_tool_result = false;
        for block in blocks {
            let Some(block) = block.as_object() else {
                return Err(invalid_anthropic_request(
                    "/messages/*/content/*",
                    "must be an object",
                ));
            };
            let block_type = block.get("type").and_then(Value::as_str);
            match block_type {
                Some("text") => {
                    saw_non_tool_result = true;
                    if block.get("text").and_then(Value::as_str).is_none() {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*/text",
                            "must be a string",
                        ));
                    }
                }
                Some("image" | "document") if role == Some("user") => {
                    saw_non_tool_result = true;
                    validate_anthropic_media_source(
                        block_type.expect("matched block type"),
                        block.get("source").unwrap_or(&Value::Null),
                        same_wire,
                    )?;
                }
                Some("tool_use") if role == Some("assistant") => {
                    saw_non_tool_result = true;
                    let Some(id) = block
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.trim().is_empty())
                    else {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*/id",
                            "tool_use id must be a non-empty string",
                        ));
                    };
                    if !tool_uses.insert(id.to_string()) {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*/id",
                            "tool_use ids must be unique",
                        ));
                    }
                    if !block
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(|name| !name.trim().is_empty())
                        || !block.get("input").is_some_and(Value::is_object)
                    {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*",
                            "tool_use requires a name and object input",
                        ));
                    }
                }
                Some("tool_result") if role == Some("user") => {
                    if saw_non_tool_result {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content",
                            "tool_result blocks must precede other user content",
                        ));
                    }
                    let Some(id) = block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.trim().is_empty())
                    else {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*/tool_use_id",
                            "must be a non-empty string",
                        ));
                    };
                    if !tool_uses.contains(id) || !tool_results.insert(id.to_string()) {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*/tool_use_id",
                            "must uniquely reference an earlier tool_use",
                        ));
                    }
                    if block
                        .get("is_error")
                        .is_some_and(|value| !value.is_null() && !value.is_boolean())
                    {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*/is_error",
                            "must be a boolean when present",
                        ));
                    }
                }
                Some("thinking") if role == Some("assistant") => {
                    saw_non_tool_result = true;
                    if block.get("thinking").and_then(Value::as_str).is_none()
                        || block
                            .get("signature")
                            .is_some_and(|value| !value.is_null() && !value.is_string())
                    {
                        return Err(invalid_anthropic_request(
                            "/messages/*/content/*",
                            "thinking blocks require string thinking and optional string signature",
                        ));
                    }
                }
                Some(_) if same_wire => {
                    saw_non_tool_result = true;
                }
                _ => {
                    return Err(invalid_anthropic_request(
                        "/messages/*/content/*/type",
                        "is not valid for the message role or target wire",
                    ));
                }
            }
        }
    }
    if tool_uses != tool_results {
        return Err(invalid_anthropic_request(
            "/messages",
            "every tool_use must have exactly one later tool_result",
        ));
    }
    Ok(())
}

fn validate_final_anthropic_request(
    data: &Value,
    same_wire: bool,
) -> Result<(), FinalRequestValidationError> {
    require_non_empty_string(data, "/model", "model")?;
    let max_tokens = data.get("max_tokens").and_then(Value::as_u64);
    if !max_tokens.is_some_and(|value| value > 0 && value <= u32::MAX as u64) {
        return Err(invalid_anthropic_request(
            "/max_tokens",
            "must be a positive u32 integer",
        ));
    }
    if data
        .get("stream")
        .is_some_and(|value| !value.is_null() && !value.is_boolean())
    {
        return Err(invalid_anthropic_request(
            "/stream",
            "must be a boolean when present",
        ));
    }
    if data.get("stop_sequences").is_some_and(|value| {
        !value.is_null()
            && !value.as_array().is_some_and(|values| {
                values
                    .iter()
                    .all(|value| value.as_str().is_some_and(|value| !value.is_empty()))
            })
    }) {
        return Err(invalid_anthropic_request(
            "/stop_sequences",
            "must be an array of non-empty strings when present",
        ));
    }
    for (field, path, min, max) in [
        ("temperature", "/temperature", 0.0, 1.0),
        ("top_p", "/top_p", 0.0, 1.0),
    ] {
        if data.get(field).is_some_and(|value| {
            !value.is_null()
                && !value
                    .as_f64()
                    .is_some_and(|value| value.is_finite() && value >= min && value <= max)
        }) {
            return Err(invalid_anthropic_request(
                path,
                "must be a finite number in the supported range",
            ));
        }
    }
    if data.get("top_k").is_some_and(|value| {
        !value.is_null()
            && !value
                .as_u64()
                .is_some_and(|value| value > 0 && value <= u32::MAX as u64)
    }) {
        return Err(invalid_anthropic_request(
            "/top_k",
            "must be a positive u32 integer when present",
        ));
    }
    if data
        .get("metadata")
        .is_some_and(|value| !value.is_null() && !value.is_object())
    {
        return Err(invalid_anthropic_request(
            "/metadata",
            "must be an object when present",
        ));
    }
    validate_anthropic_system(data)?;
    validate_final_anthropic_messages(data, same_wire)?;
    let (tool_names, has_tools) = validate_final_anthropic_tools(data, same_wire)?;
    validate_final_anthropic_tool_choice(data, &tool_names, has_tools)?;
    validate_final_anthropic_reasoning_and_output(data, same_wire)
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
    validate_final_generation_request_for_downstream(data, None, upstream_protocol, profile_type)
}

pub(in crate::service::transform) fn validate_final_generation_request_for_downstream(
    data: &Value,
    downstream_protocol: Option<DownstreamProtocol>,
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
        UpstreamProtocol::Anthropic => validate_final_anthropic_request(
            data,
            downstream_protocol == Some(DownstreamProtocol::Anthropic),
        ),
        UpstreamProtocol::Gemini => super::providers::gemini::validate_gemini_target_request(
            data,
            downstream_protocol == Some(DownstreamProtocol::Gemini),
        )
        .map_err(|error| FinalRequestValidationError {
            path: error.path,
            reason: error.reason,
        }),
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
                json!({
                    "contents": [{"parts": [{"text": "hi"}]}],
                    "generationConfig":{"candidateCount":1}
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

    #[test]
    fn anthropic_final_validation_accepts_portable_combinations_and_same_wire_extensions() {
        let portable = json!({
            "model": "claude-target",
            "max_tokens": 4096,
            "system": [{"type": "text", "text": "system"}],
            "messages": [
                {"role": "user", "content": "use the tool"},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "plan"},
                    {"type": "tool_use", "id": "tool-1", "name": "lookup", "input": {"q": "safe"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "tool-1", "content": "done"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}},
                    {"type": "document", "title": "doc", "source": {"type": "text", "media_type": "text/plain", "data": "hello"}},
                    {"type": "text", "text": "continue"}
                ]}
            ],
            "tools": [{
                "name": "lookup",
                "description": "safe description",
                "input_schema": {"type": "object"},
                "strict": true
            }],
            "tool_choice": {"type": "tool", "name": "lookup", "disable_parallel_tool_use": false},
            "thinking": {"type": "adaptive"},
            "output_config": {
                "effort": "high",
                "format": {"type": "json_schema", "schema": {"type": "object"}}
            },
            "temperature": 0.5,
            "top_p": 1.0,
            "top_k": 10,
            "stop_sequences": ["done"],
            "metadata": {"user_id": "safe"},
            "stream": true
        });
        validate_final_generation_request_for_downstream(
            &portable,
            Some(DownstreamProtocol::Openai),
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect("portable Anthropic target combination should pass");

        let enabled_thinking = json!({
            "model": "claude-target",
            "max_tokens": 2048,
            "messages": [{"role": "user", "content": "reason"}],
            "thinking": {"type": "enabled", "budget_tokens": 1024}
        });
        validate_final_generation_request_for_downstream(
            &enabled_thinking,
            Some(DownstreamProtocol::Anthropic),
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect("enabled Anthropic thinking budget below max_tokens should pass");

        let same_wire_future = json!({
            "model": "claude-target",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": [
                {"type": "future_block", "future": "private-payload-marker"},
                {"type": "document", "source": {"type": "file", "file_id": "file_beta"}}
            ]}],
            "tools": [{"type": "web_search_20990101", "name": "server-search"}]
        });
        validate_final_generation_request_for_downstream(
            &same_wire_future,
            Some(DownstreamProtocol::Anthropic),
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect("same-wire future blocks and server tools remain transparent");

        let error = validate_final_generation_request_for_downstream(
            &same_wire_future,
            Some(DownstreamProtocol::Responses),
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect_err("cross-wire future blocks must fail closed");
        assert_eq!(error.path, "/messages/*/content/*/type");
        assert!(!error.to_string().contains("private-payload-marker"));
    }

    #[test]
    fn anthropic_final_validation_rejects_invalid_core_tools_media_and_combinations() {
        let base = json!({
            "model": "claude-target",
            "max_tokens": 64,
            "messages": [{"role": "user", "content": "hello"}]
        });
        let cases = [
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[]}),
                "/messages",
            ),
            (
                json!({"model":"claude-target","max_tokens":0,"messages":[{"role":"user","content":"hello"}]}),
                "/max_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":4294967296_u64,"messages":[{"role":"user","content":"hello"}]}),
                "/max_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":-1,"messages":[{"role":"user","content":"hello"}]}),
                "/max_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":1.5,"messages":[{"role":"user","content":"hello"}]}),
                "/max_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"system","content":"private-payload-marker"}]}),
                "/messages/*/role",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"not base64"}}]}]}),
                "/messages/*/content/*/source",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":[{"type":"tool_result","tool_use_id":"missing"}]}]}),
                "/messages/*/content/*/tool_use_id",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"dup","name":"one","input":{}},{"type":"tool_use","id":"dup","name":"two","input":{}}]}]}),
                "/messages/*/content/*/id",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"call","name":"one","input":{}}]},{"role":"user","content":[{"type":"text","text":"first"},{"type":"tool_result","tool_use_id":"call"}]}]}),
                "/messages/*/content",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"tools":[{"name":"dup","input_schema":{}},{"name":"dup","input_schema":{}}]}),
                "/tools/*/name",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"tools":[{"name":"one","input_schema":{}}],"tool_choice":{"type":"tool","name":"missing"}}),
                "/tool_choice/name",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"thinking":{"type":"enabled","budget_tokens":0}}),
                "/thinking/budget_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":4096,"messages":[{"role":"user","content":"hello"}],"thinking":{"type":"enabled","budget_tokens":1023}}),
                "/thinking/budget_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":1024,"messages":[{"role":"user","content":"hello"}],"thinking":{"type":"enabled","budget_tokens":1024}}),
                "/thinking/budget_tokens",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"thinking":{"type":"disabled"},"output_config":{"effort":"high"}}),
                "/output_config/effort",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"output_config":{"format":{"type":"json_object"}}}),
                "/output_config/format",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"stream":"yes"}),
                "/stream",
            ),
            (
                json!({"model":"claude-target","max_tokens":64,"messages":[{"role":"user","content":"hello"}],"temperature":2}),
                "/temperature",
            ),
        ];

        validate_final_generation_request_for_downstream(
            &base,
            Some(DownstreamProtocol::Openai),
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect("baseline request should remain valid");
        for (payload, expected_path) in cases {
            let error = validate_final_generation_request_for_downstream(
                &payload,
                Some(DownstreamProtocol::Openai),
                UpstreamProtocol::Anthropic,
                &UpstreamProfileType::Anthropic,
            )
            .expect_err("invalid Anthropic target must fail closed");
            assert_eq!(error.path, expected_path, "payload shape: {payload}");
            assert!(!error.to_string().contains("private-payload-marker"));
        }
    }

    #[test]
    fn anthropic_max_tokens_synthesis_is_cross_wire_only_and_payload_free() {
        let missing = [
            (
                DownstreamProtocol::Openai,
                json!({"model":"alias","messages":[{"role":"user","content":"hello"}]}),
            ),
            (
                DownstreamProtocol::Responses,
                json!({"model":"alias","input":"hello"}),
            ),
            (
                DownstreamProtocol::Gemini,
                json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}]}),
            ),
        ];
        for (downstream, payload) in missing {
            let transformed = transform_request_data(
                payload,
                downstream,
                UpstreamProtocol::Anthropic,
                false,
            )
            .expect("cross-wire request without output limit should synthesize a target default");
            assert_eq!(transformed.value["max_tokens"], 4096, "{downstream:?}");
            assert!(transformed.summary.facts.iter().any(|fact| {
                fact.reason_code == TransformReasonCode::SyntheticAnthropicMaxTokens
                    && fact.action == TransformAction::Synthesize
                    && fact.safe_summary.is_none()
            }));
        }

        for (downstream, payload, expected) in [
            (
                DownstreamProtocol::Openai,
                json!({"model":"alias","max_tokens":17,"messages":[{"role":"user","content":"hello"}]}),
                17,
            ),
            (
                DownstreamProtocol::Responses,
                json!({"model":"alias","max_output_tokens":18,"input":"hello"}),
                18,
            ),
            (
                DownstreamProtocol::Gemini,
                json!({"contents":[{"role":"user","parts":[{"text":"hello"}]}],"generationConfig":{"maxOutputTokens":19}}),
                19,
            ),
        ] {
            let transformed =
                transform_request_data(payload, downstream, UpstreamProtocol::Anthropic, false)
                    .expect("explicit output limit should remain explicit");
            assert_eq!(transformed.value["max_tokens"], expected, "{downstream:?}");
            assert!(!transformed.summary.facts.iter().any(|fact| {
                fact.reason_code == TransformReasonCode::SyntheticAnthropicMaxTokens
            }));
        }

        let same_wire = transform_request_data(
            json!({
                "model":"claude-alias",
                "messages":[{"role":"user","content":"private-payload-marker"}]
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Anthropic,
            false,
        )
        .expect("same-wire request remains raw until final validation");
        assert!(same_wire.value.get("max_tokens").is_none());
        assert!(
            !same_wire.summary.facts.iter().any(|fact| {
                fact.reason_code == TransformReasonCode::SyntheticAnthropicMaxTokens
            })
        );
        let error = validate_final_generation_request_for_downstream(
            &same_wire.value,
            Some(DownstreamProtocol::Anthropic),
            UpstreamProtocol::Anthropic,
            &UpstreamProfileType::Anthropic,
        )
        .expect_err("same-wire request must provide max_tokens");
        assert_eq!(error.path, "/max_tokens");
        assert!(!error.to_string().contains("private-payload-marker"));
    }
}
