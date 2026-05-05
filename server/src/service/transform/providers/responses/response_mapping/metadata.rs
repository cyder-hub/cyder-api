use serde_json::{Value, json};

use crate::service::transform::unified::*;

use super::super::payload::*;

pub(in crate::service::transform::providers::responses) fn build_responses_response_metadata(
    output: &[ItemField],
    metadata: Value,
    safety_identifier: Option<String>,
    prompt_cache_key: Option<String>,
    status: ResponseStatus,
    incomplete_details: Option<IncompleteDetails>,
) -> Option<UnifiedProviderResponseMetadata> {
    let reasoning_metadata = build_reasoning_metadata(output);
    let citations = output
        .iter()
        .flat_map(|item| match item {
            ItemField::Message(message) => message.content.iter().collect::<Vec<_>>(),
            ItemField::Reasoning(reasoning) => {
                let mut parts = reasoning.content.iter().flatten().collect::<Vec<_>>();
                parts.extend(reasoning.summary.iter());
                parts
            }
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            ItemContentPart::OutputText { annotations, .. } => Some(annotations),
            _ => None,
        })
        .flat_map(|annotations| annotations.iter())
        .map(|annotation| match annotation {
            Annotation::UrlCitation {
                url,
                start_index,
                end_index,
                title,
            } => UnifiedResponsesUrlCitation {
                url: url.clone(),
                start_index: *start_index,
                end_index: *end_index,
                title: title.clone(),
            },
        })
        .collect::<Vec<_>>();

    let refusals = output
        .iter()
        .flat_map(|item| match item {
            ItemField::Message(message) => message.content.iter().collect::<Vec<_>>(),
            ItemField::Reasoning(reasoning) => {
                let mut parts = reasoning.content.iter().flatten().collect::<Vec<_>>();
                parts.extend(reasoning.summary.iter());
                parts
            }
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            ItemContentPart::Refusal { refusal } => Some(UnifiedResponsesRefusal {
                refusal: refusal.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let files = output
        .iter()
        .flat_map(|item| match item {
            ItemField::Message(message) => message.content.iter().collect::<Vec<_>>(),
            ItemField::Reasoning(reasoning) => {
                let mut parts = reasoning.content.iter().flatten().collect::<Vec<_>>();
                parts.extend(reasoning.summary.iter());
                parts
            }
            _ => Vec::new(),
        })
        .filter_map(|part| match part {
            ItemContentPart::InputFile {
                filename,
                file_url,
                file_id,
                file_data,
            } => Some(UnifiedResponsesFileReference {
                filename: filename.clone(),
                file_url: file_url.clone(),
                file_id: file_id.clone(),
                file_data: file_data.clone(),
            }),
            _ => None,
        })
        .collect::<Vec<_>>();

    let metadata = metadata.as_object().cloned();

    if citations.is_empty()
        && refusals.is_empty()
        && files.is_empty()
        && metadata.is_none()
        && reasoning_metadata.is_none()
        && safety_identifier.is_none()
        && prompt_cache_key.is_none()
        && matches!(status, ResponseStatus::Completed)
        && incomplete_details.is_none()
    {
        None
    } else {
        Some(UnifiedProviderResponseMetadata {
            responses: Some(UnifiedResponsesResponseMetadata {
                safety_identifier,
                prompt_cache_key,
                citations,
                refusals,
                files,
                metadata,
                reasoning: reasoning_metadata.and_then(|value| serde_json::to_value(value).ok()),
                status: Some(
                    serde_json::to_value(status)
                        .ok()
                        .and_then(|value| value.as_str().map(ToString::to_string))
                        .unwrap_or_else(|| "completed".to_string()),
                ),
                incomplete_details: incomplete_details.map(|details| {
                    UnifiedResponsesIncompleteDetails {
                        reason: details.reason,
                    }
                }),
            }),
            ..Default::default()
        })
    }
}

pub(in crate::service::transform::providers::responses) fn build_reasoning_metadata(
    output: &[ItemField],
) -> Option<ResponsesReasoningMetadata> {
    let encrypted_contents = output
        .iter()
        .filter_map(|item| match item {
            ItemField::Reasoning(reasoning) => reasoning.encrypted_content.clone(),
            _ => None,
        })
        .collect::<Vec<_>>();

    (!encrypted_contents.is_empty()).then_some(ResponsesReasoningMetadata { encrypted_contents })
}

pub(in crate::service::transform::providers::responses) fn unified_responses_metadata_to_payload(
    metadata: Option<UnifiedResponsesResponseMetadata>,
) -> (
    Value,
    Option<String>,
    Option<String>,
    ResponseStatus,
    Option<IncompleteDetails>,
) {
    match metadata {
        Some(metadata) => {
            let mut payload = metadata
                .metadata
                .map(Value::Object)
                .unwrap_or_else(|| json!({}));

            if let Some(reasoning) = metadata.reasoning.clone() {
                payload["responses_reasoning"] = reasoning;
            }

            (
                payload,
                metadata.safety_identifier,
                metadata.prompt_cache_key,
                metadata
                    .status
                    .and_then(|status| serde_json::from_value(json!(status)).ok())
                    .unwrap_or(ResponseStatus::Completed),
                metadata
                    .incomplete_details
                    .map(|details| IncompleteDetails {
                        reason: details.reason,
                    }),
            )
        }
        None => (json!({}), None, None, ResponseStatus::Completed, None),
    }
}

pub(in crate::service::transform::providers::responses) fn responses_finish_reason(
    response: &ResponsesResponse,
) -> Option<String> {
    if let Some(finish_reason) = response
        .metadata
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(ToString::to_string)
    {
        return Some(finish_reason);
    }

    match response.status {
        ResponseStatus::Incomplete => {
            response
                .incomplete_details
                .as_ref()
                .map(|details| match details.reason.as_str() {
                    "max_output_tokens" => "length".to_string(),
                    other => other.to_string(),
                })
        }
        _ => None,
    }
}

pub(in crate::service::transform::providers::responses) fn response_terminal_stream_events(
    response: ResponsesResponse,
) -> Vec<UnifiedStreamEvent> {
    let mut terminal = Vec::new();
    if let Some(finish_reason) = responses_finish_reason(&response) {
        terminal.push(UnifiedStreamEvent::MessageDelta {
            finish_reason: Some(finish_reason),
        });
    }
    if let Some(usage) = response.usage {
        terminal.push(UnifiedStreamEvent::Usage {
            usage: usage.into(),
        });
    }
    terminal
}

pub(in crate::service::transform::providers::responses) fn response_status_from_finish_reason(
    finish_reason: Option<&str>,
) -> (ResponseStatus, Option<IncompleteDetails>) {
    match finish_reason {
        Some("stop") | Some("tool_calls") | None => (ResponseStatus::Completed, None),
        Some("length") => (
            ResponseStatus::Incomplete,
            Some(IncompleteDetails {
                reason: "max_output_tokens".to_string(),
            }),
        ),
        Some(reason) => (
            ResponseStatus::Incomplete,
            Some(IncompleteDetails {
                reason: reason.to_string(),
            }),
        ),
    }
}

pub(in crate::service::transform::providers::responses) fn inject_refusals_into_output(
    output: &mut Vec<ItemField>,
    refusals: &[UnifiedResponsesRefusal],
) {
    if refusals.is_empty()
        || output.iter().any(|item| match item {
            ItemField::Message(message) => message
                .content
                .iter()
                .any(|part| matches!(part, ItemContentPart::Refusal { .. })),
            ItemField::Reasoning(reasoning) => reasoning
                .content
                .iter()
                .flatten()
                .chain(reasoning.summary.iter())
                .any(|part| matches!(part, ItemContentPart::Refusal { .. })),
            _ => false,
        })
    {
        return;
    }

    if let Some(ItemField::Message(message)) = output
        .iter_mut()
        .find(|item| matches!(item, ItemField::Message(message) if matches!(message.role, MessageRole::Assistant)))
    {
        let mut refusal_parts = refusals
            .iter()
            .map(|refusal| ItemContentPart::Refusal {
                refusal: refusal.refusal.clone(),
            })
            .collect::<Vec<_>>();
        refusal_parts.append(&mut message.content);
        message.content = refusal_parts;
        return;
    }

    output.insert(
        0,
        ItemField::Message(Message {
            _type: "message".to_string(),
            id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: refusals
                .iter()
                .map(|refusal| ItemContentPart::Refusal {
                    refusal: refusal.refusal.clone(),
                })
                .collect(),
        }),
    );
}

pub(in crate::service::transform::providers::responses) fn inject_files_into_output(
    output: &mut Vec<ItemField>,
    files: &[UnifiedResponsesFileReference],
) {
    if files.is_empty()
        || output.iter().any(|item| match item {
            ItemField::Message(message) => message
                .content
                .iter()
                .any(|part| matches!(part, ItemContentPart::InputFile { .. })),
            ItemField::Reasoning(reasoning) => reasoning
                .content
                .iter()
                .flatten()
                .chain(reasoning.summary.iter())
                .any(|part| matches!(part, ItemContentPart::InputFile { .. })),
            _ => false,
        })
    {
        return;
    }

    output.extend(files.iter().map(|file| {
        ItemField::Message(Message {
            _type: "message".to_string(),
            id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
            status: MessageStatus::Completed,
            role: MessageRole::Assistant,
            content: vec![ItemContentPart::InputFile {
                filename: file.filename.clone(),
                file_url: file.file_url.clone(),
                file_id: file.file_id.clone(),
                file_data: file.file_data.clone(),
            }],
        })
    }));
}

pub(in crate::service::transform::providers::responses) fn apply_reasoning_metadata_to_output(
    output: &mut [ItemField],
    reasoning_metadata: Option<ResponsesReasoningMetadata>,
) {
    let Some(reasoning_metadata) = reasoning_metadata else {
        return;
    };

    let mut encrypted_contents = reasoning_metadata.encrypted_contents.into_iter();
    for item in output.iter_mut() {
        if let ItemField::Reasoning(reasoning) = item {
            if reasoning.encrypted_content.is_none() {
                reasoning.encrypted_content = encrypted_contents.next();
            }
        }
    }
}
