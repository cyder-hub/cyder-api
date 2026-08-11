use serde_json::Value;

use super::capability::TransformValueKind;
use super::diagnostics::record_captured_transform_fact;
use super::unified::{
    UnifiedContentPart, UnifiedItem, UnifiedRequest, UnifiedResponse, UnifiedRole,
};
use super::{
    TransformAction, TransformDiagnosticFact, TransformOutcomeKind, TransformPhase,
    TransformProtocol, TransformReasonCode, TransformSemanticUnit, apply_transform_policy,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::service::transform) struct SourceSemanticError {
    pub semantic_unit: TransformSemanticUnit,
    pub reason_code: TransformReasonCode,
}

impl SourceSemanticError {
    const fn invalid(semantic_unit: TransformSemanticUnit) -> Self {
        Self {
            semantic_unit,
            reason_code: TransformReasonCode::InvalidProtocolShape,
        }
    }

    const fn unknown(semantic_unit: TransformSemanticUnit) -> Self {
        Self {
            semantic_unit,
            reason_code: TransformReasonCode::UnknownSemanticUnit,
        }
    }
}

fn record_fact(
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    outcome: TransformOutcomeKind,
    action: TransformAction,
    reason_code: TransformReasonCode,
) {
    record_captured_transform_fact(TransformDiagnosticFact {
        sequence: 0,
        phase,
        semantic_unit,
        outcome,
        action,
        reason_code,
        safe_summary: None,
    });
}

fn record_synthesis(
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    reason_code: TransformReasonCode,
) {
    record_fact(
        phase,
        semantic_unit,
        TransformOutcomeKind::Lossless,
        TransformAction::Synthesize,
        reason_code,
    );
}

fn record_minor_drop(phase: TransformPhase, semantic_unit: TransformSemanticUnit) {
    record_fact(
        phase,
        semantic_unit,
        TransformOutcomeKind::ControlledLossMinor,
        TransformAction::Drop,
        TransformReasonCode::UnsupportedContent,
    );
}

fn record_no_semantic_output(phase: TransformPhase, semantic_unit: TransformSemanticUnit) {
    record_fact(
        phase,
        semantic_unit,
        TransformOutcomeKind::ControlledLossMinor,
        TransformAction::Drop,
        TransformReasonCode::NoSemanticOutput,
    );
}

fn record_rejection(
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    reason_code: TransformReasonCode,
) {
    record_fact(
        phase,
        semantic_unit,
        TransformOutcomeKind::ExplicitReject,
        TransformAction::Reject,
        reason_code,
    );
}

fn object_array<'a>(value: &'a Value, field: &str) -> &'a [Value] {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn require_non_empty_string(
    value: &Value,
    field: &str,
    semantic_unit: TransformSemanticUnit,
) -> Result<(), SourceSemanticError> {
    if value
        .get(field)
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        Ok(())
    } else {
        Err(SourceSemanticError::invalid(semantic_unit))
    }
}

fn validate_openai_tool_calls(
    messages: &[Value],
    phase: TransformPhase,
) -> Result<(), SourceSemanticError> {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if matches!(role, "developer" | "function") {
            return Err(SourceSemanticError {
                semantic_unit: TransformSemanticUnit::Role,
                reason_code: TransformReasonCode::UnsupportedContent,
            });
        }
        if !matches!(role, "system" | "user" | "assistant" | "tool") {
            return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
        }

        if message.get("tool_call_id").is_some() && message.get("content").is_none() {
            record_synthesis(
                phase,
                TransformSemanticUnit::ToolResult,
                TransformReasonCode::SyntheticEnvelope,
            );
        }

        for call in object_array(message, "tool_calls") {
            if call.get("type").and_then(Value::as_str) != Some("function") {
                return Err(SourceSemanticError::unknown(
                    TransformSemanticUnit::ToolCall,
                ));
            }
            let arguments = call
                .get("function")
                .and_then(|function| function.get("arguments"))
                .and_then(Value::as_str)
                .ok_or_else(|| SourceSemanticError::invalid(TransformSemanticUnit::ToolCall))?;
            if arguments.trim().is_empty() {
                record_synthesis(
                    phase,
                    TransformSemanticUnit::ToolCall,
                    TransformReasonCode::SyntheticEnvelope,
                );
            } else if !serde_json::from_str::<Value>(arguments)
                .is_ok_and(|arguments| arguments.is_object())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::ToolCall,
                ));
            }
        }
    }
    Ok(())
}

fn validate_responses_items(
    items: &[Value],
    phase: TransformPhase,
) -> Result<(), SourceSemanticError> {
    for item in items {
        let type_name = item.get("type").and_then(Value::as_str);
        match type_name {
            Some("message") => {
                if item.get("id").is_none() {
                    record_synthesis(
                        phase,
                        TransformSemanticUnit::Metadata,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                if item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status != "completed")
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnsupportedContent,
                    });
                }
                for part in object_array(item, "content") {
                    match part.get("type").and_then(Value::as_str) {
                        Some("input_image")
                            if !part
                                .get("image_url")
                                .and_then(Value::as_str)
                                .is_some_and(|url| !url.is_empty()) =>
                        {
                            return Err(SourceSemanticError::invalid(
                                TransformSemanticUnit::ImageUrl,
                            ));
                        }
                        Some("input_file")
                            if !["file_url", "file_id", "file_data"].iter().any(|field| {
                                part.get(*field).is_some_and(|value| !value.is_null())
                            }) =>
                        {
                            return Err(SourceSemanticError::invalid(
                                TransformSemanticUnit::FileUrl,
                            ));
                        }
                        _ => {}
                    }
                }
                if object_array(item, "content").is_empty() {
                    record_no_semantic_output(phase, TransformSemanticUnit::Text);
                }
            }
            Some("function_call") => {
                if item.get("id").is_none() {
                    record_synthesis(
                        phase,
                        TransformSemanticUnit::ToolCall,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                if item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status != "completed")
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnsupportedContent,
                    });
                }
                let arguments = item
                    .get("arguments")
                    .and_then(Value::as_str)
                    .ok_or_else(|| SourceSemanticError::invalid(TransformSemanticUnit::ToolCall))?;
                if arguments.trim().is_empty() {
                    record_synthesis(
                        phase,
                        TransformSemanticUnit::ToolCall,
                        TransformReasonCode::SyntheticEnvelope,
                    );
                } else if !serde_json::from_str::<Value>(arguments)
                    .is_ok_and(|arguments| arguments.is_object())
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolCall,
                    ));
                }
            }
            Some("function_call_output") => {
                if item.get("id").is_none() {
                    record_synthesis(
                        phase,
                        TransformSemanticUnit::ToolResult,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                if item
                    .get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|status| status != "completed")
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnsupportedContent,
                    });
                }
            }
            Some("reasoning") => {}
            None if item.get("role").is_some() && item.get("content").is_some() => {
                record_synthesis(
                    phase,
                    TransformSemanticUnit::Metadata,
                    TransformReasonCode::SyntheticCorrelationId,
                );
            }
            _ => {
                return Err(SourceSemanticError::unknown(
                    TransformSemanticUnit::ResponsesUnknownItem,
                ));
            }
        }
    }
    Ok(())
}

