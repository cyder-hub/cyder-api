ALTER TABLE system_api_key
ADD COLUMN ref TEXT;

ALTER TABLE request_log
ADD COLUMN channel TEXT;

ALTER TABLE request_log
ADD COLUMN external_id TEXT;

CREATE INDEX IF NOT EXISTS idx_request_log_channel ON request_log (channel);
CREATE INDEX IF NOT EXISTS idx_request_log_external_id ON request_log (external_id);
