use super::request::{
    default_completed_status, default_function_call_id, default_function_call_output_id,
    default_message_id,
};
use super::*;

#[derive(Debug, Clone)]
pub enum ItemField {
    Message(Message),
    FunctionCall(FunctionCall),
    FunctionCallOutput(FunctionCallOutput),
    Reasoning(ReasoningBody),
    Unknown(Value),
}

impl Serialize for ItemField {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ItemField::Message(value) => value.serialize(serializer),
            ItemField::FunctionCall(value) => value.serialize(serializer),
            ItemField::FunctionCallOutput(value) => value.serialize(serializer),
            ItemField::Reasoning(value) => value.serialize(serializer),
            ItemField::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for ItemField {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let type_name = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match type_name {
            "message" => match serde_json::from_value::<Message>(value.clone()) {
                Ok(message) => Ok(ItemField::Message(message)),
                Err(_) => try_deserialize_shorthand_message(&value)
                    .map(ItemField::Message)
                    .ok_or_else(|| serde::de::Error::custom("failed to deserialize message item")),
            },
            "function_call" => serde_json::from_value(value)
                .map(ItemField::FunctionCall)
                .map_err(serde::de::Error::custom),
            "function_call_output" => serde_json::from_value(value)
                .map(ItemField::FunctionCallOutput)
                .map_err(serde::de::Error::custom),
            "reasoning" => serde_json::from_value(value)
                .map(ItemField::Reasoning)
                .map_err(serde::de::Error::custom),
            _ => try_deserialize_shorthand_message(&value)
                .map(ItemField::Message)
                .or_else(|| Some(ItemField::Unknown(value)))
                .ok_or_else(|| serde::de::Error::custom("failed to deserialize item")),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ShorthandMessageContent {
    Text(String),
    Parts(Vec<ItemContentPart>),
}

#[derive(Debug, Deserialize)]
struct ShorthandMessage {
    role: MessageRole,
    content: ShorthandMessageContent,
}

fn try_deserialize_shorthand_message(value: &Value) -> Option<Message> {
    let shorthand: ShorthandMessage = serde_json::from_value(value.clone()).ok()?;
    let content = match shorthand.content {
        ShorthandMessageContent::Text(text) => {
            shorthand_text_content_to_message_parts(&shorthand.role, text)
        }
        ShorthandMessageContent::Parts(parts) => parts,
    };

    Some(Message {
        _type: "message".to_string(),
        id: format!("msg_{}", crate::utils::ID_GENERATOR.generate_id()),
        status: MessageStatus::Completed,
        role: shorthand.role,
        content,
    })
}

fn shorthand_text_content_to_message_parts(
    role: &MessageRole,
    text: String,
) -> Vec<ItemContentPart> {
    match role {
        MessageRole::Assistant => vec![ItemContentPart::OutputText {
            text,
            annotations: Vec::new(),
            logprobs: None,
        }],
        MessageRole::User | MessageRole::System | MessageRole::Developer => {
            vec![ItemContentPart::InputText { text }]
        }
    }
}

fn default_input_image_detail() -> String {
    "auto".to_string()
}

fn serialize_output_text_logprobs<S>(
    logprobs: &Option<Vec<LogProb>>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match logprobs {
        Some(logprobs) => logprobs.serialize(serializer),
        None => Vec::<LogProb>::new().serialize(serializer),
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "type")]
    pub _type: String,
    #[serde(default = "default_message_id")]
    pub id: String,
    #[serde(default = "default_completed_status")]
    pub status: MessageStatus,
    pub role: MessageRole,
    pub content: Vec<ItemContentPart>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ItemContentPart {
    InputText {
        text: String,
    },
    OutputText {
        text: String,
        annotations: Vec<Annotation>,
        #[serde(serialize_with = "serialize_output_text_logprobs")]
        logprobs: Option<Vec<LogProb>>,
    },
    Text {
        text: String,
    },
    SummaryText {
        text: String,
    },
    ReasoningText {
        text: String,
    },
    Refusal {
        refusal: String,
    },
    InputImage {
        image_url: Option<String>,
        #[serde(default = "default_input_image_detail")]
        detail: String,
    },
    InputFile {
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_url: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        file_data: Option<String>,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Annotation {
    UrlCitation {
        url: String,
        start_index: u32,
        end_index: u32,
        title: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LogProb {
    pub token: String,
    pub logprob: f64,
    pub bytes: Vec<u8>,
    pub top_logprobs: Vec<TopLogProb>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopLogProb {
    pub token: String,
    pub logprob: f64,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCall {
    #[serde(rename = "type")]
    pub _type: String,
    #[serde(default = "default_function_call_id")]
    pub id: String,
    pub call_id: String,
    pub name: String,
    pub arguments: String,
    #[serde(default = "default_completed_status")]
    pub status: MessageStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FunctionCallOutput {
    #[serde(rename = "type")]
    pub _type: String,
    #[serde(default = "default_function_call_output_id")]
    pub id: String,
    pub call_id: String,
    pub output: FunctionCallOutputPayload,
    #[serde(default = "default_completed_status")]
    pub status: MessageStatus,
}

#[derive(Debug, Clone)]
pub enum FunctionCallOutputPayload {
    Text(String),
    Content(Vec<FunctionCallOutputContent>),
    Unknown(Value),
}

impl Serialize for FunctionCallOutputPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Text(value) => value.serialize(serializer),
            Self::Content(value) => value.serialize(serializer),
            Self::Unknown(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for FunctionCallOutputPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        match value {
            Value::String(text) => Ok(Self::Text(text)),
            Value::Array(items) => {
                let content = items
                    .into_iter()
                    .map(FunctionCallOutputContent::from_value)
                    .collect();
                Ok(Self::Content(content))
            }
            other => Ok(Self::Unknown(other)),
        }
    }
}

#[derive(Debug, Clone)]
pub enum FunctionCallOutputContent {
    Text {
        text: String,
    },
    File {
        filename: Option<String>,
        file_url: Option<String>,
    },
    Image {
        image_url: Option<String>,
        file_url: Option<String>,
    },
    Unknown(Value),
}

impl FunctionCallOutputContent {
    fn from_value(value: Value) -> Self {
        let type_name = value
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();

        match type_name {
            "text" | "output_text" => Self::Text {
                text: value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
            "file" => Self::File {
                filename: value
                    .get("filename")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                file_url: value
                    .get("file_url")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            },
            "image" => Self::Image {
                image_url: value
                    .get("image_url")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                file_url: value
                    .get("file_url")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            },
            _ => Self::Unknown(value),
        }
    }

    fn to_value(&self) -> Value {
        match self {
            Self::Text { text } => json!({
                "type": "text",
                "text": text
            }),
            Self::File { filename, file_url } => json!({
                "type": "file",
                "filename": filename,
                "file_url": file_url
            }),
            Self::Image {
                image_url,
                file_url,
            } => json!({
                "type": "image",
                "image_url": image_url,
                "file_url": file_url
            }),
            Self::Unknown(value) => value.clone(),
        }
    }
}

impl Serialize for FunctionCallOutputContent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_value().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for FunctionCallOutputContent {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Self::from_value(Value::deserialize(deserializer)?))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ReasoningBody {
    #[serde(rename = "type")]
    pub _type: String,
    pub id: String,
    pub content: Option<Vec<ItemContentPart>>,
    pub summary: Vec<ItemContentPart>,
    pub encrypted_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ResponsesReasoningMetadata {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub encrypted_contents: Vec<String>,
}