fn validate_anthropic_messages(messages: &[Value]) -> Result<(), SourceSemanticError> {
    for message in messages {
        let role = message
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if !matches!(role, "user" | "assistant") {
            return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
        }

        let Some(content) = message.get("content") else {
            return Err(SourceSemanticError::invalid(TransformSemanticUnit::Text));
        };
        if content.is_string() {
            continue;
        }
        let Some(blocks) = content.as_array() else {
            return Err(SourceSemanticError::invalid(TransformSemanticUnit::Text));
        };
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if block.get("text").and_then(Value::as_str).is_none() {
                        return Err(SourceSemanticError::invalid(TransformSemanticUnit::Text));
                    }
                    if block.get("citations").is_some() {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::Metadata,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    if block.get("cache_control").is_some() {
                        record_minor_drop(
                            TransformPhase::RequestDecode,
                            TransformSemanticUnit::Metadata,
                        );
                    }
                }
                Some("image") => {
                    let source = block.get("source").ok_or_else(|| {
                        SourceSemanticError::invalid(TransformSemanticUnit::ImageData)
                    })?;
                    let source_type = source.get("type").and_then(Value::as_str);
                    if !matches!(source_type, Some("base64") | Some("url")) {
                        return Err(SourceSemanticError::unknown(
                            TransformSemanticUnit::ImageData,
                        ));
                    }
                    let required_fields_present = match source_type {
                        Some("base64") => {
                            source.get("media_type").and_then(Value::as_str).is_some()
                                && source.get("data").and_then(Value::as_str).is_some()
                        }
                        Some("url") => source.get("url").and_then(Value::as_str).is_some(),
                        _ => false,
                    };
                    if !required_fields_present {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::ImageData,
                        ));
                    }
                }
                Some("tool_use") if role == "assistant" => {
                    if !block.get("input").is_some_and(Value::is_object) {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::ToolCall,
                        ));
                    }
                }
                Some("tool_result") if role == "user" => {
                    if block.get("is_error").is_some() {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::ToolResult,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    if block.get("content").is_none() {
                        record_synthesis(
                            TransformPhase::RequestDecode,
                            TransformSemanticUnit::ToolResult,
                            TransformReasonCode::SyntheticEnvelope,
                        );
                    }
                }
                Some("tool_use" | "tool_result") => {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolRoleMessage,
                    ));
                }
                _ => {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::RequestEnvelope,
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_gemini_parts(
    parts: &[Value],
    phase: TransformPhase,
) -> Result<(), SourceSemanticError> {
    for part in parts {
        let known_keys = [
            "text",
            "executableCode",
            "functionCall",
            "functionResponse",
            "inlineData",
            "fileData",
        ];
        if known_keys
            .iter()
            .filter(|key| part.get(**key).is_some())
            .count()
            != 1
        {
            return Err(SourceSemanticError::unknown(
                TransformSemanticUnit::RequestEnvelope,
            ));
        }
        if let Some(call) = part.get("functionCall")
            && !call.get("args").is_some_and(Value::is_object)
        {
            return Err(SourceSemanticError::invalid(
                TransformSemanticUnit::ToolCall,
            ));
        }
        if part.get("text").and_then(Value::as_str) == Some("") {
            record_no_semantic_output(phase, TransformSemanticUnit::Text);
        }
    }
    Ok(())
}

pub(in crate::service::transform) fn validate_downstream_request(
    protocol: DownstreamProtocol,
    data: &Value,
) -> Result<(), SourceSemanticError> {
    match protocol {
        DownstreamProtocol::Openai => {
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if let Some(tool_choice) = data.get("tool_choice") {
                let valid = match tool_choice {
                    Value::String(value) => matches!(value.as_str(), "none" | "auto" | "required"),
                    Value::Object(value) => {
                        value.get("type").and_then(Value::as_str) == Some("function")
                            && value
                                .get("function")
                                .and_then(|function| function.get("name"))
                                .and_then(Value::as_str)
                                .is_some_and(|name| !name.is_empty())
                    }
                    _ => false,
                };
                if !valid {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
            }
            if let Some(format) = data.get("response_format") {
                let valid = match format.get("type").and_then(Value::as_str) {
                    Some("text" | "json_object") => true,
                    Some("json_schema") => format.get("json_schema").is_some_and(|schema| {
                        schema.get("name").and_then(Value::as_str).is_some()
                            && schema.get("schema").is_some_and(Value::is_object)
                    }),
                    _ => false,
                };
                if !valid {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::StructuredOutput,
                    ));
                }
            }
            if data
                .get("logit_bias")
                .is_some_and(|value| !value.is_object())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::Metadata,
                ));
            }
            for tool in object_array(data, "tools") {
                if tool.get("type").and_then(Value::as_str) != Some("function")
                    || !tool
                        .get("function")
                        .and_then(|function| function.get("parameters"))
                        .is_some_and(Value::is_object)
                {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
            }
            validate_openai_tool_calls(
                object_array(data, "messages"),
                TransformPhase::RequestDecode,
            )
        }
        DownstreamProtocol::Responses => {
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if let Some(items) = data.get("input").and_then(Value::as_array) {
                validate_responses_items(items, TransformPhase::RequestDecode)?;
            }
            for tool in object_array(data, "tools") {
                if tool
                    .get("parameters")
                    .is_some_and(|parameters| !parameters.is_object())
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
                if tool.get("parameters").is_none() {
                    record_synthesis(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::ToolDefinitions,
                        TransformReasonCode::SyntheticEnvelope,
                    );
                }
            }
            Ok(())
        }
        DownstreamProtocol::Anthropic => {
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if let Some(blocks) = data.get("system").and_then(Value::as_array) {
                for block in blocks {
                    if block.get("type").and_then(Value::as_str) != Some("text") {
                        return Err(SourceSemanticError::unknown(
                            TransformSemanticUnit::RequestEnvelope,
                        ));
                    }
                }
            }
            if data.get("system").and_then(Value::as_str) == Some("") {
                record_no_semantic_output(
                    TransformPhase::RequestDecode,
                    TransformSemanticUnit::Text,
                );
            }
            for tool in object_array(data, "tools") {
                if !tool.get("input_schema").is_some_and(Value::is_object) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
            }
            validate_anthropic_messages(object_array(data, "messages"))
        }
        DownstreamProtocol::Gemini => {
            if !object_array(data, "safetySettings").is_empty() {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::Metadata,
                    reason_code: TransformReasonCode::UnsupportedContent,
                });
            }
            if let Some(system) = data.get("system_instruction")
                && let Some(parts) = system.get("parts").and_then(Value::as_array)
                && parts.iter().any(|part| part.get("text").is_none())
            {
                return Err(SourceSemanticError::unknown(
                    TransformSemanticUnit::RequestEnvelope,
                ));
            }
            for content in object_array(data, "contents") {
                match content.get("role").and_then(Value::as_str) {
                    Some("user" | "model") => {}
                    None => record_synthesis(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::Role,
                        TransformReasonCode::SyntheticEnvelope,
                    ),
                    Some(_) => {
                        return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
                    }
                }
                let parts = object_array(content, "parts");
                if parts.is_empty() {
                    record_no_semantic_output(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::RequestEnvelope,
                    );
                }
                validate_gemini_parts(parts, TransformPhase::RequestDecode)?;
                let role = content
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");
                let has_function_call = parts.iter().any(|part| part.get("functionCall").is_some());
                let has_function_response = parts
                    .iter()
                    .any(|part| part.get("functionResponse").is_some());
                if (has_function_call
                    && (role != "model"
                        || parts.iter().any(|part| {
                            !matches!(
                                part.as_object()
                                    .and_then(|object| object.keys().next())
                                    .map(String::as_str),
                                Some("functionCall" | "text" | "executableCode")
                            )
                        })))
                    || (has_function_response
                        && (role != "user"
                            || parts
                                .iter()
                                .any(|part| part.get("functionResponse").is_none())))
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolRoleMessage,
                    ));
                }
                for _ in parts
                    .iter()
                    .filter(|part| part.get("functionCall").is_some())
                {
                    record_synthesis(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::ToolCall,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                for _ in parts
                    .iter()
                    .filter(|part| part.get("functionResponse").is_some())
                {
                    record_synthesis(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::ToolResult,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
            }
            Ok(())
        }
    }
}

