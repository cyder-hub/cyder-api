-- Reverting also discards request-log history; no mixed identity format is
-- retained across the schema boundary.
DELETE FROM metric_ingested_request_log;
DELETE FROM request_log;

DROP INDEX IF EXISTS idx_request_log_client_request_id;
DROP INDEX IF EXISTS idx_request_log_request_id;

ALTER TABLE request_log DROP COLUMN client_request_id;
ALTER TABLE request_log DROP COLUMN request_id;
