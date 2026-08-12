use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::request::{UnifiedContentPart, UnifiedItem, UnifiedRole};
use super::response::{UnifiedProviderSessionMetadata, UnifiedSyntheticMetadata};
use super::usage::UnifiedUsage;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedToolCallDelta {
    pub index: u32,
    pub id: Option<String>,
    pub name: Option<String>,
    pub arguments: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedContentPartDelta {
    TextDelta {
        index: u32,
        text: String,
    },
    ImageDelta {
        index: u32,
        url: Option<String>,
        data: Option<String>,
    },
    ToolCallDelta(UnifiedToolCallDelta),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct UnifiedMessageDelta {
    pub role: Option<UnifiedRole>,
    pub content: Vec<UnifiedContentPartDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedChunkChoice {
    pub index: u32,
    pub delta: UnifiedMessageDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChunkResponse {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChunkChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_metadata: Option<UnifiedProviderSessionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChunkResponseCore {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub choices: Vec<UnifiedChunkChoice>,
    pub usage: Option<UnifiedUsage>,
    pub created: Option<i64>,
    pub object: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UnifiedChunkResponseContext {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_session_metadata: Option<UnifiedProviderSessionMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub synthetic_metadata: Option<UnifiedSyntheticMetadata>,
}

impl UnifiedChunkResponseContext {
    pub fn is_empty(&self) -> bool {
        self.provider_session_metadata.is_none() && self.synthetic_metadata.is_none()
    }
}

impl UnifiedChunkResponse {
    pub fn core(&self) -> UnifiedChunkResponseCore {
        UnifiedChunkResponseCore {
            id: self.id.clone(),
            model: self.model.clone(),
            choices: self.choices.clone(),
            usage: self.usage.clone(),
            created: self.created,
            object: self.object.clone(),
        }
    }

    pub fn context(&self) -> UnifiedChunkResponseContext {
        UnifiedChunkResponseContext {
            provider_session_metadata: self.provider_session_metadata.clone(),
            synthetic_metadata: self.synthetic_metadata.clone(),
        }
    }

    pub fn from_core_and_context(
        core: UnifiedChunkResponseCore,
        context: UnifiedChunkResponseContext,
    ) -> Self {
        Self {
            id: core.id,
            model: core.model,
            choices: core.choices,
            usage: core.usage,
            created: core.created,
            object: core.object,
            provider_session_metadata: context.provider_session_metadata,
            synthetic_metadata: context.synthetic_metadata,
        }
    }

    pub fn into_core_and_context(self) -> (UnifiedChunkResponseCore, UnifiedChunkResponseContext) {
        (
            UnifiedChunkResponseCore {
                id: self.id,
                model: self.model,
                choices: self.choices,
                usage: self.usage,
                created: self.created,
                object: self.object,
            },
            UnifiedChunkResponseContext {
                provider_session_metadata: self.provider_session_metadata,
                synthetic_metadata: self.synthetic_metadata,
            },
        )
    }

    pub fn synthetic_metadata(&self) -> Option<&UnifiedSyntheticMetadata> {
        self.synthetic_metadata.as_ref()
    }

    pub fn provider_session_metadata(&self) -> Option<&UnifiedProviderSessionMetadata> {
        self.provider_session_metadata.as_ref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedBlockKind {
    Text,
    ToolCall,
    Reasoning,
    Blob,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UnifiedStreamEvent {
    ItemAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        item: UnifiedItem,
    },
    ItemDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        item: UnifiedItem,
    },
    MessageStart {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        model: Option<String>,
        role: UnifiedRole,
    },
    ContentPartAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<UnifiedContentPart>,
    },
    ContentPartDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
    },
    MessageDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    MessageStop,
    ContentBlockStart {
        index: u32,
        kind: UnifiedBlockKind,
    },
    ContentBlockDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        part_index: Option<u32>,
        text: String,
    },
    RefusalDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        part_index: Option<u32>,
        text: String,
    },
    ContentBlockStop {
        index: u32,
    },
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    ToolCallArgumentsDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments: String,
    },
    ToolCallStop {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    ReasoningStart {
        index: u32,
    },
    ReasoningSummaryPartAdded {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        part: Option<UnifiedContentPart>,
    },
    ReasoningSummaryPartDone {
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        part_index: u32,
    },
    ReasoningDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_index: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        part_index: Option<u32>,
        text: String,
    },
    ReasoningStop {
        index: u32,
    },
    BlobDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        data: Value,
    },
    Usage {
        usage: UnifiedUsage,
    },
    Error {
        error: Value,
    },
}

/// Returns whether a typed source event contains the first meaningful output
/// fact used by TTFT. This deliberately ignores lifecycle and metadata-only
/// events; it never returns or stores the observed content.
pub(crate) fn meaningful_output_from_stream_event(event: &UnifiedStreamEvent) -> bool {
    match event {
        // Item/part lifecycle snapshots may carry already-populated content,
        // but the metric is established only by semantic output events.
        UnifiedStreamEvent::ItemAdded { .. }
        | UnifiedStreamEvent::ItemDone { .. }
        | UnifiedStreamEvent::ContentPartAdded { .. }
        | UnifiedStreamEvent::ReasoningSummaryPartAdded { .. } => false,
        UnifiedStreamEvent::ContentBlockDelta { text, .. }
        | UnifiedStreamEvent::RefusalDelta { text, .. }
        | UnifiedStreamEvent::ReasoningDelta { text, .. } => meaningful_text(text),
        UnifiedStreamEvent::ToolCallStart { name, .. } => meaningful_text(name),
        UnifiedStreamEvent::ToolCallArgumentsDelta {
            name, arguments, ..
        } => name.as_deref().is_some_and(meaningful_text) || meaningful_text(arguments),
        UnifiedStreamEvent::BlobDelta { data, .. } => meaningful_blob_value(data),
        UnifiedStreamEvent::MessageStart { .. }
        | UnifiedStreamEvent::ContentPartDone { .. }
        | UnifiedStreamEvent::MessageDelta { .. }
        | UnifiedStreamEvent::MessageStop
        | UnifiedStreamEvent::ContentBlockStart { .. }
        | UnifiedStreamEvent::ContentBlockStop { .. }
        | UnifiedStreamEvent::ToolCallStop { .. }
        | UnifiedStreamEvent::ReasoningStart { .. }
        | UnifiedStreamEvent::ReasoningSummaryPartDone { .. }
        | UnifiedStreamEvent::ReasoningStop { .. }
        | UnifiedStreamEvent::Usage { .. }
        | UnifiedStreamEvent::Error { .. } => false,
    }
}

pub(crate) fn meaningful_output_from_stream_events(events: &[UnifiedStreamEvent]) -> bool {
    events.iter().any(meaningful_output_from_stream_event)
}

pub(crate) fn meaningful_output_from_legacy_chunk(chunk: &UnifiedChunkResponse) -> bool {
    chunk.choices.iter().any(|choice| {
        choice.delta.content.iter().any(|part| match part {
            UnifiedContentPartDelta::TextDelta { text, .. } => meaningful_text(text),
            UnifiedContentPartDelta::ImageDelta { url, data, .. } => {
                url.as_deref().is_some_and(meaningful_text)
                    || data.as_deref().is_some_and(meaningful_text)
            }
            UnifiedContentPartDelta::ToolCallDelta(tool_call) => {
                tool_call.name.as_deref().is_some_and(meaningful_text)
                    || tool_call.arguments.as_deref().is_some_and(meaningful_text)
            }
        })
    })
}

fn meaningful_text(text: &str) -> bool {
    !text.trim().is_empty()
}

/// A Blob is meaningful when it contains a non-null scalar or recursively
/// contains a meaningful array/object member. Empty containers and null-only
/// containers are metadata, not output.
pub(crate) fn meaningful_blob_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(_) | Value::Number(_) => true,
        Value::String(text) => !text.is_empty(),
        Value::Array(values) => values.iter().any(meaningful_blob_value),
        Value::Object(values) => {
            if is_metadata_blob_object(values) {
                return false;
            }
            values.values().any(meaningful_blob_value)
        }
    }
}

