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

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default, Encode, Decode,
)]
#[db_enum(pg_type = "request_attempt_status_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RequestAttemptStatus {
    #[default]
    Skipped,
    Success,
    Error,
    Cancelled,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default, Encode, Decode,
)]
#[db_enum(pg_type = "scheduler_action_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SchedulerAction {
    ReturnSuccess,
    #[default]
    FailFast,
    RetrySameCandidate,
    FallbackNextCandidate,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Serialize,
    Deserialize,
    DbEnum,
    Default,
    strum_macros::Display,
    Encode,
    Decode,
)]
#[db_enum(pg_type = "storage_type_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum StorageType {
    #[default]
    FileSystem,
    S3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "request_replay_kind_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum RequestReplayKind {
    #[default]
    AttemptUpstream,
    GatewayRequest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "request_replay_mode_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum RequestReplayMode {
    #[default]
    DryRun,
    Live,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "request_replay_semantic_basis_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum RequestReplaySemanticBasis {
    #[default]
    HistoricalAttemptSnapshot,
    HistoricalRequestSnapshotWithCurrentConfig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, DbEnum, Default)]
#[db_enum(pg_type = "request_replay_status_enum")]
#[db_enum(value_style = "SCREAMING_SNAKE_CASE")]
#[serde(rename_all = "snake_case")]
pub enum RequestReplayStatus {
    #[default]
    Pending,
    Running,
    Success,
    Error,
    Cancelled,
    Rejected,
}

#[cfg(test)]
mod tests {
    use super::{
        RequestAttemptStatus, RequestReplayKind, RequestReplayMode, RequestReplaySemanticBasis,
        RequestReplayStatus, SchedulerAction,
    };

    #[test]
    fn request_attempt_status_serializes_to_stable_wire_values() {
        assert_eq!(
            serde_json::to_string(&RequestAttemptStatus::Skipped).unwrap(),
            "\"SKIPPED\""
        );
        assert_eq!(
            serde_json::to_string(&RequestAttemptStatus::Success).unwrap(),
            "\"SUCCESS\""
        );
        assert_eq!(
            serde_json::to_string(&RequestAttemptStatus::Error).unwrap(),
            "\"ERROR\""
        );
        assert_eq!(
            serde_json::to_string(&RequestAttemptStatus::Cancelled).unwrap(),
            "\"CANCELLED\""
        );
    }

    #[test]
    fn scheduler_action_serializes_to_stable_wire_values() {
        assert_eq!(
            serde_json::to_string(&SchedulerAction::ReturnSuccess).unwrap(),
            "\"RETURN_SUCCESS\""
        );
        assert_eq!(
            serde_json::to_string(&SchedulerAction::FailFast).unwrap(),
            "\"FAIL_FAST\""
        );
        assert_eq!(
            serde_json::to_string(&SchedulerAction::RetrySameCandidate).unwrap(),
            "\"RETRY_SAME_CANDIDATE\""
        );
        assert_eq!(
            serde_json::to_string(&SchedulerAction::FallbackNextCandidate).unwrap(),
            "\"FALLBACK_NEXT_CANDIDATE\""
        );
    }

    #[test]
    fn request_replay_enums_serialize_to_api_wire_values() {
        assert_eq!(
            serde_json::to_string(&RequestReplayKind::AttemptUpstream).unwrap(),
            "\"attempt_upstream\""
        );
        assert_eq!(
            serde_json::to_string(&RequestReplayKind::GatewayRequest).unwrap(),
            "\"gateway_request\""
        );
        assert_eq!(
            serde_json::to_string(&RequestReplayMode::DryRun).unwrap(),
            "\"dry_run\""
        );
        assert_eq!(
            serde_json::to_string(&RequestReplaySemanticBasis::HistoricalAttemptSnapshot).unwrap(),
            "\"historical_attempt_snapshot\""
        );
        assert_eq!(
            serde_json::to_string(&RequestReplayStatus::Rejected).unwrap(),
            "\"rejected\""
        );
    }
}
