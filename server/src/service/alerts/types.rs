use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
}

impl AlertSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

impl TryFrom<&str> for AlertSeverity {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("invalid alert severity '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertStatus {
    Active,
    Resolved,
}

impl AlertStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Resolved => "resolved",
        }
    }
}

impl TryFrom<&str> for AlertStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "active" => Ok(Self::Active),
            "resolved" => Ok(Self::Resolved),
            _ => Err(format!("invalid alert status '{value}'")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertScopeType {
    Global,
    Provider,
    Model,
    ApiKey,
    ProviderApiKey,
    ProviderModel,
    System,
}

impl AlertScopeType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Provider => "provider",
            Self::Model => "model",
            Self::ApiKey => "api_key",
            Self::ProviderApiKey => "provider_api_key",
            Self::ProviderModel => "provider_model",
            Self::System => "system",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertFireInput {
    pub fingerprint: String,
    pub rule_key: String,
    pub severity: AlertSeverity,
    pub scope_type: AlertScopeType,
    pub scope_id: String,
    pub title: String,
    pub summary: String,
    pub details_json: String,
    pub metrics_snapshot_json: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertAcknowledgeInput {
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertSuppressInput {
    pub suppressed_until: i64,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertEvaluationTickResult {
    pub evaluated: u64,
    pub fired: u64,
    pub resolved: u64,
    pub failed: u64,
}
