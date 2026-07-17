-- Reverting restores the retired schema only. Dropped alert and delivery data cannot be recovered.
CREATE TABLE alert_event (
    id BIGINT PRIMARY KEY NOT NULL,
    fingerprint TEXT NOT NULL UNIQUE,
    rule_key TEXT NOT NULL,
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'critical')),
    status TEXT NOT NULL CHECK (status IN ('active', 'resolved')),
    scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'provider', 'model', 'api_key', 'provider_api_key', 'provider_model', 'system')),
    scope_id TEXT NOT NULL,
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    details_json TEXT NOT NULL,
    metrics_snapshot_json TEXT NULL,
    first_seen_at BIGINT NOT NULL,
    last_seen_at BIGINT NOT NULL,
    resolved_at BIGINT NULL,
    acknowledged_at BIGINT NULL,
    acknowledged_note TEXT NULL,
    suppressed_until BIGINT NULL,
    suppressed_reason TEXT NULL,
    occurrence_count BIGINT NOT NULL CHECK (occurrence_count >= 1),
    reopened_count BIGINT NOT NULL CHECK (reopened_count >= 0),
    last_notification_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CHECK (last_seen_at >= first_seen_at)
);

CREATE INDEX idx_alert_event_status_severity
    ON alert_event (status, severity, last_seen_at);
CREATE INDEX idx_alert_event_scope_status
    ON alert_event (scope_type, scope_id, status);

CREATE TABLE alert_rule_state (
    rule_key TEXT NOT NULL,
    scope_type TEXT NOT NULL CHECK (scope_type IN ('global', 'provider', 'model', 'api_key', 'provider_api_key', 'provider_model', 'system')),
    scope_id TEXT NOT NULL,
    last_evaluated_at BIGINT NOT NULL,
    last_fired_at BIGINT NULL,
    last_resolved_at BIGINT NULL,
    cooldown_until BIGINT NULL,
    PRIMARY KEY (rule_key, scope_type, scope_id)
);

CREATE INDEX idx_alert_rule_state_cooldown
    ON alert_rule_state (cooldown_until);

CREATE TABLE notification_channel (
    id BIGINT PRIMARY KEY NOT NULL,
    channel_key TEXT NOT NULL UNIQUE,
    channel_type TEXT NOT NULL CHECK (channel_type IN ('webhook')),
    name TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,
    signing_secret TEXT NULL,
    headers_json TEXT NULL,
    cooldown_seconds BIGINT NOT NULL DEFAULT 900,
    is_enabled BOOLEAN NOT NULL,
    last_test_at BIGINT NULL,
    last_test_success BOOLEAN NULL,
    last_test_error TEXT NULL,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX idx_notification_channel_enabled
    ON notification_channel (is_enabled, deleted_at);

CREATE TABLE notification_delivery (
    id BIGINT PRIMARY KEY NOT NULL,
    channel_id BIGINT NOT NULL REFERENCES notification_channel(id),
    alert_id BIGINT NOT NULL REFERENCES alert_event(id),
    alert_fingerprint TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('alert_fired', 'alert_recovered', 'test')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'in_progress', 'retry_scheduled', 'succeeded', 'failed', 'skipped')),
    payload_json TEXT NOT NULL,
    attempt_count INTEGER NOT NULL,
    next_attempt_at BIGINT NOT NULL,
    last_attempt_at BIGINT NULL,
    delivered_at BIGINT NULL,
    last_status_code INTEGER NULL,
    last_error TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE INDEX idx_notification_delivery_due
    ON notification_delivery (status, next_attempt_at);
CREATE INDEX idx_notification_delivery_alert
    ON notification_delivery (alert_fingerprint, event_type, created_at);

CREATE TABLE notification_channel_state (
    id BIGINT PRIMARY KEY NOT NULL,
    alert_id BIGINT NOT NULL REFERENCES alert_event(id),
    alert_fingerprint TEXT NOT NULL,
    channel_id BIGINT NOT NULL REFERENCES notification_channel(id),
    event_type TEXT NOT NULL CHECK (event_type IN ('alert_fired', 'alert_recovered')),
    occurrence_key BIGINT NOT NULL,
    last_notification_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(alert_id, channel_id, event_type)
);

CREATE INDEX idx_notification_channel_state_alert
    ON notification_channel_state (alert_fingerprint, event_type, occurrence_key);
CREATE INDEX idx_notification_channel_state_channel
    ON notification_channel_state (channel_id, event_type, last_notification_at);
