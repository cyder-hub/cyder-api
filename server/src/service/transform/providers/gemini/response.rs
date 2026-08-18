use chrono::Utc;

use crate::service::transform::unified::*;

use super::metadata::*;
use super::payload::*;
use super::usage::{gemini_usage_to_unified, unified_usage_to_gemini};

impl From<GeminiResponse> for UnifiedResponse {
    fn from(gemini_res: GeminiResponse) -> Self {
        let GeminiResponse {
            response_id,
            candidates,
            prompt_feedback,
            usage_metadata,
            synthetic_metadata,
        } = gemini_res;

        let prompt_blocked = prompt_feedback
            .as_ref()
            .and_then(|feedback| feedback.block_reason.as_deref())
            .is_some_and(is_known_gemini_prompt_block_reason);

        let provider_response_metadata =
            build_gemini_response_metadata(prompt_feedback, &candidates, response_id.as_deref());

        let mut choices: Vec<UnifiedChoice> = candidates
            .into_iter()
            .enumerate()
            .map(|(candidate_position, candidate)| {
                let mut content_parts = Vec::new();
                let mut items = Vec::new();
                let mut role = UnifiedRole::Assistant;
                let mut has_function_call = false;

                if let Some(content) = candidate.content {
                    role = match content.role.as_str() {
                        "model" => UnifiedRole::Assistant,
                        "user" => UnifiedRole::User, // Should not happen in a response choice
                        _ => UnifiedRole::Assistant,
                    };

                    let candidate_index = candidate
                        .index
                        .unwrap_or_else(|| u32::try_from(candidate_position).unwrap_or(u32::MAX));
                    for (part_index, p) in content.parts.into_iter().enumerate() {
                        match p {
                            GeminiPart::Thought { text, thought, .. } => {
                                let part = if thought {
                                    UnifiedContentPart::Reasoning { text }
                                } else {
                                    UnifiedContentPart::Text { text }
                                };
                                content_parts.push(part.clone());
                                if thought {
                                    items.push(UnifiedItem::Reasoning(UnifiedReasoningItem {
                                        content: vec![part],
                                        annotations: Vec::new(),
                                    }));
                                } else {
                                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                                        role: role.clone(),
                                        content: vec![part],
                                        annotations: Vec::new(),
                                    }));
                                }
                            }
                            GeminiPart::Text { text } => {
                                content_parts.push(UnifiedContentPart::Text { text: text.clone() });
                                items.push(UnifiedItem::Message(UnifiedMessageItem {
                                    role: role.clone(),
                                    content: vec![UnifiedContentPart::Text { text }],
                                    annotations: Vec::new(),
                                }));
                            }
                            GeminiPart::InlineData { inline_data } => {
                                let part = gemini_inline_data_to_unified_content(inline_data);
                                content_parts.push(part.clone());
                                items.push(UnifiedItem::Message(UnifiedMessageItem {
                                    role: role.clone(),
                                    content: vec![part],
                                    annotations: Vec::new(),
                                }));
                            }
                            GeminiPart::FileData { file_data } => {
                                let file_part = UnifiedContentPart::FileUrl {
                                    url: file_data.file_uri.clone(),
                                    mime_type: Some(file_data.mime_type.clone()),
                                    filename: None,
                                };
                                content_parts.push(file_part);
                                items.push(UnifiedItem::FileReference(UnifiedFileReferenceItem {
                                    filename: None,
                                    mime_type: Some(file_data.mime_type),
                                    file_url: Some(file_data.file_uri),
                                    file_id: None,
                                }));
                            }
                            GeminiPart::ExecutableCode { executable_code } => {
                                let code_part = UnifiedContentPart::ExecutableCode {
                                    language: executable_code.language,
                                    code: executable_code.code,
                                };
                                content_parts.push(code_part.clone());
                                items.push(UnifiedItem::Message(UnifiedMessageItem {
                                    role: role.clone(),
                                    content: vec![code_part],
                                    annotations: Vec::new(),
                                }));
                            }
                            GeminiPart::FunctionCall {
                                function_call,
                                thought_signature: _,
                            } => {
                                has_function_call = true;
                                let id = function_call.id.clone().unwrap_or_else(|| {
                                    build_gemini_synthetic_tool_call_id(
                                        response_id.as_deref().expect(
                                            "source audit requires responseId when Gemini omits functionCall.id",
                                        ),
                                        candidate_index,
                                        part_index as u32,
                                        &function_call.name,
                                    )
                                });
                                let tool_call = UnifiedToolCall {
                                    id: id.clone(),
                                    name: function_call.name.clone(),
                                    arguments: function_call.args.clone(),
                                };
                                content_parts.push(UnifiedContentPart::ToolCall(tool_call.clone()));
                                items.push(UnifiedItem::FunctionCall(UnifiedFunctionCallItem {
                                    id,
                                    name: function_call.name,
                                    arguments: function_call.args,
                                }));
                            }
                            GeminiPart::FunctionResponse { function_response } => {
                                let tool_call_id = function_response.id.clone().unwrap_or_else(|| {
                                    build_gemini_synthetic_tool_call_id(
                                        response_id.as_deref().expect(
                                            "source audit requires responseId when Gemini omits functionResponse.id",
                                        ),
                                        candidate_index,
                                        part_index as u32,
                                        &function_response.name,
                                    )
                                });
                                let output = gemini_function_response_to_unified_output(
                                    function_response.response,
                                );
                                content_parts.push(UnifiedContentPart::ToolResult(
                                    UnifiedToolResult {
                                        tool_call_id: tool_call_id.clone(),
                                        name: Some(function_response.name.clone()),
                                        output: output.clone(),
                                    },
                                ));
                                items.push(UnifiedItem::FunctionCallOutput(
                                    UnifiedFunctionCallOutputItem {
                                        tool_call_id,
                                        name: Some(function_response.name),
                                        output,
                                    },
                                ));
                            }
                        }
                    }
                }

