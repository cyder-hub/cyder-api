DELETE FROM notification_delivery
WHERE event_type = 'test';

UPDATE notification_delivery
SET status = 'failed'
WHERE status IN ('in_progress', 'skipped');

ALTER TABLE notification_delivery
    DROP CONSTRAINT IF EXISTS notification_delivery_event_type_check;

ALTER TABLE notification_delivery
    ADD CONSTRAINT notification_delivery_event_type_check
    CHECK (event_type IN ('alert_fired', 'alert_recovered'));

ALTER TABLE notification_delivery
    DROP CONSTRAINT IF EXISTS notification_delivery_status_check;

ALTER TABLE notification_delivery
    ADD CONSTRAINT notification_delivery_status_check
    CHECK (status IN ('pending', 'retry_scheduled', 'succeeded', 'failed'));

ALTER TABLE notification_delivery
    DROP COLUMN delivered_at;

ALTER TABLE notification_channel
    DROP COLUMN cooldown_seconds;

ALTER TABLE notification_channel
    DROP COLUMN headers_json;
