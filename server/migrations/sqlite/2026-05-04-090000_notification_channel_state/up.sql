CREATE TABLE notification_channel_state (
    id BIGINT PRIMARY KEY NOT NULL,
    alert_id BIGINT NOT NULL,
    alert_fingerprint TEXT NOT NULL,
    channel_id BIGINT NOT NULL,
    event_type TEXT NOT NULL CHECK (event_type IN ('alert_fired', 'alert_recovered')),
    occurrence_key BIGINT NOT NULL,
    last_notification_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    UNIQUE(alert_id, channel_id, event_type),
    FOREIGN KEY (alert_id) REFERENCES alert_event(id),
    FOREIGN KEY (channel_id) REFERENCES notification_channel(id)
);

CREATE INDEX idx_notification_channel_state_alert
    ON notification_channel_state (alert_fingerprint, event_type, occurrence_key);

CREATE INDEX idx_notification_channel_state_channel
    ON notification_channel_state (channel_id, event_type, last_notification_at);

INSERT INTO notification_channel_state (
    id,
    alert_id,
    alert_fingerprint,
    channel_id,
    event_type,
    occurrence_key,
    last_notification_at,
    created_at,
    updated_at
)
SELECT
    ROW_NUMBER() OVER (ORDER BY alert_event.id, notification_channel.id),
    alert_event.id,
    alert_event.fingerprint,
    notification_channel.id,
    'alert_fired',
    alert_event.reopened_count,
    alert_event.last_notification_at,
    alert_event.last_notification_at,
    alert_event.last_notification_at
FROM alert_event
JOIN notification_channel
    ON notification_channel.channel_type = 'webhook'
    AND notification_channel.is_enabled = TRUE
    AND notification_channel.deleted_at IS NULL
WHERE alert_event.status = 'active'
    AND alert_event.last_notification_at IS NOT NULL;
