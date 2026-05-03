ALTER TABLE notification_channel
    ADD COLUMN headers_json TEXT NULL;

ALTER TABLE notification_channel
    ADD COLUMN cooldown_seconds BIGINT NOT NULL DEFAULT 900;

CREATE TABLE notification_delivery_new (
    id BIGINT PRIMARY KEY NOT NULL,
    channel_id BIGINT NOT NULL,
    alert_id BIGINT NOT NULL,
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
    updated_at BIGINT NOT NULL,
    FOREIGN KEY (channel_id) REFERENCES notification_channel(id),
    FOREIGN KEY (alert_id) REFERENCES alert_event(id)
);

INSERT INTO notification_delivery_new (
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
    delivered_at,
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
    status,
    payload_json,
    attempt_count,
    next_attempt_at,
    last_attempt_at,
    CASE WHEN status = 'succeeded' THEN updated_at ELSE NULL END,
    last_status_code,
    last_error,
    created_at,
    updated_at
FROM notification_delivery;

DROP TABLE notification_delivery;
ALTER TABLE notification_delivery_new RENAME TO notification_delivery;

CREATE INDEX idx_notification_delivery_due
    ON notification_delivery (status, next_attempt_at);

CREATE INDEX idx_notification_delivery_alert
    ON notification_delivery (alert_fingerprint, event_type, created_at);
