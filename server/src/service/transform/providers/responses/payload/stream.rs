use super::*;

#[derive(Debug, Clone)]
pub enum ResponsesStreamEvent {
    ResponseCreated {
        response: ResponsesResponse,
    },
    ResponseCompleted {
        response: ResponsesResponse,
    },
    ResponseIncomplete {
        response: ResponsesResponse,
    },
    OutputItemAdded {
        output_index: u32,
        item: ItemField,
    },
    OutputItemDone {
        output_index: u32,
        item: ItemField,
    },
    ContentPartAdded {
        item_id: String,
        content_index: u32,
    },
    ContentPartDone {
        item_id: String,
        content_index: u32,
    },
    ReasoningSummaryPartAdded {
        item_id: String,
        summary_index: u32,
    },
    ReasoningSummaryPartDone {
        item_id: String,
        summary_index: u32,
    },
    MessageStart {
        id: Option<String>,
        role: UnifiedRole,
    },
    MessageDelta {
        finish_reason: Option<String>,
    },
    MessageStop,
    ContentBlockStart {
        index: u32,
        kind: UnifiedBlockKind,
    },
    ContentBlockDelta {
        index: u32,
        item_index: Option<u32>,
        item_id: Option<String>,
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
        item_index: Option<u32>,
        item_id: Option<String>,
        id: Option<String>,
        name: Option<String>,
        arguments: String,
    },
    ToolCallArgumentsDone {
        index: u32,
        item_index: Option<u32>,
        item_id: Option<String>,
        id: Option<String>,
        arguments: String,
    },
    ToolCallStop {
        index: u32,
        id: Option<String>,
    },
    ReasoningStart {
        index: u32,
    },
    ReasoningDelta {
        index: u32,
        item_index: Option<u32>,
        item_id: Option<String>,
        part_index: Option<u32>,
        text: String,
    },
    ReasoningStop {
        index: u32,
    },
    Usage {
        usage: UnifiedUsage,
    },
    Blob {
        index: Option<u32>,
        data: Value,
    },
    Error {
        error: Value,
    },
    Item(ItemField),
    Unknown(Value),
}
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
enum TypedResponsesStreamEvent {
    #[serde(rename = "response.created")]
    ResponseCreated { response: ResponsesResponse },
    #[serde(rename = "response.completed")]
    ResponseCompleted { response: ResponsesResponse },
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete { response: ResponsesResponse },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded { output_index: u32, item: ItemField },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone { output_index: u32, item: ItemField },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        item_id: String,
        output_index: u32,
        content_index: u32,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        item_id: String,
        output_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        delta: String,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        item_id: String,
        output_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        arguments: String,
    },
    #[serde(rename = "response.reasoning_summary_part.added")]
    ReasoningSummaryPartAdded { item_id: String, summary_index: u32 },
    #[serde(rename = "response.reasoning_summary_part.done")]
    ReasoningSummaryPartDone { item_id: String, summary_index: u32 },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta {
        item_id: String,
        summary_index: u32,
        delta: String,
    },
    #[serde(rename = "response.content_part.added")]
    ContentPartAdded { item_id: String, content_index: u32 },
    #[serde(rename = "response.content_part.done")]
    ContentPartDone { item_id: String, content_index: u32 },
    #[serde(rename = "response.message.start")]
    MessageStart {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        role: UnifiedRole,
    },
    #[serde(rename = "response.message.delta")]
    MessageDelta {
        #[serde(skip_serializing_if = "Option::is_none")]
        finish_reason: Option<String>,
    },
    #[serde(rename = "response.message.stop")]
    MessageStop,
    #[serde(rename = "response.content_block.start")]
    ContentBlockStart { index: u32, kind: UnifiedBlockKind },
    #[serde(rename = "response.content_block.delta")]
    ContentBlockDelta { index: u32, text: String },
    #[serde(rename = "response.content_block.stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "response.tool_call.start")]
    ToolCallStart {
        index: u32,
        id: String,
        name: String,
    },
    #[serde(rename = "response.tool_call.arguments.delta")]
    LegacyToolCallArgumentsDelta {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        arguments: String,
    },
    #[serde(rename = "response.tool_call.stop")]
    ToolCallStop {
        index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    #[serde(rename = "response.reasoning.start")]
    ReasoningStart { index: u32 },
    #[serde(rename = "response.reasoning.delta")]
    LegacyReasoningDelta { index: u32, text: String },
    #[serde(rename = "response.reasoning.stop")]
    ReasoningStop { index: u32 },
    #[serde(rename = "response.usage")]
    Usage { usage: UnifiedUsage },
    #[serde(rename = "response.blob")]
    Blob {
        #[serde(skip_serializing_if = "Option::is_none")]
        index: Option<u32>,
        data: Value,
    },
    #[serde(rename = "response.error")]
    Error { error: Value },
}

