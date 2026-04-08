PRAGMA foreign_keys=off;

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
    cached_tokens INTEGER NOT NULL DEFAULT 0,
    input_image_tokens INTEGER NOT NULL DEFAULT 0,
    output_image_tokens INTEGER NOT NULL DEFAULT 0,
    user_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(user_api_type <> ''),
    llm_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(llm_api_type <> ''),

    CONSTRAINT fk_request_log_system_api_key_id FOREIGN KEY (system_api_key_id) REFERENCES system_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id FOREIGN KEY (provider_id) REFERENCES provider(id) ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id FOREIGN KEY (model_id) REFERENCES model(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
        input_tokens >= 0 AND output_tokens >= 0 AND
        reasoning_tokens >= 0 AND total_tokens >= 0 AND
        cached_tokens >= 0 AND input_image_tokens >= 0 AND output_image_tokens >= 0
    ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (updated_at >= created_at),
    CONSTRAINT chk_request_log_status CHECK (status IN ('PENDING', 'SUCCESS', 'ERROR'))
);

INSERT INTO request_log_new (
    id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
    real_model_name, request_received_at, llm_request_sent_at, llm_response_first_chunk_at,
    llm_response_completed_at, client_ip, llm_request_uri, llm_response_status, status,
    is_stream, calculated_cost, cost_currency, created_at, updated_at,
    input_tokens, output_tokens, reasoning_tokens, total_tokens, storage_type,
    user_request_body, llm_request_body, llm_response_body, user_response_body,
    cached_tokens, input_image_tokens, output_image_tokens, user_api_type, llm_api_type
)
SELECT
    id, system_api_key_id, provider_id, model_id, provider_api_key_id, model_name,
    real_model_name, request_received_at, llm_request_sent_at, llm_response_first_chunk_at,
    llm_response_completed_at, client_ip, llm_request_uri, llm_response_status, status,
    is_stream, calculated_cost, cost_currency, created_at, updated_at,
    input_tokens, output_tokens, reasoning_tokens, total_tokens, storage_type,
    user_request_body, llm_request_body, llm_response_body, user_response_body,
    cached_tokens, input_image_tokens, output_image_tokens, user_api_type, llm_api_type
FROM request_log;

DROP TABLE request_log;
ALTER TABLE request_log_new RENAME TO request_log;

PRAGMA foreign_keys=on;
