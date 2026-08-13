use std::collections::HashSet;

use chrono::DateTime;
use serde_json::Value;

use super::capability::TransformValueKind;
use super::diagnostics::record_captured_transform_fact;
use super::providers::{anthropic, gemini, openai, responses};
use super::stream::session::{MAX_STREAM_TOOL_ARGUMENT_BYTES, try_append_tool_arguments};
use super::stream::{AnthropicActiveBlockKind, StreamTransformContext};
use super::unified::{
    UnifiedChunkResponse, UnifiedContentPartDelta, UnifiedItem, UnifiedRole, UnifiedStreamEvent,
};
use super::{
    TransformAction, TransformDiagnosticFact, TransformOutcomeKind, TransformPhase,
    TransformProtocol, TransformReasonCode, TransformSemanticUnit, apply_transform_policy,
};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::service::transform) struct SourceStreamSemanticError {
    pub semantic_unit: TransformSemanticUnit,
    pub reason_code: TransformReasonCode,
}

impl SourceStreamSemanticError {
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

    const fn decode(semantic_unit: TransformSemanticUnit) -> Self {
        Self {
            semantic_unit,
            reason_code: TransformReasonCode::SourceDecodeFailed,
        }
    }

    const fn usage_overflow() -> Self {
        Self {
            semantic_unit: TransformSemanticUnit::Usage,
            reason_code: TransformReasonCode::UsageOverflow,
        }
    }
}

fn record_fact(
    semantic_unit: TransformSemanticUnit,
    outcome: TransformOutcomeKind,
    action: TransformAction,
    reason_code: TransformReasonCode,
) {
    record_captured_transform_fact(TransformDiagnosticFact {
        sequence: 0,
        phase: TransformPhase::StreamEncode,
        semantic_unit,
        outcome,
        action,
        reason_code,
        safe_summary: None,
    });
}

fn record_source_fact(
    semantic_unit: TransformSemanticUnit,
    outcome: TransformOutcomeKind,
    action: TransformAction,
    reason_code: TransformReasonCode,
) {
    record_captured_transform_fact(TransformDiagnosticFact {
        sequence: 0,
        phase: TransformPhase::StreamDecode,
        semantic_unit,
        outcome,
        action,
        reason_code,
        safe_summary: None,
    });
}

fn record_source_synthesis(semantic_unit: TransformSemanticUnit, reason_code: TransformReasonCode) {
    record_source_fact(
        semantic_unit,
        TransformOutcomeKind::Lossless,
        TransformAction::Synthesize,
        reason_code,
    );
}

fn record_source_no_output(semantic_unit: TransformSemanticUnit) {
    record_source_fact(
        semantic_unit,
        TransformOutcomeKind::Lossless,
        TransformAction::Drop,
        TransformReasonCode::NoSemanticOutput,
    );
}

fn record_source_drop(semantic_unit: TransformSemanticUnit) {
    record_source_fact(
        semantic_unit,
        TransformOutcomeKind::ControlledLossMinor,
        TransformAction::Drop,
        TransformReasonCode::UnsupportedContent,
    );
}

fn record_synthesis(semantic_unit: TransformSemanticUnit, reason_code: TransformReasonCode) {
    record_fact(
        semantic_unit,
        TransformOutcomeKind::Lossless,
        TransformAction::Synthesize,
        reason_code,
    );
}

fn record_no_output(semantic_unit: TransformSemanticUnit) {
    record_fact(
        semantic_unit,
        TransformOutcomeKind::Lossless,
        TransformAction::Drop,
        TransformReasonCode::NoSemanticOutput,
    );
}

fn record_rejection(semantic_unit: TransformSemanticUnit, reason_code: TransformReasonCode) {
    record_fact(
        semantic_unit,
        TransformOutcomeKind::ExplicitReject,
        TransformAction::Reject,
        reason_code,
    );
}

fn require_object(
    value: &Value,
    semantic_unit: TransformSemanticUnit,
) -> Result<(), SourceStreamSemanticError> {
    if value.is_object() {
        Ok(())
    } else {
        Err(SourceStreamSemanticError::invalid(semantic_unit))
    }
}

fn require_array_field<'a>(
    value: &'a Value,
    field: &str,
    semantic_unit: TransformSemanticUnit,
) -> Result<&'a [Value], SourceStreamSemanticError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| SourceStreamSemanticError::invalid(semantic_unit))
}

fn require_non_empty_string<'a>(
    value: &'a Value,
    field: &str,
    semantic_unit: TransformSemanticUnit,
) -> Result<&'a str, SourceStreamSemanticError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| SourceStreamSemanticError::invalid(semantic_unit))
}

fn require_u32(
    value: &Value,
    field: &str,
    semantic_unit: TransformSemanticUnit,
) -> Result<u32, SourceStreamSemanticError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| SourceStreamSemanticError::invalid(semantic_unit))
}

fn optional_string_is_valid(value: &Value, field: &str) -> bool {
    value
        .get(field)
        .is_none_or(|value| value.is_null() || value.is_string())
}

fn validate_json_object_text(arguments: &str) -> bool {
    if arguments.trim().is_empty() {
        return true;
    }
    serde_json::from_str::<Value>(arguments).is_ok_and(|value| value.is_object())
}

fn validate_openai_stream_frame(
    value: &Value,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    require_object(value, TransformSemanticUnit::StreamFrame)?;
    require_non_empty_string(value, "id", TransformSemanticUnit::Lifecycle)?;
    require_non_empty_string(value, "model", TransformSemanticUnit::Model)?;
    if value.get("object").and_then(Value::as_str) != Some("chat.completion.chunk") {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::StreamFrame,
        ));
    }

    let choices = require_array_field(value, "choices", TransformSemanticUnit::StreamFrame)?;
    if choices.len() > 1 {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }

    for choice in choices {
        if require_u32(choice, "index", TransformSemanticUnit::Lifecycle)? != 0 {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        let delta = choice.get("delta").ok_or_else(|| {
            SourceStreamSemanticError::invalid(TransformSemanticUnit::StreamFrame)
        })?;
        require_object(delta, TransformSemanticUnit::StreamFrame)?;

        let known_delta_fields = [
            "role",
            "content",
            "reasoning_content",
            "tool_calls",
            "refusal",
            "name",
        ];
        if delta.as_object().is_some_and(|object| {
            object
                .keys()
                .any(|key| !known_delta_fields.contains(&key.as_str()))
        }) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::StreamFrame,
            ));
        }

        if let Some(role) = delta.get("role").filter(|value| !value.is_null()) {
            if role.as_str() != Some("assistant") {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Role,
                ));
            }
        }
        if !optional_string_is_valid(delta, "content")
            || !optional_string_is_valid(delta, "reasoning_content")
        {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Text,
            ));
        }
        if delta.get("refusal").is_some_and(|value| !value.is_null()) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Refusal,
            ));
        }
        if delta.get("name").is_some_and(|value| !value.is_null()) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Role,
            ));
        }
        if delta
            .get("reasoning_content")
            .and_then(Value::as_str)
            .is_some_and(|text| !text.is_empty())
            && context.current_content_block_index().is_some()
        {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }

        if let Some(tool_calls) = delta.get("tool_calls").filter(|value| !value.is_null()) {
            let tool_calls = tool_calls.as_array().ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCallDelta)
            })?;
            for call in tool_calls {
                let index = require_u32(call, "index", TransformSemanticUnit::ToolCallDelta)?;
                if call
                    .get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "function")
                {
                    return Err(SourceStreamSemanticError::unknown(
                        TransformSemanticUnit::ToolCallDelta,
                    ));
                }
                let function = call.get("function").ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCallDelta)
                })?;
                require_object(function, TransformSemanticUnit::ToolCallDelta)?;
                if !optional_string_is_valid(call, "id")
                    || !optional_string_is_valid(function, "name")
                    || !optional_string_is_valid(function, "arguments")
                {
                    return Err(SourceStreamSemanticError::invalid(
                        TransformSemanticUnit::ToolCallDelta,
                    ));
                }

                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty());
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|value| !value.is_empty());
                let arguments = function.get("arguments").and_then(Value::as_str);
                let state = context
                    .openai_source_tool_calls_mut()
                    .entry(index)
                    .or_default();
                if let Some(id) = id {
                    if state.id.as_deref().is_some_and(|known| known != id) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state.id = Some(id.to_string());
                }
                if let Some(name) = name {
                    if state.name.as_deref().is_some_and(|known| known != name) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state.name = Some(name.to_string());
                }
                if let Some(arguments) = arguments {
                    if !try_append_tool_arguments(&mut state.arguments, arguments) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::ToolCallDelta,
                        ));
                    }
                }
            }
        }

        if choice.get("logprobs").is_some_and(|value| !value.is_null()) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Metadata,
            ));
        }
        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        if finish_reason.is_some_and(|reason| {
            !matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
        }) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        if finish_reason == Some("tool_calls") {
            if context.openai_source_tool_calls().is_empty()
                || context.openai_source_tool_calls().values().any(|state| {
                    state.id.as_deref().is_none_or(str::is_empty)
                        || state.name.as_deref().is_none_or(str::is_empty)
                        || !validate_json_object_text(&state.arguments)
                })
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
            context.openai_source_tool_calls_mut().clear();
        } else if finish_reason.is_some() && !context.openai_source_tool_calls().is_empty() {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
    }

    if let Some(usage) = value.get("usage").filter(|value| !value.is_null()) {
        require_object(usage, TransformSemanticUnit::Usage)?;
        for field in ["prompt_tokens", "completion_tokens", "total_tokens"] {
            require_u32(usage, field, TransformSemanticUnit::Usage)?;
        }
    }
    Ok(())
}

fn responses_item_identity(item: &Value) -> Result<(&str, &str), SourceStreamSemanticError> {
    let kind = require_non_empty_string(item, "type", TransformSemanticUnit::ResponsesUnknownItem)?;
    if !matches!(
        kind,
        "message" | "function_call" | "function_call_output" | "reasoning"
    ) {
        return Err(SourceStreamSemanticError::unknown(
            TransformSemanticUnit::ResponsesUnknownItem,
        ));
    }
    let id = require_non_empty_string(item, "id", TransformSemanticUnit::Lifecycle)?;
    Ok((kind, id))
}

fn responses_message_text(item: &Value) -> Result<String, SourceStreamSemanticError> {
    let mut text = String::new();
    for part in require_array_field(item, "content", TransformSemanticUnit::Text)? {
        let kind =
            require_non_empty_string(part, "type", TransformSemanticUnit::ResponsesUnknownItem)?;
        match kind {
            "output_text" | "text" | "summary_text" | "reasoning_text" => {
                text.push_str(part.get("text").and_then(Value::as_str).ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Text)
                })?);
                if part
                    .get("annotations")
                    .and_then(Value::as_array)
                    .is_some_and(|annotations| !annotations.is_empty())
                    || part
                        .get("logprobs")
                        .and_then(Value::as_array)
                        .is_some_and(|logprobs| !logprobs.is_empty())
                {
                    return Err(SourceStreamSemanticError::unknown(
                        TransformSemanticUnit::Metadata,
                    ));
                }
            }
            "refusal" => {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Refusal,
                ));
            }
            _ => {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::ResponsesUnknownItem,
                ));
            }
        }
    }
    Ok(text)
}

