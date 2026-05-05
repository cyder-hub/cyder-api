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
        _ => "end_turn".to_string(),
    }
}