                let message = UnifiedMessage {
                    role,
                    content: content_parts,
                    ..Default::default()
                };

                let items = if message.content.is_empty() {
                    items
                } else {
                    let annotations = candidate
                        .citation_metadata
                        .clone()
                        .map(|metadata| gemini_citation_metadata_to_annotations(Some(metadata)))
                        .unwrap_or_default();
                    if !annotations.is_empty() || items.is_empty() {
                        items.insert(
                            0,
                            UnifiedItem::Message(UnifiedMessageItem {
                                role: message.role.clone(),
                                content: message.content.clone(),
                                annotations,
                            }),
                        );
                    }
                    items
                };

                let finish_reason = candidate.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_gemini_finish_reason_to_openai(
                        &fr,
                        has_function_call,
                    )
                });

                UnifiedChoice {
                    index: candidate
                        .index
                        .unwrap_or_else(|| u32::try_from(candidate_position).unwrap_or(u32::MAX)),
                    message,
                    items,
                    finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        if choices.is_empty() && prompt_blocked {
            choices.push(UnifiedChoice {
                index: 0,
                message: UnifiedMessage {
                    role: UnifiedRole::Assistant,
                    content: Vec::new(),
                    ..Default::default()
                },
                items: Vec::new(),
                finish_reason: Some("content_filter".to_string()),
                logprobs: None,
            });
        }

        let usage = usage_metadata.map(|usage| {
            gemini_usage_to_unified(&usage)
                .expect("Gemini response usage must pass source audit before conversion")
        });

        let synthetic_id = response_id.is_none();
        let synthetic_model = true;

        UnifiedResponse {
            id: response_id.unwrap_or_else(|| build_gemini_synthetic_response_id("response")),
            model: Some("gemini".to_string()),
            choices,
            usage,
            created: Some(Utc::now().timestamp()),
            object: Some("chat.completion".to_string()),
            system_fingerprint: None,
            provider_response_metadata,
            synthetic_metadata: merge_gemini_synthetic_metadata(
                synthetic_metadata,
                build_gemini_synthetic_metadata(synthetic_id, synthetic_model, false),
            ),
        }
    }
}

