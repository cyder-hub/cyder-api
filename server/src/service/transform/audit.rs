use serde_json::Value;
use std::collections::VecDeque;

use super::capability::TransformValueKind;
use super::diagnostics::record_captured_transform_fact;
use super::media::{
    InlineMediaKind, classify_inline_mime, is_valid_base64, is_valid_http_url,
    is_valid_image_reference, mime_type_from_filename, parse_base64_data_url,
};
use super::structured::{is_valid_schema_name, schema_contains_property_ordering};
use super::unified::{
    UnifiedContentPart, UnifiedItem, UnifiedRequest, UnifiedResponse, UnifiedRole,
    UnifiedToolChoice, UnifiedToolResultOutput,
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

fn record_minor_reasoning_drop(phase: TransformPhase) {
    record_fact(
        phase,
        TransformSemanticUnit::ReasoningContent,
        TransformOutcomeKind::ControlledLossMinor,
        TransformAction::Drop,
        TransformReasonCode::UnsupportedReasoning,
    );
}

fn record_minor_reasoning_mapping(phase: TransformPhase) {
    record_fact(
        phase,
        TransformSemanticUnit::ReasoningContent,
        TransformOutcomeKind::ControlledLossMinor,
        TransformAction::Synthesize,
        TransformReasonCode::UnsupportedReasoning,
    );
}

fn record_minor_mapping(
    phase: TransformPhase,
    semantic_unit: TransformSemanticUnit,
    reason_code: TransformReasonCode,
) {
    record_fact(
        phase,
        semantic_unit,
        TransformOutcomeKind::ControlledLossMinor,
        TransformAction::Synthesize,
        reason_code,
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

fn stable_tool_call_id(
    protocol: &str,
    message_index: usize,
    call_index: usize,
    name: &str,
) -> String {
    let normalized = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    format!("call-{protocol}-{message_index}-{call_index}-{normalized}")
}

fn consume_pending_tool_call_id(pending: &mut VecDeque<String>, explicit_id: Option<&str>) {
    let Some(explicit_id) = explicit_id.filter(|id| !id.is_empty()) else {
        return;
    };
    if let Some(index) = pending.iter().position(|id| id == explicit_id) {
        pending.remove(index);
    }
}

fn validate_portable_function_definition(
    definition: &Value,
    name_field: &str,
    parameters_field: &str,
) -> Result<(), SourceSemanticError> {
    if !definition
        .get(name_field)
        .and_then(Value::as_str)
        .is_some_and(|name| !name.trim().is_empty())
        || !definition
            .get(parameters_field)
            .is_some_and(Value::is_object)
        || definition
            .get("description")
            .is_some_and(|description| !description.is_null() && !description.is_string())
        || definition
            .get("strict")
            .is_some_and(|strict| !strict.is_null() && !strict.is_boolean())
    {
        return Err(SourceSemanticError::invalid(
            TransformSemanticUnit::ToolDefinitions,
        ));
    }
    Ok(())
}

fn source_forces_tool_selection(protocol: DownstreamProtocol, data: &Value) -> bool {
    match protocol {
        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
            match data.get("tool_choice") {
                Some(Value::String(value)) => value == "required",
                Some(Value::Object(_)) => true,
                _ => false,
            }
        }
        DownstreamProtocol::Anthropic => data
            .get("tool_choice")
            .and_then(|choice| choice.get("type"))
            .and_then(Value::as_str)
            .is_some_and(|choice| matches!(choice, "any" | "tool")),
        DownstreamProtocol::Gemini => {
            let config = data
                .get("toolConfig")
                .and_then(|config| config.get("functionCallingConfig"));
            config
                .and_then(|config| config.get("mode"))
                .and_then(Value::as_str)
                .is_some_and(|mode| mode == "ANY")
                || config
                    .and_then(|config| config.get("allowedFunctionNames"))
                    .and_then(Value::as_array)
                    .is_some_and(|names| !names.is_empty())
        }
    }
}

fn normalize_openai_tool_call_ids(data: &mut Value) {
    let Some(messages) = data.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut pending = VecDeque::new();
    for (message_index, message) in messages.iter_mut().enumerate() {
        if let Some(calls) = message.get_mut("tool_calls").and_then(Value::as_array_mut) {
            for (call_index, call) in calls.iter_mut().enumerate() {
                let name = call
                    .get("function")
                    .and_then(|function| function.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("function")
                    .to_string();
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| {
                        let id = stable_tool_call_id("openai", message_index, call_index, &name);
                        call.as_object_mut()
                            .expect("OpenAI tool call must be an object")
                            .insert("id".to_string(), Value::String(id.clone()));
                        record_synthesis(
                            TransformPhase::RequestDecode,
                            TransformSemanticUnit::ToolCall,
                            TransformReasonCode::SyntheticCorrelationId,
                        );
                        id
                    });
                pending.push_back(id);
            }
        }
        if message.get("role").and_then(Value::as_str) == Some("tool")
            && message
                .get("tool_call_id")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            let id = pending.pop_front().unwrap_or_else(|| {
                stable_tool_call_id("openai-result", message_index, 0, "function")
            });
            message
                .as_object_mut()
                .expect("OpenAI message must be an object")
                .insert("tool_call_id".to_string(), Value::String(id));
            record_synthesis(
                TransformPhase::RequestDecode,
                TransformSemanticUnit::ToolResult,
                TransformReasonCode::SyntheticCorrelationId,
            );
        } else if message.get("role").and_then(Value::as_str) == Some("tool") {
            consume_pending_tool_call_id(
                &mut pending,
                message.get("tool_call_id").and_then(Value::as_str),
            );
        }
    }
}

fn normalize_responses_tool_call_ids(data: &mut Value) {
    let Some(items) = data.get_mut("input").and_then(Value::as_array_mut) else {
        return;
    };
    let mut pending = VecDeque::new();
    for (item_index, item) in items.iter_mut().enumerate() {
        let type_name = item.get("type").and_then(Value::as_str);
        match type_name {
            Some("function_call") => {
                let name = item
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("function")
                    .to_string();
                let call_id = item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .filter(|id| !id.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| {
                        let id = stable_tool_call_id("responses", item_index, 0, &name);
                        item.as_object_mut()
                            .expect("Responses input item must be an object")
                            .insert("call_id".to_string(), Value::String(id.clone()));
                        record_synthesis(
                            TransformPhase::RequestDecode,
                            TransformSemanticUnit::ToolCall,
                            TransformReasonCode::SyntheticCorrelationId,
                        );
                        id
                    });
                pending.push_back(call_id);
                if item.get("id").is_none() {
                    item.as_object_mut()
                        .expect("Responses input item must be an object")
                        .insert("id".to_string(), Value::String(format!("fc-{item_index}")));
                }
            }
            Some("function_call_output") => {
                if item
                    .get("call_id")
                    .and_then(Value::as_str)
                    .is_none_or(str::is_empty)
                {
                    let id = pending.pop_front().unwrap_or_else(|| {
                        stable_tool_call_id("responses-output", item_index, 0, "function")
                    });
                    item.as_object_mut()
                        .expect("Responses input item must be an object")
                        .insert("call_id".to_string(), Value::String(id));
                    record_synthesis(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::ToolResult,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                } else {
                    consume_pending_tool_call_id(
                        &mut pending,
                        item.get("call_id").and_then(Value::as_str),
                    );
                }
                if item.get("id").is_none() {
                    item.as_object_mut()
                        .expect("Responses input item must be an object")
                        .insert("id".to_string(), Value::String(format!("fco-{item_index}")));
                }
            }
            _ => {}
        }
    }
}

pub(in crate::service::transform) fn normalize_same_wire_responses_tool_call_ids(data: &mut Value) {
    normalize_responses_tool_call_ids(data);
}

fn normalize_anthropic_tool_call_ids(data: &mut Value) {
    let Some(messages) = data.get_mut("messages").and_then(Value::as_array_mut) else {
        return;
    };
    let mut pending = VecDeque::new();
    for (message_index, message) in messages.iter_mut().enumerate() {
        let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) else {
            continue;
        };
        for (block_index, block) in blocks.iter_mut().enumerate() {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("function")
                        .to_string();
                    let id = block
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|id| !id.is_empty())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| {
                            let id =
                                stable_tool_call_id("anthropic", message_index, block_index, &name);
                            block
                                .as_object_mut()
                                .expect("Anthropic content block must be an object")
                                .insert("id".to_string(), Value::String(id.clone()));
                            record_synthesis(
                                TransformPhase::RequestDecode,
                                TransformSemanticUnit::ToolCall,
                                TransformReasonCode::SyntheticCorrelationId,
                            );
                            id
                        });
                    pending.push_back(id);
                }
                Some("tool_result")
                    if block
                        .get("tool_use_id")
                        .and_then(Value::as_str)
                        .is_none_or(str::is_empty) =>
                {
                    let id = pending.pop_front().unwrap_or_else(|| {
                        stable_tool_call_id(
                            "anthropic-result",
                            message_index,
                            block_index,
                            "function",
                        )
                    });
                    block
                        .as_object_mut()
                        .expect("Anthropic content block must be an object")
                        .insert("tool_use_id".to_string(), Value::String(id));
                    record_synthesis(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::ToolResult,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                Some("tool_result") => consume_pending_tool_call_id(
                    &mut pending,
                    block.get("tool_use_id").and_then(Value::as_str),
                ),
                _ => {}
            }
        }
    }
}

