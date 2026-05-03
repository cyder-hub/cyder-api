CREATE TABLE alert_event (
    id BIGINT PRIMARY KEY,
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
