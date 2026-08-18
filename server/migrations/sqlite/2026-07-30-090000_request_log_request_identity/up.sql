-- R3.2 intentionally discards request-log history so every persisted request
-- identity follows one canonical UUID v4 contract.
DELETE FROM metric_ingested_request_log;
DELETE FROM request_log;

ALTER TABLE request_log
    ADD COLUMN request_id TEXT NOT NULL
    CONSTRAINT chk_request_log_request_id CHECK (
        length(request_id) = 36
        AND request_id = lower(request_id)
        AND substr(request_id, 9, 1) = '-'
        AND substr(request_id, 14, 1) = '-'
        AND substr(request_id, 15, 1) = '4'
        AND substr(request_id, 19, 1) = '-'
        AND substr(request_id, 20, 1) GLOB '[89ab]'
        AND substr(request_id, 24, 1) = '-'
        AND length(replace(request_id, '-', '')) = 32
        AND replace(request_id, '-', '') NOT GLOB '*[^0-9a-f]*'
    );

ALTER TABLE request_log
    ADD COLUMN client_request_id TEXT NULL
    CONSTRAINT chk_request_log_client_request_id CHECK (
        client_request_id IS NULL
        OR (
            length(client_request_id) BETWEEN 1 AND 64
            AND client_request_id NOT GLOB '*[^A-Za-z0-9._:-]*'
        )
    );

CREATE UNIQUE INDEX idx_request_log_request_id
    ON request_log (request_id);
CREATE INDEX idx_request_log_client_request_id
    ON request_log (client_request_id);