fn validate_responses_stream_frame(
    value: &Value,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    require_object(value, TransformSemanticUnit::StreamFrame)?;
    if value.get("type").is_none() {
        require_non_empty_string(value, "id", TransformSemanticUnit::Lifecycle)?;
        require_non_empty_string(value, "model", TransformSemanticUnit::Model)?;
        let item = value.get("delta").ok_or_else(|| {
            SourceStreamSemanticError::invalid(TransformSemanticUnit::ResponsesUnknownItem)
        })?;
        let (kind, _) = responses_item_identity(item)?;
        if kind == "function_call" {
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCallDelta)
                })?;
            if !validate_json_object_text(arguments) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
        }
        record_source_synthesis(
            TransformSemanticUnit::Lifecycle,
            TransformReasonCode::SyntheticEnvelope,
        );
        record_source_synthesis(
            TransformSemanticUnit::Lifecycle,
            TransformReasonCode::SyntheticIndex,
        );
        return Ok(());
    }
    let event_type = require_non_empty_string(value, "type", TransformSemanticUnit::Lifecycle)?;
    let state = context.responses_mut();
    if state.source_terminal_seen {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }

    match event_type {
        "response.created" => {
            if state.source_created_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let response = value.get("response").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ResponseEnvelope)
            })?;
            let response_id =
                require_non_empty_string(response, "id", TransformSemanticUnit::Lifecycle)?;
            let response_model =
                require_non_empty_string(response, "model", TransformSemanticUnit::Model)?;
            if response.get("status").and_then(Value::as_str) != Some("in_progress") {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state.source_created_seen = true;
            state.source_response_id = Some(response_id.to_string());
            state.source_response_model = Some(response_model.to_string());
        }
        "response.output_item.added" => {
            if !state.source_created_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let output_index =
                require_u32(value, "output_index", TransformSemanticUnit::Lifecycle)?;
            let item = value.get("item").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ResponsesUnknownItem)
            })?;
            let (kind, id) = responses_item_identity(item)?;
            if state
                .source_output_item_ids
                .insert(output_index, id.to_string())
                .is_some()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            match kind {
                "message" => {
                    if !responses_message_text(item)?.is_empty() {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state
                        .source_output_text
                        .insert(id.to_string(), String::new());
                }
                "function_call" => {
                    require_non_empty_string(item, "call_id", TransformSemanticUnit::ToolCall)?;
                    require_non_empty_string(item, "name", TransformSemanticUnit::ToolCall)?;
                    let arguments =
                        item.get("arguments")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                SourceStreamSemanticError::invalid(
                                    TransformSemanticUnit::ToolCallDelta,
                                )
                            })?;
                    if !arguments.is_empty() {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state
                        .source_tool_arguments
                        .insert(id.to_string(), String::new());
                }
                "function_call_output" | "reasoning" => {}
                _ => unreachable!("responses item kind was checked above"),
            }
        }
        "response.output_text.delta" => {
            let item_id =
                require_non_empty_string(value, "item_id", TransformSemanticUnit::Lifecycle)?;
            require_u32(value, "output_index", TransformSemanticUnit::Lifecycle)?;
            require_u32(value, "content_index", TransformSemanticUnit::Lifecycle)?;
            let delta = value
                .get("delta")
                .and_then(Value::as_str)
                .ok_or_else(|| SourceStreamSemanticError::invalid(TransformSemanticUnit::Text))?;
            state
                .source_output_text
                .get_mut(item_id)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?
                .push_str(delta);
        }
        "response.function_call_arguments.delta" => {
            let item_id =
                require_non_empty_string(value, "item_id", TransformSemanticUnit::Lifecycle)?;
            require_u32(value, "output_index", TransformSemanticUnit::Lifecycle)?;
            let delta = value.get("delta").and_then(Value::as_str).ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCallDelta)
            })?;
            let arguments_buffer =
                state
                    .source_tool_arguments
                    .get_mut(item_id)
                    .ok_or_else(|| {
                        SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                    })?;
            if !try_append_tool_arguments(arguments_buffer, delta) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
        }
        "response.function_call_arguments.done" => {
            let item_id =
                require_non_empty_string(value, "item_id", TransformSemanticUnit::Lifecycle)?;
            require_u32(value, "output_index", TransformSemanticUnit::Lifecycle)?;
            let arguments = value
                .get("arguments")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCallDelta)
                })?;
            if state.source_tool_arguments.get(item_id).map(String::as_str) != Some(arguments)
                || !validate_json_object_text(arguments)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        "response.output_item.done" => {
            let output_index =
                require_u32(value, "output_index", TransformSemanticUnit::Lifecycle)?;
            let item = value.get("item").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ResponsesUnknownItem)
            })?;
            let (kind, id) = responses_item_identity(item)?;
            if state
                .source_output_item_ids
                .get(&output_index)
                .map(String::as_str)
                != Some(id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            match kind {
                "message" => {
                    if state.source_output_text.get(id) != Some(&responses_message_text(item)?) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                }
                "function_call" => {
                    let arguments =
                        item.get("arguments")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                SourceStreamSemanticError::invalid(
                                    TransformSemanticUnit::ToolCallDelta,
                                )
                            })?;
                    if state.source_tool_arguments.get(id).map(String::as_str) != Some(arguments)
                        || !validate_json_object_text(arguments)
                    {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::ToolCallDelta,
                        ));
                    }
                }
                "function_call_output" | "reasoning" => {}
                _ => unreachable!("responses item kind was checked above"),
            }
        }
        "response.content_part.added" | "response.content_part.done" => {
            require_non_empty_string(value, "item_id", TransformSemanticUnit::Lifecycle)?;
            require_u32(value, "content_index", TransformSemanticUnit::Lifecycle)?;
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done" => {
            require_non_empty_string(value, "item_id", TransformSemanticUnit::Lifecycle)?;
            require_u32(value, "summary_index", TransformSemanticUnit::Lifecycle)?;
        }
        "response.completed" | "response.incomplete" => {
            if !state.source_created_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let response = value.get("response").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ResponseEnvelope)
            })?;
            let response_id =
                require_non_empty_string(response, "id", TransformSemanticUnit::Lifecycle)?;
            let response_model =
                require_non_empty_string(response, "model", TransformSemanticUnit::Model)?;
            if state.source_response_id.as_deref() != Some(response_id)
                || state.source_response_model.as_deref() != Some(response_model)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let expected_status = if event_type == "response.completed" {
                "completed"
            } else {
                "incomplete"
            };
            if response.get("status").and_then(Value::as_str) != Some(expected_status)
                || response.get("error").is_some_and(|error| !error.is_null())
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if response
                .get("metadata")
                .and_then(Value::as_object)
                .is_some_and(|metadata| !metadata.is_empty())
            {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Metadata,
                ));
            }
            state.source_terminal_seen = true;
        }
        "response.message.start" => {
            if let Some(id) = value.get("id").filter(|value| !value.is_null()) {
                if id.as_str().is_none_or(str::is_empty) {
                    return Err(SourceStreamSemanticError::invalid(
                        TransformSemanticUnit::Lifecycle,
                    ));
                }
            } else {
                record_source_synthesis(
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::SyntheticCorrelationId,
                );
            }
            let role = require_non_empty_string(value, "role", TransformSemanticUnit::Role)?;
            if role != "assistant" {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Role,
                ));
            }
        }
        "response.message.delta" => {
            if !optional_string_is_valid(value, "finish_reason") {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
        }
        "response.message.stop" => record_source_no_output(TransformSemanticUnit::Lifecycle),
        "response.content_block.start" | "response.content_block.stop" => {
            require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        "response.content_block.delta" => {
            require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            if value.get("text").and_then(Value::as_str).is_none() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Text,
                ));
            }
        }
        "response.tool_call.start" => {
            require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            require_non_empty_string(value, "id", TransformSemanticUnit::ToolCall)?;
            require_non_empty_string(value, "name", TransformSemanticUnit::ToolCall)?;
        }
        "response.tool_call.arguments.delta" => {
            require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            if value.get("arguments").and_then(Value::as_str).is_none() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
        }
        "response.tool_call.stop" | "response.reasoning.start" | "response.reasoning.stop" => {
            require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        "response.reasoning.delta" => {
            require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            if value.get("text").and_then(Value::as_str).is_none() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ReasoningDelta,
                ));
            }
        }
        "response.usage" => {
            require_object(
                value.get("usage").ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Usage)
                })?,
                TransformSemanticUnit::Usage,
            )?;
        }
        "response.blob" => {
            if value.get("data").is_none() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::BlobDelta,
                ));
            }
        }
        "response.error" => {
            if value.get("error").is_none_or(Value::is_null) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::StreamError,
                ));
            }
        }
        _ => {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::ResponsesUnknownItem,
            ));
        }
    }
    Ok(())
}

fn validate_anthropic_content_block(
    block: &Value,
) -> Result<AnthropicActiveBlockKind, SourceStreamSemanticError> {
    require_object(block, TransformSemanticUnit::StreamFrame)?;
    match require_non_empty_string(block, "type", TransformSemanticUnit::StreamFrame)? {
        "text" => {
            if block.get("text").and_then(Value::as_str).is_none() {
                Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Text,
                ))
            } else {
                Ok(AnthropicActiveBlockKind::Text)
            }
        }
        "thinking" => {
            if block.get("thinking").and_then(Value::as_str).is_none()
                || !optional_string_is_valid(block, "signature")
            {
                Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ReasoningDelta,
                ))
            } else {
                Ok(AnthropicActiveBlockKind::Thinking)
            }
        }
        "tool_use" => {
            require_non_empty_string(block, "id", TransformSemanticUnit::ToolCall)?;
            require_non_empty_string(block, "name", TransformSemanticUnit::ToolCall)?;
            require_object(
                block.get("input").ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCall)
                })?,
                TransformSemanticUnit::ToolCall,
            )?;
            Ok(AnthropicActiveBlockKind::ToolUse)
        }
        _ => Err(SourceStreamSemanticError::unknown(
            TransformSemanticUnit::StreamFrame,
        )),
    }
}

