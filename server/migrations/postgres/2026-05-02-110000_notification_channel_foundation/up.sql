CREATE TABLE notification_channel (
    id BIGINT PRIMARY KEY,
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

CREATE INDEX idx_notification_channel_enabled
    ON notification_channel (is_enabled, deleted_at);