impl ResponsesStreamEvent {
    fn to_public_value(&self) -> Value {
        match self {
            Self::ContentBlockDelta {
                item_index: Some(output_index),
                item_id: Some(item_id),
                part_index: Some(content_index),
                text,
                ..
            } => json!({
                "type": "response.output_text.delta",
                "item_id": item_id,
                "output_index": output_index,
                "content_index": content_index,
                "delta": text
            }),
            Self::ToolCallArgumentsDelta {
                item_index: Some(output_index),
                item_id: Some(item_id),
                name,
                arguments,
                ..
            } => json!({
                "type": "response.function_call_arguments.delta",
                "item_id": item_id,
                "output_index": output_index,
                "name": name,
                "delta": arguments
            }),
            Self::ToolCallArgumentsDone {
                item_index: Some(output_index),
                item_id: Some(item_id),
                id,
                arguments,
                ..
            } => json!({
                "type": "response.function_call_arguments.done",
                "item_id": item_id,
                "output_index": output_index,
                "call_id": id,
                "arguments": arguments
            }),
            Self::ReasoningDelta {
                item_id: Some(item_id),
                part_index: Some(summary_index),
                text,
                ..
            } => json!({
                "type": "response.reasoning_summary_text.delta",
                "item_id": item_id,
                "summary_index": summary_index,
                "delta": text
            }),
            _ => self.to_value(),
        }
    }

