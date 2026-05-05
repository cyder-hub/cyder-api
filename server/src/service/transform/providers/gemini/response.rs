use chrono::Utc;

use crate::service::transform::unified::*;

use super::metadata::*;
use super::payload::*;

impl From<GeminiResponse> for UnifiedResponse {
    fn from(gemini_res: GeminiResponse) -> Self {
        let GeminiResponse {
            candidates,
            prompt_feedback,
            usage_metadata,
            synthetic_metadata,
        } = gemini_res;

        let provider_response_metadata =
            build_gemini_response_metadata(prompt_feedback, &candidates);

        let choices = candidates
            .into_iter()
            .map(|candidate| {
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

                    let candidate_index = candidate.index.unwrap_or(0);
                    for (part_index, p) in content.parts.into_iter().enumerate() {
                        match p {
                            GeminiPart::Text { text } => {
                                if !text.is_empty() {
                                    content_parts
                                        .push(UnifiedContentPart::Text { text: text.clone() });
                                    items.push(UnifiedItem::Message(UnifiedMessageItem {
                                        role: role.clone(),
                                        content: vec![UnifiedContentPart::Text { text }],
                                        annotations: Vec::new(),
                                    }));
                                }
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
                            GeminiPart::FunctionCall { function_call } => {
                                has_function_call = true;
                                let id = build_gemini_synthetic_tool_call_id(
                                    candidate_index,
                                    0,
                                    part_index as u32,
                                    &function_call.name,
                                );
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
                                let tool_call_id = build_gemini_synthetic_tool_call_id(
                                    candidate_index,
                                    0,
                                    part_index as u32,
                                    &function_response.name,
                                );
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
                    index: candidate.index.unwrap_or(0),
                    message,
                    items,
                    finish_reason,
                    logprobs: None,
                }
            })
            .collect();

        let usage = usage_metadata.map(|u| {
            let mut usage = UnifiedUsage {
                input_tokens: u.prompt_token_count,
                output_tokens: u.candidates_token_count,
                total_tokens: u.total_token_count,
                reasoning_tokens: u.thoughts_token_count,
                cached_tokens: u.cached_content_token_count,
                ..Default::default()
            };

            // Handle image tokens from details
            let input_image_tokens = u
                .prompt_tokens_details
                .iter()
                .find(|d| d.modality == Modality::Image)
                .map(|d| d.token_count);
            if input_image_tokens.is_some() {
                usage.input_image_tokens = input_image_tokens;
            }

            let output_image_tokens = u
                .candidates_tokens_details
                .iter()
                .find(|d| d.modality == Modality::Image)
                .map(|d| d.token_count);
            if output_image_tokens.is_some() {
                usage.output_image_tokens = output_image_tokens;
            }

            usage
        });

        let synthetic_id = true;
        let synthetic_model = false;

        UnifiedResponse {
            id: build_gemini_synthetic_response_id("response"),
            model: None,
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
                                    | UnifiedContentPart::Reasoning { text }
                                    | UnifiedContentPart::Refusal { text } => {
                                        parts.push(GeminiPart::Text { text });
                                    }
                                    UnifiedContentPart::ImageData { mime_type, data } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData { mime_type, data },
                                        });
                                    }
                                    UnifiedContentPart::FileUrl { url, mime_type, .. } => {
                                        parts.push(GeminiPart::FileData {
                                            file_data: GeminiFileData {
                                                mime_type: mime_type.unwrap_or_else(|| {
                                                    "application/octet-stream".to_string()
                                                }),
                                                file_uri: url,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::FileData {
                                        data, mime_type, ..
                                    } => {
                                        parts.push(GeminiPart::InlineData {
                                            inline_data: GeminiInlineData { mime_type, data },
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
                                    UnifiedContentPart::ToolCall(call) => {
                                        parts.push(GeminiPart::FunctionCall {
                                            function_call: GeminiFunctionCall {
                                                name: call.name,
                                                args: call.arguments,
                                            },
                                        });
                                    }
                                    UnifiedContentPart::ToolResult(result) => {
                                        let name = result.name.unwrap_or_else(|| {
                                            build_gemini_fallback_tool_name(&result.tool_call_id)
                                        });
                                        parts.push(GeminiPart::FunctionResponse {
                                            function_response: GeminiFunctionResponse {
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
                                        parts.push(GeminiPart::Text { text });
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
                                    name: call.name,
                                    args: call.arguments,
                                },
                            });
                        }
                        UnifiedItem::FunctionCallOutput(output) => {
                            let name = output.name.unwrap_or_else(|| {
                                build_gemini_fallback_tool_name(&output.tool_call_id)
                            });
                            parts.push(GeminiPart::FunctionResponse {
                                function_response: GeminiFunctionResponse {
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

        let usage_metadata = unified_res.usage.map(|u| {
            let mut prompt_tokens_details = vec![];
            let text_prompt_tokens = u
                .input_tokens
                .saturating_sub(u.input_image_tokens.unwrap_or(0));
            if text_prompt_tokens > 0 {
                prompt_tokens_details.push(ModalityTokenCount {
                    modality: Modality::Text,
                    token_count: text_prompt_tokens,
                });
            }
            if let Some(token_count) = u.input_image_tokens {
                if token_count > 0 {
                    prompt_tokens_details.push(ModalityTokenCount {
                        modality: Modality::Image,
                        token_count,
                    });
                }
            }

            let mut candidates_tokens_details = vec![];
            let text_candidates_tokens = u
                .output_tokens
                .saturating_sub(u.output_image_tokens.unwrap_or(0));
            if text_candidates_tokens > 0 {
                candidates_tokens_details.push(ModalityTokenCount {
                    modality: Modality::Text,
                    token_count: text_candidates_tokens,
                });
            }
            if let Some(token_count) = u.output_image_tokens {
                if token_count > 0 {
                    candidates_tokens_details.push(ModalityTokenCount {
                        modality: Modality::Image,
                        token_count,
                    });
                }
            }

            GeminiUsageMetadata {
                prompt_token_count: u.input_tokens,
                candidates_token_count: u.output_tokens,
                total_token_count: u.total_tokens,
                thoughts_token_count: u.reasoning_tokens,
                cached_content_token_count: u.cached_tokens,
                tool_use_prompt_token_count: None,
                prompt_tokens_details,
                candidates_tokens_details,
                cache_tokens_details: vec![],
                tool_use_prompt_tokens_details: vec![],
            }
        });

        GeminiResponse {
            candidates,
            prompt_feedback: gemini_metadata
                .and_then(|metadata| unified_prompt_feedback_to_gemini(metadata.prompt_feedback)),
            usage_metadata,
            synthetic_metadata: unified_res.synthetic_metadata,
        }
    }
}