pub(in crate::service::transform) fn normalize_portable_tool_request(
    protocol: DownstreamProtocol,
    data: &mut Value,
) -> Result<(), SourceSemanticError> {
    let forced = source_forces_tool_selection(protocol, data);
    let mut dropped = false;
    if let Some(tools) = data.get_mut("tools").and_then(Value::as_array_mut) {
        match protocol {
            DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                tools.retain(|tool| {
                    let portable = tool.get("type").and_then(Value::as_str) == Some("function");
                    dropped |= !portable;
                    portable
                });
            }
            DownstreamProtocol::Anthropic => {
                tools.retain(|tool| {
                    let portable = tool.get("type").is_none()
                        && tool.get("name").and_then(Value::as_str).is_some()
                        && tool.get("input_schema").is_some();
                    dropped |= !portable;
                    portable
                });
            }
            DownstreamProtocol::Gemini => {
                let mut declarations = Vec::new();
                for tool in std::mem::take(tools) {
                    if let Some(functions) =
                        tool.get("functionDeclarations").and_then(Value::as_array)
                    {
                        declarations.extend(functions.iter().cloned());
                    }
                    dropped |= tool.as_object().is_none_or(|object| {
                        object.len() != 1 || !object.contains_key("functionDeclarations")
                    });
                }
                if !declarations.is_empty() {
                    tools.push(serde_json::json!({ "functionDeclarations": declarations }));
                }
            }
        }
    }
    if data
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        data.as_object_mut()
            .expect("request envelope must be an object")
            .remove("tools");
    }
    if dropped && forced {
        return Err(SourceSemanticError {
            semantic_unit: TransformSemanticUnit::ToolDefinitions,
            reason_code: TransformReasonCode::UnsupportedToolDefinitions,
        });
    }
    if dropped {
        record_minor_drop(
            TransformPhase::RequestDecode,
            TransformSemanticUnit::ToolDefinitions,
        );
    }
    match protocol {
        DownstreamProtocol::Openai => normalize_openai_tool_call_ids(data),
        DownstreamProtocol::Responses => normalize_responses_tool_call_ids(data),
        DownstreamProtocol::Anthropic => normalize_anthropic_tool_call_ids(data),
        DownstreamProtocol::Gemini => {}
    }
    Ok(())
}

fn validate_named_json_schema(
    definition: Option<&serde_json::Map<String, Value>>,
) -> Result<(), SourceSemanticError> {
    let Some(definition) = definition else {
        return Err(SourceSemanticError::invalid(
            TransformSemanticUnit::StructuredOutput,
        ));
    };
    if !definition
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(is_valid_schema_name)
        || !definition.get("schema").is_some_and(Value::is_object)
        || definition
            .get("description")
            .is_some_and(|description| !description.is_null() && !description.is_string())
        || definition
            .get("strict")
            .is_some_and(|strict| !strict.is_null() && !strict.is_boolean())
    {
        return Err(SourceSemanticError::invalid(
            TransformSemanticUnit::StructuredOutput,
        ));
    }
    Ok(())
}