pub(in crate::service::transform) fn validate_upstream_response(
    protocol: UpstreamProtocol,
    data: &Value,
) -> Result<(), SourceSemanticError> {
    match protocol {
        UpstreamProtocol::Openai => {
            require_non_empty_string(data, "id", TransformSemanticUnit::Metadata)?;
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if data.get("object").and_then(Value::as_str) != Some("chat.completion") {
                return Err(SourceSemanticError::unknown(
                    TransformSemanticUnit::ResponseEnvelope,
                ));
            }
            let messages = object_array(data, "choices")
                .iter()
                .filter_map(|choice| choice.get("message").cloned())
                .collect::<Vec<_>>();
            if messages
                .iter()
                .any(|message| message.get("role").and_then(Value::as_str) != Some("assistant"))
            {
                return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
            }
            validate_openai_tool_calls(&messages, TransformPhase::ResponseDecode)
        }
        UpstreamProtocol::Responses => {
            require_non_empty_string(data, "id", TransformSemanticUnit::Metadata)?;
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if data.get("error").is_some_and(|error| !error.is_null()) {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::ResponseEnvelope,
                    reason_code: TransformReasonCode::UnsupportedContent,
                });
            }
            if data.get("status").and_then(Value::as_str) != Some("completed") {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::Lifecycle,
                    reason_code: TransformReasonCode::UnsupportedContent,
                });
            }
            if data
                .get("metadata")
                .is_some_and(|metadata| !metadata.is_object())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::Metadata,
                ));
            }
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::SyntheticIndex,
            );
            validate_responses_items(object_array(data, "output"), TransformPhase::ResponseDecode)
        }
        UpstreamProtocol::Anthropic => {
            require_non_empty_string(data, "id", TransformSemanticUnit::Metadata)?;
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if data.get("type").and_then(Value::as_str) != Some("message")
                || data.get("role").and_then(Value::as_str) != Some("assistant")
            {
                return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
            }
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::SyntheticIndex,
            );
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::ResponseEnvelope,
                TransformReasonCode::SyntheticEnvelope,
            );
            for block in object_array(data, "content") {
                if !matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("text" | "thinking" | "tool_use")
                ) {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::ResponseEnvelope,
                    ));
                }
            }
            if let Some(reason) = data.get("stop_reason").and_then(Value::as_str)
                && !matches!(
                    reason,
                    "end_turn" | "stop_sequence" | "tool_use" | "max_tokens"
                )
            {
                return Err(SourceSemanticError::unknown(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            Ok(())
        }
        UpstreamProtocol::Gemini => {
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::SyntheticCorrelationId,
            );
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::Model,
                TransformReasonCode::SyntheticEnvelope,
            );
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::ResponseEnvelope,
                TransformReasonCode::SyntheticEnvelope,
            );
            for (position, candidate) in object_array(data, "candidates").iter().enumerate() {
                if candidate.get("index").is_none() {
                    let _ = position;
                    record_synthesis(
                        TransformPhase::ResponseDecode,
                        TransformSemanticUnit::Metadata,
                        TransformReasonCode::SyntheticIndex,
                    );
                }
                if let Some(content) = candidate.get("content") {
                    if content.get("role").and_then(Value::as_str) != Some("model") {
                        return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
                    }
                    validate_gemini_parts(
                        object_array(content, "parts"),
                        TransformPhase::ResponseDecode,
                    )?;
                }
                if let Some(reason) = candidate.get("finishReason").and_then(Value::as_str)
                    && !matches!(
                        reason,
                        "STOP" | "TOOL_USE" | "MAX_TOKENS" | "SAFETY" | "RECITATION"
                    )
                {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::Lifecycle,
                    ));
                }
            }
            Ok(())
        }
        UpstreamProtocol::Ollama => {
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if data
                .get("message")
                .and_then(|message| message.get("role"))
                .and_then(Value::as_str)
                != Some("assistant")
            {
                return Err(SourceSemanticError::unknown(TransformSemanticUnit::Role));
            }
            if data.get("message").is_some_and(|message| {
                message.get("thinking").is_some()
                    || message.get("tool_calls").is_some()
                    || message
                        .get("images")
                        .and_then(Value::as_array)
                        .is_some_and(|images| !images.is_empty())
            }) {
                return Err(SourceSemanticError::unknown(
                    TransformSemanticUnit::ResponseEnvelope,
                ));
            }
            if data.get("done").and_then(Value::as_bool) != Some(true) {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if data.get("done_reason").is_none() {
                record_synthesis(
                    TransformPhase::ResponseDecode,
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::SyntheticEnvelope,
                );
            }
            if data
                .get("created_at")
                .and_then(Value::as_str)
                .is_none_or(|created_at| chrono::DateTime::parse_from_rfc3339(created_at).is_err())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::Metadata,
                ));
            }
            let has_prompt_tokens = data.get("prompt_eval_count").is_some();
            let has_completion_tokens = data.get("eval_count").is_some();
            if has_prompt_tokens != has_completion_tokens {
                record_minor_drop(TransformPhase::ResponseDecode, TransformSemanticUnit::Usage);
            }
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::SyntheticCorrelationId,
            );
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::SyntheticIndex,
            );
            record_synthesis(
                TransformPhase::ResponseDecode,
                TransformSemanticUnit::ResponseEnvelope,
                TransformReasonCode::SyntheticEnvelope,
            );
            Ok(())
        }
    }
}

