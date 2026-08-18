-- Revert the R3.8 timing and rollup columns. Existing first-response-body values
-- remain in the pre-R3.8 response_started column; newer evidence is dropped
-- because the old schema has no lossless storage for it.

DROP INDEX IF EXISTS idx_metric_request_rollup_scope_time;
ALTER TABLE metric_request_rollup_minute RENAME TO metric_request_rollup_timing_v1;

CREATE TABLE metric_request_rollup_timing_old (
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

INSERT INTO metric_request_rollup_timing_old (
    bucket_start_ms, scope_type, scope_id, scope_label,
    request_count, success_count, error_count, cancelled_count,
    first_byte_latency_sum_ms, first_byte_latency_count,
    total_latency_sum_ms, total_latency_count,
    input_tokens, output_tokens, reasoning_tokens, total_tokens,
    created_at, updated_at
)
SELECT
    bucket_start_ms, scope_type, scope_id, scope_label,
    request_count, success_count, error_count, cancelled_count,
    time_to_first_response_body_sum_ms, time_to_first_response_body_count,
    total_latency_sum_ms, total_latency_count,
    input_tokens, output_tokens, reasoning_tokens, total_tokens,
    created_at, updated_at
FROM metric_request_rollup_timing_v1;

DROP TABLE metric_request_rollup_timing_v1;
ALTER TABLE metric_request_rollup_timing_old RENAME TO metric_request_rollup_minute;
CREATE INDEX idx_metric_request_rollup_scope_time
    ON metric_request_rollup_minute (scope_type, scope_id, bucket_start_ms);

ALTER TABLE request_log RENAME TO request_log_timing_v1;

CREATE TABLE request_log_timing_old (
    id BIGINT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL
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
        ),
    client_request_id TEXT NULL
        CONSTRAINT chk_request_log_client_request_id CHECK (
            client_request_id IS NULL
            OR (
                length(client_request_id) BETWEEN 1 AND 64
                AND client_request_id NOT GLOB '*[^A-Za-z0-9._:-]*'
            )
        ),
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT,
    base_requested_model_name TEXT,
    resolved_reasoning_suffix TEXT,
    resolved_reasoning_preset TEXT,
    downstream_protocol TEXT NOT NULL,
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
    upstream_protocol TEXT,
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
    CONSTRAINT chk_request_log_downstream_protocol CHECK (downstream_protocol IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI')),
    CONSTRAINT chk_request_log_upstream_protocol CHECK (upstream_protocol IS NULL OR upstream_protocol IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI', 'OLLAMA')),
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

INSERT INTO request_log_timing_old (
    id, request_id, client_request_id, api_key_id,
    requested_model_name, base_requested_model_name,
    resolved_reasoning_suffix, resolved_reasoning_preset,
    downstream_protocol, overall_status, final_error_code, final_error_message,
    request_received_at, upstream_request_sent_at,
    response_started_to_client_at, completed_at, is_stream, client_ip,
    provider_id, provider_api_key_id, model_id, provider_key_snapshot,
    provider_name_snapshot, model_name_snapshot, real_model_name_snapshot,
    upstream_protocol, upstream_http_status, estimated_cost_nanos,
    estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
    cost_snapshot_json, total_input_tokens, total_output_tokens,
    input_text_tokens, output_text_tokens, input_image_tokens,
    output_image_tokens, cache_read_tokens, cache_write_tokens,
    reasoning_tokens, total_tokens, created_at, updated_at
)
SELECT
    id, request_id, client_request_id, api_key_id,
    requested_model_name, base_requested_model_name,
    resolved_reasoning_suffix, resolved_reasoning_preset,
    downstream_protocol, overall_status, final_error_code, final_error_message,
    request_received_at, upstream_request_sent_at,
    first_response_body_at, completed_at, is_stream, client_ip,
    provider_id, provider_api_key_id, model_id, provider_key_snapshot,
    provider_name_snapshot, model_name_snapshot, real_model_name_snapshot,
    upstream_protocol, upstream_http_status, estimated_cost_nanos,
    estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
    cost_snapshot_json, total_input_tokens, total_output_tokens,
    input_text_tokens, output_text_tokens, input_image_tokens,
    output_image_tokens, cache_read_tokens, cache_write_tokens,
    reasoning_tokens, total_tokens, created_at, updated_at
FROM request_log_timing_v1;

DROP TABLE request_log_timing_v1;
ALTER TABLE request_log_timing_old RENAME TO request_log;

CREATE INDEX idx_request_log_received_at ON request_log (request_received_at DESC);
CREATE INDEX idx_request_log_api_key_received_at ON request_log (api_key_id, request_received_at DESC);
CREATE INDEX idx_request_log_provider_received_at ON request_log (provider_id, request_received_at DESC);
CREATE INDEX idx_request_log_model_received_at ON request_log (model_id, request_received_at DESC);
CREATE INDEX idx_request_log_status_received_at ON request_log (overall_status, request_received_at DESC);
CREATE INDEX idx_request_log_downstream_protocol_received_at
    ON request_log (downstream_protocol, request_received_at DESC);
CREATE UNIQUE INDEX idx_request_log_request_id ON request_log (request_id);
CREATE INDEX idx_request_log_client_request_id ON request_log (client_request_id);