fn validate_anthropic_stream_frame(
    value: &Value,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    require_object(value, TransformSemanticUnit::StreamFrame)?;
    let event_type = require_non_empty_string(value, "type", TransformSemanticUnit::Lifecycle)?;
    if context.anthropic_session_mut().source_message_stopped
        || context.anthropic_session_mut().source_error_seen
    {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }
    match event_type {
        "message_start" => {
            if context.anthropic_session_mut().source_message_started {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let message = value.get("message").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::ResponseEnvelope)
            })?;
            require_non_empty_string(message, "id", TransformSemanticUnit::Lifecycle)?;
            require_non_empty_string(message, "model", TransformSemanticUnit::Model)?;
            if message.get("type").and_then(Value::as_str) != Some("message")
                || message.get("role").and_then(Value::as_str) != Some("assistant")
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Role,
                ));
            }
            if let Some(blocks) = message.get("content").filter(|value| !value.is_null()) {
                for block in blocks.as_array().ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::StreamFrame)
                })? {
                    validate_anthropic_content_block(block)?;
                }
            }
            context.anthropic_session_mut().source_message_started = true;
        }
        "content_block_start" => {
            if !context.anthropic_session_mut().source_message_started {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let index = require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            if context.anthropic_active_blocks().contains_key(&index) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let content_block = value.get("content_block").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::StreamFrame)
            })?;
            validate_anthropic_content_block(content_block)?;
            if content_block.get("type").and_then(Value::as_str) == Some("tool_use")
                && content_block
                    .get("input")
                    .and_then(Value::as_object)
                    .is_some_and(|input| !input.is_empty())
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
        }
        "content_block_delta" => {
            let index = require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            let active_kind = context
                .anthropic_active_blocks()
                .get(&index)
                .map(|block| block.kind)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?;
            let delta = value.get("delta").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::StreamFrame)
            })?;
            let delta_type =
                require_non_empty_string(delta, "type", TransformSemanticUnit::StreamFrame)?;
            let valid = match delta_type {
                "text_delta" => {
                    active_kind == AnthropicActiveBlockKind::Text
                        && delta.get("text").and_then(Value::as_str).is_some()
                }
                "input_json_delta" => {
                    active_kind == AnthropicActiveBlockKind::ToolUse
                        && delta.get("partial_json").and_then(Value::as_str).is_some()
                }
                "thinking_delta" => {
                    active_kind == AnthropicActiveBlockKind::Thinking
                        && delta.get("thinking").and_then(Value::as_str).is_some()
                }
                "signature_delta" => {
                    active_kind == AnthropicActiveBlockKind::Thinking
                        && delta.get("signature").and_then(Value::as_str).is_some()
                }
                _ => {
                    return Err(SourceStreamSemanticError::unknown(
                        TransformSemanticUnit::StreamFrame,
                    ));
                }
            };
            if !valid {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
        }
        "content_block_stop" => {
            let index = require_u32(value, "index", TransformSemanticUnit::Lifecycle)?;
            let block = context
                .anthropic_active_blocks()
                .get(&index)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?;
            if block.kind == AnthropicActiveBlockKind::ToolUse
                && !validate_json_object_text(&block.text)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
        }
        "message_delta" => {
            if !context.anthropic_session_mut().source_message_started {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let delta = value.get("delta").ok_or_else(|| {
                SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
            })?;
            if let Some(reason) = delta.get("stop_reason").and_then(Value::as_str) {
                if !matches!(
                    reason,
                    "end_turn" | "stop_sequence" | "tool_use" | "max_tokens"
                ) {
                    return Err(SourceStreamSemanticError::unknown(
                        TransformSemanticUnit::Lifecycle,
                    ));
                }
            }
            if let Some(usage) = value
                .get("usage")
                .or_else(|| delta.get("usage"))
                .filter(|value| !value.is_null())
            {
                require_u32(usage, "input_tokens", TransformSemanticUnit::Usage)?;
                require_u32(usage, "output_tokens", TransformSemanticUnit::Usage)?;
            }
        }
        "message_stop" => {
            if !context.anthropic_session_mut().source_message_started
                || !context.anthropic_active_blocks().is_empty()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            context.anthropic_session_mut().source_message_stopped = true;
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        "ping" => record_source_no_output(TransformSemanticUnit::Lifecycle),
        "error" => {
            if value.get("error").is_none_or(Value::is_null) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::StreamError,
                ));
            }
        }
        _ => {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::StreamFrame,
            ));
        }
    }
    Ok(())
}

fn validate_gemini_part(
    part: &Value,
    has_response_id: bool,
) -> Result<(), SourceStreamSemanticError> {
    require_object(part, TransformSemanticUnit::StreamFrame)?;
    let known = [
        "text",
        "functionCall",
        "inlineData",
        "executableCode",
        "functionResponse",
        "fileData",
    ];
    let present = known
        .iter()
        .filter(|field| part.get(**field).is_some())
        .copied()
        .collect::<Vec<_>>();
    if present.len() != 1 {
        return Err(if present.is_empty() {
            SourceStreamSemanticError::unknown(TransformSemanticUnit::StreamFrame)
        } else {
            SourceStreamSemanticError::invalid(TransformSemanticUnit::StreamFrame)
        });
    }
    if part
        .get("thought")
        .is_some_and(|thought| !thought.is_null() && !thought.is_boolean())
    {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::ReasoningContent,
        ));
    }
    if part.get("thoughtSignature").is_some_and(|signature| {
        !signature.is_null()
            && !signature
                .as_str()
                .is_some_and(|signature| !signature.is_empty())
    }) {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Metadata,
        ));
    }
    match present[0] {
        "text" => {
            if !part.get("text").is_some_and(Value::is_string) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Text,
                ));
            }
        }
        "functionCall" => {
            let call = &part["functionCall"];
            require_non_empty_string(call, "name", TransformSemanticUnit::ToolCall)?;
            match call.get("id").filter(|value| !value.is_null()) {
                Some(id) if !id.as_str().is_some_and(|id| !id.trim().is_empty()) => {
                    return Err(SourceStreamSemanticError::invalid(
                        TransformSemanticUnit::ToolCall,
                    ));
                }
                None if !has_response_id => {
                    return Err(SourceStreamSemanticError {
                        semantic_unit: TransformSemanticUnit::ToolCall,
                        reason_code: TransformReasonCode::ToolCorrelationSeedRequired,
                    });
                }
                None => record_source_synthesis(
                    TransformSemanticUnit::ToolCall,
                    TransformReasonCode::SyntheticCorrelationId,
                ),
                Some(_) => {}
            }
            require_object(
                call.get("args").ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::ToolCallDelta)
                })?,
                TransformSemanticUnit::ToolCallDelta,
            )?;
        }
        "inlineData" => {
            let data = &part["inlineData"];
            require_non_empty_string(data, "mimeType", TransformSemanticUnit::ImageDelta)?;
            require_non_empty_string(data, "data", TransformSemanticUnit::ImageDelta)?;
        }
        "executableCode" => {
            let code = &part["executableCode"];
            require_non_empty_string(code, "language", TransformSemanticUnit::ToolCall)?;
            require_non_empty_string(code, "code", TransformSemanticUnit::ToolCall)?;
            record_source_synthesis(
                TransformSemanticUnit::ToolCall,
                TransformReasonCode::SyntheticCorrelationId,
            );
        }
        "functionResponse" | "fileData" => {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::StreamFrame,
            ));
        }
        _ => unreachable!("Gemini part discriminator was checked above"),
    }
    Ok(())
}

const MAX_GEMINI_STREAM_CANDIDATES: usize = 32;

fn validate_gemini_stream_frame(
    value: &Value,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    require_object(value, TransformSemanticUnit::StreamFrame)?;
    let response_id = match value.get("responseId").filter(|value| !value.is_null()) {
        Some(response_id)
            if !response_id
                .as_str()
                .is_some_and(|response_id| !response_id.trim().is_empty()) =>
        {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Metadata,
            ));
        }
        Some(response_id) => response_id.as_str(),
        None => None,
    };
    if let (Some(expected), Some(actual)) =
        (context.gemini().source_response_id.as_deref(), response_id)
        && expected != actual
    {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }

    if let Some(error) = value.get("error") {
        require_object(error, TransformSemanticUnit::StreamError)?;
        let state = context.gemini();
        let success_terminal_complete = state.source_prompt_block_seen
            || (!state.source_candidate_indices.is_empty()
                && state.source_candidate_indices == state.source_terminal_candidate_indices);
        if state.source_terminal_failed || success_terminal_complete {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        if value.get("candidates").is_some()
            || value.get("promptFeedback").is_some()
            || value.get("usageMetadata").is_some()
        {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::StreamError,
            ));
        }
        let state = context.gemini_mut();
        if state.source_response_id.is_none() {
            state.source_response_id = response_id.map(str::to_string);
        }
        state.source_terminal_failed = true;
        return Ok(());
    }

    let candidates: &[Value] = match value.get("candidates") {
        Some(candidates) => candidates.as_array().map(Vec::as_slice).ok_or_else(|| {
            SourceStreamSemanticError::invalid(TransformSemanticUnit::StreamFrame)
        })?,
        None if value.get("usageMetadata").is_some() || value.get("promptFeedback").is_some() => {
            &[]
        }
        None => {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::StreamFrame,
            ));
        }
    };
    if candidates.len() > MAX_GEMINI_STREAM_CANDIDATES {
        return Err(SourceStreamSemanticError {
            semantic_unit: TransformSemanticUnit::Lifecycle,
            reason_code: TransformReasonCode::UnsupportedContent,
        });
    }
    let mut frame_indices = HashSet::new();
    let mut candidate_frames = Vec::with_capacity(candidates.len());
    for (position, candidate) in candidates.iter().enumerate() {
        require_object(candidate, TransformSemanticUnit::StreamFrame)?;
        let index = match candidate.get("index") {
            Some(value) => value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?,
            None => {
                record_source_synthesis(
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::SyntheticIndex,
                );
                u32::try_from(position).map_err(|_| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?
            }
        };
        if !frame_indices.insert(index) {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        candidate_frames.push((index, candidate));
    }
    let new_candidate_count = frame_indices
        .iter()
        .filter(|index| !context.gemini().source_candidate_indices.contains(index))
        .count();
    if context
        .gemini()
        .source_candidate_indices
        .len()
        .saturating_add(new_candidate_count)
        > MAX_GEMINI_STREAM_CANDIDATES
    {
        return Err(SourceStreamSemanticError {
            semantic_unit: TransformSemanticUnit::Lifecycle,
            reason_code: TransformReasonCode::UnsupportedContent,
        });
    }
    if !context.is_same_wire()
        && (candidate_frames.len() > 1
            || candidate_frames
                .first()
                .is_some_and(|(index, _)| *index != 0))
    {
        return Err(SourceStreamSemanticError {
            semantic_unit: TransformSemanticUnit::Lifecycle,
            reason_code: TransformReasonCode::UnsupportedContent,
        });
    }

    if !candidate_frames.is_empty()
        && (context.gemini().source_prompt_block_seen || context.gemini().source_terminal_failed)
    {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }

    let has_response_id = response_id.is_some() || context.gemini().source_response_id.is_some();
    let mut terminal_indices = HashSet::new();
    let mut application_failed = false;
    for (index, candidate) in &candidate_frames {
        let candidate_already_terminal = context
            .gemini()
            .source_terminal_candidate_indices
            .contains(index);
        if let Some(content) = candidate.get("content").filter(|value| !value.is_null()) {
            if candidate_already_terminal {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if content.get("role").and_then(Value::as_str) != Some("model") {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Role,
                ));
            }
            let parts = require_array_field(content, "parts", TransformSemanticUnit::StreamFrame)?;
            if parts.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::StreamFrame,
                ));
            }
            for part in parts {
                validate_gemini_part(part, has_response_id)?;
            }
        }
        if let Some(reason) = candidate.get("finishReason") {
            if candidate_already_terminal {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let reason = reason
                .as_str()
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?;
            let classification = gemini::classify_gemini_finish_reason(Some(reason));
            if classification.kind == gemini::GeminiTerminalKind::ObservationDegraded {
                return Err(SourceStreamSemanticError {
                    semantic_unit: TransformSemanticUnit::Lifecycle,
                    reason_code: classification
                        .reason_code
                        .unwrap_or(TransformReasonCode::IllegalUpstreamTerminal),
                });
            }
            terminal_indices.insert(*index);
            application_failed |=
                classification.kind == gemini::GeminiTerminalKind::ApplicationFailure;
        }
    }

    let block_reason = value
        .get("promptFeedback")
        .and_then(|feedback| feedback.get("blockReason"))
        .filter(|reason| !reason.is_null())
        .map(|reason| {
            reason
                .as_str()
                .filter(|reason| !reason.is_empty())
                .ok_or_else(|| SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle))
        })
        .transpose()?;
    let prompt_blocked = if block_reason.is_some() {
        if !candidate_frames.is_empty()
            || !context.gemini().source_candidate_indices.is_empty()
            || context.gemini().source_prompt_block_seen
            || context.gemini().source_terminal_failed
        {
            return Err(SourceStreamSemanticError {
                semantic_unit: TransformSemanticUnit::Lifecycle,
                reason_code: TransformReasonCode::UnsupportedContent,
            });
        }
        let classification = gemini::classify_gemini_terminal(value);
        if classification.kind == gemini::GeminiTerminalKind::ObservationDegraded {
            return Err(SourceStreamSemanticError {
                semantic_unit: TransformSemanticUnit::Lifecycle,
                reason_code: classification
                    .reason_code
                    .unwrap_or(TransformReasonCode::IllegalUpstreamTerminal),
            });
        }
        true
    } else {
        false
    };

    if candidates.is_empty()
        && value.get("usageMetadata").is_none()
        && value.get("promptFeedback").is_none()
    {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::StreamFrame,
        ));
    }

    // Commit the independently validated Gemini lifecycle core before observing usage.
    // Same-wire streams may preserve a valid terminal frame even when usage observation
    // degrades; cross-wire failures remain transactional at the transformer boundary.
    let state = context.gemini_mut();
    if state.source_response_id.is_none() {
        state.source_response_id = response_id.map(str::to_string);
    }
    state
        .source_candidate_indices
        .extend(candidate_frames.iter().map(|(index, _)| *index));
    state
        .source_terminal_candidate_indices
        .extend(terminal_indices);
    state.source_prompt_block_seen |= prompt_blocked;
    state.source_terminal_failed |= application_failed;

    if let Some(usage) = value.get("usageMetadata").filter(|value| !value.is_null()) {
        let usage =
            gemini::decode_gemini_usage(usage).map_err(|error| SourceStreamSemanticError {
                semantic_unit: TransformSemanticUnit::Usage,
                reason_code: error.reason_code(),
            })?;
        if context
            .gemini_source_usage()
            .is_some_and(|previous| gemini::gemini_usage_snapshot_regressed(previous, &usage))
        {
            return Err(SourceStreamSemanticError {
                semantic_unit: TransformSemanticUnit::Usage,
                reason_code: TransformReasonCode::UsageSnapshotRegressed,
            });
        }
        context.set_gemini_source_usage(usage);
    }
    record_source_synthesis(
        TransformSemanticUnit::Lifecycle,
        TransformReasonCode::SyntheticCorrelationId,
    );
    record_source_synthesis(
        TransformSemanticUnit::Model,
        TransformReasonCode::SyntheticEnvelope,
    );
    Ok(())
}

