CREATE TABLE notification_delivery_old (
    id BIGINT PRIMARY KEY NOT NULL,
    channel_id BIGINT NOT NULL,
    alert_id BIGINT NOT NULL,
    alert_fingerprint TEXT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('alert_fired', 'alert_recovered')),
    status TEXT NOT NULL CHECK (status IN ('pending', 'retry_scheduled', 'succeeded', 'failed')),
    payload_json TEXT NOT NULL,
    attempt_count INTEGER NOT NULL,
    next_attempt_at BIGINT NOT NULL,
    last_attempt_at BIGINT NULL,
    last_status_code INTEGER NULL,
    last_error TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    FOREIGN KEY (channel_id) REFERENCES notification_channel(id),
    FOREIGN KEY (alert_id) REFERENCES alert_event(id)
);

INSERT INTO notification_delivery_old (
    id,
    channel_id,
    alert_id,
    alert_fingerprint,
    event_type,
    status,
    payload_json,
    attempt_count,
    next_attempt_at,
    last_attempt_at,
    last_status_code,
    last_error,
    created_at,
    updated_at
)
SELECT
    id,
    channel_id,
    alert_id,
    alert_fingerprint,
    event_type,
    CASE WHEN status IN ('in_progress', 'skipped') THEN 'failed' ELSE status END,
    payload_json,
    attempt_count,
    next_attempt_at,
    last_attempt_at,
    last_status_code,
    last_error,
    created_at,
    updated_at
FROM notification_delivery
WHERE event_type IN ('alert_fired', 'alert_recovered');

DROP TABLE notification_delivery;
ALTER TABLE notification_delivery_old RENAME TO notification_delivery;

CREATE INDEX idx_notification_delivery_due
    ON notification_delivery (status, next_attempt_at);

CREATE INDEX idx_notification_delivery_alert
    ON notification_delivery (alert_fingerprint, event_type, created_at);

CREATE TABLE notification_channel_old (
    id BIGINT PRIMARY KEY NOT NULL,
    channel_key TEXT NOT NULL UNIQUE,
    channel_type TEXT NOT NULL CHECK (channel_type IN ('webhook')),
    name TEXT NOT NULL,
    endpoint_url TEXT NOT NULL,
    signing_secret TEXT NULL,
    is_enabled BOOLEAN NOT NULL,
    last_test_at BIGINT NULL,
    last_test_success BOOLEAN NULL,
    last_test_error TEXT NULL,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

INSERT INTO notification_channel_old (
    id,
    channel_key,
    channel_type,
    name,
    endpoint_url,
    signing_secret,
    is_enabled,
    last_test_at,
    last_test_success,
    last_test_error,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    id,
    channel_key,
    channel_type,
    name,
    endpoint_url,
    signing_secret,
    is_enabled,
    last_test_at,
    last_test_success,
    last_test_error,
    deleted_at,
    created_at,
    updated_at
FROM notification_channel;

DROP TABLE notification_channel;
ALTER TABLE notification_channel_old RENAME TO notification_channel;

CREATE INDEX idx_notification_channel_enabled
    ON notification_channel (is_enabled, deleted_at);
