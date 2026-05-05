use chrono::Utc;

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::unified::*;
use crate::service::transform::{TransformProtocol, TransformValueKind, apply_transform_policy};

use super::payload::*;
pub(super) use super::response_mapping::*;

impl From<ResponsesResponse> for UnifiedResponse {
    fn from(responses_res: ResponsesResponse) -> Self {
        let provider_response_metadata = build_responses_response_metadata(
            &responses_res.output,
            responses_res.metadata.clone(),
            responses_res.safety_identifier.clone(),
            responses_res.prompt_cache_key.clone(),
            responses_res.status.clone(),
            responses_res.incomplete_details.clone(),
        );
        let mut content = Vec::new();
        let mut response_items = Vec::new();

        for item in responses_res.output {
            match item {
                ItemField::Message(msg) => {
                    let (unified_content, annotations, files) =
                        message_content_parts_to_unified(msg.content);
                    content.extend(unified_content.clone());
                    if !unified_content.is_empty() || !annotations.is_empty() {
                        response_items.push(UnifiedItem::Message(UnifiedMessageItem {
                            role: message_role_to_unified(msg.role),
                            content: unified_content,
                            annotations,
                        }));
                    }
                    response_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                }
                ItemField::FunctionCall(call) => {
                    response_items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                        id: call.call_id.clone(),
                        name: call.name.clone(),
                        arguments: parse_function_arguments(&call.arguments),
                    }));
                    content.push(UnifiedContentPart::ToolCall(UnifiedToolCall {
                        id: call.call_id,
                        name: call.name,
                        arguments: parse_function_arguments(&call.arguments),
                    }));
                }
                ItemField::FunctionCallOutput(output) => {
                    response_items.push(UnifiedItem::FunctionCallOutput(
                        UnifiedFunctionCallOutputItem {
                            tool_call_id: output.call_id.clone(),
                            name: None,
                            output: function_output_payload_to_unified(output.output.clone()),
                        },
                    ));
                    content.push(UnifiedContentPart::ToolResult(UnifiedToolResult {
                        tool_call_id: output.call_id,
                        name: None,
                        output: function_output_payload_to_unified(output.output),
                    }));
                }
                ItemField::Reasoning(reasoning) => {
                    let (reasoning_content, annotations, files) =
                        reasoning_parts_to_unified(reasoning);
                    content.extend(reasoning_content.clone());
                    response_items.push(UnifiedItem::Reasoning(UnifiedReasoningItem {
                        content: reasoning_content,
                        annotations,
                    }));
                    response_items.extend(files.into_iter().map(UnifiedItem::FileReference));
                }
                ItemField::Unknown(_) => {
                    apply_transform_policy(
                        TransformProtocol::Api(LlmApiType::Responses),
                        TransformProtocol::Unified,
                        TransformValueKind::ResponsesUnknownItem,
                        "Dropping unknown Responses item from Responses response conversion.",
                    );
                }
            }
        }

        let choices = if content.is_empty() && response_items.is_empty() {
            Vec::new()
        } else {
            vec![UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content,
                },
                items: response_items,
                finish_reason: Some("stop".to_string()),
                logprobs: None,
            }]
        };

        UnifiedResponse {
            id: responses_res.id,
            model: Some(responses_res.model),
            choices,
            usage: responses_res.usage.map(Into::into),
            created: Some(responses_res.created_at),
            object: Some(
                serde_json::to_value(responses_res.object)
                    .ok()
                    .and_then(|value| value.as_str().map(ToString::to_string))
                    .unwrap_or_else(|| "response".to_string()),
            ),
            system_fingerprint: None,
            provider_response_metadata,
            synthetic_metadata: None,
        }
    }
}

impl From<UnifiedResponse> for ResponsesResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let responses_metadata = unified_res
            .provider_response_metadata
            .clone()
            .and_then(|metadata| metadata.responses);
        let reasoning_metadata = responses_metadata
            .as_ref()
            .and_then(|metadata| metadata.reasoning.clone())
            .and_then(|value| serde_json::from_value(value).ok());
        let refusals = responses_metadata
            .as_ref()
            .map(|metadata| metadata.refusals.clone())
            .unwrap_or_default();
        let files = responses_metadata
            .as_ref()
            .map(|metadata| metadata.files.clone())
            .unwrap_or_default();
        let (metadata, safety_identifier, prompt_cache_key, status, incomplete_details) =
            unified_responses_metadata_to_payload(responses_metadata);
        let mut output = Vec::new();

        for choice in unified_res.choices {
            output.extend(unified_choice_to_responses_items(choice));
        }

        inject_refusals_into_output(&mut output, &refusals);
        inject_files_into_output(&mut output, &files);
        apply_reasoning_metadata_to_output(&mut output, reasoning_metadata);

        ResponsesResponse {
            id: unified_res.id,
            object: ResponseObject::Response,
            created_at: unified_res
                .created
                .unwrap_or_else(|| Utc::now().timestamp()),
            completed_at: matches!(status, ResponseStatus::Completed).then_some(
                unified_res
                    .created
                    .unwrap_or_else(|| Utc::now().timestamp()),
            ),
            status,
            incomplete_details,
            model: unified_res.model.unwrap_or_default(),
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
            usage: unified_res.usage.map(Into::into),
            max_output_tokens: None,
            max_tool_calls: None,
            store: true,
            background: false,
            service_tier: ServiceTier::Default,
            metadata,
            safety_identifier,
            prompt_cache_key,
        }
    }
}
