use serde_json::{Value, json};

use crate::schema::enum_def::LlmApiType;
use crate::service::transform::stream::StreamTransformContext;
use crate::service::transform::unified::*;
use crate::service::transform::{
    TransformProtocol, TransformValueKind, build_stream_diagnostic_sse,
};
use crate::utils::ID_GENERATOR;
use crate::utils::sse::SseEvent;

use super::payload::*;

pub(crate) fn build_gemini_tool_call_key(
    provider_order: u32,
    message_index: u32,
    part_index: u32,
    function_name: &str,
) -> String {
    format!(
        "provider_order={provider_order}:message_index={message_index}:part_index={part_index}:function_name={function_name}"
    )
}

pub(crate) fn build_gemini_synthetic_tool_call_id(
    provider_order: u32,
    message_index: u32,
    part_index: u32,
    function_name: &str,
) -> String {
    let normalized_name: String = function_name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();

    format!("gemini-call-{provider_order}-{message_index}-{part_index}-{normalized_name}")
}

pub(crate) fn build_gemini_synthetic_response_id(kind: &str) -> String {
    format!("gemini-{kind}-{}", ID_GENERATOR.generate_id())
}

pub(crate) fn build_gemini_fallback_tool_name(tool_call_id: &str) -> String {
    let normalized: String = tool_call_id
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    format!("gemini-tool-result-{normalized}")
}

pub(crate) fn build_gemini_stream_diagnostic(
    context: &mut StreamTransformContext<'_>,
    kind: TransformValueKind,
    context_message: String,
) -> SseEvent {
    build_stream_diagnostic_sse(
        context,
        TransformProtocol::Unified,
        TransformProtocol::Api(LlmApiType::Gemini),
        kind,
        "gemini_stream_encoding",
        context_message,
        None,
        Some(
            "Use a Responses or Anthropic target when structured reasoning/blob stream events must remain recoverable.".to_string(),
        ),
    )
}

pub(crate) fn gemini_inline_data_from_blob(value: &Value) -> Option<GeminiInlineData> {
    let object = value.as_object()?;
    let mime_type = object.get("mime_type")?.as_str()?;
    let data = object.get("data")?.as_str()?;
    Some(GeminiInlineData {
        mime_type: mime_type.to_string(),
        data: data.to_string(),
    })
}

pub(crate) fn render_gemini_image_reference_text(url: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) if !detail.is_empty() => format!("image_url: {url}\ndetail: {detail}"),
        _ => format!("image_url: {url}"),
    }
}

pub(crate) fn render_gemini_tool_call_text(call: &UnifiedToolCall) -> String {
    format!(
        "tool_call: {}\narguments: {}",
        call.name,
        serde_json::to_string(&call.arguments).unwrap_or_default()
    )
}

pub(crate) fn render_gemini_tool_result_text(result: &UnifiedToolResult) -> String {
    match result.name.as_deref() {
        Some(name) if !name.is_empty() => format!(
            "tool_result: {name}\ntool_call_id: {}\ncontent: {}",
            result.tool_call_id,
            result.legacy_content()
        ),
        _ => format!(
            "tool_result_id: {}\ncontent: {}",
            result.tool_call_id,
            result.legacy_content()
        ),
    }
}

pub(crate) fn gemini_function_response_to_unified_output(
    response: Value,
) -> UnifiedToolResultOutput {
    match response {
        Value::Object(object) => {
            if let Some(result) = object.get("result") {
                unified_tool_result_output_from_value(result.clone())
            } else {
                UnifiedToolResultOutput::Json {
                    value: Value::Object(object),
                }
            }
        }
        other => unified_tool_result_output_from_value(other),
    }
}

pub(crate) fn unified_tool_result_to_gemini_response(output: &UnifiedToolResultOutput) -> Value {
    match output {
        UnifiedToolResultOutput::Text { text } => json!({ "result": text }),
        other => unified_tool_result_output_to_value(other),
    }
}

pub(crate) fn build_gemini_synthetic_metadata(
    id: bool,
    model: bool,
    gemini_safety_ratings: bool,
) -> Option<UnifiedSyntheticMetadata> {
    let metadata = UnifiedSyntheticMetadata {
        id,
        model,
        gemini_safety_ratings,
    };

    (metadata.id || metadata.model || metadata.gemini_safety_ratings).then_some(metadata)
}

