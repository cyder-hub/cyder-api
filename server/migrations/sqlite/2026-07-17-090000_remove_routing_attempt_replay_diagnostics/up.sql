DROP TABLE IF EXISTS request_replay_run;
DROP TABLE IF EXISTS request_attempt;
DROP TABLE IF EXISTS api_key_model_override;
DROP TABLE IF EXISTS model_route_candidate;
DROP TABLE IF EXISTS model_route;

CREATE TABLE request_log_v1 (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT,
    base_requested_model_name TEXT,
    resolved_reasoning_suffix TEXT,
    resolved_reasoning_preset TEXT,
    user_api_type TEXT NOT NULL,
    overall_status TEXT NOT NULL,
    final_error_code TEXT,
    final_error_message TEXT,
    request_received_at BIGINT NOT NULL,
    upstream_request_sent_at BIGINT,
    response_started_to_client_at BIGINT,
    completed_at BIGINT,
    is_stream BOOLEAN NOT NULL DEFAULT 0,
    client_ip TEXT,
    provider_id BIGINT,
    provider_api_key_id BIGINT,
    model_id BIGINT,
    provider_key_snapshot TEXT,
    provider_name_snapshot TEXT,
    model_name_snapshot TEXT,
    real_model_name_snapshot TEXT,
    llm_api_type TEXT,
    upstream_http_status INTEGER,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    cost_catalog_id BIGINT,
    cost_catalog_version_id BIGINT,
    cost_snapshot_json TEXT,
    total_input_tokens INTEGER,
    total_output_tokens INTEGER,
    input_text_tokens INTEGER,
    output_text_tokens INTEGER,
    input_image_tokens INTEGER,
    output_image_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens INTEGER,
    total_tokens INTEGER,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_log_api_key_id FOREIGN KEY (api_key_id) REFERENCES api_key(id) ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id FOREIGN KEY (provider_id) REFERENCES provider(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id FOREIGN KEY (model_id) REFERENCES model(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_overall_status CHECK (overall_status IN ('SUCCESS', 'ERROR', 'CANCELLED')),
    CONSTRAINT chk_request_log_user_api_type CHECK (user_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI')),
    CONSTRAINT chk_request_log_llm_api_type CHECK (llm_api_type IS NULL OR llm_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI')),
    CONSTRAINT chk_request_log_upstream_http_status CHECK (upstream_http_status IS NULL OR upstream_http_status BETWEEN 100 AND 599),
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
        (total_input_tokens IS NULL OR total_input_tokens >= 0)
        AND (total_output_tokens IS NULL OR total_output_tokens >= 0)
        AND (input_text_tokens IS NULL OR input_text_tokens >= 0)
        AND (output_text_tokens IS NULL OR output_text_tokens >= 0)
        AND (input_image_tokens IS NULL OR input_image_tokens >= 0)
        AND (output_image_tokens IS NULL OR output_image_tokens >= 0)
        AND (cache_read_tokens IS NULL OR cache_read_tokens >= 0)
        AND (cache_write_tokens IS NULL OR cache_write_tokens >= 0)
        AND (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
        AND (total_tokens IS NULL OR total_tokens >= 0)
    ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (
        updated_at >= created_at
        AND (upstream_request_sent_at IS NULL OR upstream_request_sent_at >= request_received_at)
        AND (response_started_to_client_at IS NULL OR response_started_to_client_at >= request_received_at)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
    )
);

INSERT INTO request_log_v1 (
    id, api_key_id, requested_model_name, base_requested_model_name,
    resolved_reasoning_suffix, resolved_reasoning_preset, user_api_type,
    overall_status, final_error_code, final_error_message, request_received_at,
    upstream_request_sent_at, response_started_to_client_at, completed_at,
    is_stream, client_ip, provider_id, provider_api_key_id, model_id,
    provider_key_snapshot, provider_name_snapshot, model_name_snapshot,
    real_model_name_snapshot, llm_api_type, estimated_cost_nanos,
    estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
    cost_snapshot_json, total_input_tokens, total_output_tokens,
    input_text_tokens, output_text_tokens, input_image_tokens,
    output_image_tokens, cache_read_tokens, cache_write_tokens,
    reasoning_tokens, total_tokens, created_at, updated_at
)
SELECT
    id, api_key_id, requested_model_name, base_requested_model_name,
    resolved_reasoning_suffix, resolved_reasoning_preset, user_api_type,
    overall_status, final_error_code, final_error_message, request_received_at,
    first_attempt_started_at, response_started_to_client_at, completed_at,
    is_stream, client_ip, final_provider_id, final_provider_api_key_id,
    final_model_id, final_provider_key_snapshot, final_provider_name_snapshot,
    final_model_name_snapshot, final_real_model_name_snapshot, final_llm_api_type,
    estimated_cost_nanos, estimated_cost_currency, cost_catalog_id,
    cost_catalog_version_id, cost_snapshot_json, total_input_tokens,
    total_output_tokens, input_text_tokens, output_text_tokens,
    input_image_tokens, output_image_tokens, cache_read_tokens,
    cache_write_tokens, reasoning_tokens, total_tokens, created_at, updated_at
FROM request_log;

DROP TABLE request_log;
ALTER TABLE request_log_v1 RENAME TO request_log;

CREATE INDEX idx_request_log_received_at ON request_log (request_received_at DESC);
CREATE INDEX idx_request_log_api_key_received_at ON request_log (api_key_id, request_received_at DESC);
CREATE INDEX idx_request_log_provider_received_at ON request_log (provider_id, request_received_at DESC);
CREATE INDEX idx_request_log_model_received_at ON request_log (model_id, request_received_at DESC);
CREATE INDEX idx_request_log_status_received_at ON request_log (overall_status, request_received_at DESC);

DROP TABLE IF EXISTS metric_attempt_rollup_minute;
DROP TABLE IF EXISTS metric_request_rollup_minute;
DROP TABLE IF EXISTS metric_http_status_rollup_minute;
DROP TABLE IF EXISTS metric_cost_rollup_minute;

CREATE TABLE metric_request_rollup_minute (
    bucket_start_ms BIGINT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    scope_label TEXT,
    request_count BIGINT NOT NULL,
    success_count BIGINT NOT NULL,
    error_count BIGINT NOT NULL,
    cancelled_count BIGINT NOT NULL,
    first_byte_latency_sum_ms BIGINT NOT NULL,
    first_byte_latency_count BIGINT NOT NULL,
    total_latency_sum_ms BIGINT NOT NULL,
    total_latency_count BIGINT NOT NULL,
    input_tokens BIGINT NOT NULL,
    output_tokens BIGINT NOT NULL,
    reasoning_tokens BIGINT NOT NULL,
    total_tokens BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (bucket_start_ms, scope_type, scope_id)
);
CREATE INDEX idx_metric_request_rollup_scope_time ON metric_request_rollup_minute (scope_type, scope_id, bucket_start_ms);

CREATE TABLE metric_http_status_rollup_minute (
    bucket_start_ms BIGINT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    http_status INTEGER NOT NULL,
    count BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (bucket_start_ms, scope_type, scope_id, http_status)
);
CREATE INDEX idx_metric_http_status_rollup_scope_time ON metric_http_status_rollup_minute (scope_type, scope_id, bucket_start_ms);

CREATE TABLE metric_cost_rollup_minute (
    bucket_start_ms BIGINT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    currency TEXT NOT NULL,
    amount_nanos BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (bucket_start_ms, scope_type, scope_id, currency)
);
CREATE INDEX idx_metric_cost_rollup_scope_time ON metric_cost_rollup_minute (scope_type, scope_id, bucket_start_ms);

DELETE FROM metric_ingested_request_log;
