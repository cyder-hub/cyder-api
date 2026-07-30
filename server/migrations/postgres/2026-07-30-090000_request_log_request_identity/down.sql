-- Reverting also discards request-log history; no mixed identity format is
-- retained across the schema boundary.
TRUNCATE TABLE metric_ingested_request_log;
TRUNCATE TABLE request_log;

DROP INDEX IF EXISTS idx_request_log_client_request_id;
DROP INDEX IF EXISTS idx_request_log_request_id;

ALTER TABLE request_log
    DROP CONSTRAINT IF EXISTS chk_request_log_client_request_id,
    DROP CONSTRAINT IF EXISTS chk_request_log_request_id,
    DROP COLUMN client_request_id,
    DROP COLUMN request_id;
