ALTER TABLE request_log
DROP COLUMN external_id;

ALTER TABLE request_log
DROP COLUMN channel;

ALTER TABLE system_api_key
DROP COLUMN ref;

DROP INDEX IF EXISTS idx_request_log_channel;
DROP INDEX IF EXISTS idx_request_log_external_id;
