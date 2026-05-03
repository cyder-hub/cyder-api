CREATE TABLE notification_delivery (
    id BIGINT PRIMARY KEY,
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

CREATE INDEX idx_notification_delivery_due
    ON notification_delivery (status, next_attempt_at);

CREATE INDEX idx_notification_delivery_alert
    ON notification_delivery (alert_fingerprint, event_type, created_at);