fn is_metadata_blob_object(values: &serde_json::Map<String, Value>) -> bool {
    let kind = values
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    kind == "signature_delta"
        || kind.contains("metadata")
        || values.keys().any(|key| {
            matches!(
                key.as_str(),
                "provider_metadata"
                    | "provider_session_metadata"
                    | "response_metadata"
                    | "synthetic_metadata"
            )
        })
        || values.contains_key("provider") && values.contains_key("metadata")
}

#[cfg(test)]
mod meaningful_output_tests {
    use serde_json::json;

    use super::{
        UnifiedBlockKind, UnifiedContentPartDelta, UnifiedStreamEvent, UnifiedToolCallDelta,
        meaningful_blob_value, meaningful_output_from_legacy_chunk,
        meaningful_output_from_stream_event,
    };
    use crate::service::transform::unified::{
        UnifiedChunkChoice, UnifiedChunkResponse, UnifiedMessageDelta,
    };

    #[test]
    fn blob_meaningfulness_is_recursive_and_does_not_use_json_field_names() {
        for value in [
            json!(null),
            json!(""),
            json!([]),
            json!({}),
            json!([null, {}]),
        ] {
            assert!(!meaningful_blob_value(&value), "{value}");
        }
        for value in [
            json!(false),
            json!(0),
            json!(" "),
            json!([null, "payload"]),
            json!({"metadata": {"blob": "payload"}}),
        ] {
            assert!(meaningful_blob_value(&value), "{value}");
        }
        assert!(!meaningful_blob_value(&json!({
            "provider": "anthropic",
            "type": "signature_delta",
            "signature": "opaque"
        })));
        for value in [
            json!({"provider": "responses", "type": "provider_metadata", "value": "opaque"}),
            json!({"type": "response.metadata", "value": "opaque"}),
            json!({"type": "metadata", "value": "opaque"}),
            json!({"provider": "unknown", "metadata": {"value": "opaque"}}),
        ] {
            assert!(!meaningful_blob_value(&value), "{value}");
        }
    }