fn push_kind(kinds: &mut Vec<TransformValueKind>, kind: TransformValueKind) {
    if !kinds.contains(&kind) {
        kinds.push(kind);
    }
}

fn collect_content_kind(kinds: &mut Vec<TransformValueKind>, part: &UnifiedContentPart) {
    push_kind(kinds, TransformValueKind::from(part));
}

fn collect_item_kinds(kinds: &mut Vec<TransformValueKind>, item: &UnifiedItem) {
    match item {
        UnifiedItem::Message(message) => {
            for part in &message.content {
                collect_content_kind(kinds, part);
            }
        }
        UnifiedItem::Reasoning(reasoning) => {
            for part in &reasoning.content {
                collect_content_kind(kinds, part);
            }
        }
        UnifiedItem::FunctionCall(_) => push_kind(kinds, TransformValueKind::ToolCall),
        UnifiedItem::FunctionCallOutput(_) => push_kind(kinds, TransformValueKind::ToolResult),
        UnifiedItem::FileReference(_) => push_kind(kinds, TransformValueKind::FileUrl),
    }
}

fn collect_request_kinds(request: &UnifiedRequest) -> Vec<TransformValueKind> {
    let mut kinds = Vec::new();
    if request.tools.is_some() {
        push_kind(&mut kinds, TransformValueKind::ToolDefinitions);
    }
    if request.top_k().is_some() {
        push_kind(&mut kinds, TransformValueKind::TopKParameter);
    }
    for message in &request.messages {
        if message.role == UnifiedRole::Tool {
            push_kind(&mut kinds, TransformValueKind::ToolRoleMessage);
        }
        for part in &message.content {
            collect_content_kind(&mut kinds, part);
        }
    }
    for item in &request.items {
        collect_item_kinds(&mut kinds, item);
        if let UnifiedItem::FileReference(file) = item
            && file.file_url.is_none()
        {
            // Provider-native file IDs have no cross-wire dereference contract.
            push_kind(&mut kinds, TransformValueKind::FileUrl);
        }
    }
    kinds
}

fn collect_response_kinds(response: &UnifiedResponse) -> Vec<TransformValueKind> {
    let mut kinds = Vec::new();
    for choice in &response.choices {
        for item in choice.content_items() {
            collect_item_kinds(&mut kinds, &item);
        }
    }
    kinds
}

