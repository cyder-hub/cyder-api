use crate::config::CONFIG;

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

pub(crate) fn replay_preview_confirmation_ttl_ms() -> i64 {
    seconds_to_millis(CONFIG.diagnostics.replay_preview_confirmation_ttl_seconds)
}

pub(crate) fn replay_preview_confirmation_clock_skew_ms() -> i64 {
    seconds_to_millis(
        CONFIG
            .diagnostics
            .replay_preview_confirmation_clock_skew_seconds,
    )
}

pub(crate) fn response_capture_max_bytes() -> usize {
    CONFIG.diagnostics.response_capture_max_bytes.max(1)
}

pub(crate) fn raw_bundle_download_enabled() -> bool {
    CONFIG.diagnostics.raw_bundle_download_enabled
}

#[allow(dead_code)]
pub(crate) fn retention_enabled() -> bool {
    CONFIG.diagnostics.retention.enabled
}

pub(crate) fn request_log_bundle_retention_days() -> u64 {
    CONFIG
        .diagnostics
        .retention
        .request_log_bundle_retention_days
}

pub(crate) fn replay_artifact_retention_days() -> u64 {
    CONFIG.diagnostics.retention.replay_artifact_retention_days
}

pub(crate) fn retention_delete_batch_size() -> usize {
    CONFIG.diagnostics.retention.delete_batch_size.max(1)
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
        assert_eq!(response_capture_max_bytes(), 4 * 1024 * 1024);
        assert_eq!(replay_preview_confirmation_ttl_ms(), 900_000);
        assert_eq!(replay_preview_confirmation_clock_skew_ms(), 60_000);
        assert!(raw_bundle_download_enabled());
        assert!(!retention_enabled());
    }
}