fn validate_ollama_stream_frame(value: &Value) -> Result<(), SourceStreamSemanticError> {
    require_object(value, TransformSemanticUnit::StreamFrame)?;
    require_non_empty_string(value, "model", TransformSemanticUnit::Model)?;
    let created_at =
        require_non_empty_string(value, "created_at", TransformSemanticUnit::Lifecycle)?;
    DateTime::parse_from_rfc3339(created_at)
        .map_err(|_| SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle))?;
    let done = value
        .get("done")
        .and_then(Value::as_bool)
        .ok_or_else(|| SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle))?;
    if let Some(message) = value.get("message").filter(|value| !value.is_null()) {
        require_object(message, TransformSemanticUnit::StreamFrame)?;
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Role,
            ));
        }
        if message.get("content").and_then(Value::as_str).is_none() {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Text,
            ));
        }
        if message
            .get("images")
            .and_then(Value::as_array)
            .is_some_and(|images| !images.is_empty())
            || message
                .get("thinking")
                .is_some_and(|value| !value.is_null())
            || message
                .get("tool_calls")
                .is_some_and(|value| !value.is_null())
        {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::StreamFrame,
            ));
        }
    }
    let done_reason = value.get("done_reason").and_then(Value::as_str);
    if done {
        if done_reason.is_none() {
            record_source_synthesis(
                TransformSemanticUnit::Lifecycle,
                TransformReasonCode::SyntheticEnvelope,
            );
        } else if !matches!(done_reason, Some("stop" | "length")) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Lifecycle,
            ));
        }
    } else if done_reason.is_some() {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }
    let prompt = value
        .get("prompt_eval_count")
        .filter(|value| !value.is_null());
    let completion = value.get("eval_count").filter(|value| !value.is_null());
    if prompt.is_some() != completion.is_some() || (!done && prompt.is_some()) {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Usage,
        ));
    }
    if let Some(prompt) = prompt {
        if prompt.as_u64().is_none() || completion.and_then(Value::as_u64).is_none() {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Usage,
            ));
        }
    }
    record_source_synthesis(
        TransformSemanticUnit::Lifecycle,
        TransformReasonCode::SyntheticCorrelationId,
    );
    Ok(())
}

pub(in crate::service::transform) fn validate_openai_stream_chunk(
    chunk: &openai::OpenAiChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    if chunk.id.is_empty() {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }
    if chunk.model.is_empty() {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Model,
        ));
    }
    if chunk.object != "chat.completion.chunk" {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::StreamFrame,
        ));
    }
    if chunk.choices.len() > 1 {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }

    for choice in &chunk.choices {
        if choice.index != 0 {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        if choice
            .delta
            .role
            .as_deref()
            .is_some_and(|role| role != "assistant")
        {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Role,
            ));
        }
        if choice.delta.refusal.is_some() {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Refusal,
            ));
        }
        if choice.delta.name.is_some() {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Role,
            ));
        }
        if choice
            .delta
            .reasoning_content
            .as_deref()
            .is_some_and(|text| !text.is_empty())
            && context.current_content_block_index().is_some()
        {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }

        if let Some(tool_calls) = &choice.delta.tool_calls {
            for call in tool_calls {
                if call.type_.as_deref().is_some_and(|kind| kind != "function") {
                    return Err(SourceStreamSemanticError::unknown(
                        TransformSemanticUnit::ToolCallDelta,
                    ));
                }
                let id = call.id.as_deref().filter(|value| !value.is_empty());
                let name = call
                    .function
                    .name
                    .as_deref()
                    .filter(|value| !value.is_empty());
                let state = context
                    .openai_source_tool_calls_mut()
                    .entry(call.index)
                    .or_default();
                if let Some(id) = id {
                    if state.id.as_deref().is_some_and(|known| known != id) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state.id = Some(id.to_string());
                }
                if let Some(name) = name {
                    if state.name.as_deref().is_some_and(|known| known != name) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state.name = Some(name.to_string());
                }
                if let Some(arguments) = &call.function.arguments {
                    if !try_append_tool_arguments(&mut state.arguments, arguments) {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::ToolCallDelta,
                        ));
                    }
                }
            }
        }

        if choice.logprobs.is_some() {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Metadata,
            ));
        }
        if choice.finish_reason.as_deref().is_some_and(|reason| {
            !matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
        }) {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        if choice.finish_reason.as_deref() == Some("tool_calls") {
            if context.openai_source_tool_calls().is_empty()
                || context.openai_source_tool_calls().values().any(|state| {
                    state.id.as_deref().is_none_or(str::is_empty)
                        || state.name.as_deref().is_none_or(str::is_empty)
                        || !validate_json_object_text(&state.arguments)
                })
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
            context.openai_source_tool_calls_mut().clear();
        } else if choice.finish_reason.is_some() && !context.openai_source_tool_calls().is_empty() {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
    }
    Ok(())
}

fn validate_typed_anthropic_content_block(
    block: &anthropic::AnthropicContentBlock,
) -> Result<AnthropicActiveBlockKind, SourceStreamSemanticError> {
    match block {
        anthropic::AnthropicContentBlock::Text { .. } => Ok(AnthropicActiveBlockKind::Text),
        anthropic::AnthropicContentBlock::Thinking { .. } => Ok(AnthropicActiveBlockKind::Thinking),
        anthropic::AnthropicContentBlock::ToolUse { id, name, input } => {
            if id.is_empty() || name.is_empty() || !input.is_object() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCall,
                ));
            }
            Ok(AnthropicActiveBlockKind::ToolUse)
        }
    }
}

