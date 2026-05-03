ALTER TABLE notification_channel
    ADD COLUMN headers_json TEXT NULL;

ALTER TABLE notification_channel
    ADD COLUMN cooldown_seconds BIGINT NOT NULL DEFAULT 900;

ALTER TABLE notification_delivery
    ADD COLUMN delivered_at BIGINT NULL;

UPDATE notification_delivery
SET delivered_at = updated_at
WHERE status = 'succeeded'
  AND delivered_at IS NULL;

ALTER TABLE notification_delivery
    DROP CONSTRAINT IF EXISTS notification_delivery_event_type_check;

ALTER TABLE notification_delivery
    ADD CONSTRAINT notification_delivery_event_type_check
    CHECK (event_type IN ('alert_fired', 'alert_recovered', 'test'));

ALTER TABLE notification_delivery
    DROP CONSTRAINT IF EXISTS notification_delivery_status_check;

ALTER TABLE notification_delivery
    ADD CONSTRAINT notification_delivery_status_check
    CHECK (status IN ('pending', 'in_progress', 'retry_scheduled', 'succeeded', 'failed', 'skipped'));