    #[test]
    fn unified_stream_event_predicate_separates_output_from_lifecycle_and_metadata() {
        assert!(!meaningful_output_from_stream_event(
            &UnifiedStreamEvent::MessageStart {
                id: Some("id-only".to_string()),
                model: Some("model-only".to_string()),
                role: crate::service::transform::unified::UnifiedRole::Assistant,
            }
        ));
        assert!(!meaningful_output_from_stream_event(
            &UnifiedStreamEvent::ContentBlockStart {
                index: 0,
                kind: UnifiedBlockKind::Text,
            }
        ));
        assert!(!meaningful_output_from_stream_event(
            &UnifiedStreamEvent::ContentBlockDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: " \n".to_string(),
            }
        ));
        assert!(meaningful_output_from_stream_event(
            &UnifiedStreamEvent::ReasoningDelta {
                index: 0,
                item_index: None,
                item_id: None,
                part_index: None,
                text: "thinking".to_string(),
            }
        ));
        assert!(meaningful_output_from_stream_event(
            &UnifiedStreamEvent::ToolCallStart {
                index: 0,
                id: "id-only".to_string(),
                name: "lookup".to_string(),
            }
        ));
        assert!(meaningful_output_from_stream_event(
            &UnifiedStreamEvent::ToolCallArgumentsDelta {
                index: 0,
                item_index: None,
                item_id: None,
                id: Some("id-only".to_string()),
                name: None,
                arguments: "{}".to_string(),
            }
        ));
        assert!(!meaningful_output_from_stream_event(
            &UnifiedStreamEvent::BlobDelta {
                index: None,
                data: json!({
                    "provider": "anthropic",
                    "type": "signature_delta",
                    "signature": "opaque"
                }),
            }
        ));
        assert!(meaningful_output_from_stream_event(
            &UnifiedStreamEvent::BlobDelta {
                index: None,
                data: json!({"image": {"data": "payload"}}),
            }
        ));
        assert!(!meaningful_output_from_stream_event(
            &UnifiedStreamEvent::Usage {
                usage: Default::default(),
            }
        ));
        assert!(!meaningful_output_from_stream_event(
            &UnifiedStreamEvent::MessageDelta {
                finish_reason: Some("stop".to_string()),
            }
        ));
    }

    #[test]
    fn legacy_chunk_predicate_ignores_role_and_ids_but_accepts_tool_and_multimodal_delta() {
        let role_only = UnifiedChunkResponse {
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: Some(crate::service::transform::unified::UnifiedRole::Assistant),
                    content: vec![],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        assert!(!meaningful_output_from_legacy_chunk(&role_only));

        let tool = UnifiedChunkResponse {
            choices: vec![UnifiedChunkChoice {
                index: 0,
                delta: UnifiedMessageDelta {
                    role: None,
                    content: vec![UnifiedContentPartDelta::ToolCallDelta(
                        UnifiedToolCallDelta {
                            index: 0,
                            id: Some("id-only".to_string()),
                            name: Some("lookup".to_string()),
                            arguments: None,
                        },
                    )],
                },
                finish_reason: None,
            }],
            ..Default::default()
        };
        assert!(meaningful_output_from_legacy_chunk(&tool));
    }
}

pub fn map_gemini_finish_reason_to_openai(reason: &str, has_tool_call: bool) -> String {
    match reason {
        "STOP" => {
            if has_tool_call {
                "tool_calls".to_string()
            } else {
                "stop".to_string()
            }
        }
        "TOOL_USE" => "tool_calls".to_string(),
        "MAX_TOKENS" => "length".to_string(),
        "SAFETY" | "RECITATION" => "content_filter".to_string(),
        _ => "stop".to_string(),
    }
}

pub fn map_openai_finish_reason_to_gemini(reason: &str) -> String {
    match reason {
        "stop" => "STOP".to_string(),
        "length" => "MAX_TOKENS".to_string(),
        "content_filter" => "SAFETY".to_string(),
        "tool_calls" => "TOOL_USE".to_string(),
        _ => "FINISH_REASON_UNSPECIFIED".to_string(),
    }
}

pub fn map_anthropic_finish_reason_to_openai(reason: &str) -> String {
    match reason {
        "end_turn" | "stop_sequence" => "stop".to_string(),
        "tool_use" => "tool_calls".to_string(),
        "max_tokens" => "length".to_string(),
        _ => "stop".to_string(),
    }
}

pub fn map_openai_finish_reason_to_anthropic(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "length" => "max_tokens".to_string(),
        "content_filter" => "refusal".to_string(),
        _ => "end_turn".to_string(),
    }
}