pub(in crate::service::transform) fn audit_target_request(
    target: UpstreamProtocol,
    request: &UnifiedRequest,
) {
    for kind in collect_request_kinds(request) {
        apply_transform_policy(
            TransformProtocol::Unified,
            TransformProtocol::Upstream(target),
            kind,
            "non-stream request adapter audit",
        );
    }

    for message in request
        .messages
        .iter()
        .filter(|message| message.role == UnifiedRole::System)
    {
        for part in &message.content {
            let supported = match target {
                UpstreamProtocol::Anthropic => matches!(
                    part,
                    UnifiedContentPart::Text { .. }
                        | UnifiedContentPart::Reasoning { .. }
                        | UnifiedContentPart::ImageUrl { .. }
                        | UnifiedContentPart::FileUrl { .. }
                        | UnifiedContentPart::FileData { .. }
                        | UnifiedContentPart::ExecutableCode { .. }
                ),
                UpstreamProtocol::Gemini => matches!(
                    part,
                    UnifiedContentPart::Text { .. } | UnifiedContentPart::ImageUrl { .. }
                ),
                UpstreamProtocol::Openai => matches!(
                    part,
                    UnifiedContentPart::Text { .. } | UnifiedContentPart::Reasoning { .. }
                ),
                UpstreamProtocol::Responses | UpstreamProtocol::Ollama => true,
            };
            if !supported {
                record_rejection(
                    TransformPhase::RequestEncode,
                    TransformSemanticUnit::from(TransformValueKind::from(part)),
                    TransformReasonCode::UnsupportedContent,
                );
            }
        }
    }

    if target != UpstreamProtocol::Responses
        && request
            .items
            .iter()
            .any(|item| matches!(item, UnifiedItem::FileReference(file) if file.file_url.is_none()))
    {
        record_rejection(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::FileUrl,
            TransformReasonCode::UnsupportedContent,
        );
    }

    if request.model.as_deref().is_none_or(str::is_empty) {
        // Gemini identifies the model in the URL. Every other upstream model field is
        // overwritten by materialization after this adapter step.
        record_synthesis(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::Model,
            TransformReasonCode::SyntheticEnvelope,
        );
    }

    if request.max_tokens.is_none() && target == UpstreamProtocol::Anthropic {
        record_synthesis(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::RequestEnvelope,
            TransformReasonCode::SyntheticEnvelope,
        );
    }

    let has_structured_output = request
        .openai_extension()
        .and_then(|extension| extension.response_format.as_ref())
        .is_some()
        || request
            .responses_extension()
            .and_then(|extension| extension.text_format.as_ref())
            .is_some();
    let structured_output_supported = matches!(
        target,
        UpstreamProtocol::Openai | UpstreamProtocol::Responses
    ) && !(target == UpstreamProtocol::Openai
        && request
            .responses_extension()
            .and_then(|extension| extension.text_format.as_ref())
            .is_some());
    if has_structured_output && !structured_output_supported {
        record_rejection(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::StructuredOutput,
            TransformReasonCode::UnsupportedContent,
        );
    }

    if request
        .anthropic_extension()
        .and_then(|extension| extension.metadata.as_ref())
        .is_some()
        && target != UpstreamProtocol::Anthropic
    {
        record_minor_drop(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::Metadata,
        );
    }

    if let Some(openai) = request.openai_extension() {
        if openai.tool_choice.is_some()
            && !matches!(
                target,
                UpstreamProtocol::Openai | UpstreamProtocol::Responses
            )
        {
            record_rejection(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::ToolDefinitions,
                TransformReasonCode::UnsupportedContent,
            );
        }
        if openai.n.is_some() && target != UpstreamProtocol::Openai {
            record_rejection(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::RequestEnvelope,
                TransformReasonCode::UnsupportedContent,
            );
        }
        if openai.logit_bias.is_some() && target != UpstreamProtocol::Openai {
            record_rejection(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::UnsupportedContent,
            );
        }
        if openai.user.is_some() && target != UpstreamProtocol::Openai {
            record_minor_drop(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::Metadata,
            );
        }
        if let Some(passthrough) = openai.passthrough.as_ref() {
            if passthrough.get("reasoning_effort").is_some()
                && target != UpstreamProtocol::Responses
            {
                record_rejection(
                    TransformPhase::RequestEncode,
                    TransformSemanticUnit::ReasoningContent,
                    TransformReasonCode::UnsupportedReasoning,
                );
            }
            if passthrough.get("parallel_tool_calls").is_some()
                && !matches!(
                    target,
                    UpstreamProtocol::Openai | UpstreamProtocol::Responses
                )
            {
                record_rejection(
                    TransformPhase::RequestEncode,
                    TransformSemanticUnit::ToolDefinitions,
                    TransformReasonCode::UnsupportedContent,
                );
            }
            if target != UpstreamProtocol::Openai
                && (passthrough.get("logprobs").is_some()
                    || passthrough.get("top_logprobs").is_some())
            {
                record_rejection(
                    TransformPhase::RequestEncode,
                    TransformSemanticUnit::Metadata,
                    TransformReasonCode::UnsupportedContent,
                );
            }
        }
    }

    if let Some(responses) = request.responses_extension() {
        if responses.tool_choice.is_some() && target != UpstreamProtocol::Responses {
            record_rejection(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::ToolDefinitions,
                TransformReasonCode::UnsupportedContent,
            );
        }
        if responses.reasoning.is_some() && target != UpstreamProtocol::Responses {
            record_rejection(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::ReasoningContent,
                TransformReasonCode::UnsupportedReasoning,
            );
        }
        if responses.parallel_tool_calls.is_some() && target != UpstreamProtocol::Responses {
            record_rejection(
                TransformPhase::RequestEncode,
                TransformSemanticUnit::ToolDefinitions,
                TransformReasonCode::UnsupportedContent,
            );
        }
    }

    if target == UpstreamProtocol::Responses
        && (!request.messages.is_empty() || !request.items.is_empty())
    {
        record_synthesis(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::Metadata,
            TransformReasonCode::SyntheticCorrelationId,
        );
    }
}

