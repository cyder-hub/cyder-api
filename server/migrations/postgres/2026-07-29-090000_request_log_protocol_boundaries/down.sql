-- Reverting also discards request-log history because the legacy enum can
-- represent invalid downstream states that have no lossless reverse mapping.
TRUNCATE TABLE metric_ingested_request_log;
TRUNCATE TABLE request_log;

DROP INDEX IF EXISTS idx_request_log_downstream_protocol_received_at;

ALTER TABLE request_log
    DROP COLUMN downstream_protocol,
    DROP COLUMN upstream_protocol;

DROP TYPE downstream_protocol_enum;
DROP TYPE upstream_protocol_enum;

CREATE TYPE llm_api_type_enum AS ENUM (
    'OPENAI',
    'GEMINI',
    'OLLAMA',
    'ANTHROPIC',
    'RESPONSES',
    'GEMINI_OPENAI'
);

ALTER TABLE request_log
    ADD COLUMN user_api_type llm_api_type_enum NOT NULL,
    ADD COLUMN llm_api_type llm_api_type_enum NULL;
