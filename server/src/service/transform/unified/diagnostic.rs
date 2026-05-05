use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedTransformDiagnosticKind {
    FatalTransformError,
    LossyTransform,
    CapabilityDowngrade,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedTransformDiagnosticAction {
    Send,
    Drop,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UnifiedTransformDiagnosticLossLevel {
    Lossless,
    LossyMinor,
    LossyMajor,
    Reject,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnifiedTransformDiagnostic {
    #[serde(rename = "type")]
    pub type_: String,
    pub diagnostic_kind: UnifiedTransformDiagnosticKind,
    pub provider: String,
    pub target_provider: String,
    pub source: String,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    pub loss_level: UnifiedTransformDiagnosticLossLevel,
    pub action: UnifiedTransformDiagnosticAction,
    pub semantic_unit: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_data_summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery_hint: Option<String>,
}