    fn from_value(value: Value) -> Self {
        if let Ok(event) = serde_json::from_value::<TypedResponsesStreamEvent>(value.clone()) {
            return match event {
                TypedResponsesStreamEvent::ResponseCreated { response } => {
                    Self::ResponseCreated { response }
                }
                TypedResponsesStreamEvent::ResponseCompleted { response } => {
                    Self::ResponseCompleted { response }
                }
                TypedResponsesStreamEvent::ResponseIncomplete { response } => {
                    Self::ResponseIncomplete { response }
                }
                TypedResponsesStreamEvent::OutputItemAdded { output_index, item } => {
                    Self::OutputItemAdded { output_index, item }
                }
                TypedResponsesStreamEvent::OutputItemDone { output_index, item } => {
                    Self::OutputItemDone { output_index, item }
                }
                TypedResponsesStreamEvent::OutputTextDelta {
                    item_id,
                    output_index,
                    content_index,
                    delta,
                } => Self::ContentBlockDelta {
                    index: content_index,
                    item_index: Some(output_index),
                    item_id: Some(item_id),
                    part_index: Some(content_index),
                    text: delta,
                },
                TypedResponsesStreamEvent::FunctionCallArgumentsDelta {
                    item_id,
                    output_index,
                    name,
                    delta,
                } => Self::ToolCallArgumentsDelta {
                    index: output_index,
                    item_index: Some(output_index),
                    item_id: Some(item_id.clone()),
                    id: Some(item_id),
                    name,
                    arguments: delta,
                },
                TypedResponsesStreamEvent::FunctionCallArgumentsDone {
                    item_id,
                    output_index,
                    call_id,
                    arguments,
                } => Self::ToolCallArgumentsDone {
                    index: output_index,
                    item_index: Some(output_index),
                    item_id: Some(item_id),
                    id: call_id,
                    arguments,
                },
                TypedResponsesStreamEvent::ReasoningSummaryPartAdded {
                    item_id,
                    summary_index,
                } => Self::ReasoningSummaryPartAdded {
                    item_id,
                    summary_index,
                },
                TypedResponsesStreamEvent::ReasoningSummaryPartDone {
                    item_id,
                    summary_index,
                } => Self::ReasoningSummaryPartDone {
                    item_id,
                    summary_index,
                },
                TypedResponsesStreamEvent::ContentPartAdded {
                    item_id,
                    content_index,
                } => Self::ContentPartAdded {
                    item_id,
                    content_index,
                },
                TypedResponsesStreamEvent::ContentPartDone {
                    item_id,
                    content_index,
                } => Self::ContentPartDone {
                    item_id,
                    content_index,
                },
                TypedResponsesStreamEvent::ReasoningSummaryTextDelta {
                    item_id,
                    summary_index,
                    delta,
                } => Self::ReasoningDelta {
                    index: summary_index,
                    item_index: None,
                    item_id: Some(item_id),
                    part_index: Some(summary_index),
                    text: delta,
                },
                TypedResponsesStreamEvent::MessageStart { id, role } => {
                    Self::MessageStart { id, role }
                }
                TypedResponsesStreamEvent::MessageDelta { finish_reason } => {
                    Self::MessageDelta { finish_reason }
                }
                TypedResponsesStreamEvent::MessageStop => Self::MessageStop,
                TypedResponsesStreamEvent::ContentBlockStart { index, kind } => {
                    Self::ContentBlockStart { index, kind }
                }
                TypedResponsesStreamEvent::ContentBlockDelta { index, text } => {
                    Self::ContentBlockDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    }
                }
                TypedResponsesStreamEvent::ContentBlockStop { index } => {
                    Self::ContentBlockStop { index }
                }
                TypedResponsesStreamEvent::ToolCallStart { index, id, name } => {
                    Self::ToolCallStart { index, id, name }
                }
                TypedResponsesStreamEvent::LegacyToolCallArgumentsDelta {
                    index,
                    id,
                    name,
                    arguments,
                } => Self::ToolCallArgumentsDelta {
                    index,
                    item_index: None,
                    item_id: None,
                    id,
                    name,
                    arguments,
                },
                TypedResponsesStreamEvent::ToolCallStop { index, id } => {
                    Self::ToolCallStop { index, id }
                }
                TypedResponsesStreamEvent::ReasoningStart { index } => {
                    Self::ReasoningStart { index }
                }
                TypedResponsesStreamEvent::LegacyReasoningDelta { index, text } => {
                    Self::ReasoningDelta {
                        index,
                        item_index: None,
                        item_id: None,
                        part_index: None,
                        text,
                    }
                }
                TypedResponsesStreamEvent::ReasoningStop { index } => Self::ReasoningStop { index },
                TypedResponsesStreamEvent::Usage { usage } => Self::Usage { usage },
                TypedResponsesStreamEvent::Blob { index, data } => Self::Blob { index, data },
                TypedResponsesStreamEvent::Error { error } => Self::Error { error },
            };
        }

        let Some(_event_type) = value.get("type").and_then(Value::as_str) else {
            return serde_json::from_value::<ItemField>(value.clone())
                .map(Self::Item)
                .unwrap_or(Self::Unknown(value));
        };

