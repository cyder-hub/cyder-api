use chrono::Utc;
use serde_json::{Value, json};

use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::unified::*;

use super::payload::*;
use super::response::*;

pub(super) fn build_formal_responses_message_item(
    item_id: &str,
    role: UnifiedRole,
    text: &str,
    status: MessageStatus,
) -> ItemField {
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(ItemContentPart::OutputText {
            text: text.to_string(),
            annotations: Vec::new(),
            logprobs: None,
        });
    }

    ItemField::Message(Message {
        _type: "message".to_string(),
        id: item_id.to_string(),
        status,
        role: unified_role_to_message(role),
        content,
    })
}

pub(super) fn collect_completed_output_items(state: &StreamTransformContext<'_>) -> Vec<ItemField> {
    state
        .responses()
        .completed_output
        .values()
        .cloned()
        .collect()
}

pub(super) fn clear_current_message_item(state: &mut StreamTransformContext<'_>) {
    state.responses_mut().current_item_id = None;
    state.responses_mut().current_item_role = None;
    state.responses_mut().output_text.clear();
}

pub(super) fn complete_current_message_item(
    state: &mut StreamTransformContext<'_>,
) -> Option<(u32, ItemField)> {
    let output_index = state.responses().current_output_index;
    let item_id = state.responses().current_item_id.clone()?;
    let role = state.responses().current_item_role.clone()?;
    let output_text = state.responses().output_text.clone();
    let item =
        build_formal_responses_message_item(&item_id, role, &output_text, MessageStatus::Completed);
    state
        .responses_mut()
        .completed_output
        .insert(output_index, item.clone());
    clear_current_message_item(state);
    Some((output_index, item))
}

pub(super) fn next_responses_sequence_number(state: &mut StreamTransformContext<'_>) -> u64 {
    let responses = state.responses_mut();
    let sequence_number = responses.next_sequence_number;
    responses.next_sequence_number = responses.next_sequence_number.saturating_add(1);
    sequence_number
}

pub(super) fn finalize_public_responses_stream_frame(
    mut frame: Value,
    state: &mut StreamTransformContext<'_>,
) -> Value {
    let Some(obj) = frame.as_object_mut() else {
        return frame;
    };

    let Some(event_type) = obj
        .get("type")
        .and_then(Value::as_str)
        .map(ToString::to_string)
    else {
        return frame;
    };

    obj.insert(
        "sequence_number".to_string(),
        json!(next_responses_sequence_number(state)),
    );

    if event_type == "response.output_text.delta" && !obj.contains_key("logprobs") {
        obj.insert("logprobs".to_string(), json!([]));
    }

    frame
}

pub(super) fn default_text_output_part(text: impl Into<String>) -> Value {
    json!({
        "type": "output_text",
        "text": text.into(),
        "annotations": [],
        "logprobs": []
    })
}

pub(super) fn default_reasoning_summary_part(text: impl Into<String>) -> Value {
    json!({
        "type": "summary_text",
        "text": text.into()
    })
}

pub(super) fn resolve_responses_stream_item_identity(
    state: &mut StreamTransformContext<'_>,
    item_index: Option<u32>,
    item_id: Option<String>,
    prefix: &str,
) -> (u32, String) {
    let output_index = item_index.unwrap_or_else(|| {
        let next = state.responses_mut().next_output_index;
        state.responses_mut().next_output_index = next.saturating_add(1);
        next
    });
    let item_id =
        item_id.unwrap_or_else(|| format!("{prefix}_{}", crate::utils::ID_GENERATOR.generate_id()));
    state
        .responses_mut()
        .output_item_ids
        .insert(output_index, item_id.clone());
    if output_index >= state.responses_mut().next_output_index {
        state.responses_mut().next_output_index = output_index.saturating_add(1);
    }
    (output_index, item_id)
}

pub(super) fn item_field_to_formal_responses_item(
    item: &UnifiedItem,
    item_id: &str,
) -> Option<ItemField> {
    match item {
        UnifiedItem::Message(message) => Some(ItemField::Message(Message {
            _type: "message".to_string(),
            id: item_id.to_string(),
            status: MessageStatus::InProgress,
            role: unified_role_to_message_role(message.role.clone()),
            content: Vec::new(),
        })),
        UnifiedItem::FunctionCall(call) => Some(ItemField::FunctionCall(FunctionCall {
            _type: "function_call".to_string(),
            id: item_id.to_string(),
            call_id: call.id.clone(),
            name: call.name.clone(),
            arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
            status: MessageStatus::InProgress,
        })),
        UnifiedItem::FunctionCallOutput(output) => {
            Some(ItemField::FunctionCallOutput(FunctionCallOutput {
                _type: "function_call_output".to_string(),
                id: item_id.to_string(),
                call_id: output.tool_call_id.clone(),
                output: unified_tool_result_to_function_output_payload(output.output.clone()),
                status: MessageStatus::Completed,
            }))
        }
        UnifiedItem::Reasoning(_) => Some(ItemField::Reasoning(ReasoningBody {
            _type: "reasoning".to_string(),
            id: item_id.to_string(),
            content: None,
            summary: Vec::new(),
            encrypted_content: None,
        })),
        UnifiedItem::FileReference(_) => None,
    }
}

