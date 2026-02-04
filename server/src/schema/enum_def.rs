use bincode::{Decode, Encode};
use diesel_derive_enum::DbEnum;
use serde::{Serialize, Deserialize};

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
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, Encode, Decode)]
#[db_enum(pg_type = "request_status_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestStatus {
    #[default]
    Pending,
    Success,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, DbEnum, Default, strum_macros::Display, Encode, Decode)]
#[db_enum(pg_type = "storage_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageType {
    #[default]
    FileSystem,
    S3,
}