pub(in crate::service::transform) fn audit_target_response(
    target: DownstreamProtocol,
    response: &UnifiedResponse,
) {
    let response_kinds = collect_response_kinds(response);
    for kind in response_kinds.iter().copied() {
        if kind == TransformValueKind::ToolResult && target != DownstreamProtocol::Responses {
            record_rejection(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::ToolResult,
                TransformReasonCode::UnsupportedContent,
            );
            continue;
        }
        if kind == TransformValueKind::ImageData && target == DownstreamProtocol::Anthropic {
            record_rejection(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::ImageData,
                TransformReasonCode::UnsupportedContent,
            );
            continue;
        }
        apply_transform_policy(
            TransformProtocol::Unified,
            TransformProtocol::Downstream(target),
            kind,
            "non-stream response adapter audit",
        );
    }

    let has_annotations = response.choices.iter().any(|choice| {
        choice.content_items().iter().any(|item| match item {
            UnifiedItem::Message(message) => !message.annotations.is_empty(),
            UnifiedItem::Reasoning(reasoning) => !reasoning.annotations.is_empty(),
            _ => false,
        })
    });
    if has_annotations
        && matches!(
            target,
            DownstreamProtocol::Openai | DownstreamProtocol::Anthropic
        )
    {
        record_rejection(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::Metadata,
            TransformReasonCode::UnsupportedContent,
        );
    }

    for choice in &response.choices {
        if choice.logprobs.is_some() && target != DownstreamProtocol::Openai {
            record_rejection(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::UnsupportedContent,
            );
        }
        if let Some(reason) = choice.finish_reason.as_deref() {
            let supported = match target {
                DownstreamProtocol::Openai => {
                    matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
                }
                DownstreamProtocol::Gemini | DownstreamProtocol::Anthropic => {
                    matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
                }
                DownstreamProtocol::Responses => reason == "stop",
            };
            if !supported {
                record_rejection(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::UnknownSemanticUnit,
                );
            }
        }
        if choice.content_items().is_empty() && choice.finish_reason.is_none() {
            record_no_semantic_output(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::ResponseEnvelope,
            );
        }
        for item in choice.content_items() {
            if let UnifiedItem::FileReference(file) = item
                && file.file_url.is_none()
                && target != DownstreamProtocol::Responses
            {
                record_rejection(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::FileUrl,
                    TransformReasonCode::UnsupportedContent,
                );
            }
        }
    }

    if response.choices.len() > 1 && target == DownstreamProtocol::Anthropic {
        record_rejection(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::ResponseEnvelope,
            TransformReasonCode::UnsupportedContent,
        );
    }
    if response.choices.len() > 1 && target == DownstreamProtocol::Responses {
        record_rejection(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::ResponseEnvelope,
            TransformReasonCode::UnsupportedContent,
        );
    }
    let gemini_blocked_response = target == DownstreamProtocol::Gemini
        && response
            .provider_response_metadata
            .as_ref()
            .and_then(|metadata| metadata.gemini.as_ref())
            .and_then(|metadata| metadata.prompt_feedback.as_ref())
            .is_some();
    if response.choices.is_empty() && !gemini_blocked_response {
        record_rejection(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::ResponseEnvelope,
            TransformReasonCode::NoSemanticOutput,
        );
    }

    if target != DownstreamProtocol::Gemini && response.model.as_deref().is_none_or(str::is_empty) {
        record_rejection(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::Model,
            TransformReasonCode::TargetEncodeFailed,
        );
    }

    match target {
        DownstreamProtocol::Openai => {
            if response.object.is_none() {
                record_synthesis(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::ResponseEnvelope,
                    TransformReasonCode::SyntheticEnvelope,
                );
            }
            if response.created.is_none() {
                record_synthesis(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Metadata,
                    TransformReasonCode::SyntheticEnvelope,
                );
            }
        }
        DownstreamProtocol::Responses => record_synthesis(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::ResponseEnvelope,
            TransformReasonCode::SyntheticEnvelope,
        ),
        DownstreamProtocol::Anthropic => {
            record_synthesis(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::ResponseEnvelope,
                TransformReasonCode::SyntheticEnvelope,
            );
            if response.usage.is_none() {
                record_synthesis(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Usage,
                    TransformReasonCode::SyntheticEnvelope,
                );
            }
            if response.created.is_some() || response.object.is_some() {
                record_minor_drop(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Metadata,
                );
            }
        }
        DownstreamProtocol::Gemini => {}
    }

    if target == DownstreamProtocol::Responses && !response.choices.is_empty() {
        record_synthesis(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::Metadata,
            TransformReasonCode::SyntheticCorrelationId,
        );
    }

    if target == DownstreamProtocol::Gemini {
        record_minor_drop(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::Metadata,
        );
        if response.model.is_some() {
            record_minor_drop(TransformPhase::ResponseEncode, TransformSemanticUnit::Model);
        }
    }

    if response.system_fingerprint.is_some() && target != DownstreamProtocol::Openai {
        record_minor_drop(
            TransformPhase::ResponseEncode,
            TransformSemanticUnit::Metadata,
        );
    }
    if let Some(metadata) = response.provider_response_metadata.as_ref() {
        if metadata.gemini.is_some() && target != DownstreamProtocol::Gemini {
            record_rejection(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::Metadata,
                TransformReasonCode::UnsupportedContent,
            );
        }
        if metadata.anthropic.is_some() && target != DownstreamProtocol::Anthropic {
            record_minor_drop(
                TransformPhase::ResponseEncode,
                TransformSemanticUnit::Metadata,
            );
        }
        if let Some(responses) = metadata.responses.as_ref()
            && target != DownstreamProtocol::Responses
        {
            let semantically_material = !responses.citations.is_empty()
                || !responses.refusals.is_empty()
                || !responses.files.is_empty()
                || responses.incomplete_details.is_some()
                || responses
                    .status
                    .as_deref()
                    .is_some_and(|status| status != "completed");
            if semantically_material {
                record_rejection(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Metadata,
                    TransformReasonCode::UnsupportedContent,
                );
            } else {
                record_minor_drop(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Metadata,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::service::transform::{
        TransformFailureOrigin, transform_request_data, transform_result,
    };

    fn has_action(
        summary: &super::super::TransformOutcomeSummary,
        action: TransformAction,
    ) -> bool {
        summary.facts.iter().any(|fact| fact.action == action)
    }

    fn has_reason(
        summary: &super::super::TransformOutcomeSummary,
        reason: TransformReasonCode,
    ) -> bool {
        summary.facts.iter().any(|fact| fact.reason_code == reason)
    }

    #[test]
    fn adapter_audit_openai_covers_all_non_stream_outcome_classes() {
        let lossless = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}]
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            false,
        )
        .expect("ordinary OpenAI text must convert");
        assert!(has_reason(
            &lossless.summary,
            TransformReasonCode::LosslessConversion
        ));

        let minor = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}],
                "user": "operator-tag"
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            false,
        )
        .expect("non-semantic OpenAI user metadata may be dropped");
        assert!(minor.summary.facts.iter().any(|fact| {
            fact.outcome == TransformOutcomeKind::ControlledLossMinor
                && fact.semantic_unit == TransformSemanticUnit::Metadata
        }));

        let major = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}],
                "response_format": {"type": "json_object"}
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            false,
        )
        .expect_err("unimplemented OpenAI structured output must reject");
        assert_eq!(major.origin, TransformFailureOrigin::TargetCapability);
        assert_eq!(major.semantic_unit, TransformSemanticUnit::StructuredOutput);

        let unknown = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "alien", "content": "hello"}]
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Gemini,
            false,
        )
        .expect_err("unknown OpenAI role must reject");
        assert_eq!(
            unknown.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );

        let synthesis = transform_request_data(
            json!({"contents": [{"parts": [{"text": "hello"}]}]}),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("the materializer-owned OpenAI model placeholder is legal");
        assert!(has_action(&synthesis.summary, TransformAction::Synthesize));
    }

    #[test]
    fn adapter_audit_responses_covers_all_non_stream_outcome_classes() {
        let lossless = transform_request_data(
            json!({"model": "gpt-4.1", "input": "hello"}),
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("ordinary Responses text must convert");
        assert_eq!(lossless.value["messages"][0]["content"], "hello");

        let minor = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}],
                "user": "operator-tag"
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Responses,
            false,
        )
        .expect("non-semantic metadata may be dropped for Responses");
        assert!(
            minor
                .summary
                .facts
                .iter()
                .any(|fact| { fact.outcome == TransformOutcomeKind::ControlledLossMinor })
        );

        let major = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "input": "hello",
                "text": {"format": {"type": "json_object"}}
            }),
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            false,
        )
        .expect_err("Responses structured output cannot be silently dropped by OpenAI");
        assert_eq!(major.origin, TransformFailureOrigin::TargetCapability);

        let unknown = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "input": [{"type": "future_item", "payload": "secret"}]
            }),
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            false,
        )
        .expect_err("unknown Responses items must reject");
        assert_eq!(
            unknown.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );

        let synthesis = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "input": [{"role": "user", "content": "hello"}]
            }),
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("Responses shorthand message IDs may be synthesized");
        assert!(has_reason(
            &synthesis.summary,
            TransformReasonCode::SyntheticCorrelationId
        ));
    }

    #[test]
    fn adapter_audit_anthropic_covers_all_non_stream_outcome_classes() {
        let lossless = transform_request_data(
            json!({
                "model": "claude-sonnet",
                "max_tokens": 64,
                "messages": [{
                    "role": "user",
                    "content": [{
                        "type": "image",
                        "source": {"type": "base64", "media_type": "image/png", "data": "aGVsbG8="}
                    }]
                }]
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Gemini,
            false,
        )
        .expect("Anthropic base64 image must survive as Gemini inlineData");
        assert_eq!(
            lossless.value["contents"][0]["parts"][0]["inlineData"]["data"],
            "aGVsbG8="
        );

        let minor = transform_request_data(
            json!({
                "model": "claude-sonnet",
                "max_tokens": 64,
                "metadata": {"user_id": "opaque"},
                "messages": [{"role": "user", "content": "hello"}]
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("Anthropic metadata is an explicit minor drop");
        assert!(minor.summary.facts.iter().any(|fact| {
            fact.outcome == TransformOutcomeKind::ControlledLossMinor
                && fact.semantic_unit == TransformSemanticUnit::Metadata
        }));

        let major = transform_request_data(
            json!({
                "model": "claude-sonnet",
                "max_tokens": 64,
                "messages": [{
                    "role": "user",
                    "content": [{"type": "tool_result", "tool_use_id": "toolu_1", "content": "bad", "is_error": true}]
                }]
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Responses,
            false,
        )
        .expect_err("Anthropic tool error semantics cannot be erased");
        assert_eq!(major.semantic_unit, TransformSemanticUnit::ToolResult);

        let unknown = transform_request_data(
            json!({
                "model": "claude-sonnet",
                "max_tokens": 64,
                "messages": [{"role": "system", "content": "hello"}]
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Openai,
            false,
        )
        .expect_err("Anthropic has no system message role");
        assert_eq!(
            unknown.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );

        let synthesis = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}]
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Anthropic,
            false,
        )
        .expect("Anthropic max_tokens has a documented adapter default");
        assert_eq!(synthesis.value["max_tokens"], 4096);
        assert!(has_action(&synthesis.summary, TransformAction::Synthesize));
    }

    #[test]
    fn adapter_audit_gemini_covers_all_non_stream_outcome_classes() {
        let lossless = transform_request_data(
            json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("ordinary Gemini text must convert");
        assert_eq!(lossless.value["messages"][0]["content"], "hello");

        let minor = transform_result(
            json!({
                "id": "chatcmpl_1",
                "object": "chat.completion",
                "created": 1,
                "model": "gpt-4.1",
                "choices": [{
                    "index": 0,
                    "message": {"role": "assistant", "content": "hello"},
                    "finish_reason": "stop"
                }]
            }),
            UpstreamProtocol::Openai,
            DownstreamProtocol::Gemini,
        )
        .expect("Gemini response correlation metadata is an explicit minor drop");
        assert!(minor.summary.facts.iter().any(|fact| {
            fact.outcome == TransformOutcomeKind::ControlledLossMinor
                && fact.semantic_unit == TransformSemanticUnit::Metadata
        }));

        let major = transform_request_data(
            json!({
                "contents": [{"role": "user", "parts": [{"text": "hello"}]}],
                "safetySettings": [{"category": "HARM_CATEGORY_HATE_SPEECH", "threshold": "BLOCK_LOW_AND_ABOVE"}]
            }),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            false,
        )
        .expect_err("Gemini safety policy cannot be silently discarded");
        assert_eq!(major.semantic_unit, TransformSemanticUnit::Metadata);

        let unknown = transform_request_data(
            json!({"contents": [{"role": "alien", "parts": [{"text": "hello"}]}]}),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            false,
        )
        .expect_err("unknown Gemini role must reject");
        assert_eq!(
            unknown.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );

        let synthesis = transform_request_data(
            json!({"contents": [{"parts": [{"text": "hello"}]}]}),
            DownstreamProtocol::Gemini,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("missing Gemini role and body model have legal synthesis contracts");
        assert!(has_action(&synthesis.summary, TransformAction::Synthesize));
    }

    #[test]
    fn adapter_audit_ollama_covers_all_non_stream_outcome_classes() {
        let lossless = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}]
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Ollama,
            false,
        )
        .expect("ordinary text must convert to Ollama");
        assert_eq!(lossless.value["messages"][0]["content"], "hello");

        let minor = transform_request_data(
            json!({
                "model": "claude-sonnet",
                "max_tokens": 64,
                "top_k": 16,
                "messages": [{"role": "user", "content": "hello"}]
            }),
            DownstreamProtocol::Anthropic,
            UpstreamProtocol::Ollama,
            false,
        )
        .expect("unsupported top_k is an explicit minor drop");
        assert!(has_reason(
            &minor.summary,
            TransformReasonCode::UnsupportedTopK
        ));

        let major = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "messages": [{"role": "user", "content": "hello"}],
                "tools": [{
                    "type": "function",
                    "function": {"name": "lookup", "parameters": {"type": "object"}}
                }]
            }),
            DownstreamProtocol::Openai,
            UpstreamProtocol::Ollama,
            false,
        )
        .expect_err("unimplemented Ollama tool definitions must reject");
        assert_eq!(
            major.reason_code,
            TransformReasonCode::UnsupportedToolDefinitions
        );

        let unknown = transform_result(
            json!({
                "model": "llama3",
                "created_at": "2026-08-11T12:00:00Z",
                "message": {"role": "assistant", "content": "hello", "thinking": "private"},
                "done": true,
                "done_reason": "stop"
            }),
            UpstreamProtocol::Ollama,
            DownstreamProtocol::Openai,
        )
        .expect_err("unsupported Ollama thinking must reject instead of disappearing");
        assert_eq!(
            unknown.reason_code,
            TransformReasonCode::UnknownSemanticUnit
        );

        let synthesis = transform_result(
            json!({
                "model": "llama3",
                "created_at": "2026-08-11T12:00:00Z",
                "message": {"role": "assistant", "content": "hello"},
                "done": true,
                "done_reason": "stop"
            }),
            UpstreamProtocol::Ollama,
            DownstreamProtocol::Openai,
        )
        .expect("Ollama response ID/index/envelope synthesis is legal and explicit");
        assert!(has_reason(
            &synthesis.summary,
            TransformReasonCode::SyntheticCorrelationId
        ));
    }

    #[test]
    fn adapter_audit_rejects_illegal_non_stream_tool_arguments_for_each_source_wire() {
        let cases = [
            (
                DownstreamProtocol::Openai,
                UpstreamProtocol::Gemini,
                json!({
                    "model": "gpt-4.1",
                    "messages": [{
                        "role": "assistant",
                        "tool_calls": [{
                            "id": "call_1",
                            "type": "function",
                            "function": {"name": "lookup", "arguments": "{bad"}
                        }]
                    }]
                }),
            ),
            (
                DownstreamProtocol::Responses,
                UpstreamProtocol::Openai,
                json!({
                    "model": "gpt-4.1",
                    "input": [{
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "lookup",
                        "arguments": "{bad"
                    }]
                }),
            ),
            (
                DownstreamProtocol::Anthropic,
                UpstreamProtocol::Openai,
                json!({
                    "model": "claude-sonnet",
                    "max_tokens": 64,
                    "messages": [{
                        "role": "assistant",
                        "content": [{
                            "type": "tool_use",
                            "id": "toolu_1",
                            "name": "lookup",
                            "input": "not-an-object"
                        }]
                    }]
                }),
            ),
            (
                DownstreamProtocol::Gemini,
                UpstreamProtocol::Openai,
                json!({
                    "contents": [{
                        "role": "model",
                        "parts": [{
                            "functionCall": {"name": "lookup", "args": "not-an-object"}
                        }]
                    }]
                }),
            ),
        ];

        for (protocol, target, body) in cases {
            let failure = transform_request_data(body, protocol, target, false).expect_err(
                "illegal complete tool arguments must never default to an empty object",
            );
            assert_eq!(failure.origin, TransformFailureOrigin::DownstreamInput);
            assert_eq!(failure.semantic_unit, TransformSemanticUnit::ToolCall);
            assert_eq!(
                failure.reason_code,
                TransformReasonCode::InvalidProtocolShape
            );
        }
    }

    #[test]
    fn adapter_audit_rejects_reasoning_when_the_target_has_no_lossless_contract() {
        let failure = transform_result(
            json!({
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "content": [{"type": "thinking", "thinking": "private", "signature": "sig"}],
                "model": "claude-sonnet",
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }),
            UpstreamProtocol::Anthropic,
            DownstreamProtocol::Openai,
        )
        .expect_err("Anthropic reasoning cannot be flattened into OpenAI answer text");

        assert_eq!(failure.origin, TransformFailureOrigin::TargetCapability);
        assert_eq!(
            failure.semantic_unit,
            TransformSemanticUnit::ReasoningContent
        );
        assert_eq!(
            failure.reason_code,
            TransformReasonCode::UnsupportedReasoning
        );
    }
}