pub(in crate::service::transform) fn validate_anthropic_stream_event(
    event: &anthropic::AnthropicEvent,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    if context.anthropic_session_mut().source_message_stopped
        || context.anthropic_session_mut().source_error_seen
    {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }
    match event {
        anthropic::AnthropicEvent::MessageStart { message } => {
            if context.anthropic_session_mut().source_message_started
                || context.anthropic_session_mut().source_message_delta_seen
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if message.id.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if message.model.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Model,
                ));
            }
            if message.type_ != "message" || message.role != "assistant" {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Role,
                ));
            }
            if message
                .content
                .as_ref()
                .is_some_and(|blocks| !blocks.is_empty())
                || message.stop_reason.is_some()
                || message.stop_sequence.is_some()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if message
                .usage
                .as_ref()
                .is_some_and(|usage| anthropic::anthropic_usage_to_unified(usage).is_none())
            {
                return Err(SourceStreamSemanticError::usage_overflow());
            }
            context.anthropic_session_mut().source_message_started = true;
        }
        anthropic::AnthropicEvent::ContentBlockStart {
            index,
            content_block,
        } => {
            if !context.anthropic_session_mut().source_message_started
                || context.anthropic_session_mut().source_message_delta_seen
                || !context.anthropic_active_blocks().is_empty()
                || context.anthropic_active_blocks().contains_key(index)
                || *index != context.anthropic_session_mut().source_next_block_index
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let kind = validate_typed_anthropic_content_block(content_block)?;
            if kind == AnthropicActiveBlockKind::ToolUse
                && matches!(
                    content_block,
                    anthropic::AnthropicContentBlock::ToolUse { input, .. }
                        if input.as_object().is_some_and(|input| !input.is_empty())
                )
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            context.anthropic_session_mut().source_next_block_index = context
                .anthropic_session_mut()
                .source_next_block_index
                .checked_add(1)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?;
        }
        anthropic::AnthropicEvent::ContentBlockDelta { index, delta } => {
            if context.anthropic_session_mut().source_message_delta_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let active_kind = context
                .anthropic_active_blocks()
                .get(index)
                .map(|block| block.kind)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?;
            let valid = match (active_kind, delta) {
                (
                    AnthropicActiveBlockKind::Text,
                    anthropic::AnthropicContentDelta::TextDelta { .. },
                )
                | (
                    AnthropicActiveBlockKind::ToolUse,
                    anthropic::AnthropicContentDelta::InputJsonDelta { .. },
                ) => true,
                (
                    AnthropicActiveBlockKind::Thinking,
                    anthropic::AnthropicContentDelta::ThinkingDelta { .. },
                ) => !context
                    .anthropic_active_blocks()
                    .get(index)
                    .is_some_and(|block| block.signature_seen),
                (
                    AnthropicActiveBlockKind::Thinking,
                    anthropic::AnthropicContentDelta::SignatureDelta { signature },
                ) => {
                    !signature.is_empty()
                        && !context
                            .anthropic_active_blocks()
                            .get(index)
                            .is_some_and(|block| block.signature_seen)
                }
                _ => false,
            };
            if !valid {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if matches!(
                delta,
                anthropic::AnthropicContentDelta::SignatureDelta { .. }
            ) && let Some(block) = context.anthropic_active_blocks_mut().get_mut(index)
            {
                block.signature_seen = true;
            }
        }
        anthropic::AnthropicEvent::ContentBlockStop { index } => {
            if context.anthropic_session_mut().source_message_delta_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let block = context
                .anthropic_active_blocks()
                .get(index)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?;
            if block.kind == AnthropicActiveBlockKind::ToolUse
                && !validate_json_object_text(&block.text)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
            if block.kind == AnthropicActiveBlockKind::Thinking && !block.signature_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ReasoningDelta,
                ));
            }
        }
        anthropic::AnthropicEvent::MessageDelta { delta, usage } => {
            if !context.anthropic_session_mut().source_message_started
                || context.anthropic_session_mut().source_message_delta_seen
                || !context.anthropic_active_blocks().is_empty()
                || delta.stop_reason.is_none()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if delta.stop_reason.as_deref().is_some_and(|reason| {
                !matches!(
                    reason,
                    "end_turn"
                        | "stop_sequence"
                        | "tool_use"
                        | "max_tokens"
                        | "model_context_window_exceeded"
                        | "refusal"
                )
            }) {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if usage
                .as_ref()
                .or(delta.usage.as_ref())
                .is_some_and(|usage| anthropic::anthropic_stream_usage_to_unified(usage).is_none())
            {
                return Err(SourceStreamSemanticError::usage_overflow());
            }
            context.anthropic_session_mut().source_message_delta_seen = true;
        }
        anthropic::AnthropicEvent::MessageStop => {
            if !context.anthropic_session_mut().source_message_started
                || !context.anthropic_session_mut().source_message_delta_seen
                || !context.anthropic_active_blocks().is_empty()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            context.anthropic_session_mut().source_message_stopped = true;
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        anthropic::AnthropicEvent::Ping => {
            if !context.anthropic_session_mut().source_message_started {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle)
        }
        anthropic::AnthropicEvent::Error { error } => {
            if error.is_null() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::StreamError,
                ));
            }
            context.anthropic_session_mut().source_error_seen = true;
        }
        anthropic::AnthropicEvent::Unknown => {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::StreamFrame,
            ));
        }
    }
    Ok(())
}

fn typed_responses_item_id(item: &responses::ItemField) -> Result<&str, SourceStreamSemanticError> {
    let id = match item {
        responses::ItemField::Message(item) if item._type == "message" => &item.id,
        responses::ItemField::FunctionCall(item) if item._type == "function_call" => &item.id,
        responses::ItemField::FunctionCallOutput(item) if item._type == "function_call_output" => {
            &item.id
        }
        responses::ItemField::Reasoning(item) if item._type == "reasoning" => &item.id,
        responses::ItemField::Unknown(_) => {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::ResponsesUnknownItem,
            ));
        }
        _ => {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::ResponsesUnknownItem,
            ));
        }
    };
    if id.is_empty() {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }
    Ok(id)
}

fn typed_responses_message_content(
    item: &responses::Message,
) -> Result<(String, String), SourceStreamSemanticError> {
    let mut text = String::new();
    let mut refusal = String::new();
    for part in &item.content {
        match part {
            responses::ItemContentPart::OutputText {
                text: part_text,
                annotations,
                logprobs,
            } => {
                text.push_str(part_text);
                if !annotations.is_empty() {
                    record_source_drop(TransformSemanticUnit::Metadata);
                }
                if logprobs
                    .as_ref()
                    .is_some_and(|logprobs| !logprobs.is_empty())
                {
                    return Err(SourceStreamSemanticError::unknown(
                        TransformSemanticUnit::Metadata,
                    ));
                }
            }
            responses::ItemContentPart::Text { text: part_text }
            | responses::ItemContentPart::SummaryText { text: part_text }
            | responses::ItemContentPart::ReasoningText { text: part_text } => {
                text.push_str(part_text);
            }
            responses::ItemContentPart::Refusal {
                refusal: part_refusal,
            } => {
                refusal.push_str(part_refusal);
            }
            responses::ItemContentPart::InputText { .. }
            | responses::ItemContentPart::InputImage { .. }
            | responses::ItemContentPart::InputAudio { .. }
            | responses::ItemContentPart::InputFile { .. } => {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::ResponsesUnknownItem,
                ));
            }
        }
    }
    Ok((text, refusal))
}