        serde_json::from_value::<ItemField>(value.clone())
            .map(Self::Item)
            .unwrap_or(Self::Unknown(value))
    }

    fn to_value(&self) -> Value {
        let typed = match self {
            Self::ResponseCreated { response } => TypedResponsesStreamEvent::ResponseCreated {
                response: response.clone(),
            },
            Self::ResponseCompleted { response } => TypedResponsesStreamEvent::ResponseCompleted {
                response: response.clone(),
            },
            Self::ResponseIncomplete { response } => {
                TypedResponsesStreamEvent::ResponseIncomplete {
                    response: response.clone(),
                }
            }
            Self::OutputItemAdded { output_index, item } => {
                TypedResponsesStreamEvent::OutputItemAdded {
                    output_index: *output_index,
                    item: item.clone(),
                }
            }
            Self::OutputItemDone { output_index, item } => {
                TypedResponsesStreamEvent::OutputItemDone {
                    output_index: *output_index,
                    item: item.clone(),
                }
            }
            Self::ContentPartAdded {
                item_id,
                content_index,
            } => TypedResponsesStreamEvent::ContentPartAdded {
                item_id: item_id.clone(),
                content_index: *content_index,
            },
            Self::ContentPartDone {
                item_id,
                content_index,
            } => TypedResponsesStreamEvent::ContentPartDone {
                item_id: item_id.clone(),
                content_index: *content_index,
            },
            Self::ReasoningSummaryPartAdded {
                item_id,
                summary_index,
            } => TypedResponsesStreamEvent::ReasoningSummaryPartAdded {
                item_id: item_id.clone(),
                summary_index: *summary_index,
            },
            Self::ReasoningSummaryPartDone {
                item_id,
                summary_index,
            } => TypedResponsesStreamEvent::ReasoningSummaryPartDone {
                item_id: item_id.clone(),
                summary_index: *summary_index,
            },
            Self::MessageStart { id, role } => TypedResponsesStreamEvent::MessageStart {
                id: id.clone(),
                role: role.clone(),
            },
            Self::MessageDelta { finish_reason } => TypedResponsesStreamEvent::MessageDelta {
                finish_reason: finish_reason.clone(),
            },
            Self::MessageStop => TypedResponsesStreamEvent::MessageStop,
            Self::ContentBlockStart { index, kind } => {
                TypedResponsesStreamEvent::ContentBlockStart {
                    index: *index,
                    kind: kind.clone(),
                }
            }
            Self::ContentBlockDelta {
                index,
                item_index: _,
                item_id: _,
                part_index: _,
                text,
            } => TypedResponsesStreamEvent::ContentBlockDelta {
                index: *index,
                text: text.clone(),
            },
            Self::ContentBlockStop { index } => {
                TypedResponsesStreamEvent::ContentBlockStop { index: *index }
            }
            Self::ToolCallStart { index, id, name } => TypedResponsesStreamEvent::ToolCallStart {
                index: *index,
                id: id.clone(),
                name: name.clone(),
            },
            Self::ToolCallArgumentsDelta {
                index,
                item_index: _,
                item_id: _,
                id,
                name,
                arguments,
            } => TypedResponsesStreamEvent::LegacyToolCallArgumentsDelta {
                index: *index,
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
            Self::ToolCallArgumentsDone {
                index,
                item_index,
                item_id,
                id,
                arguments,
            } => TypedResponsesStreamEvent::FunctionCallArgumentsDone {
                item_id: item_id.clone().or_else(|| id.clone()).unwrap_or_default(),
                output_index: item_index.unwrap_or(*index),
                call_id: id.clone(),
                arguments: arguments.clone(),
            },
            Self::ToolCallStop { index, id } => TypedResponsesStreamEvent::ToolCallStop {
                index: *index,
                id: id.clone(),
            },
            Self::ReasoningStart { index } => {
                TypedResponsesStreamEvent::ReasoningStart { index: *index }
            }
            Self::ReasoningDelta {
                index,
                item_index: _,
                item_id: _,
                part_index: _,
                text,
            } => TypedResponsesStreamEvent::LegacyReasoningDelta {
                index: *index,
                text: text.clone(),
            },
            Self::ReasoningStop { index } => {
                TypedResponsesStreamEvent::ReasoningStop { index: *index }
            }
            Self::Usage { usage } => TypedResponsesStreamEvent::Usage {
                usage: usage.clone(),
            },
            Self::Blob { index, data } => TypedResponsesStreamEvent::Blob {
                index: *index,
                data: data.clone(),
            },
            Self::Error { error } => TypedResponsesStreamEvent::Error {
                error: error.clone(),
            },
            Self::Item(item) => return serde_json::to_value(item).unwrap_or(Value::Null),
            Self::Unknown(value) => return value.clone(),
        };

        serde_json::to_value(typed).unwrap_or(Value::Null)
    }
}

#[derive(Debug, Clone)]
pub struct ResponsesChunkResponse {
    pub id: String,
    pub model: String,
    pub event: ResponsesStreamEvent,
}

impl Serialize for ResponsesChunkResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.event.to_public_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ResponsesChunkResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;

        #[derive(Deserialize)]
        struct LegacyWrappedResponsesChunkResponse {
            id: String,
            model: String,
            delta: Value,
        }

        if let Ok(raw) =
            serde_json::from_value::<LegacyWrappedResponsesChunkResponse>(value.clone())
        {
            return Ok(Self {
                id: raw.id,
                model: raw.model,
                event: ResponsesStreamEvent::from_value(raw.delta),
            });
        }

        let id = value
            .get("response")
            .and_then(|response| response.get("id"))
            .or_else(|| value.get("id"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let model = value
            .get("response")
            .and_then(|response| response.get("model"))
            .or_else(|| value.get("model"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();

        Ok(Self {
            id,
            model,
            event: ResponsesStreamEvent::from_value(value),
        })
    }
}