pub(super) fn build_formal_responses_response(
    state: &mut StreamTransformContext<'_>,
    status: ResponseStatus,
    incomplete_details: Option<IncompleteDetails>,
    output: Vec<ItemField>,
) -> ResponsesResponse {
    let stream_id = state.get_or_generate_stream_id();
    let stream_model = state.get_or_default_stream_model();
    let usage = state.usage_cache_clone().map(|usage| Usage {
        input_tokens: usage.input_tokens as u32,
        output_tokens: usage.output_tokens as u32,
        total_tokens: usage.total_tokens as u32,
        input_tokens_details: InputTokensDetails {
            cached_tokens: usage.cached_tokens as u32,
        },
        output_tokens_details: OutputTokensDetails {
            reasoning_tokens: usage.reasoning_tokens as u32,
        },
    });

    let mut metadata = json!({});
    if let Some(finish_reason) = state.finish_reason_cache_clone() {
        metadata["finish_reason"] = Value::String(finish_reason);
    }

    ResponsesResponse {
        id: stream_id,
        object: ResponseObject::Response,
        created_at: Utc::now().timestamp(),
        completed_at: matches!(status, ResponseStatus::Completed).then_some(Utc::now().timestamp()),
        status,
        incomplete_details,
        model: stream_model,
        previous_response_id: None,
        instructions: None,
        output,
        error: None,
        tools: Vec::new(),
        tool_choice: ToolChoice::Value(ToolChoiceValue::Auto),
        truncation: Truncation::Disabled,
        parallel_tool_calls: true,
        text: TextField {
            format: TextResponseFormat::Text,
            verbosity: None,
        },
        top_p: 1.0,
        presence_penalty: 0.0,
        frequency_penalty: 0.0,
        top_logprobs: 0,
        temperature: 1.0,
        reasoning: None,
        usage,
        max_output_tokens: None,
        max_tool_calls: None,
        store: false,
        background: false,
        service_tier: ServiceTier::Default,
        metadata,
        safety_identifier: None,
        prompt_cache_key: None,
    }
}

