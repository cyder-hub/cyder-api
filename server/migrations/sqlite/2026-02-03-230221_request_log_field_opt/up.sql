-- Your SQL goes here
PRAGMA foreign_keys=off;

-- Rebuild request_log to apply NOT NULL constraints and other schema changes
CREATE TABLE request_log_new (
    id BIGINT PRIMARY KEY NOT NULL,
    system_api_key_id BIGINT NOT NULL,
    provider_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    provider_api_key_id BIGINT NOT NULL,
    model_name TEXT NOT NULL,
    real_model_name TEXT NOT NULL,
    request_received_at BIGINT NOT NULL,
    llm_request_sent_at BIGINT NOT NULL,
    llm_response_first_chunk_at BIGINT,
    llm_response_completed_at BIGINT,
    client_ip TEXT,
    llm_request_uri TEXT,
    llm_response_status INTEGER,
    status TEXT DEFAULT 'PENDING',
    is_stream BOOLEAN NOT NULL DEFAULT false,
    calculated_cost BIGINT DEFAULT 0,
    cost_currency TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    input_tokens INTEGER DEFAULT 0,
    output_tokens INTEGER DEFAULT 0,
    reasoning_tokens INTEGER DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    storage_type TEXT,
    user_request_body TEXT,
    llm_request_body TEXT,
    llm_response_body TEXT,
    user_response_body TEXT,

    -- new fields for this migration
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    input_image_tokens INTEGER NOT NULL DEFAULT 0,
    output_image_tokens INTEGER NOT NULL DEFAULT 0,
    user_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(user_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES')),
    llm_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(llm_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES')),

    -- Foreign Key Constraints from previous migration
    CONSTRAINT fk_request_log_system_api_key_id FOREIGN KEY (system_api_key_id) REFERENCES system_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id FOREIGN KEY (provider_id) REFERENCES provider(id) ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id FOREIGN KEY (model_id) REFERENCES model(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,

    -- Data Integrity Constraints from previous migration, updated for new columns
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
        input_tokens >= 0 AND output_tokens >= 0 AND
        reasoning_tokens >= 0 AND total_tokens >= 0 AND
        cached_tokens >= 0 AND input_image_tokens >= 0 AND output_image_tokens >= 0
    ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (updated_at >= created_at),
    CONSTRAINT chk_request_log_status CHECK (status IN ('PENDING', 'SUCCESS', 'ERROR'))
);


-- Copy data from the old table to the new one, mapping renamed columns correctly
INSERT INTO request_log_new (
    id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
    real_model_name, request_received_at, llm_request_sent_at, llm_response_first_chunk_at,
    llm_response_completed_at, client_ip, llm_request_uri, llm_response_status, status,
    is_stream, calculated_cost, cost_currency, created_at, updated_at,
    input_tokens, output_tokens, reasoning_tokens, total_tokens, storage_type,
    user_request_body, llm_request_body, llm_response_body, user_response_body
)
SELECT
    id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
    real_model_name, request_received_at, llm_request_sent_at, llm_response_first_chunk_at,
    llm_response_completed_at, client_ip, llm_request_uri, llm_response_status, status,
    is_stream, calculated_cost, cost_currency, created_at, updated_at,
    prompt_tokens, completion_tokens, reasoning_tokens, total_tokens, storage_type,
    user_request_body, llm_request_body, llm_response_body, user_response_body
FROM request_log;

DROP TABLE request_log;
ALTER TABLE request_log_new RENAME TO request_log;

PRAGMA foreign_keys=on;

-- Manually update the CHECK constraint for provider_type in the provider table for SQLite
PRAGMA foreign_keys=off;

CREATE TABLE provider_new
(
    id            BIGINT PRIMARY KEY NOT NULL,
    provider_key  TEXT    NOT NULL,
    name          TEXT    NOT NULL,
    endpoint      TEXT    NOT NULL,
    use_proxy     BOOLEAN NOT NULL DEFAULT false,
    is_enabled    BOOLEAN NOT NULL DEFAULT true,
    deleted_at    BIGINT DEFAULT NULL,
    created_at    BIGINT  NOT NULL,
    updated_at    BIGINT  NOT NULL,
    provider_type TEXT    NOT NULL DEFAULT 'OPENAI',
    provider_api_key_mode  TEXT    NOT NULL DEFAULT 'QUEUE',

    -- only check not empty, other check should be done in the application level
    CONSTRAINT provider_type_check CHECK (provider_type <> ''),
    CONSTRAINT provider_api_key_mode_check CHECK (provider_api_key_mode <> ''),
    CONSTRAINT provider_created_at_updated_at_check CHECK (updated_at >= created_at),
    CONSTRAINT provider_key_not_empty_check CHECK (provider_key <> ''),
    CONSTRAINT provider_name_not_empty_check CHECK (name <> ''),
    CONSTRAINT provider_endpoint_not_empty_check CHECK (endpoint <> '')
);

INSERT INTO provider_new SELECT * FROM provider;

DROP TABLE provider;

ALTER TABLE provider_new RENAME TO provider;

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_key_unique_when_active
    ON provider (provider_key)
    WHERE deleted_at IS NULL AND is_enabled = true;

PRAGMA foreign_keys=on;

ALTER TABLE system_api_key DROP COLUMN ref;