pub(in crate::service::transform) fn validate_responses_stream_chunk(
    chunk: &responses::ResponsesChunkResponse,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    use responses::{ItemField, ResponseStatus, ResponsesStreamEvent};

    let state = context.responses_mut();
    if state.source_terminal_seen {
        return Err(SourceStreamSemanticError::invalid(
            TransformSemanticUnit::Lifecycle,
        ));
    }
    if let Some(sequence_number) = chunk.sequence_number {
        if state
            .source_last_sequence_number
            .is_some_and(|previous| sequence_number <= previous)
        {
            return Err(SourceStreamSemanticError::invalid(
                TransformSemanticUnit::Lifecycle,
            ));
        }
        state.source_last_sequence_number = Some(sequence_number);
    }
    match &chunk.event {
        ResponsesStreamEvent::Item(item) => {
            if chunk.id.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if chunk.model.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Model,
                ));
            }
            typed_responses_item_id(item)?;
            if let ItemField::FunctionCall(call) = item {
                if !validate_json_object_text(&call.arguments) {
                    return Err(SourceStreamSemanticError::invalid(
                        TransformSemanticUnit::ToolCallDelta,
                    ));
                }
            }
            record_source_synthesis(
                TransformSemanticUnit::Lifecycle,
                TransformReasonCode::SyntheticEnvelope,
            );
            record_source_synthesis(
                TransformSemanticUnit::Lifecycle,
                TransformReasonCode::SyntheticIndex,
            );
        }
        ResponsesStreamEvent::ResponseCreated { response } => {
            if state.source_created_seen || response.id.is_empty() || response.model.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if response.status != ResponseStatus::InProgress {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state.source_created_seen = true;
            state.source_response_id = Some(response.id.clone());
            state.source_response_model = Some(response.model.clone());
        }
        ResponsesStreamEvent::ResponseQueued { response }
        | ResponsesStreamEvent::ResponseInProgress { response } => {
            if !state.source_created_seen
                || response.id.is_empty()
                || response.model.is_empty()
                || state.source_response_id.as_deref() != Some(response.id.as_str())
                || state.source_response_model.as_deref() != Some(response.model.as_str())
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let (expected_status, already_seen) =
                if matches!(&chunk.event, ResponsesStreamEvent::ResponseQueued { .. }) {
                    (ResponseStatus::Queued, &mut state.source_queued_seen)
                } else {
                    (
                        ResponseStatus::InProgress,
                        &mut state.source_in_progress_seen,
                    )
                };
            if *already_seen || response.status != expected_status || response.error.is_some() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            *already_seen = true;
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::OutputItemAdded { output_index, item } => {
            if !state.source_created_seen {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let id = typed_responses_item_id(item)?;
            if state
                .source_output_item_ids
                .insert(*output_index, id.to_string())
                .is_some()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            match item {
                ItemField::Message(message) => {
                    let (text, refusal) = typed_responses_message_content(message)?;
                    if !text.is_empty() || !refusal.is_empty() {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                    state
                        .source_output_text
                        .insert(id.to_string(), String::new());
                    state
                        .source_refusal_text
                        .insert(id.to_string(), String::new());
                }
                ItemField::FunctionCall(call) => {
                    if call.call_id.is_empty() || call.name.is_empty() || !call.arguments.is_empty()
                    {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::ToolCall,
                        ));
                    }
                    state
                        .source_tool_arguments
                        .insert(id.to_string(), String::new());
                }
                ItemField::FunctionCallOutput(_) | ItemField::Reasoning(_) => {}
                ItemField::Unknown(_) => unreachable!("item identity rejected unknown item"),
            }
        }
        ResponsesStreamEvent::ContentBlockDelta {
            item_index,
            item_id,
            part_index,
            text,
            ..
        } if item_id.is_some() => {
            let Some(output_index) = item_index else {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            };
            let item_id = item_id.as_deref().expect("guarded item id");
            if part_index.is_none()
                || state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state
                .source_output_text
                .get_mut(item_id)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?
                .push_str(text);
        }
        ResponsesStreamEvent::ToolCallArgumentsDelta {
            item_index,
            item_id,
            arguments,
            ..
        } if item_id.is_some() => {
            let Some(output_index) = item_index else {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            };
            let item_id = item_id.as_deref().expect("guarded item id");
            if state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let arguments_buffer =
                state
                    .source_tool_arguments
                    .get_mut(item_id)
                    .ok_or_else(|| {
                        SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                    })?;
            if !try_append_tool_arguments(arguments_buffer, arguments) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
        }
        ResponsesStreamEvent::ToolCallArgumentsDone {
            item_index,
            item_id,
            arguments,
            ..
        } => {
            let Some(output_index) = item_index else {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            };
            let Some(item_id) = item_id.as_deref() else {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            };
            if state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if state.source_tool_arguments.get(item_id).map(String::as_str) != Some(arguments)
                || !validate_json_object_text(arguments)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCallDelta,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::OutputItemDone { output_index, item } => {
            let id = typed_responses_item_id(item)?;
            if state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            match item {
                ItemField::Message(message) => {
                    let (text, refusal) = typed_responses_message_content(message)?;
                    if state.source_output_text.get(id) != Some(&text)
                        || state.source_refusal_text.get(id) != Some(&refusal)
                    {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::Lifecycle,
                        ));
                    }
                }
                ItemField::FunctionCall(call) => {
                    if state.source_tool_arguments.get(id).map(String::as_str)
                        != Some(call.arguments.as_str())
                        || !validate_json_object_text(&call.arguments)
                    {
                        return Err(SourceStreamSemanticError::invalid(
                            TransformSemanticUnit::ToolCallDelta,
                        ));
                    }
                }
                ItemField::FunctionCallOutput(_) | ItemField::Reasoning(_) => {}
                ItemField::Unknown(_) => unreachable!("item identity rejected unknown item"),
            }
            state.source_output_items_done.insert(*output_index);
        }
        ResponsesStreamEvent::ContentPartAdded {
            item_id,
            content_index,
        } => {
            if item_id.is_empty()
                || !state
                    .source_output_item_ids
                    .values()
                    .any(|known_id| known_id == item_id)
                || !state
                    .source_content_parts
                    .insert((item_id.clone(), *content_index))
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::ContentPartDone {
            item_id,
            content_index,
        } => {
            if !state
                .source_content_parts
                .remove(&(item_id.clone(), *content_index))
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::OutputTextDone {
            item_id,
            output_index,
            content_index: _,
            text,
        } => {
            if state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
                || state.source_output_text.get(item_id) != Some(text)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::RefusalDelta {
            item_id,
            output_index,
            delta,
            ..
        } => {
            if state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state
                .source_refusal_text
                .get_mut(item_id)
                .ok_or_else(|| {
                    SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                })?
                .push_str(delta);
        }
        ResponsesStreamEvent::RefusalDone {
            item_id,
            output_index,
            refusal,
            ..
        } => {
            if state.source_output_items_done.contains(output_index)
                || state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
                || state.source_refusal_text.get(item_id) != Some(refusal)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::AnnotationAdded {
            item_id,
            output_index,
            annotation,
            ..
        } => {
            if state
                .source_output_item_ids
                .get(output_index)
                .map(String::as_str)
                != Some(item_id)
                || !annotation.is_object()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Metadata,
                ));
            }
            record_source_drop(TransformSemanticUnit::Metadata);
        }
        ResponsesStreamEvent::ReasoningSummaryPartAdded {
            item_id,
            summary_index,
        } => {
            if item_id.is_empty()
                || !state
                    .source_output_item_ids
                    .values()
                    .any(|known_id| known_id == item_id)
                || !state
                    .source_reasoning_parts
                    .insert((item_id.clone(), *summary_index))
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state
                .source_reasoning_text
                .insert((item_id.clone(), *summary_index), String::new());
        }
        ResponsesStreamEvent::ReasoningSummaryPartDone {
            item_id,
            summary_index,
        } => {
            if !state
                .source_reasoning_parts
                .remove(&(item_id.clone(), *summary_index))
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
        }
        ResponsesStreamEvent::ReasoningDelta {
            item_index,
            item_id: Some(item_id),
            part_index: Some(part_index),
            text,
            ..
        } => {
            if let Some(output_index) = item_index {
                if state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
                {
                    return Err(SourceStreamSemanticError::invalid(
                        TransformSemanticUnit::Lifecycle,
                    ));
                }
                state
                    .source_reasoning_text
                    .entry((item_id.clone(), *part_index))
                    .or_default()
                    .push_str(text);
            } else {
                state
                    .source_reasoning_text
                    .get_mut(&(item_id.clone(), *part_index))
                    .ok_or_else(|| {
                        SourceStreamSemanticError::invalid(TransformSemanticUnit::Lifecycle)
                    })?
                    .push_str(text);
            }
        }
        ResponsesStreamEvent::ReasoningDone {
            item_id,
            item_index,
            part_index,
            text,
            ..
        } => {
            if let Some(output_index) = item_index
                && state
                    .source_output_item_ids
                    .get(output_index)
                    .map(String::as_str)
                    != Some(item_id)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if state
                .source_reasoning_text
                .get(&(item_id.clone(), *part_index))
                != Some(text)
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            record_source_no_output(TransformSemanticUnit::Lifecycle);
        }
        ResponsesStreamEvent::ResponseCompleted { response }
        | ResponsesStreamEvent::ResponseIncomplete { response } => {
            if !state.source_created_seen
                || response.id.is_empty()
                || response.model.is_empty()
                || state.source_response_id.as_deref() != Some(response.id.as_str())
                || state.source_response_model.as_deref() != Some(response.model.as_str())
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            let expected = if matches!(&chunk.event, ResponsesStreamEvent::ResponseCompleted { .. })
            {
                ResponseStatus::Completed
            } else {
                ResponseStatus::Incomplete
            };
            if response.status != expected
                || response.error.is_some()
                || response
                    .metadata
                    .as_object()
                    .is_some_and(|metadata| !metadata.is_empty())
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if response.status == ResponseStatus::Incomplete {
                let reason = response
                    .incomplete_details
                    .as_ref()
                    .map(|details| details.reason.as_str());
                if !matches!(
                    reason,
                    Some("max_tokens" | "max_output_tokens" | "content_filter")
                ) {
                    return Err(SourceStreamSemanticError {
                        semantic_unit: TransformSemanticUnit::Lifecycle,
                        reason_code: TransformReasonCode::UnknownIncompleteReason,
                    });
                }
            }
            if !state.source_content_parts.is_empty()
                || !state.source_reasoning_parts.is_empty()
                || state.source_output_items_done.len() != state.source_output_item_ids.len()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state.source_terminal_seen = true;
        }
        ResponsesStreamEvent::ResponseFailed { response } => {
            if !state.source_created_seen
                || response.id.is_empty()
                || response.model.is_empty()
                || state.source_response_id.as_deref() != Some(response.id.as_str())
                || state.source_response_model.as_deref() != Some(response.model.as_str())
                || response.status != ResponseStatus::Failed
                || response.error.is_none()
            {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            state.source_terminal_seen = true;
        }
        ResponsesStreamEvent::MessageStart { id, role } => {
            if id.as_deref().is_some_and(str::is_empty) {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::Lifecycle,
                ));
            }
            if id.is_none() {
                record_source_synthesis(
                    TransformSemanticUnit::Lifecycle,
                    TransformReasonCode::SyntheticCorrelationId,
                );
            }
            if !matches!(role, UnifiedRole::Assistant) {
                return Err(SourceStreamSemanticError::unknown(
                    TransformSemanticUnit::Role,
                ));
            }
        }
        ResponsesStreamEvent::MessageDelta { .. } => {}
        ResponsesStreamEvent::ToolCallStart { id, name, .. } => {
            if id.is_empty() || name.is_empty() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::ToolCall,
                ));
            }
        }
        ResponsesStreamEvent::MessageStop
        | ResponsesStreamEvent::ContentBlockStart { .. }
        | ResponsesStreamEvent::ContentBlockStop { .. }
        | ResponsesStreamEvent::ToolCallStop { .. }
        | ResponsesStreamEvent::ReasoningStart { .. }
        | ResponsesStreamEvent::ReasoningStop { .. } => {
            record_source_no_output(TransformSemanticUnit::Lifecycle)
        }
        ResponsesStreamEvent::ContentBlockDelta { .. }
        | ResponsesStreamEvent::ToolCallArgumentsDelta { .. }
        | ResponsesStreamEvent::ReasoningDelta { .. }
        | ResponsesStreamEvent::Usage { .. }
        | ResponsesStreamEvent::Blob { .. } => {}
        ResponsesStreamEvent::Error { error } => {
            if error.is_null() {
                return Err(SourceStreamSemanticError::invalid(
                    TransformSemanticUnit::StreamError,
                ));
            }
            state.source_terminal_seen = true;
        }
        ResponsesStreamEvent::Unknown(_) => {
            return Err(SourceStreamSemanticError::unknown(
                TransformSemanticUnit::ResponsesUnknownItem,
            ));
        }
    }
    Ok(())
}

pub(in crate::service::transform) fn validate_upstream_stream_frame(
    protocol: UpstreamProtocol,
    raw: &str,
    context: &mut StreamTransformContext<'_>,
) -> Result<(), SourceStreamSemanticError> {
    let value = serde_json::from_str::<Value>(raw)
        .map_err(|_| SourceStreamSemanticError::decode(TransformSemanticUnit::StreamFrame))?;
    match protocol {
        UpstreamProtocol::Openai => validate_openai_stream_frame(&value, context),
        UpstreamProtocol::Responses => validate_responses_stream_frame(&value, context),
        UpstreamProtocol::Anthropic => validate_anthropic_stream_frame(&value, context),
        UpstreamProtocol::Gemini => validate_gemini_stream_frame(&value, context),
        UpstreamProtocol::Ollama => validate_ollama_stream_frame(&value),
    }
}

fn item_has_semantic_content(item: &UnifiedItem) -> bool {
    match item {
        UnifiedItem::Message(message) => {
            message.content.iter().any(|part| !part.is_empty()) || !message.annotations.is_empty()
        }
        UnifiedItem::FunctionCall(call) => {
            !call.id.is_empty() || !call.name.is_empty() || !call.arguments.is_null()
        }
        UnifiedItem::FunctionCallOutput(_) => true,
        UnifiedItem::Reasoning(reasoning) => {
            reasoning.content.iter().any(|part| !part.is_empty())
                || !reasoning.annotations.is_empty()
        }
        UnifiedItem::FileReference(_) => true,
    }
}

fn audit_target_finish_reason(target: DownstreamProtocol, reason: &str) {
    let supported = match target {
        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
            matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
        }
        DownstreamProtocol::Gemini => {
            matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
        }
        DownstreamProtocol::Anthropic => {
            matches!(reason, "stop" | "length" | "tool_calls" | "content_filter")
        }
    };
    if !supported {
        record_rejection(
            TransformSemanticUnit::Lifecycle,
            TransformReasonCode::UnknownSemanticUnit,
        );
    }
}

