-- This file should undo anything in `up.sql`
PRAGMA foreign_keys=off;

-- Create a new table with the schema from *before* the up migration
CREATE TABLE request_log_old_schema (
    id BIGINT PRIMARY KEY NOT NULL,
    system_api_key_id BIGINT,
    provider_id BIGINT,
    model_id BIGINT,
    provider_api_key_id BIGINT,
    model_name TEXT,
    real_model_name TEXT,
    request_received_at BIGINT NOT NULL,
    llm_request_sent_at BIGINT,
    llm_response_first_chunk_at BIGINT,
    llm_response_completed_at BIGINT,
    response_sent_to_client_at BIGINT, -- re-add
    client_ip TEXT,
    llm_request_uri TEXT,
    llm_response_status INTEGER,
    status TEXT DEFAULT 'PENDING',
    is_stream BOOLEAN NOT NULL DEFAULT false,
    calculated_cost BIGINT DEFAULT 0,
    cost_currency TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    prompt_tokens INTEGER DEFAULT 0, -- rename back from input_tokens
    completion_tokens INTEGER DEFAULT 0, -- rename back from output_tokens
    reasoning_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    storage_type TEXT,
    user_request_body TEXT,
    llm_request_body TEXT,
    llm_response_body TEXT,
    user_response_body TEXT,
    channel TEXT, -- re-add
    external_id TEXT, -- re-add

    -- Foreign Key Constraints from the previous state
    CONSTRAINT fk_request_log_system_api_key_id FOREIGN KEY (system_api_key_id) REFERENCES system_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id FOREIGN KEY (provider_id) REFERENCES provider(id) ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id FOREIGN KEY (model_id) REFERENCES model(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,

    -- Data Integrity Constraints from the previous state
    CONSTRAINT chk_request_log_tokens_non_negative CHECK ( prompt_tokens >= 0 AND completion_tokens >= 0 AND reasoning_tokens >= 0 AND total_tokens >= 0 ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (updated_at >= created_at),
    CONSTRAINT chk_request_log_status CHECK (status IN ('PENDING', 'SUCCESS', 'ERROR'))
);

-- Copy data from the migrated table back to the old schema, mapping column names
INSERT INTO request_log_old_schema (
    id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
    real_model_name, request_received_at, llm_request_sent_at, llm_response_first_chunk_at,
    llm_response_completed_at, client_ip, llm_request_uri, llm_response_status,
    status, is_stream, calculated_cost, cost_currency, created_at, updated_at,
    prompt_tokens, completion_tokens, reasoning_tokens, total_tokens, storage_type,
    user_request_body, llm_request_body, llm_response_body, user_response_body
)
SELECT
    id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
    real_model_name, request_received_at, llm_request_sent_at, llm_response_first_chunk_at,
    llm_response_completed_at, client_ip, llm_request_uri, llm_response_status,
    status, is_stream, calculated_cost, cost_currency, created_at, updated_at,
    input_tokens, output_tokens, reasoning_tokens, total_tokens, storage_type,
    user_request_body, llm_request_body, llm_response_body, user_response_body
FROM request_log;

-- Drop the migrated table and rename the old schema table back
DROP TABLE request_log;
ALTER TABLE request_log_old_schema RENAME TO request_log;

-- Recreate indexes that were dropped in the 'up' migration
CREATE INDEX idx_request_log_channel ON request_log (channel);
CREATE INDEX idx_request_log_external_id ON request_log (external_id);

PRAGMA foreign_keys=on;

-- Revert changes to other tables
ALTER TABLE system_api_key ADD COLUMN ref TEXT;