pub(crate) fn merge_gemini_synthetic_metadata(
    existing: Option<UnifiedSyntheticMetadata>,
    generated: Option<UnifiedSyntheticMetadata>,
) -> Option<UnifiedSyntheticMetadata> {
    match (existing, generated) {
        (Some(existing), Some(generated)) => Some(UnifiedSyntheticMetadata {
            id: existing.id || generated.id,
            model: existing.model || generated.model,
            gemini_safety_ratings: existing.gemini_safety_ratings
                || generated.gemini_safety_ratings,
        }),
        (Some(existing), None) => Some(existing),
        (None, Some(generated)) => Some(generated),
        (None, None) => None,
    }
}

pub(crate) fn build_unified_tool_name_lookup(
    request: &UnifiedRequest,
) -> std::collections::HashMap<String, String> {
    let mut tool_name_by_id = std::collections::HashMap::new();
    for item in request.content_items() {
        match item {
            UnifiedItem::FunctionCall(call) => {
                tool_name_by_id.insert(call.id, call.name);
            }
            UnifiedItem::Message(message) => {
                for part in &message.content {
                    if let UnifiedContentPart::ToolCall(call) = part {
                        tool_name_by_id.insert(call.id.clone(), call.name.clone());
                    }
                }
            }
            _ => {}
        }
    }
    tool_name_by_id
}

pub(crate) fn gemini_inline_data_to_unified_content(
    inline_data: GeminiInlineData,
) -> UnifiedContentPart {
    if inline_data.mime_type.starts_with("image/") {
        UnifiedContentPart::ImageData {
            mime_type: inline_data.mime_type,
            data: inline_data.data,
        }
    } else {
        UnifiedContentPart::FileData {
            data: inline_data.data,
            mime_type: inline_data.mime_type,
            filename: None,
        }
    }
}
pub(crate) fn gemini_safety_ratings_to_unified(
    safety_ratings: Option<Vec<GeminiSafetyRating>>,
) -> Vec<UnifiedGeminiSafetyRating> {
    safety_ratings
        .unwrap_or_default()
        .into_iter()
        .map(|rating| UnifiedGeminiSafetyRating {
            category: rating.category,
            probability: rating.probability,
        })
        .collect()
}

pub(crate) fn gemini_citation_metadata_to_unified(
    citation_metadata: Option<GeminiCitationMetadata>,
) -> Option<UnifiedGeminiCitationMetadata> {
    citation_metadata.map(|metadata| UnifiedGeminiCitationMetadata {
        citation_sources: metadata
            .citation_sources
            .into_iter()
            .map(|source| UnifiedGeminiCitationSource {
                start_index: source.start_index,
                end_index: source.end_index,
                uri: source.uri,
                license: source.license,
            })
            .collect(),
    })
}