pub(in crate::service::transform) fn audit_target_stream_events(
    target: DownstreamProtocol,
    events: &[UnifiedStreamEvent],
    context: &mut StreamTransformContext<'_>,
) {
    let target_protocol = TransformProtocol::Downstream(target);
    for (event_position, event) in events.iter().enumerate() {
        let UnifiedStreamEvent::ToolCallArgumentsDelta {
            index, arguments, ..
        } = event
        else {
            continue;
        };
        let existing_len = match target {
            DownstreamProtocol::Anthropic => context
                .anthropic_active_blocks()
                .get(index)
                .map_or(0, |state| state.text.len()),
            DownstreamProtocol::Responses => context
                .responses()
                .active_tool_calls
                .get(index)
                .map_or(0, |call| call.arguments.len()),
            DownstreamProtocol::Gemini => context
                .gemini_target_tool_calls()
                .get(index)
                .map_or(0, |state| state.arguments.len()),
            DownstreamProtocol::Openai => 0,
        };
        let prior_batch_len = events[..event_position]
            .iter()
            .filter_map(|prior| match prior {
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index: prior_index,
                    arguments,
                    ..
                } if prior_index == index => Some(arguments.len()),
                _ => None,
            })
            .sum::<usize>();
        if target != DownstreamProtocol::Openai
            && existing_len
                .saturating_add(prior_batch_len)
                .saturating_add(arguments.len())
                > MAX_STREAM_TOOL_ARGUMENT_BYTES
        {
            record_rejection(
                TransformSemanticUnit::ToolCallDelta,
                TransformReasonCode::InvalidProtocolShape,
            );
        }
    }
    if target == DownstreamProtocol::Gemini {
        let mut tool_calls = context.gemini_target_tool_calls().clone();
        for event in events {
            match event {
                UnifiedStreamEvent::ToolCallStart { index, id, name } => {
                    let state = tool_calls.entry(*index).or_default();
                    if state.id.as_ref().is_some_and(|known| known != id)
                        || state.name.as_ref().is_some_and(|known| known != name)
                    {
                        record_rejection(
                            TransformSemanticUnit::Lifecycle,
                            TransformReasonCode::InvalidProtocolShape,
                        );
                    }
                    state.id = Some(id.clone());
                    state.name = Some(name.clone());
                }
                UnifiedStreamEvent::ToolCallArgumentsDelta {
                    index,
                    id,
                    name,
                    arguments,
                    ..
                } => {
                    let state = tool_calls.entry(*index).or_default();
                    if let Some(id) = id {
                        if state.id.as_ref().is_some_and(|known| known != id) {
                            record_rejection(
                                TransformSemanticUnit::Lifecycle,
                                TransformReasonCode::InvalidProtocolShape,
                            );
                        }
                        state.id = Some(id.clone());
                    }
                    if let Some(name) = name {
                        if state.name.as_ref().is_some_and(|known| known != name) {
                            record_rejection(
                                TransformSemanticUnit::Lifecycle,
                                TransformReasonCode::InvalidProtocolShape,
                            );
                        }
                        state.name = Some(name.clone());
                    }
                    if !try_append_tool_arguments(&mut state.arguments, arguments) {
                        record_rejection(
                            TransformSemanticUnit::ToolCallDelta,
                            TransformReasonCode::InvalidProtocolShape,
                        );
                    }
                }
                UnifiedStreamEvent::ToolCallStop { index, .. } => {
                    let Some(state) = tool_calls.remove(index) else {
                        record_rejection(
                            TransformSemanticUnit::Lifecycle,
                            TransformReasonCode::InvalidProtocolShape,
                        );
                        continue;
                    };
                    if state.name.as_deref().is_none_or(str::is_empty) {
                        record_rejection(
                            TransformSemanticUnit::ToolCallDelta,
                            TransformReasonCode::InvalidProtocolShape,
                        );
                    }
                    if state.arguments.trim().is_empty() {
                        record_synthesis(
                            TransformSemanticUnit::ToolCallDelta,
                            TransformReasonCode::SyntheticEnvelope,
                        );
                    } else if !validate_json_object_text(&state.arguments) {
                        record_rejection(
                            TransformSemanticUnit::ToolCallDelta,
                            TransformReasonCode::InvalidProtocolShape,
                        );
                    }
                }
                _ => {}
            }
        }
    }
    let mut tool_starts = HashSet::new();
    for event in events {
        if let UnifiedStreamEvent::ToolCallStart { index, .. } = event {
            tool_starts.insert(*index);
        }
    }

    for event in events {
        match event {
            UnifiedStreamEvent::MessageStart { id, model, role } => {
                if *role != UnifiedRole::Assistant {
                    record_rejection(
                        TransformSemanticUnit::Role,
                        TransformReasonCode::UnknownSemanticUnit,
                    );
                }
                if id.as_deref().is_none_or(str::is_empty) {
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
                if model.as_deref().is_none_or(str::is_empty)
                    && context
                        .stream_model_clone()
                        .as_deref()
                        .is_none_or(str::is_empty)
                {
                    record_synthesis(
                        TransformSemanticUnit::Model,
                        TransformReasonCode::SyntheticEnvelope,
                    );
                }
            }
            UnifiedStreamEvent::ItemAdded { item, .. } => {
                if target != DownstreamProtocol::Responses && item_has_semantic_content(item) {
                    let redundant_tool_start = matches!(
                        item,
                        UnifiedItem::FunctionCall(call)
                            if events.iter().any(|event| matches!(event, UnifiedStreamEvent::ToolCallStart { id, name, .. } if id == &call.id && name == &call.name))
                    );
                    if !redundant_tool_start {
                        record_rejection(
                            TransformSemanticUnit::Lifecycle,
                            TransformReasonCode::UnsupportedContent,
                        );
                    } else {
                        record_no_output(TransformSemanticUnit::Lifecycle);
                    }
                }
                if matches!(item, UnifiedItem::FileReference(_)) {
                    record_rejection(
                        TransformSemanticUnit::FileData,
                        TransformReasonCode::UnsupportedContent,
                    );
                }
            }
            UnifiedStreamEvent::ItemDone { item, .. } => {
                if matches!(item, UnifiedItem::FileReference(_)) {
                    record_rejection(
                        TransformSemanticUnit::FileData,
                        TransformReasonCode::UnsupportedContent,
                    );
                } else if target != DownstreamProtocol::Responses {
                    record_no_output(TransformSemanticUnit::Lifecycle);
                }
            }
            UnifiedStreamEvent::ContentPartAdded { part, .. } => {
                if target != DownstreamProtocol::Responses
                    && part.as_ref().is_some_and(|part| !part.is_empty())
                {
                    record_rejection(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::UnsupportedContent,
                    );
                } else if part.is_none() {
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticEnvelope,
                    );
                }
            }
            UnifiedStreamEvent::ContentPartDone { .. }
            | UnifiedStreamEvent::ContentBlockStart { .. }
            | UnifiedStreamEvent::ContentBlockStop { .. }
            | UnifiedStreamEvent::ToolCallStop { .. }
            | UnifiedStreamEvent::MessageStop => {
                if matches!(
                    target,
                    DownstreamProtocol::Openai | DownstreamProtocol::Gemini
                ) {
                    record_no_output(TransformSemanticUnit::Lifecycle);
                }
            }
            UnifiedStreamEvent::ContentBlockDelta {
                item_index,
                item_id,
                ..
            } => {
                if target == DownstreamProtocol::Responses
                    && item_index.is_none()
                    && item_id.is_none()
                    && context.responses().current_item_id.is_none()
                {
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticIndex,
                    );
                }
            }
            UnifiedStreamEvent::RefusalDelta {
                item_index,
                item_id,
                ..
            } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    target_protocol,
                    TransformValueKind::Refusal,
                    "Auditing target stream refusal capability.",
                );
                if target == DownstreamProtocol::Responses
                    && item_index.is_none()
                    && item_id.is_none()
                    && context.responses().current_item_id.is_none()
                {
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticIndex,
                    );
                }
            }
            UnifiedStreamEvent::Usage { .. } => {}
            UnifiedStreamEvent::MessageDelta { finish_reason } => {
                if let Some(reason) = finish_reason {
                    audit_target_finish_reason(target, reason);
                }
            }
            UnifiedStreamEvent::ToolCallStart { id, name, .. } => {
                if id.is_empty() || name.is_empty() {
                    record_rejection(
                        TransformSemanticUnit::ToolCallDelta,
                        TransformReasonCode::InvalidProtocolShape,
                    );
                }
                apply_transform_policy(
                    TransformProtocol::Unified,
                    target_protocol,
                    TransformValueKind::ToolCallDelta,
                    "Auditing target stream tool-call capability.",
                );
            }
            UnifiedStreamEvent::ToolCallArgumentsDelta {
                index, id, name, ..
            } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    target_protocol,
                    TransformValueKind::ToolCallDelta,
                    "Auditing target stream tool-argument capability.",
                );
                if target == DownstreamProtocol::Anthropic
                    && !context.anthropic_active_blocks().contains_key(index)
                    && !tool_starts.contains(index)
                    && (id.as_deref().is_none_or(str::is_empty)
                        || name.as_deref().is_none_or(str::is_empty))
                {
                    record_rejection(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::InvalidProtocolShape,
                    );
                }
                if target == DownstreamProtocol::Responses
                    && !context.responses().active_tool_calls.contains_key(index)
                {
                    if id.as_deref().is_some_and(|id| !id.is_empty())
                        && name.as_deref().is_some_and(|name| !name.is_empty())
                    {
                        record_synthesis(
                            TransformSemanticUnit::Lifecycle,
                            TransformReasonCode::SyntheticEnvelope,
                        );
                    } else {
                        record_rejection(
                            TransformSemanticUnit::Lifecycle,
                            TransformReasonCode::InvalidProtocolShape,
                        );
                    }
                }
            }
            UnifiedStreamEvent::ReasoningDelta {
                index,
                item_index,
                item_id,
                ..
            } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    target_protocol,
                    TransformValueKind::ReasoningDelta,
                    "Auditing target stream reasoning capability.",
                );
                let output_index = item_index.unwrap_or(*index);
                if target == DownstreamProtocol::Responses
                    && item_id.is_none()
                    && !context
                        .responses()
                        .reasoning_item_ids
                        .contains_key(&output_index)
                    && !events.iter().any(|event| matches!(event, UnifiedStreamEvent::ReasoningStart { index } if *index == output_index))
                {
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticCorrelationId,
                    );
                }
            }
            UnifiedStreamEvent::ReasoningStart { .. }
            | UnifiedStreamEvent::ReasoningStop { .. } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    target_protocol,
                    TransformValueKind::ReasoningDelta,
                    "Auditing target stream reasoning capability.",
                );
            }
            UnifiedStreamEvent::ReasoningSummaryPartAdded {
                item_index,
                item_id,
                ..
            }
            | UnifiedStreamEvent::ReasoningSummaryPartDone {
                item_index,
                item_id,
                ..
            } => {
                apply_transform_policy(
                    TransformProtocol::Unified,
                    target_protocol,
                    TransformValueKind::ReasoningDelta,
                    "Auditing target stream reasoning summary capability.",
                );
                if target == DownstreamProtocol::Responses
                    && item_index.is_none()
                    && item_id.is_none()
                {
                    record_rejection(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::InvalidProtocolShape,
                    );
                }
            }
            UnifiedStreamEvent::BlobDelta { data, .. } => {
                let native_anthropic_signature = target == DownstreamProtocol::Anthropic
                    && data.get("provider").and_then(Value::as_str) == Some("anthropic")
                    && data.get("type").and_then(Value::as_str) == Some("signature_delta")
                    && data.get("signature").is_some_and(Value::is_string);
                let native_gemini_inline = target == DownstreamProtocol::Gemini
                    && data.get("type").and_then(Value::as_str) == Some("inline_data")
                    && data.get("data").is_some_and(Value::is_string);
                if !native_anthropic_signature && !native_gemini_inline {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        target_protocol,
                        TransformValueKind::BlobDelta,
                        "Auditing target stream blob capability.",
                    );
                }
                if target == DownstreamProtocol::Responses
                    && matches!(event, UnifiedStreamEvent::BlobDelta { index: None, .. })
                {
                    record_synthesis(
                        TransformSemanticUnit::Lifecycle,
                        TransformReasonCode::SyntheticIndex,
                    );
                }
            }
            UnifiedStreamEvent::Error { .. } => {
                record_rejection(
                    TransformSemanticUnit::StreamError,
                    TransformReasonCode::UnsupportedStructuredError,
                );
            }
        }
    }
}

