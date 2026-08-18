-- R3.2 intentionally discards request-log history so every persisted request
-- identity follows one canonical UUID v4 contract.
TRUNCATE TABLE metric_ingested_request_log;
TRUNCATE TABLE request_log;

ALTER TABLE request_log
    ADD COLUMN request_id TEXT NOT NULL,
    ADD COLUMN client_request_id TEXT NULL,
    ADD CONSTRAINT chk_request_log_request_id CHECK (
        request_id ~ '^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$'
    ),
    ADD CONSTRAINT chk_request_log_client_request_id CHECK (
        client_request_id IS NULL
        OR client_request_id ~ '^[A-Za-z0-9._:-]{1,64}$'
    );

CREATE UNIQUE INDEX idx_request_log_request_id
    ON request_log (request_id);
CREATE INDEX idx_request_log_client_request_id
    ON request_log (client_request_id);