pub(crate) fn gemini_citation_metadata_to_annotations(
    citation_metadata: Option<GeminiCitationMetadata>,
) -> Vec<UnifiedAnnotation> {
    citation_metadata
        .map(|metadata| {
            metadata
                .citation_sources
                .into_iter()
                .map(|source| {
                    UnifiedAnnotation::Citation(UnifiedCitation {
                        part_index: None,
                        start_index: source.start_index,
                        end_index: source.end_index,
                        url: source.uri,
                        title: None,
                        license: source.license,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn gemini_prompt_feedback_to_unified(
    prompt_feedback: Option<GeminiPromptFeedback>,
) -> Option<UnifiedGeminiPromptFeedback> {
    prompt_feedback.map(|feedback| UnifiedGeminiPromptFeedback {
        block_reason: feedback.block_reason,
        safety_ratings: feedback
            .safety_ratings
            .into_iter()
            .map(|rating| UnifiedGeminiSafetyRating {
                category: rating.category,
                probability: rating.probability,
            })
            .collect(),
    })
}

pub(crate) fn build_gemini_response_metadata(
    prompt_feedback: Option<GeminiPromptFeedback>,
    candidates: &[GeminiCandidate],
) -> Option<UnifiedProviderResponseMetadata> {
    let candidates = candidates
        .iter()
        .map(|candidate| UnifiedGeminiCandidateMetadata {
            index: candidate.index.unwrap_or(0),
            safety_ratings: gemini_safety_ratings_to_unified(candidate.safety_ratings.clone()),
            citation_metadata: gemini_citation_metadata_to_unified(
                candidate.citation_metadata.clone(),
            ),
            token_count: candidate.token_count,
        })
        .filter(|candidate| {
            !candidate.safety_ratings.is_empty()
                || candidate.citation_metadata.is_some()
                || candidate.token_count.is_some()
        })
        .collect::<Vec<_>>();

    let prompt_feedback = gemini_prompt_feedback_to_unified(prompt_feedback);

    if prompt_feedback.is_none() && candidates.is_empty() {
        None
    } else {
        Some(UnifiedProviderResponseMetadata {
            gemini: Some(UnifiedGeminiResponseMetadata {
                prompt_feedback,
                candidates,
            }),
            ..Default::default()
        })
    }
}

pub(crate) fn build_gemini_session_metadata(
    prompt_feedback: Option<GeminiPromptFeedback>,
    candidates: &[GeminiCandidate],
) -> Option<UnifiedProviderSessionMetadata> {
    build_gemini_response_metadata(prompt_feedback, candidates).map(|metadata| {
        UnifiedProviderSessionMetadata {
            gemini: metadata.gemini,
            anthropic: None,
            responses: None,
        }
    })
}

pub(crate) fn unified_safety_ratings_to_gemini(
    safety_ratings: Vec<UnifiedGeminiSafetyRating>,
) -> Option<Vec<GeminiSafetyRating>> {
    let ratings = safety_ratings
        .into_iter()
        .map(|rating| GeminiSafetyRating {
            category: rating.category,
            probability: rating.probability,
        })
        .collect::<Vec<_>>();

    (!ratings.is_empty()).then_some(ratings)
}

pub(crate) fn unified_citation_metadata_to_gemini(
    citation_metadata: Option<UnifiedGeminiCitationMetadata>,
) -> Option<GeminiCitationMetadata> {
    citation_metadata.map(|metadata| GeminiCitationMetadata {
        citation_sources: metadata
            .citation_sources
            .into_iter()
            .map(|source| GeminiCitationSource {
                start_index: source.start_index,
                end_index: source.end_index,
                uri: source.uri,
                license: source.license,
            })
            .collect(),
    })
}

pub(crate) fn unified_annotations_to_gemini_citation_metadata(
    annotations: &[UnifiedAnnotation],
) -> Option<GeminiCitationMetadata> {
    let citation_sources = annotations
        .iter()
        .filter_map(|annotation| match annotation {
            UnifiedAnnotation::Citation(citation) => Some(GeminiCitationSource {
                start_index: citation.start_index,
                end_index: citation.end_index,
                uri: citation.url.clone(),
                license: citation.license.clone(),
            }),
        })
        .collect::<Vec<_>>();

    (!citation_sources.is_empty()).then_some(GeminiCitationMetadata { citation_sources })
}

pub(crate) fn unified_prompt_feedback_to_gemini(
    prompt_feedback: Option<UnifiedGeminiPromptFeedback>,
) -> Option<GeminiPromptFeedback> {
    prompt_feedback.map(|feedback| GeminiPromptFeedback {
        block_reason: feedback.block_reason,
        safety_ratings: feedback
            .safety_ratings
            .into_iter()
            .map(|rating| GeminiSafetyRating {
                category: rating.category,
                probability: rating.probability,
            })
            .collect(),
    })
}

// Helper to recursively transform Gemini tool parameter types to lowercase for OpenAI.
pub(crate) fn transform_gemini_tool_params_to_openai(params: &mut Value) {
    if let Some(obj) = params.as_object_mut() {
        // Transform "type" field
        if let Some(type_val) = obj.get_mut("type") {
            if let Some(type_str) = type_val.as_str() {
                *type_val = json!(type_str.to_lowercase());
            }
        }
        // Recurse into "properties"
        if let Some(properties) = obj.get_mut("properties") {
            if let Some(props_obj) = properties.as_object_mut() {
                for (_, prop_val) in props_obj.iter_mut() {
                    transform_gemini_tool_params_to_openai(prop_val);
                }
            }
        }
        // Recurse into "items" for arrays
        if let Some(items) = obj.get_mut("items") {
            transform_gemini_tool_params_to_openai(items);
        }
    }
}
