use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannelType {
    Webhook,
}

impl NotificationChannelType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Webhook => "webhook",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationEventType {
    AlertFired,
    AlertRecovered,
    Test,
}

impl NotificationEventType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AlertFired => "alert_fired",
            Self::AlertRecovered => "alert_recovered",
            Self::Test => "test",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationChannelResponse {
    pub id: i64,
    pub channel_key: String,
    pub channel_type: String,
    pub name: String,
    pub endpoint_url: String,
    pub signing_secret_redacted: Option<String>,
    pub headers_json: Option<String>,
    pub cooldown_seconds: i64,
    pub is_enabled: bool,
    pub last_test_at: Option<i64>,
    pub last_test_success: Option<bool>,
    pub last_test_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateNotificationChannelInput {
    pub channel_key: String,
    pub name: String,
    pub endpoint_url: String,
    pub signing_secret: Option<String>,
    pub headers_json: Option<String>,
    pub cooldown_seconds: Option<i64>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateNotificationChannelInput {
    pub name: Option<String>,
    pub endpoint_url: Option<String>,
    pub signing_secret: Option<String>,
    pub clear_signing_secret: bool,
    pub headers_json: Option<String>,
    pub clear_headers: bool,
    pub cooldown_seconds: Option<i64>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationWebhookTestResult {
    pub success: bool,
    pub status: Option<u16>,
    pub error: Option<String>,
    pub response_body_preview: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryStatus {
    Pending,
    InProgress,
    RetryScheduled,
    Succeeded,
    Failed,
    Skipped,
}

impl NotificationDeliveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::RetryScheduled => "retry_scheduled",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationDeliveryResponse {
    pub id: i64,
    pub channel_id: i64,
    pub alert_id: i64,
    pub alert_fingerprint: String,
    pub event_type: String,
    pub status: String,
    pub payload_json: String,
    pub attempt_count: i32,
    pub next_attempt_at: i64,
    pub last_attempt_at: Option<i64>,
    pub delivered_at: Option<i64>,
    pub last_status_code: Option<i32>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationDeliveryListInput {
    pub alert_id: Option<i64>,
    pub channel_id: Option<i64>,
    pub status: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationDeliveryListResponse {
    pub items: Vec<NotificationDeliveryResponse>,
    pub next_offset: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationWorkerTickResult {
    pub processed: u64,
    pub succeeded: u64,
    pub retry_scheduled: u64,
    pub failed: u64,
    pub skipped: u64,
}