pub(super) fn encode_formal_responses_stream_event(
    event: UnifiedStreamEvent,
    state: &mut StreamTransformContext<'_>,
) -> Vec<Value> {
    let mut frames = Vec::new();

    match event {
        UnifiedStreamEvent::ItemAdded {
            item_index,
            item_id,
            item,
        } => {
            if !state.responses_mut().created_sent {
                state.responses_mut().created_sent = true;
                frames.push(json!({
                    "type": "response.created",
                    "response": build_formal_responses_response(
                        state,
                        ResponseStatus::InProgress,
                        None,
                        Vec::new()
                    )
                }));
            }

            let prefix = match &item {
                UnifiedItem::Message(_) => "msg",
                UnifiedItem::Reasoning(_) => "rs",
                UnifiedItem::FunctionCall(_) => "fc",
                UnifiedItem::FunctionCallOutput(_) => "fco",
                UnifiedItem::FileReference(_) => "file",
            };
            let (output_index, item_id) =
                resolve_responses_stream_item_identity(state, item_index, item_id, prefix);

            if let Some(item) = item_field_to_formal_responses_item(&item, &item_id) {
                frames.push(json!({
                    "type": "response.output_item.added",
                    "output_index": output_index,
                    "item": item
                }));
            }
        }
        UnifiedStreamEvent::ItemDone {
            item_index,
            item_id,
            item,
        } => {
            let output_index = item_index.unwrap_or(state.responses_mut().current_output_index);
            let item_id = item_id
                .or_else(|| {
                    state
                        .responses_mut()
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .unwrap_or_else(|| format!("item_{}", crate::utils::ID_GENERATOR.generate_id()));

            let item = match item {
                UnifiedItem::Message(message) => build_formal_responses_message_item(
                    &item_id,
                    message.role,
                    &state.responses_mut().output_text,
                    MessageStatus::Completed,
                ),
                UnifiedItem::Reasoning(_) => ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: item_id.clone(),
                    content: None,
                    summary: Vec::new(),
                    encrypted_content: None,
                }),
                UnifiedItem::FunctionCall(call) => ItemField::FunctionCall(FunctionCall {
                    _type: "function_call".to_string(),
                    id: item_id.clone(),
                    call_id: call.id,
                    name: call.name,
                    arguments: serde_json::to_string(&call.arguments).unwrap_or_default(),
                    status: MessageStatus::Completed,
                }),
                UnifiedItem::FunctionCallOutput(output) => {
                    ItemField::FunctionCallOutput(FunctionCallOutput {
                        _type: "function_call_output".to_string(),
                        id: item_id.clone(),
                        call_id: output.tool_call_id,
                        output: unified_tool_result_to_function_output_payload(output.output),
                        status: MessageStatus::Completed,
                    })
                }
                UnifiedItem::FileReference(_) => return frames,
            };
            state
                .responses_mut()
                .completed_output
                .insert(output_index, item.clone());
            frames.push(json!({
                "type": "response.output_item.done",
                "output_index": output_index,
                "item": item
            }));
        }
        UnifiedStreamEvent::MessageStart { id, model, role } => {
            if let Some(id) = id {
                state.set_stream_id(id);
            }
            if let Some(model) = model {
                state.set_stream_model(model);
            }

            if state.responses_mut().current_item_id.is_some()
                && state.responses_mut().current_item_role.as_ref() == Some(&role)
                && !state.responses_mut().completion_pending
            {
                return frames;
            }

            if !state.responses_mut().created_sent {
                state.responses_mut().created_sent = true;
                frames.push(json!({
                    "type": "response.created",
                    "response": build_formal_responses_response(
                        state,
                        ResponseStatus::InProgress,
                        None,
                        Vec::new()
                    )
                }));
            }

            let item_id = format!("msg_{}", crate::utils::ID_GENERATOR.generate_id());
            let output_index = state.responses_mut().next_output_index;
            state.responses_mut().current_item_id = Some(item_id.clone());
            state.responses_mut().current_item_role = Some(role.clone());
            state.responses_mut().current_output_index = output_index;
            state.responses_mut().next_output_index = output_index.saturating_add(1);
            state
                .responses_mut()
                .output_item_ids
                .insert(output_index, item_id.clone());
            state.responses_mut().output_text.clear();
            state.responses_mut().completion_pending = false;

            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": output_index,
                "item": build_formal_responses_message_item(
                    &item_id,
                    role,
                    "",
                    MessageStatus::InProgress
                )
            }));
        }
        UnifiedStreamEvent::ContentBlockDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            let output_index = item_index.unwrap_or(state.responses_mut().current_output_index);
            let response_item_id = item_id
                .or_else(|| {
                    state
                        .responses_mut()
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .or_else(|| state.responses_mut().current_item_id.clone());
            if let Some(item_id) = response_item_id {
                state.responses_mut().output_text.push_str(&text);
                let content_index = part_index
                    .or(state.current_content_part_index())
                    .unwrap_or(index);
                frames.push(json!({
                    "type": "response.output_text.delta",
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": content_index,
                    "delta": text
                }));
            }
        }
        UnifiedStreamEvent::ContentPartAdded {
            item_index,
            item_id,
            part_index,
            part,
        } => {
            let output_index = item_index.unwrap_or(state.responses_mut().current_output_index);
            let item_id = item_id
                .or_else(|| {
                    state
                        .responses_mut()
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .or_else(|| state.responses_mut().current_item_id.clone());
            if let Some(item_id) = item_id {
                let part = part
                    .and_then(|part| serde_json::to_value(part).ok())
                    .unwrap_or_else(|| default_text_output_part(""));
                frames.push(json!({
                    "type": "response.content_part.added",
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": part_index,
                    "part": part
                }));
            }
        }
        UnifiedStreamEvent::ContentPartDone {
            item_index,
            item_id,
            part_index,
        } => {
            let output_index = item_index.unwrap_or(state.responses_mut().current_output_index);
            let item_id = item_id
                .or_else(|| {
                    state
                        .responses_mut()
                        .output_item_ids
                        .get(&output_index)
                        .cloned()
                })
                .or_else(|| state.responses_mut().current_item_id.clone());
            if let Some(item_id) = item_id {
                frames.push(json!({
                    "type": "response.content_part.done",
                    "item_id": item_id,
                    "output_index": output_index,
                    "content_index": part_index,
                    "part": default_text_output_part(state.responses_mut().output_text.clone())
                }));
            }
        }
        UnifiedStreamEvent::MessageDelta { finish_reason } => {
            state.set_finish_reason_cache(finish_reason);
            state.responses_mut().completion_pending = true;
        }
        UnifiedStreamEvent::Usage { usage } => {
            state.set_usage(usage.clone());

            if state.responses_mut().completion_pending {
                let finish_reason = state.finish_reason_cache();
                let (status, incomplete_details) =
                    response_status_from_finish_reason(finish_reason);
                let response_event_type = match status {
                    ResponseStatus::Incomplete => "response.incomplete",
                    _ => "response.completed",
                };
                if let Some((output_index, item)) = complete_current_message_item(state) {
                    frames.push(json!({
                        "type": "response.output_item.done",
                        "output_index": output_index,
                        "item": item.clone()
                    }));
                    frames.push(json!({
                        "type": response_event_type,
                        "response": build_formal_responses_response(
                            state,
                            status.clone(),
                            incomplete_details.clone(),
                            collect_completed_output_items(state)
                        )
                    }));
                } else {
                    frames.push(json!({
                        "type": response_event_type,
                        "response": build_formal_responses_response(
                            state,
                            status,
                            incomplete_details,
                            collect_completed_output_items(state)
                        )
                    }));
                }
                state.responses_mut().completion_pending = false;
            }
        }
        UnifiedStreamEvent::ToolCallStart { index, id, name } => {
            let function_call = FunctionCall {
                _type: "function_call".to_string(),
                id: id.clone(),
                call_id: id,
                name,
                arguments: String::new(),
                status: MessageStatus::InProgress,
            };
            state
                .responses_mut()
                .active_tool_calls
                .insert(index, function_call.clone());
            let item = ItemField::FunctionCall(function_call);
            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": item
            }));
        }
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            index,
            item_index,
            item_id,
            id,
            name,
            arguments,
        } => {
            let mut response_item_id = item_id
                .clone()
                .or_else(|| {
                    state
                        .responses_mut()
                        .active_tool_calls
                        .get(&index)
                        .map(|call| call.id.clone())
                })
                .or_else(|| {
                    state
                        .responses_mut()
                        .output_item_ids
                        .get(&item_index.unwrap_or(index))
                        .cloned()
                });
            if let Some(active_call) = state.responses_mut().active_tool_calls.get_mut(&index) {
                if let Some(explicit_item_id) = item_id.clone() {
                    active_call.id = explicit_item_id.clone();
                    response_item_id = Some(explicit_item_id);
                }
                active_call.arguments.push_str(&arguments);
                if let Some(name) = name.clone() {
                    active_call.name = name;
                }
                if let Some(id) = id.clone() {
                    active_call.call_id = id;
                }
            }
            if let Some(item_id) = response_item_id {
                frames.push(json!({
                    "type": "response.function_call_arguments.delta",
                    "item_id": item_id,
                    "output_index": item_index.unwrap_or(index),
                    "name": name,
                    "delta": arguments
                }));
            }
        }
        UnifiedStreamEvent::ReasoningStart { index } => {
            let item_id = format!("rs_{}", crate::utils::ID_GENERATOR.generate_id());
            state
                .responses_mut()
                .reasoning_item_ids
                .insert(index, item_id.clone());
            state
                .responses_mut()
                .reasoning_summaries
                .insert(index, String::new());
            let item = ItemField::Reasoning(ReasoningBody {
                _type: "reasoning".to_string(),
                id: item_id.clone(),
                content: None,
                summary: Vec::new(),
                encrypted_content: None,
            });
            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": index,
                "item": item
            }));
        }
        UnifiedStreamEvent::ReasoningSummaryPartAdded {
            item_index,
            item_id,
            part_index,
            ..
        } => {
            let output_index = item_index.unwrap_or_default();
            let item_id = item_id.or_else(|| {
                state
                    .responses_mut()
                    .reasoning_item_ids
                    .get(&output_index)
                    .cloned()
            });
            if let Some(item_id) = item_id {
                frames.push(json!({
                    "type": "response.reasoning_summary_part.added",
                    "item_id": item_id,
                    "output_index": output_index,
                    "summary_index": part_index,
                    "part": default_reasoning_summary_part("")
                }));
            }
        }
        UnifiedStreamEvent::ReasoningDelta {
            index,
            item_index,
            item_id,
            part_index,
            text,
        } => {
            let output_index = item_index.unwrap_or(index);
            state
                .responses_mut()
                .reasoning_summaries
                .entry(output_index)
                .and_modify(|summary| summary.push_str(&text))
                .or_insert_with(|| text.clone());
            let summary_index = part_index
                .or(state.current_reasoning_part_index())
                .unwrap_or(index);
            let response_item_id = item_id.or_else(|| {
                state
                    .responses_mut()
                    .reasoning_item_ids
                    .get(&output_index)
                    .cloned()
            });
            frames.push(json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": response_item_id.unwrap_or_default(),
                "output_index": output_index,
                "summary_index": summary_index,
                "delta": text
            }));
        }
        UnifiedStreamEvent::ReasoningSummaryPartDone {
            item_index,
            item_id,
            part_index,
        } => {
            let output_index = item_index.unwrap_or_default();
            let item_id = item_id.or_else(|| {
                state
                    .responses_mut()
                    .reasoning_item_ids
                    .get(&output_index)
                    .cloned()
            });
            if let Some(item_id) = item_id {
                let summary_text = state
                    .responses_mut()
                    .reasoning_summaries
                    .get(&output_index)
                    .cloned()
                    .unwrap_or_default();
                frames.push(json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": item_id,
                    "output_index": output_index,
                    "summary_index": part_index,
                    "part": default_reasoning_summary_part(summary_text)
                }));
            }
        }
        UnifiedStreamEvent::ReasoningStop { index } => {
            if let Some(item_id) = state.responses_mut().reasoning_item_ids.remove(&index) {
                let summary = state
                    .responses_mut()
                    .reasoning_summaries
                    .remove(&index)
                    .unwrap_or_default();
                let item = ItemField::Reasoning(ReasoningBody {
                    _type: "reasoning".to_string(),
                    id: item_id.clone(),
                    content: None,
                    summary: (!summary.is_empty())
                        .then_some(vec![ItemContentPart::SummaryText {
                            text: summary.clone(),
                        }])
                        .unwrap_or_default(),
                    encrypted_content: None,
                });
                state
                    .responses_mut()
                    .completed_output
                    .insert(index, item.clone());
                frames.push(json!({
                    "type": "response.reasoning_summary_part.done",
                    "item_id": item_id,
                    "output_index": index,
                    "summary_index": 0,
                    "part": default_reasoning_summary_part(summary.clone())
                }));
                frames.push(json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item
                }));
            }
        }
        UnifiedStreamEvent::ToolCallStop { index, .. } => {
            if let Some(mut function_call) = state.responses_mut().active_tool_calls.remove(&index)
            {
                function_call.status = MessageStatus::Completed;
                let item = ItemField::FunctionCall(function_call);
                state
                    .responses_mut()
                    .completed_output
                    .insert(index, item.clone());
                if let ItemField::FunctionCall(function_call) = &item {
                    frames.push(json!({
                        "type": "response.function_call_arguments.done",
                        "item_id": function_call.id,
                        "output_index": index,
                        "call_id": function_call.call_id,
                        "arguments": function_call.arguments
                    }));
                }
                frames.push(json!({
                    "type": "response.output_item.done",
                    "output_index": index,
                    "item": item
                }));
            }
        }
        UnifiedStreamEvent::MessageStop => {
            if state.responses_mut().completion_pending {
                let finish_reason = state.finish_reason_cache();
                let (status, incomplete_details) =
                    response_status_from_finish_reason(finish_reason);
                let response_event_type = match status {
                    ResponseStatus::Incomplete => "response.incomplete",
                    _ => "response.completed",
                };
                if let Some((output_index, item)) = complete_current_message_item(state) {
                    frames.push(json!({
                        "type": "response.output_item.done",
                        "output_index": output_index,
                        "item": item
                    }));
                }
                frames.push(json!({
                    "type": response_event_type,
                    "response": build_formal_responses_response(
                        state,
                        status,
                        incomplete_details,
                        collect_completed_output_items(state)
                    )
                }));
                state.responses_mut().completion_pending = false;
            }
        }
        UnifiedStreamEvent::ContentBlockStart { .. }
        | UnifiedStreamEvent::ContentBlockStop { .. }
        | UnifiedStreamEvent::Error { .. } => {}
        UnifiedStreamEvent::BlobDelta { index, data } => {
            if !state.responses_mut().created_sent {
                state.responses_mut().created_sent = true;
                frames.push(json!({
                    "type": "response.created",
                    "response": build_formal_responses_response(
                        state,
                        ResponseStatus::InProgress,
                        None,
                        Vec::new()
                    )
                }));
            }

            frames.push(json!({
                "type": "response.output_item.added",
                "output_index": index.unwrap_or_default(),
                "item": data
            }));
        }
    }

    frames
}
