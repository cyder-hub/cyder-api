use bincode::{Decode, Encode};
use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "provider_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderType {
    #[default]
    Openai,
    Gemini,
    Vertex,
    VertexOpenai,
    Ollama,
    Anthropic,
    Responses,
    GeminiOpenai,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "llm_api_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LlmApiType {
    #[default]
    Openai,
    Gemini,
    Ollama,
    Anthropic,
    Responses,
    GeminiOpenai,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "provider_api_key_mode_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ProviderApiKeyMode {
    #[default]
    Queue,
    Random,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "action_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Action {
    #[default]
    Deny,
    Allow,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "rule_scope_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuleScope {
    #[default]
    Provider,
    Model,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "field_placement_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FieldPlacement {
    #[default]
    Body,
    Header,
    Query,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "field_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FieldType {
    #[default]
    Unset,
    String,
    Integer,
    Number,
    Boolean,
    JsonString,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default, Encode, Decode,
)]
#[db_enum(pg_type = "request_patch_placement_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestPatchPlacement {
    Header,
    Query,
    #[default]
    Body,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default, Encode, Decode,
)]
#[db_enum(pg_type = "request_patch_operation_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestPatchOperation {
    #[default]
    Set,
    Remove,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "request_status_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
/// Request-level aggregate status for `request_log`.
pub enum RequestStatus {
    #[default]
    Pending,
    Success,
    Error,
    Cancelled,
}