fn validate_openai_response_format(format: &Value) -> Result<(), SourceSemanticError> {
    let Some(format) = format.as_object() else {
        return Err(SourceSemanticError::invalid(
            TransformSemanticUnit::StructuredOutput,
        ));
    };
    match format.get("type").and_then(Value::as_str) {
        Some("text" | "json_object") => Ok(()),
        Some("json_schema") => {
            validate_named_json_schema(format.get("json_schema").and_then(Value::as_object))
        }
        Some(_) => Err(SourceSemanticError {
            semantic_unit: TransformSemanticUnit::StructuredOutput,
            reason_code: TransformReasonCode::UnsupportedContent,
        }),
        None => Err(SourceSemanticError::invalid(
            TransformSemanticUnit::StructuredOutput,
        )),
    }
}

fn validate_responses_text_format(format: &Value) -> Result<(), SourceSemanticError> {
    let Some(format) = format.as_object() else {
        return Err(SourceSemanticError::invalid(
            TransformSemanticUnit::StructuredOutput,
        ));
    };
    match format.get("type").and_then(Value::as_str) {
        Some("text" | "json_object") => Ok(()),
        Some("json_schema") => validate_named_json_schema(Some(format)),
        Some(_) => Err(SourceSemanticError {
            semantic_unit: TransformSemanticUnit::StructuredOutput,
            reason_code: TransformReasonCode::UnsupportedContent,
        }),
        None => Err(SourceSemanticError::invalid(
            TransformSemanticUnit::StructuredOutput,
        )),
    }
}

