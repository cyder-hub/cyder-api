use bincode::{Decode, Encode};
use diesel_derive_enum::DbEnum;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, DbEnum, Default, Encode, Decode,
)]
#[db_enum(pg_type = "upstream_profile_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpstreamProfileType {
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

impl UpstreamProfileType {
    pub const ALL: [Self; 8] = [
        Self::Openai,
        Self::Gemini,
        Self::Vertex,
        Self::VertexOpenai,
        Self::Ollama,
        Self::Anthropic,
        Self::Responses,
        Self::GeminiOpenai,
    ];
}

/// Public wire protocol accepted by the gateway.
///
/// This type intentionally excludes provider identities and upstream-only
/// protocols so invalid downstream states cannot be represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "downstream_protocol_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DownstreamProtocol {
    #[default]
    Openai,
    Responses,
    Anthropic,
    Gemini,
}

impl DownstreamProtocol {
    pub const ALL: [Self; 4] = [Self::Openai, Self::Responses, Self::Anthropic, Self::Gemini];
}

/// Wire protocol used for the selected upstream provider.
///
/// Provider-specific dialects and authentication behavior belong to
/// `UpstreamRuntimeProfile`, not this protocol identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "upstream_protocol_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UpstreamProtocol {
    #[default]
    Openai,
    Responses,
    Anthropic,
    Gemini,
    Ollama,
}

impl UpstreamProtocol {
    pub const ALL: [Self; 5] = [
        Self::Openai,
        Self::Responses,
        Self::Anthropic,
        Self::Gemini,
        Self::Ollama,
    ];
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
