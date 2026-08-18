-- R3.1 intentionally discards request-log history. The gateway has no
-- compatibility obligation for mixed-direction protocol values.
TRUNCATE TABLE metric_ingested_request_log;
TRUNCATE TABLE request_log;

ALTER TABLE request_log
    DROP COLUMN user_api_type,
    DROP COLUMN llm_api_type;

DROP TYPE llm_api_type_enum;

CREATE TYPE downstream_protocol_enum AS ENUM (
    'OPENAI',
    'RESPONSES',
    'ANTHROPIC',
    'GEMINI'
);

CREATE TYPE upstream_protocol_enum AS ENUM (
    'OPENAI',
    'RESPONSES',
    'ANTHROPIC',
    'GEMINI',
    'OLLAMA'
);

ALTER TABLE request_log
    ADD COLUMN downstream_protocol downstream_protocol_enum NOT NULL,
    ADD COLUMN upstream_protocol upstream_protocol_enum NULL;

CREATE INDEX idx_request_log_downstream_protocol_received_at
    ON request_log (downstream_protocol, request_received_at DESC);