fn record_structured_name_synthesis() {
    record_minor_mapping(
        TransformPhase::RequestDecode,
        TransformSemanticUnit::StructuredOutput,
        TransformReasonCode::SyntheticEnvelope,
    );
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

        if let Some(parts) = message.get("content").and_then(Value::as_array) {
            for part in parts {
                let kind = part.get("type").and_then(Value::as_str);
                if role != "user" && !matches!(kind, Some("text")) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::RequestEnvelope,
                    ));
                }
                match kind {
                    Some("text") if part.get("text").and_then(Value::as_str).is_some() => {}
                    Some("image_url") => {
                        let image_url = part.get("image_url").and_then(Value::as_object);
                        if !image_url
                            .and_then(|image| image.get("url"))
                            .and_then(Value::as_str)
                            .is_some_and(is_valid_image_reference)
                            || image_url.and_then(|image| image.get("detail")).is_some_and(
                                |detail| {
                                    !detail.is_null()
                                        && !matches!(detail.as_str(), Some("auto" | "low" | "high"))
                                },
                            )
                        {
                            return Err(SourceSemanticError::invalid(
                                TransformSemanticUnit::ImageUrl,
                            ));
                        }
                    }
                    Some("input_audio") => {
                        let input_audio = part.get("input_audio").and_then(Value::as_object);
                        if !input_audio
                            .and_then(|audio| audio.get("data"))
                            .and_then(Value::as_str)
                            .is_some_and(is_valid_base64)
                            || !matches!(
                                input_audio
                                    .and_then(|audio| audio.get("format"))
                                    .and_then(Value::as_str),
                                Some("wav" | "mp3")
                            )
                        {
                            return Err(SourceSemanticError::invalid(
                                TransformSemanticUnit::AudioData,
                            ));
                        }
                    }
                    Some("file") => {
                        let Some(file) = part.get("file").and_then(Value::as_object) else {
                            return Err(SourceSemanticError::invalid(
                                TransformSemanticUnit::FileData,
                            ));
                        };
                        let file_data = file.get("file_data").filter(|value| !value.is_null());
                        let file_id = file.get("file_id").filter(|value| !value.is_null());
                        if (file_data.is_some() == file_id.is_some())
                            || file_data.is_some_and(|data| {
                                !data.as_str().is_some_and(is_valid_base64)
                                    || !file
                                        .get("filename")
                                        .and_then(Value::as_str)
                                        .is_some_and(|filename| !filename.trim().is_empty())
                                    || !file
                                        .get("filename")
                                        .and_then(Value::as_str)
                                        .and_then(mime_type_from_filename)
                                        .is_some_and(|mime_type| {
                                            classify_inline_mime(mime_type)
                                                == Some(InlineMediaKind::File)
                                        })
                            })
                            || file_id.is_some_and(|id| {
                                !id.as_str().is_some_and(|id| !id.trim().is_empty())
                            })
                        {
                            return Err(SourceSemanticError::invalid(
                                TransformSemanticUnit::FileData,
                            ));
                        }
                    }
                    _ => {
                        return Err(SourceSemanticError::unknown(
                            TransformSemanticUnit::RequestEnvelope,
                        ));
                    }
                }
            }
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
            let function = call
                .get("function")
                .ok_or_else(|| SourceSemanticError::invalid(TransformSemanticUnit::ToolCall))?;
            require_non_empty_string(function, "name", TransformSemanticUnit::ToolCall)?;
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
    let status_supported = |status: &str| {
        status == "completed" || (phase == TransformPhase::ResponseDecode && status == "incomplete")
    };
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
                    .is_some_and(|status| !status_supported(status))
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnsupportedContent,
                    });
                }
                for part in object_array(item, "content") {
                    let role = item.get("role").and_then(Value::as_str);
                    let is_media = matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("input_image" | "input_audio" | "input_file")
                    );
                    if is_media && role != Some("user") {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::Role,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    match part.get("type").and_then(Value::as_str) {
                        Some("input_image") => {
                            let image_url = part.get("image_url").filter(|value| !value.is_null());
                            let file_id = part.get("file_id").filter(|value| !value.is_null());
                            if image_url.is_some() == file_id.is_some()
                                || image_url.is_some_and(|url| {
                                    !url.as_str().is_some_and(is_valid_image_reference)
                                })
                                || file_id.is_some_and(|id| {
                                    !id.as_str().is_some_and(|id| !id.trim().is_empty())
                                })
                                || part.get("detail").is_some_and(|detail| {
                                    !detail.is_null()
                                        && !matches!(detail.as_str(), Some("auto" | "low" | "high"))
                                })
                            {
                                return Err(SourceSemanticError::invalid(
                                    TransformSemanticUnit::ImageUrl,
                                ));
                            }
                            if file_id.is_some() {
                                record_minor_mapping(
                                    phase,
                                    TransformSemanticUnit::FileId,
                                    TransformReasonCode::UnsupportedContent,
                                );
                            }
                        }
                        Some("input_audio") => {
                            let input_audio = part.get("input_audio").and_then(Value::as_object);
                            if !input_audio
                                .and_then(|audio| audio.get("data"))
                                .and_then(Value::as_str)
                                .is_some_and(is_valid_base64)
                                || !matches!(
                                    input_audio
                                        .and_then(|audio| audio.get("format"))
                                        .and_then(Value::as_str),
                                    Some("wav" | "mp3")
                                )
                            {
                                return Err(SourceSemanticError::invalid(
                                    TransformSemanticUnit::AudioData,
                                ));
                            }
                        }
                        Some("input_file") => {
                            let present = ["file_url", "file_id", "file_data"]
                                .into_iter()
                                .filter(|field| {
                                    part.get(*field).is_some_and(|value| !value.is_null())
                                })
                                .collect::<Vec<_>>();
                            if present.len() != 1 {
                                return Err(SourceSemanticError::invalid(
                                    TransformSemanticUnit::FileData,
                                ));
                            }
                            match present[0] {
                                "file_url" => {
                                    return Err(SourceSemanticError {
                                        semantic_unit: TransformSemanticUnit::FileUrl,
                                        reason_code: TransformReasonCode::UnsupportedContent,
                                    });
                                }
                                "file_id"
                                    if !part
                                        .get("file_id")
                                        .and_then(Value::as_str)
                                        .is_some_and(|id| !id.trim().is_empty()) =>
                                {
                                    return Err(SourceSemanticError::invalid(
                                        TransformSemanticUnit::FileId,
                                    ));
                                }
                                "file_data" => {
                                    let filename = part
                                        .get("filename")
                                        .and_then(Value::as_str)
                                        .filter(|filename| !filename.trim().is_empty());
                                    let Some(file_data) =
                                        part.get("file_data").and_then(Value::as_str)
                                    else {
                                        return Err(SourceSemanticError::invalid(
                                            TransformSemanticUnit::FileData,
                                        ));
                                    };
                                    let valid =
                                        if let Some(data_url) = parse_base64_data_url(file_data) {
                                            classify_inline_mime(data_url.mime_type)
                                                == Some(InlineMediaKind::File)
                                        } else {
                                            filename.and_then(mime_type_from_filename).is_some_and(
                                                |mime_type| {
                                                    classify_inline_mime(mime_type)
                                                        == Some(InlineMediaKind::File)
                                                },
                                            ) && is_valid_base64(file_data)
                                        };
                                    if filename.is_none() || !valid {
                                        return Err(SourceSemanticError::invalid(
                                            TransformSemanticUnit::FileData,
                                        ));
                                    }
                                }
                                _ => {}
                            }
                        }
                        Some(
                            "input_text" | "output_text" | "text" | "summary_text"
                            | "reasoning_text" | "refusal",
                        ) => {}
                        _ => {
                            return Err(SourceSemanticError::unknown(
                                TransformSemanticUnit::ResponsesUnknownItem,
                            ));
                        }
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
                    .is_some_and(|status| !status_supported(status))
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnsupportedContent,
                    });
                }
                require_non_empty_string(item, "name", TransformSemanticUnit::ToolCall)?;
                require_non_empty_string(item, "call_id", TransformSemanticUnit::ToolCall)?;
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
                    .is_some_and(|status| !status_supported(status))
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnsupportedContent,
                    });
                }
                require_non_empty_string(item, "call_id", TransformSemanticUnit::ToolResult)?;
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
                            source
                                .get("media_type")
                                .and_then(Value::as_str)
                                .is_some_and(|mime_type| {
                                    classify_inline_mime(mime_type) == Some(InlineMediaKind::Image)
                                })
                                && source
                                    .get("data")
                                    .and_then(Value::as_str)
                                    .is_some_and(is_valid_base64)
                        }
                        Some("url") => source
                            .get("url")
                            .and_then(Value::as_str)
                            .is_some_and(is_valid_image_reference),
                        _ => false,
                    };
                    if !required_fields_present {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::ImageData,
                        ));
                    }
                    if block.get("cache_control").is_some() {
                        record_minor_drop(
                            TransformPhase::RequestDecode,
                            TransformSemanticUnit::Metadata,
                        );
                    }
                }
                Some("document") => {
                    let source = block.get("source").ok_or_else(|| {
                        SourceSemanticError::invalid(TransformSemanticUnit::FileData)
                    })?;
                    if source.get("type").and_then(Value::as_str) != Some("base64")
                        || source.get("media_type").and_then(Value::as_str).is_none_or(
                            |mime_type| {
                                classify_inline_mime(mime_type) != Some(InlineMediaKind::File)
                            },
                        )
                        || !source
                            .get("data")
                            .and_then(Value::as_str)
                            .is_some_and(is_valid_base64)
                    {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::FileData,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    if block
                        .get("title")
                        .is_some_and(|title| !title.is_string() && !title.is_null())
                    {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::Metadata,
                        ));
                    }
                    if !block
                        .get("title")
                        .and_then(Value::as_str)
                        .is_some_and(|title| !title.trim().is_empty())
                    {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::FileData,
                        ));
                    }
                    if block.get("context").is_some() || block.get("citations").is_some() {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::FileData,
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
                Some("tool_use") if role == "assistant" => {
                    require_non_empty_string(block, "id", TransformSemanticUnit::ToolCall)?;
                    require_non_empty_string(block, "name", TransformSemanticUnit::ToolCall)?;
                    if !block.get("input").is_some_and(Value::is_object) {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::ToolCall,
                        ));
                    }
                }
                Some("tool_result") if role == "user" => {
                    require_non_empty_string(
                        block,
                        "tool_use_id",
                        TransformSemanticUnit::ToolResult,
                    )?;
                    if block
                        .get("is_error")
                        .is_some_and(|value| !value.is_boolean() && !value.is_null())
                    {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::ToolResult,
                        ));
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
        if part.get("videoMetadata").is_some() {
            return Err(SourceSemanticError {
                semantic_unit: TransformSemanticUnit::ImageData,
                reason_code: TransformReasonCode::UnsupportedContent,
            });
        }
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
        if part
            .get("thought")
            .is_some_and(|thought| !thought.is_boolean() && !thought.is_null())
        {
            return Err(SourceSemanticError::invalid(
                TransformSemanticUnit::ReasoningContent,
            ));
        }
        if part
            .get("thoughtSignature")
            .is_some_and(|signature| !signature.is_string() && !signature.is_null())
        {
            return Err(SourceSemanticError::invalid(
                TransformSemanticUnit::Metadata,
            ));
        }
        if let Some(call) = part.get("functionCall") {
            require_non_empty_string(call, "name", TransformSemanticUnit::ToolCall)?;
            if !call.get("args").is_some_and(Value::is_object) {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::ToolCall,
                ));
            }
        }
        if let Some(result) = part.get("functionResponse") {
            require_non_empty_string(result, "name", TransformSemanticUnit::ToolResult)?;
            if result.get("response").is_none() {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::ToolResult,
                ));
            }
        }
        if part.get("executableCode").is_some() {
            return Err(SourceSemanticError {
                semantic_unit: TransformSemanticUnit::ExecutableCode,
                reason_code: TransformReasonCode::UnsupportedContent,
            });
        }
        if let Some(inline_data) = part.get("inlineData") {
            let Some(inline_data) = inline_data.as_object() else {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::FileData,
                ));
            };
            let Some(mime_type) = inline_data.get("mimeType").and_then(Value::as_str) else {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::FileData,
                ));
            };
            let Some(kind) = classify_inline_mime(mime_type) else {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::FileData,
                    reason_code: TransformReasonCode::UnsupportedContent,
                });
            };
            if kind == InlineMediaKind::Video
                || !inline_data
                    .get("data")
                    .and_then(Value::as_str)
                    .is_some_and(is_valid_base64)
            {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::FileData,
                    reason_code: TransformReasonCode::UnsupportedContent,
                });
            }
            if inline_data
                .get("displayName")
                .is_some_and(|name| !name.is_string() && !name.is_null())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::Metadata,
                ));
            }
            let display_name = inline_data
                .get("displayName")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty());
            if matches!(kind, InlineMediaKind::Image | InlineMediaKind::Audio(_))
                && display_name.is_some()
            {
                record_minor_drop(phase, TransformSemanticUnit::Metadata);
            }
            if kind == InlineMediaKind::File && display_name.is_none() {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::FileData,
                ));
            }
        }
        if let Some(file_data) = part.get("fileData") {
            let Some(file_data) = file_data.as_object() else {
                return Err(SourceSemanticError::invalid(TransformSemanticUnit::FileUrl));
            };
            let mime_type = file_data.get("mimeType").and_then(Value::as_str);
            let file_uri = file_data.get("fileUri").and_then(Value::as_str);
            let public_image_url = mime_type.is_some_and(|mime_type| {
                classify_inline_mime(mime_type) == Some(InlineMediaKind::Image)
            }) && file_uri.is_some_and(|uri| {
                is_valid_http_url(uri)
                    && reqwest::Url::parse(uri).is_ok_and(|url| {
                        url.host_str() != Some("generativelanguage.googleapis.com")
                    })
            });
            if !public_image_url {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::FileUrl,
                    reason_code: TransformReasonCode::UnsupportedContent,
                });
            }
            if file_data
                .get("displayName")
                .is_some_and(|name| !name.is_string() && !name.is_null())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::Metadata,
                ));
            }
            if file_data
                .get("displayName")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.trim().is_empty())
            {
                record_minor_drop(phase, TransformSemanticUnit::Metadata);
            }
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
            if let Some(effort) = data
                .get("reasoning_effort")
                .filter(|value| !value.is_null())
            {
                if !matches!(
                    effort.as_str(),
                    Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh")
                ) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                record_minor_reasoning_mapping(TransformPhase::RequestDecode);
            }
            if data
                .get("parallel_tool_calls")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::ToolDefinitions,
                ));
            }
            if let Some(tool_choice) = data.get("tool_choice") {
                let valid = match tool_choice {
                    Value::String(value) => matches!(value.as_str(), "none" | "auto" | "required"),
                    Value::Object(value) => match value.get("type").and_then(Value::as_str) {
                        Some("function") => value
                            .get("function")
                            .and_then(|function| function.get("name"))
                            .and_then(Value::as_str)
                            .is_some_and(|name| !name.is_empty()),
                        Some("allowed_tools") => {
                            value.get("allowed_tools").is_some_and(|allowed| {
                                matches!(
                                    allowed.get("mode").and_then(Value::as_str),
                                    Some("auto" | "required")
                                ) && allowed.get("tools").and_then(Value::as_array).is_some_and(
                                    |tools| {
                                        !tools.is_empty()
                                            && tools.iter().all(|tool| {
                                                tool.get("type").and_then(Value::as_str)
                                                    == Some("function")
                                                    && tool
                                                        .get("function")
                                                        .and_then(|function| function.get("name"))
                                                        .and_then(Value::as_str)
                                                        .is_some_and(|name| !name.is_empty())
                                            })
                                    },
                                )
                            })
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !valid {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
            }
            if let Some(format) = data.get("response_format") {
                validate_openai_response_format(format)?;
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
                if tool.get("type").and_then(Value::as_str) != Some("function") {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
                let function = tool.get("function").ok_or_else(|| {
                    SourceSemanticError::invalid(TransformSemanticUnit::ToolDefinitions)
                })?;
                validate_portable_function_definition(function, "name", "parameters")?;
            }
            validate_openai_tool_calls(
                object_array(data, "messages"),
                TransformPhase::RequestDecode,
            )
        }
        DownstreamProtocol::Responses => {
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if data
                .get("parallel_tool_calls")
                .is_some_and(|value| !value.is_null() && !value.is_boolean())
            {
                return Err(SourceSemanticError::invalid(
                    TransformSemanticUnit::ToolDefinitions,
                ));
            }
            if let Some(choice) = data.get("tool_choice").filter(|value| !value.is_null()) {
                let valid = match choice {
                    Value::String(value) => matches!(value.as_str(), "none" | "auto" | "required"),
                    Value::Object(value) => match value.get("type").and_then(Value::as_str) {
                        Some("function") => value
                            .get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|name| !name.is_empty()),
                        Some("allowed_tools") => {
                            matches!(
                                value.get("mode").and_then(Value::as_str),
                                Some("auto" | "required")
                            ) && value
                                .get("tools")
                                .and_then(Value::as_array)
                                .is_some_and(|tools| {
                                    !tools.is_empty()
                                        && tools.iter().all(|tool| {
                                            tool.get("type").and_then(Value::as_str)
                                                == Some("function")
                                                && tool
                                                    .get("name")
                                                    .and_then(Value::as_str)
                                                    .is_some_and(|name| !name.is_empty())
                                        })
                                })
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if !valid {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
            }
            if let Some(text) = data.get("text").filter(|value| !value.is_null()) {
                let Some(text) = text.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::StructuredOutput,
                    ));
                };
                if let Some(format) = text.get("format").filter(|value| !value.is_null()) {
                    validate_responses_text_format(format)?;
                }
            }
            if let Some(reasoning) = data.get("reasoning").filter(|value| !value.is_null()) {
                let Some(reasoning) = reasoning.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                };
                if reasoning.get("effort").is_some_and(|effort| {
                    !effort.is_null()
                        && !matches!(
                            effort.as_str(),
                            Some("none" | "minimal" | "low" | "medium" | "high" | "xhigh")
                        )
                }) || reasoning.get("summary").is_some_and(|summary| {
                    !summary.is_null()
                        && !matches!(summary.as_str(), Some("concise" | "detailed" | "auto"))
                }) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if reasoning
                    .get("summary")
                    .is_some_and(|summary| !summary.is_null())
                {
                    record_minor_reasoning_drop(TransformPhase::RequestDecode);
                }
            }
            if let Some(items) = data.get("input").and_then(Value::as_array) {
                validate_responses_items(items, TransformPhase::RequestDecode)?;
            }
            for tool in object_array(data, "tools") {
                if tool.get("type").and_then(Value::as_str) != Some("function") {
                    return Err(SourceSemanticError::unknown(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
                validate_portable_function_definition(tool, "name", "parameters")?;
            }
            Ok(())
        }
        DownstreamProtocol::Anthropic => {
            require_non_empty_string(data, "model", TransformSemanticUnit::Model)?;
            if let Some(choice) = data.get("tool_choice").filter(|value| !value.is_null()) {
                let Some(choice) = choice.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                };
                let choice_type = choice.get("type").and_then(Value::as_str);
                if !matches!(choice_type, Some("auto" | "any" | "tool"))
                    || (choice_type == Some("tool")
                        && !choice
                            .get("name")
                            .and_then(Value::as_str)
                            .is_some_and(|name| !name.is_empty()))
                    || choice
                        .get("disable_parallel_tool_use")
                        .is_some_and(|disabled| !disabled.is_boolean() && !disabled.is_null())
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
            }
            let thinking = data.get("thinking").filter(|value| !value.is_null());
            let output_config = data.get("output_config").filter(|value| !value.is_null());
            if let Some(thinking) = thinking {
                let Some(thinking) = thinking.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                };
                let Some(thinking_type) = thinking.get("type").and_then(Value::as_str) else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                };
                if !matches!(thinking_type, "adaptive" | "enabled" | "disabled") {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if thinking_type == "enabled"
                    && !thinking
                        .get("budget_tokens")
                        .and_then(Value::as_u64)
                        .is_some_and(|budget| budget > 0)
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if thinking.get("budget_tokens").is_some() && thinking_type != "enabled" {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                record_minor_reasoning_mapping(TransformPhase::RequestDecode);
                if thinking.get("display").is_some_and(|display| {
                    !display.is_null()
                        && !matches!(display.as_str(), Some("summarized" | "omitted"))
                }) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if thinking
                    .get("display")
                    .is_some_and(|display| !display.is_null())
                {
                    record_minor_reasoning_drop(TransformPhase::RequestDecode);
                }
            }
            if let Some(output_config) = output_config {
                let Some(output_config) = output_config.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                };
                let effort = output_config.get("effort").filter(|value| !value.is_null());
                if effort.is_some_and(|effort| {
                    !matches!(
                        effort.as_str(),
                        Some("low" | "medium" | "high" | "xhigh" | "max")
                    )
                }) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if effort.is_some() {
                    record_minor_reasoning_mapping(TransformPhase::RequestDecode);
                }
                if let Some(format) = output_config.get("format").filter(|value| !value.is_null()) {
                    let Some(format) = format.as_object() else {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::StructuredOutput,
                        ));
                    };
                    if format.get("type").and_then(Value::as_str) != Some("json_schema")
                        || !format.get("schema").is_some_and(Value::is_object)
                    {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::StructuredOutput,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    record_structured_name_synthesis();
                }
            }
            if thinking
                .and_then(|thinking| thinking.get("type"))
                .and_then(Value::as_str)
                == Some("disabled")
                && output_config
                    .and_then(|config| config.get("effort"))
                    .is_some_and(|effort| !effort.is_null())
            {
                return Err(SourceSemanticError {
                    semantic_unit: TransformSemanticUnit::ReasoningContent,
                    reason_code: TransformReasonCode::UnsupportedReasoning,
                });
            }
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
                validate_portable_function_definition(tool, "name", "input_schema")?;
            }
            validate_anthropic_messages(object_array(data, "messages"))
        }
        DownstreamProtocol::Gemini => {
            if let Some(tool_config) = data.get("toolConfig").filter(|value| !value.is_null()) {
                let Some(config) = tool_config
                    .get("functionCallingConfig")
                    .and_then(Value::as_object)
                else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                };
                let mode = config.get("mode").and_then(Value::as_str);
                if !matches!(mode, Some("AUTO" | "ANY" | "NONE" | "VALIDATED")) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
                if let Some(names) = config.get("allowedFunctionNames") {
                    if !names.as_array().is_some_and(|names| {
                        !names.is_empty()
                            && names
                                .iter()
                                .all(|name| name.as_str().is_some_and(|name| !name.is_empty()))
                    }) || !matches!(mode, Some("ANY" | "VALIDATED"))
                    {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::ToolDefinitions,
                        ));
                    }
                }
            }
            for tool in object_array(data, "tools") {
                let declarations = tool
                    .get("functionDeclarations")
                    .and_then(Value::as_array)
                    .ok_or_else(|| {
                        SourceSemanticError::invalid(TransformSemanticUnit::ToolDefinitions)
                    })?;
                if declarations.is_empty() {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ToolDefinitions,
                    ));
                }
                for definition in declarations {
                    validate_portable_function_definition(definition, "name", "parameters")?;
                }
            }
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
            if let Some(config) = data
                .get("generationConfig")
                .filter(|value| !value.is_null())
            {
                let Some(config) = config.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::RequestEnvelope,
                    ));
                };
                let response_format = config
                    .get("responseFormat")
                    .filter(|value| !value.is_null());
                let response_mime_type = config
                    .get("responseMimeType")
                    .filter(|value| !value.is_null());
                let response_schema = config
                    .get("responseSchema")
                    .filter(|value| !value.is_null());
                let response_json_schema = config
                    .get("responseJsonSchema")
                    .filter(|value| !value.is_null());
                let legacy_schema_count = usize::from(response_schema.is_some())
                    + usize::from(response_json_schema.is_some());
                if response_format.is_some()
                    && (response_mime_type.is_some() || legacy_schema_count > 0)
                    || legacy_schema_count > 1
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::StructuredOutput,
                    ));
                }
                let schema = if let Some(response_format) = response_format {
                    let Some(response_format) = response_format.as_object() else {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::StructuredOutput,
                        ));
                    };
                    if response_format.keys().any(|key| key != "text") {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::StructuredOutput,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    let Some(text) = response_format.get("text").and_then(Value::as_object) else {
                        return Err(SourceSemanticError::invalid(
                            TransformSemanticUnit::StructuredOutput,
                        ));
                    };
                    if text.get("mimeType").and_then(Value::as_str) != Some("application/json") {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::StructuredOutput,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    text.get("schema").filter(|value| !value.is_null())
                } else if response_mime_type.is_some() || legacy_schema_count > 0 {
                    if response_mime_type.and_then(Value::as_str) != Some("application/json") {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::StructuredOutput,
                            reason_code: TransformReasonCode::UnsupportedContent,
                        });
                    }
                    response_json_schema.or(response_schema)
                } else {
                    None
                };
                if schema.is_some_and(|schema| !schema.is_object()) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::StructuredOutput,
                    ));
                }
                if let Some(schema) = schema {
                    record_structured_name_synthesis();
                    if schema_contains_property_ordering(schema) {
                        record_minor_drop(
                            TransformPhase::RequestDecode,
                            TransformSemanticUnit::StructuredOutput,
                        );
                    }
                }
            }
            if let Some(thinking) = data
                .get("generationConfig")
                .and_then(|config| config.get("thinkingConfig"))
                .filter(|value| !value.is_null())
            {
                let Some(thinking) = thinking.as_object() else {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                };
                let level = thinking
                    .get("thinkingLevel")
                    .filter(|value| !value.is_null());
                if level.is_some_and(|level| {
                    !matches!(level.as_str(), Some("minimal" | "low" | "medium" | "high"))
                }) {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                let budget = thinking
                    .get("thinkingBudget")
                    .filter(|value| !value.is_null());
                if budget.is_some_and(|budget| budget.as_i64().is_none())
                    || (level.is_some() && budget.is_some())
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if budget
                    .and_then(Value::as_i64)
                    .is_some_and(|budget| budget < -1)
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if budget
                    .and_then(Value::as_i64)
                    .is_some_and(|budget| budget > 0)
                {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::ReasoningContent,
                        reason_code: TransformReasonCode::UnsupportedReasoning,
                    });
                }
                if level.is_some() || budget.is_some() {
                    record_minor_reasoning_mapping(TransformPhase::RequestDecode);
                }
                if thinking
                    .get("includeThoughts")
                    .is_some_and(|include| !include.is_null() && include.as_bool().is_none())
                {
                    return Err(SourceSemanticError::invalid(
                        TransformSemanticUnit::ReasoningContent,
                    ));
                }
                if thinking.get("includeThoughts").and_then(Value::as_bool) == Some(true) {
                    record_minor_reasoning_drop(TransformPhase::RequestDecode);
                }
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
                    record_minor_mapping(
                        TransformPhase::RequestDecode,
                        TransformSemanticUnit::ToolCall,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                for _ in parts
                    .iter()
                    .filter(|part| part.get("functionResponse").is_some())
                {
                    record_minor_mapping(
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
            match data.get("status").and_then(Value::as_str) {
                Some("completed") => {}
                Some("incomplete") => {
                    let reason = data
                        .get("incomplete_details")
                        .and_then(|details| details.get("reason"))
                        .and_then(Value::as_str);
                    if !matches!(
                        reason,
                        Some("max_tokens" | "max_output_tokens" | "content_filter")
                    ) {
                        return Err(SourceSemanticError {
                            semantic_unit: TransformSemanticUnit::Lifecycle,
                            reason_code: TransformReasonCode::UnknownIncompleteReason,
                        });
                    }
                }
                _ => {
                    return Err(SourceSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::IllegalUpstreamTerminal,
                    });
                }
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
        UnifiedItem::FileReference(file) => {
            if file.file_url.is_some() {
                push_kind(kinds, TransformValueKind::FileUrl);
            }
            if file.file_id.is_some() {
                push_kind(kinds, TransformValueKind::FileId);
            }
        }
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

    let tool_names = request
        .tools
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|tool| tool.function.name.as_str())
        .collect::<std::collections::HashSet<_>>();
    let selected_names = match request.tool_choice.as_ref() {
        Some(UnifiedToolChoice::Named { name }) => vec![name.as_str()],
        Some(UnifiedToolChoice::Allowed { names, .. }) => {
            names.iter().map(String::as_str).collect()
        }
        _ => Vec::new(),
    };
    if !selected_names.is_empty() && selected_names.iter().any(|name| !tool_names.contains(name)) {
        record_rejection(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::ToolDefinitions,
            TransformReasonCode::InvalidProtocolShape,
        );
    }

    if target == UpstreamProtocol::Openai {
        for message in &request.messages {
            for part in &message.content {
                if let UnifiedContentPart::ToolResult(result) = part
                    && !matches!(result.output, UnifiedToolResultOutput::Text { .. })
                {
                    record_fact(
                        TransformPhase::RequestEncode,
                        TransformSemanticUnit::ToolResult,
                        TransformOutcomeKind::ControlledLossMajor,
                        TransformAction::Send,
                        TransformReasonCode::DeterministicTextDowngrade,
                    );
                }
            }
        }
    }

    if target == UpstreamProtocol::Gemini
        && request
            .tools
            .as_deref()
            .is_some_and(|tools| tools.iter().any(|tool| tool.function.strict == Some(true)))
    {
        record_minor_mapping(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::ToolDefinitions,
            TransformReasonCode::UnsupportedContent,
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

    for message in request
        .messages
        .iter()
        .filter(|message| message.role != UnifiedRole::User)
    {
        for part in &message.content {
            if matches!(
                part,
                UnifiedContentPart::ImageUrl { .. }
                    | UnifiedContentPart::ImageData { .. }
                    | UnifiedContentPart::AudioData { .. }
                    | UnifiedContentPart::FileUrl { .. }
                    | UnifiedContentPart::FileData { .. }
                    | UnifiedContentPart::FileId { .. }
            ) {
                record_rejection(
                    TransformPhase::RequestEncode,
                    TransformSemanticUnit::from(TransformValueKind::from(part)),
                    TransformReasonCode::UnsupportedContent,
                );
            }
        }
    }

    if target == UpstreamProtocol::Openai {
        for message in &request.messages {
            for part in &message.content {
                match part {
                    UnifiedContentPart::AudioData { format, .. }
                        if !matches!(format.as_str(), "wav" | "mp3") =>
                    {
                        record_rejection(
                            TransformPhase::RequestEncode,
                            TransformSemanticUnit::AudioData,
                            TransformReasonCode::UnsupportedContent,
                        );
                    }
                    UnifiedContentPart::FileData { filename, .. }
                        if filename
                            .as_deref()
                            .is_none_or(|filename| filename.trim().is_empty()) =>
                    {
                        record_rejection(
                            TransformPhase::RequestEncode,
                            TransformSemanticUnit::FileData,
                            TransformReasonCode::UnsupportedContent,
                        );
                    }
                    UnifiedContentPart::FileId { file_id, .. } if file_id.trim().is_empty() => {
                        record_rejection(
                            TransformPhase::RequestEncode,
                            TransformSemanticUnit::FileId,
                            TransformReasonCode::UnsupportedContent,
                        );
                    }
                    _ => {}
                }
            }
        }
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

    if request.reasoning_effort.is_some()
        && !matches!(
            target,
            UpstreamProtocol::Openai | UpstreamProtocol::Responses
        )
    {
        record_rejection(
            TransformPhase::RequestEncode,
            TransformSemanticUnit::ReasoningContent,
            TransformReasonCode::UnsupportedReasoning,
        );
    }

    let has_structured_output = request.structured_output.is_some();
    let structured_output_supported = matches!(
        target,
        UpstreamProtocol::Openai | UpstreamProtocol::Responses
    );
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
        if responses.reasoning.is_some()
            && !matches!(
                target,
                UpstreamProtocol::Openai | UpstreamProtocol::Responses
            )
        {
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
                || !responses.files.is_empty();
            if semantically_material {
                record_rejection(
                    TransformPhase::ResponseEncode,
                    TransformSemanticUnit::Metadata,
                    TransformReasonCode::UnsupportedContent,
                );
            } else if responses.safety_identifier.is_some()
                || responses.prompt_cache_key.is_some()
                || responses.metadata.is_some()
                || responses.reasoning.is_some()
            {
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

        let structured = transform_request_data(
            json!({
                "model": "gpt-4.1",
                "input": "hello",
                "text": {"format": {"type": "json_object"}}
            }),
            DownstreamProtocol::Responses,
            UpstreamProtocol::Openai,
            false,
        )
        .expect("Responses structured output must map to OpenAI");
        assert_eq!(
            structured.value["response_format"],
            json!({"type":"json_object"})
        );

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

        let preserved = transform_request_data(
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
        .expect("Anthropic tool error semantics must survive in the Responses output");
        assert_eq!(
            preserved.value["input"][0]["output"],
            json!({"error": "bad"})
        );

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
    fn adapter_audit_drops_reasoning_without_flattening_it_into_openai_answer_text() {
        let transformed = transform_result(
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
        .expect("Anthropic reasoning may be dropped for Chat Completions");

        assert!(!transformed.value.0.to_string().contains("private"));
        assert!(!transformed.value.0.to_string().contains("sig"));
        assert!(transformed.summary.facts.iter().any(|fact| {
            fact.semantic_unit == TransformSemanticUnit::ReasoningContent
                && fact.outcome == TransformOutcomeKind::ControlledLossMinor
                && fact.action == TransformAction::Drop
                && fact.reason_code == TransformReasonCode::UnsupportedReasoning
                && fact.safe_summary.is_none()
        }));
    }
}