pub(in crate::service::transform) fn audit_target_legacy_chunk(
    target: DownstreamProtocol,
    chunk: &UnifiedChunkResponse,
) {
    if target == DownstreamProtocol::Anthropic && chunk.choices.len() > 1 {
        record_rejection(
            TransformSemanticUnit::Lifecycle,
            TransformReasonCode::UnsupportedContent,
        );
    }
    if chunk.model.as_deref().is_none_or(str::is_empty) {
        record_synthesis(
            TransformSemanticUnit::Model,
            TransformReasonCode::SyntheticEnvelope,
        );
    }
    for choice in &chunk.choices {
        if choice
            .delta
            .role
            .as_ref()
            .is_some_and(|role| *role != UnifiedRole::Assistant)
        {
            record_rejection(
                TransformSemanticUnit::Role,
                TransformReasonCode::UnknownSemanticUnit,
            );
        }
        if let Some(reason) = &choice.finish_reason {
            audit_target_finish_reason(target, reason);
        }
        for part in &choice.delta.content {
            match part {
                UnifiedContentPartDelta::TextDelta { .. } => {}
                UnifiedContentPartDelta::ReasoningDelta { .. } => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Downstream(target),
                        TransformValueKind::ReasoningDelta,
                        "Auditing target legacy reasoning delta capability.",
                    );
                }
                UnifiedContentPartDelta::ImageDelta { .. } => {
                    if target == DownstreamProtocol::Responses {
                        record_fact(
                            TransformSemanticUnit::ImageDelta,
                            TransformOutcomeKind::Lossless,
                            TransformAction::Send,
                            TransformReasonCode::LosslessConversion,
                        );
                    } else {
                        apply_transform_policy(
                            TransformProtocol::Unified,
                            TransformProtocol::Downstream(target),
                            TransformValueKind::ImageDelta,
                            "Auditing target legacy image delta capability.",
                        );
                    }
                }
                UnifiedContentPartDelta::ToolCallDelta(tool_call) => {
                    apply_transform_policy(
                        TransformProtocol::Unified,
                        TransformProtocol::Downstream(target),
                        TransformValueKind::ToolCallDelta,
                        "Auditing target legacy tool-call delta capability.",
                    );
                    if target == DownstreamProtocol::Gemini {
                        match (&tool_call.name, &tool_call.arguments) {
                            (Some(name), Some(arguments))
                                if !name.is_empty() && validate_json_object_text(arguments) => {}
                            _ => record_rejection(
                                TransformSemanticUnit::ToolCallDelta,
                                TransformReasonCode::InvalidProtocolShape,
                            ),
                        }
                    }
                }
            }
        }
    }
    if target != DownstreamProtocol::Gemini
        && let Some(metadata) = chunk.provider_session_metadata.as_ref()
    {
        if let Some(gemini) = metadata.gemini.as_ref() {
            if gemini.candidates.iter().any(|candidate| {
                candidate.thought_signature_present
                    || candidate
                        .tool_associations
                        .iter()
                        .any(|association| association.thought_signature.is_some())
            }) {
                record_fact(
                    TransformSemanticUnit::Metadata,
                    TransformOutcomeKind::ControlledLossMinor,
                    TransformAction::Drop,
                    TransformReasonCode::ThoughtSignatureNotPortable,
                );
            }
            if gemini.prompt_feedback.is_some()
                || gemini.candidates.iter().any(|candidate| {
                    !candidate.safety_ratings.is_empty() || candidate.token_count.is_some()
                })
            {
                record_fact(
                    TransformSemanticUnit::Metadata,
                    TransformOutcomeKind::ControlledLossMinor,
                    TransformAction::Drop,
                    TransformReasonCode::UnsupportedContent,
                );
            }
            if gemini
                .candidates
                .iter()
                .any(|candidate| candidate.citation_metadata.is_some())
            {
                record_rejection(
                    TransformSemanticUnit::Metadata,
                    TransformReasonCode::UnsupportedContent,
                );
            }
        }
        if metadata.responses.is_some() || metadata.anthropic.is_some() {
            record_rejection(
                TransformSemanticUnit::Metadata,
                TransformReasonCode::UnsupportedContent,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::service::transform::diagnostics::capture_transform_diagnostics;
    use crate::service::transform::stream::StreamTransformer;

    fn validate(protocol: UpstreamProtocol, value: Value) -> Result<(), SourceStreamSemanticError> {
        let mut transformer = StreamTransformer::new(protocol, DownstreamProtocol::Openai);
        validate_upstream_stream_frame(
            protocol,
            &value.to_string(),
            &mut transformer.stream_context(),
        )
    }

    #[test]
    fn gemini_stream_tool_calls_require_a_stable_correlation_seed() {
        let without_seed = validate(
            UpstreamProtocol::Gemini,
            json!({
                "candidates":[{"index":0,"content":{"role":"model","parts":[{
                    "functionCall":{"name":"weather","args":{"city":"Paris"}},
                    "thoughtSignature":"private-signature"
                }]}}]
            }),
        )
        .expect_err("legacy tool call without an id or responseId must reject");
        assert_eq!(
            without_seed.reason_code,
            TransformReasonCode::ToolCorrelationSeedRequired
        );

        validate(
            UpstreamProtocol::Gemini,
            json!({
                "responseId":"gemini-response-1",
                "candidates":[{"index":0,"content":{"role":"model","parts":[{
                    "functionCall":{"name":"weather","args":{"city":"Paris"}},
                    "thoughtSignature":"private-signature"
                }]}}]
            }),
        )
        .expect("responseId is a stable legacy tool-call correlation seed");
    }

    #[test]
    fn source_stream_audit_rejects_unknown_semantics_for_all_wire_families() {
        let cases = [
            (
                UpstreamProtocol::Openai,
                json!({"id":"c","object":"chat.completion.chunk","created":1,"model":"m","choices":[{"index":0,"delta":{"audio":{"id":"a"}},"finish_reason":null}]}),
            ),
            (
                UpstreamProtocol::Responses,
                json!({"type":"response.future.delta"}),
            ),
            (UpstreamProtocol::Anthropic, json!({"type":"future_event"})),
            (
                UpstreamProtocol::Gemini,
                json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"futurePart":{}}]}}]}),
            ),
            (
                UpstreamProtocol::Ollama,
                json!({"model":"m","created_at":"2026-08-11T00:00:00Z","message":{"role":"assistant","content":"","thinking":"secret"},"done":false}),
            ),
        ];
        for (protocol, value) in cases {
            assert_eq!(
                validate(protocol, value)
                    .expect_err("unknown stream semantic must reject")
                    .reason_code,
                TransformReasonCode::UnknownSemanticUnit
            );
        }
    }

    #[test]
    fn source_stream_audit_classifies_legal_synthesis_without_public_diagnostics() {
        let mut transformer =
            StreamTransformer::new(UpstreamProtocol::Gemini, DownstreamProtocol::Responses);
        let (_, summary) = capture_transform_diagnostics(|| {
            validate_upstream_stream_frame(
                UpstreamProtocol::Gemini,
                &json!({
                    "candidates":[{"content":{"role":"model","parts":[{"text":"ok"}]}}]
                })
                .to_string(),
                &mut transformer.stream_context(),
            )
        });
        assert!(
            summary
                .action_counts
                .contains_key(&TransformAction::Synthesize)
        );
        assert!(
            summary
                .facts
                .iter()
                .all(|fact| fact.phase == TransformPhase::StreamDecode)
        );
    }

    #[test]
    fn source_stream_audit_rejects_invalid_lifecycle_for_all_wire_families() {
        let mut openai =
            StreamTransformer::new(UpstreamProtocol::Openai, DownstreamProtocol::Responses);
        validate_upstream_stream_frame(
            UpstreamProtocol::Openai,
            &json!({
                "id":"c",
                "object":"chat.completion.chunk",
                "created":1,
                "model":"m",
                "choices":[{
                    "index":0,
                    "delta":{"tool_calls":[{
                        "index":0,
                        "id":"call_1",
                        "type":"function",
                        "function":{"name":"lookup","arguments":"{\"city\":"}
                    }]},
                    "finish_reason":null
                }]
            })
            .to_string(),
            &mut openai.stream_context(),
        )
        .expect("partial OpenAI tool arguments remain valid until terminal correlation");
        let openai_error = validate_upstream_stream_frame(
            UpstreamProtocol::Openai,
            &json!({
                "id":"c",
                "object":"chat.completion.chunk",
                "created":1,
                "model":"m",
                "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]
            })
            .to_string(),
            &mut openai.stream_context(),
        )
        .expect_err("terminal OpenAI tool arguments must form a JSON object");
        assert_eq!(
            openai_error.semantic_unit,
            TransformSemanticUnit::ToolCallDelta
        );

        let cases = [
            (
                UpstreamProtocol::Responses,
                json!({
                    "type":"response.output_text.delta",
                    "item_id":"msg_1",
                    "output_index":0,
                    "content_index":0,
                    "delta":"orphan"
                }),
                TransformSemanticUnit::Lifecycle,
            ),
            (
                UpstreamProtocol::Anthropic,
                json!({
                    "type":"content_block_delta",
                    "index":0,
                    "delta":{"type":"text_delta","text":"orphan"}
                }),
                TransformSemanticUnit::Lifecycle,
            ),
            (
                UpstreamProtocol::Gemini,
                json!({
                    "candidates":[
                        {"index":0,"content":{"role":"model","parts":[{"text":"a"}]}},
                        {"index":0,"content":{"role":"model","parts":[{"text":"b"}]}}
                    ]
                }),
                TransformSemanticUnit::Lifecycle,
            ),
            (
                UpstreamProtocol::Ollama,
                json!({
                    "model":"m",
                    "created_at":"2026-08-11T00:00:00Z",
                    "message":{"role":"assistant","content":""},
                    "done":true,
                    "done_reason":"stop",
                    "prompt_eval_count":1
                }),
                TransformSemanticUnit::Usage,
            ),
        ];
        for (protocol, value, semantic_unit) in cases {
            let error = validate(protocol, value)
                .expect_err("invalid source lifecycle must reject before DTO conversion");
            assert_eq!(error.reason_code, TransformReasonCode::InvalidProtocolShape);
            assert_eq!(error.semantic_unit, semantic_unit);
        }
    }

    #[test]
    fn target_stream_audit_accepts_openai_reasoning_channel() {
        let mut transformer =
            StreamTransformer::new(UpstreamProtocol::Responses, DownstreamProtocol::Openai);
        let (_, summary) = capture_transform_diagnostics(|| {
            audit_target_stream_events(
                DownstreamProtocol::Openai,
                &[UnifiedStreamEvent::ReasoningDelta {
                    index: 0,
                    item_index: None,
                    item_id: None,
                    part_index: None,
                    text: "hidden".to_string(),
                }],
                &mut transformer.stream_context(),
            )
        });
        assert!(
            !summary
                .outcome_counts
                .contains_key(&TransformOutcomeKind::ControlledLossMinor)
        );
        assert!(!summary.action_counts.contains_key(&TransformAction::Drop));
        assert!(!summary.action_counts.contains_key(&TransformAction::Reject));
    }
}