impl From<UnifiedResponse> for GeminiResponse {
    fn from(unified_res: UnifiedResponse) -> Self {
        let response_id = Some(unified_res.id.clone());
        let gemini_metadata = unified_res
            .provider_response_metadata
            .clone()
            .and_then(|metadata| metadata.gemini);
        let candidates = unified_res
            .choices
            .into_iter()
            .filter_map(|choice| {
                let choice_items = choice.content_items();
                let candidate_metadata = gemini_metadata
                    .as_ref()
                    .and_then(|metadata| {
                        metadata
                            .candidates
                            .iter()
                            .find(|candidate| candidate.index == choice.index)
                    })
                    .cloned()
                    .or_else(|| {
                        choice_items.iter().find_map(|item| match item {
                            UnifiedItem::Message(message) if !message.annotations.is_empty() => {
                                Some(UnifiedGeminiCandidateMetadata {
                                    index: choice.index,
                                    thought_signature_present: false,
                                    tool_associations: Vec::new(),
                                    safety_ratings: Vec::new(),
                                    citation_metadata: gemini_citation_metadata_to_unified(
                                        unified_annotations_to_gemini_citation_metadata(
                                            &message.annotations,
                                        ),
                                    ),
                                    token_count: None,
                                })
                            }
                            _ => None,
                        })
                    });
                let response_role = choice_items
                    .iter()
                    .find_map(|item| match item {
                        UnifiedItem::Message(message) => Some(message.role.clone()),
                        _ => None,
                    })
                    .unwrap_or(choice.message.role.clone());
                let role = match response_role {
                    UnifiedRole::Assistant => "model",
                    _ => "user",
                }
                .to_string();

                let mut parts = Vec::new();
                for item in choice_items {
                    match item {
                        UnifiedItem::Message(message) => {
                            for part in message.content {
                                match part {
                                    UnifiedContentPart::Text { text }
                                    | UnifiedContentPart::Refusal { text } => {
                                        parts.push(GeminiPart::Text { text });
                                    }
                                    UnifiedContentPart::Reasoning { text } => {
                                        parts.push(GeminiPart::Thought {
                                            text,
                                            thought: true,
                                            thought_signature: None,
                                        });
                                    }
                                    UnifiedContentPart::ImageData { mime_type, data } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData {
                                                mime_type,
                                                data,
                                                display_name: None,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::AudioData { data, format } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData {
                                                mime_type: if format == "mp3" {
                                                    "audio/mpeg".to_string()
                                                } else {
                                                    "audio/wav".to_string()
                                                },
                                                data,
                                                display_name: None,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                        parts.push(GeminiPart::FileData {
                                            file_data: GeminiFileData {
                                                mime_type: mime_type.unwrap_or_else(|| {
                                                    "application/octet-stream".to_string()
                                                }),
                                                file_uri: url,
                                                display_name: None,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::FileData {
                                        data, mime_type, ..
                                    } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData {
                                                mime_type,
                                                data,
                                                display_name: None,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::FileId { .. } => {}
                                    UnifiedContentPart::ExecutableCode { language, code } => {
                                        parts.push(GeminiPart::ExecutableCode {
                                            executable_code: GeminiExecutableCode {
                                                language,
                                                code,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::ToolCall(call) => {
                                        parts.push(GeminiPart::FunctionCall {
                                            function_call: GeminiFunctionCall {
                                                id: Some(call.id),
                                                name: call.name,
                                                args: call.arguments,
                                            },
                                            thought_signature: None,
                                        });
                                    }
                                    UnifiedContentPart::ToolResult(result) => {
                                        let name = result.name.unwrap_or_else(|| {
                                            build_gemini_fallback_tool_name(&result.tool_call_id)
                                        });
                                        parts.push(GeminiPart::FunctionResponse {
                                            function_response: GeminiFunctionResponse {
                                                id: Some(result.tool_call_id),
                                                name,
                                                response: unified_tool_result_to_gemini_response(
                                                    &result.output,
                                                ),
                                            },
                                        });
                                    }
                                    UnifiedContentPart::ImageUrl { url, detail } => {
                                        parts.push(GeminiPart::Text {
                                            text: render_gemini_image_reference_text(
                                                &url,
                                                detail.as_deref(),
                                            ),
                                        });
                                    }
                                }
                            }
                        }
                        UnifiedItem::Reasoning(reasoning) => {
                            for part in reasoning.content {
                                match part {
                                    UnifiedContentPart::Reasoning { text }
                                    | UnifiedContentPart::Text { text }
                                    | UnifiedContentPart::Refusal { text } => {
                                        parts.push(GeminiPart::Thought {
                                            text,
                                            thought: true,
                                            thought_signature: None,
                                        });
                                    }
                                    UnifiedContentPart::ExecutableCode { language, code } => {
                                        parts.push(GeminiPart::ExecutableCode {
                                            executable_code: GeminiExecutableCode {
                                                language,
                                                code,
                                            },
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        UnifiedItem::FunctionCall(call) => {
                            parts.push(GeminiPart::FunctionCall {
                                function_call: GeminiFunctionCall {
                                    id: Some(call.id),
                                    name: call.name,
                                    args: call.arguments,
                                },
                                thought_signature: None,
                            });
                        }
                        UnifiedItem::FunctionCallOutput(output) => {
                            let name = output.name.unwrap_or_else(|| {
                                build_gemini_fallback_tool_name(&output.tool_call_id)
                            });
                            parts.push(GeminiPart::FunctionResponse {
                                function_response: GeminiFunctionResponse {
                                    id: Some(output.tool_call_id),
                                    name,
                                    response: unified_tool_result_to_gemini_response(
                                        &output.output,
                                    ),
                                },
                            });
                        }
                        UnifiedItem::FileReference(file) => {
                            if let Some(file_uri) = file.file_url {
                                parts.push(GeminiPart::FileData {
                                    file_data: GeminiFileData {
                                        mime_type: file.mime_type.unwrap_or_else(|| {
                                            "application/octet-stream".to_string()
                                        }),
                                        file_uri,
                                        display_name: file.filename,
                                    },
                                });
                            }
                        }
                    }
                }

                let content = if parts.is_empty() {
                    None
                } else {
                    Some(GeminiResponseContent { role, parts })
                };

                let finish_reason = choice.finish_reason.map(|fr| {
                    crate::service::transform::unified::map_openai_finish_reason_to_gemini(&fr)
                });

                if content.is_some() || finish_reason.is_some() {
                    Some(GeminiCandidate {
                        index: Some(choice.index),
                        content,
                        finish_reason,
                        safety_ratings: candidate_metadata.as_ref().and_then(|metadata| {
                            unified_safety_ratings_to_gemini(metadata.safety_ratings.clone())
                        }),
                        token_count: candidate_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.token_count),
                        citation_metadata: candidate_metadata.and_then(|metadata| {
                            unified_citation_metadata_to_gemini(metadata.citation_metadata)
                        }),
                    })
                } else {
                    None
                }
            })
            .collect();

        let usage_metadata = unified_res.usage.map(unified_usage_to_gemini);

        GeminiResponse {
            response_id,
            candidates,
            prompt_feedback: gemini_metadata
                .and_then(|metadata| unified_prompt_feedback_to_gemini(metadata.prompt_feedback)),
            usage_metadata,
            synthetic_metadata: unified_res.synthetic_metadata,
        }
    }
}
