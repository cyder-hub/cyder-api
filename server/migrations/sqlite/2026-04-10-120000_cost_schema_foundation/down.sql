PRAGMA foreign_keys=off;

DROP INDEX IF EXISTS idx_request_log_received_at;
DROP INDEX IF EXISTS idx_request_log_cost_catalog_version_id;
DROP INDEX IF EXISTS idx_request_log_cost_catalog_id;

CREATE TABLE request_log_old (
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
    input_image_tokens INTEGER NOT NULL DEFAULT 0,
    output_image_tokens INTEGER NOT NULL DEFAULT 0,
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER DEFAULT 0,
    storage_type TEXT,
    user_request_body TEXT,
    llm_request_body TEXT,
    llm_response_body TEXT,
    user_response_body TEXT,
    user_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(user_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES')),
    llm_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(llm_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES')),
    CONSTRAINT fk_request_log_system_api_key_id FOREIGN KEY (system_api_key_id) REFERENCES system_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id FOREIGN KEY (provider_id) REFERENCES provider(id) ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id FOREIGN KEY (model_id) REFERENCES model(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
        (input_tokens IS NULL OR input_tokens >= 0) AND
        (output_tokens IS NULL OR output_tokens >= 0) AND
        (input_image_tokens IS NULL OR input_image_tokens >= 0) AND
        (output_image_tokens IS NULL OR output_image_tokens >= 0) AND
        (cached_tokens IS NULL OR cached_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0) AND
        (total_tokens IS NULL OR total_tokens >= 0)
    ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (updated_at >= created_at),
    CONSTRAINT chk_request_log_status CHECK (status IN ('PENDING', 'SUCCESS', 'ERROR'))
);

INSERT INTO request_log_old (
    id,
    system_api_key_id,
    provider_id,
    model_id,
    provider_api_key_id,
    model_name,
    real_model_name,
    request_received_at,
    llm_request_sent_at,
    llm_response_first_chunk_at,
    llm_response_completed_at,
    client_ip,
    llm_request_uri,
    llm_response_status,
    status,
    is_stream,
    calculated_cost,
    cost_currency,
    created_at,
    updated_at,
    input_tokens,
    output_tokens,
    input_image_tokens,
    output_image_tokens,
    cached_tokens,
    reasoning_tokens,
    total_tokens,
    storage_type,
    user_request_body,
    llm_request_body,
    llm_response_body,
    user_response_body,
    user_api_type,
    llm_api_type
)
SELECT
    id,
    system_api_key_id,
    provider_id,
    model_id,
    provider_api_key_id,
    model_name,
    real_model_name,
    request_received_at,
    llm_request_sent_at,
    llm_response_first_chunk_at,
    llm_response_completed_at,
    client_ip,
    llm_request_uri,
    llm_response_status,
    CASE
        WHEN status = 'CANCELLED' THEN 'ERROR'
        ELSE status
    END,
    is_stream,
    estimated_cost_nanos,
    estimated_cost_currency,
    created_at,
    updated_at,
    total_input_tokens,
    total_output_tokens,
    input_image_tokens,
    output_image_tokens,
    cache_read_tokens,
    reasoning_tokens,
    total_tokens,
    storage_type,
    user_request_body,
    llm_request_body,
    llm_response_body,
    user_response_body,
    user_api_type,
    llm_api_type
FROM request_log;

DROP TABLE request_log;
ALTER TABLE request_log_old RENAME TO request_log;

DROP INDEX IF EXISTS idx_model_cost_catalog_id;

CREATE TABLE model_old (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    billing_plan_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_model_billing_plan_id
        FOREIGN KEY (billing_plan_id) REFERENCES billing_plans (id)
            ON DELETE RESTRICT,
    CONSTRAINT chk_model_name_not_empty CHECK (model_name <> ''),
    CONSTRAINT chk_model_real_model_name_format CHECK (
        real_model_name IS NULL OR real_model_name <> ''
    ),
    CONSTRAINT chk_model_timestamps CHECK (updated_at >= created_at)
);

INSERT INTO model_old (
    id,
    provider_id,
    billing_plan_id,
    model_name,
    real_model_name,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    id,
    provider_id,
    NULL,
    model_name,
    real_model_name,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
FROM model;

DROP TABLE model;
ALTER TABLE model_old RENAME TO model;

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_pid_name_uq_active
    ON model (provider_id, model_name)
    WHERE deleted_at IS NULL AND is_enabled = true;
CREATE INDEX IF NOT EXISTS idx_model_provider_id ON model(provider_id);

DROP INDEX IF EXISTS idx_cost_components_version_priority;
DROP INDEX IF EXISTS idx_cost_catalog_versions_lookup;
DROP INDEX IF EXISTS idx_cost_catalog_versions_catalog_version;
DROP INDEX IF EXISTS idx_cost_catalogs_name_active;

DROP TABLE IF EXISTS cost_components;
DROP TABLE IF EXISTS cost_catalog_versions;
DROP TABLE IF EXISTS cost_catalogs;

PRAGMA foreign_keys=on;
