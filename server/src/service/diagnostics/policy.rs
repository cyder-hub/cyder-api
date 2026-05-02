use tokio::sync::RwLock;

use crate::config::{DiagnosticsConfig, DiagnosticsRetentionConfig};

#[cfg(test)]
use crate::config::{
    DEFAULT_DIAGNOSTICS_RAW_BUNDLE_DOWNLOAD_ENABLED,
    DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_CLOCK_SKEW_SECONDS,
    DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_TTL_SECONDS,
    DEFAULT_DIAGNOSTICS_RESPONSE_CAPTURE_MAX_BYTES,
};

const REDACTED_HEADER_NAMES: &[&str] = &["authorization", "x-api-key", "x-goog-api-key", "cookie"];
const DISALLOWED_REPLAY_REQUEST_HEADER_NAMES: &[&str] = &[
    "authorization",
    "x-api-key",
    "x-goog-api-key",
    "cookie",
    "host",
    "content-length",
    "accept-encoding",
    "transfer-encoding",
];
const STRIPPED_PREVIEW_REQUEST_HEADER_NAMES: &[&str] = &[
    "host",
    "content-length",
    "accept-encoding",
    "transfer-encoding",
];
const STRIPPED_RESPONSE_HEADER_NAMES: &[&str] =
    &["set-cookie", "content-length", "transfer-encoding"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiagnosticsPolicy {
    replay_preview_confirmation_ttl_seconds: u64,
    replay_preview_confirmation_clock_skew_seconds: u64,
    response_capture_max_bytes: usize,
    raw_bundle_download_enabled: bool,
    retention: DiagnosticsRetentionConfig,
}

impl DiagnosticsPolicy {
    pub fn from_config(config: &DiagnosticsConfig) -> Self {
        Self {
            replay_preview_confirmation_ttl_seconds: config.replay_preview_confirmation_ttl_seconds,
            replay_preview_confirmation_clock_skew_seconds: config
                .replay_preview_confirmation_clock_skew_seconds,
            response_capture_max_bytes: config.response_capture_max_bytes,
            raw_bundle_download_enabled: config.raw_bundle_download_enabled,
            retention: config.retention.clone(),
        }
    }

    pub fn replay_preview_confirmation_ttl_ms(&self) -> i64 {
        seconds_to_millis(self.replay_preview_confirmation_ttl_seconds)
    }

    pub fn replay_preview_confirmation_clock_skew_ms(&self) -> i64 {
        seconds_to_millis(self.replay_preview_confirmation_clock_skew_seconds)
    }

    pub fn response_capture_max_bytes(&self) -> usize {
        self.response_capture_max_bytes.max(1)
    }

    pub fn raw_bundle_download_enabled(&self) -> bool {
        self.raw_bundle_download_enabled
    }

    pub fn retention_enabled(&self) -> bool {
        self.retention.enabled
    }

    pub fn request_log_bundle_retention_days(&self) -> u64 {
        self.retention.request_log_bundle_retention_days
    }

    pub fn replay_artifact_retention_days(&self) -> u64 {
        self.retention.replay_artifact_retention_days
    }

    pub fn retention_delete_batch_size(&self) -> usize {
        self.retention.delete_batch_size.max(1)
    }
}

impl Default for DiagnosticsPolicy {
    fn default() -> Self {
        Self::from_config(&DiagnosticsConfig::default())
    }
}

#[derive(Debug)]
pub struct DiagnosticsPolicyManager {
    current: RwLock<DiagnosticsPolicy>,
}

impl DiagnosticsPolicyManager {
    pub fn new(policy: DiagnosticsPolicy) -> Self {
        Self {
            current: RwLock::new(policy),
        }
    }

    pub async fn current(&self) -> DiagnosticsPolicy {
        self.current.read().await.clone()
    }

    pub async fn update(&self, policy: DiagnosticsPolicy) {
        *self.current.write().await = policy;
    }
}

pub(crate) fn redacted_header_names() -> &'static [&'static str] {
    REDACTED_HEADER_NAMES
}

pub(crate) fn disallowed_replay_request_header_names() -> &'static [&'static str] {
    DISALLOWED_REPLAY_REQUEST_HEADER_NAMES
}

pub(crate) fn stripped_preview_request_header_names() -> &'static [&'static str] {
    STRIPPED_PREVIEW_REQUEST_HEADER_NAMES
}

pub(crate) fn stripped_response_header_names() -> &'static [&'static str] {
    STRIPPED_RESPONSE_HEADER_NAMES
}

fn seconds_to_millis(seconds: u64) -> i64 {
    let millis = seconds.saturating_mul(1000);
    i64::try_from(millis).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_policy_defaults_match_existing_replay_behavior() {
        assert_eq!(
            DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_TTL_SECONDS,
            900
        );
        assert_eq!(
            DEFAULT_DIAGNOSTICS_REPLAY_PREVIEW_CONFIRMATION_CLOCK_SKEW_SECONDS,
            60
        );
        assert_eq!(
            DEFAULT_DIAGNOSTICS_RESPONSE_CAPTURE_MAX_BYTES,
            4 * 1024 * 1024
        );
        assert!(DEFAULT_DIAGNOSTICS_RAW_BUNDLE_DOWNLOAD_ENABLED);
    }

    #[test]
    fn diagnostics_policy_keeps_header_sets_centralized() {
        assert!(redacted_header_names().contains(&"authorization"));
        assert!(redacted_header_names().contains(&"x-goog-api-key"));
        assert!(disallowed_replay_request_header_names().contains(&"authorization"));
        assert!(disallowed_replay_request_header_names().contains(&"host"));
        assert!(stripped_preview_request_header_names().contains(&"content-length"));
        assert!(stripped_response_header_names().contains(&"set-cookie"));
    }

    #[test]
    fn diagnostics_policy_seconds_to_millis_saturates() {
        assert_eq!(seconds_to_millis(900), 900_000);
        assert_eq!(seconds_to_millis(u64::MAX), i64::MAX);
    }

    #[test]
    fn diagnostics_policy_reads_runtime_defaults() {
        let policy = DiagnosticsPolicy::default();

        assert_eq!(policy.response_capture_max_bytes(), 4 * 1024 * 1024);
        assert_eq!(policy.replay_preview_confirmation_ttl_ms(), 900_000);
        assert_eq!(policy.replay_preview_confirmation_clock_skew_ms(), 60_000);
        assert!(policy.raw_bundle_download_enabled());
        assert!(!policy.retention_enabled());
    }

    #[tokio::test]
    async fn diagnostics_policy_manager_returns_updated_policy() {
        let manager = DiagnosticsPolicyManager::new(DiagnosticsPolicy::default());
        let mut config = DiagnosticsConfig::default();
        config.response_capture_max_bytes = 128;

        manager
            .update(DiagnosticsPolicy::from_config(&config))
            .await;

        assert_eq!(manager.current().await.response_capture_max_bytes(), 128);
    }
}
