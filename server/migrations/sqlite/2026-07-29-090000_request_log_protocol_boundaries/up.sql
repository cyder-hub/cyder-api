-- R3.1 intentionally discards request-log history. The gateway has no
-- compatibility obligation for mixed-direction protocol values.
DELETE FROM metric_ingested_request_log;

CREATE TABLE request_log_protocol_v2 (
    id BIGINT PRIMARY KEY NOT NULL,
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

DROP TABLE request_log;
ALTER TABLE request_log_protocol_v2 RENAME TO request_log;

CREATE INDEX idx_request_log_received_at ON request_log (request_received_at DESC);
CREATE INDEX idx_request_log_api_key_received_at ON request_log (api_key_id, request_received_at DESC);
CREATE INDEX idx_request_log_provider_received_at ON request_log (provider_id, request_received_at DESC);
CREATE INDEX idx_request_log_model_received_at ON request_log (model_id, request_received_at DESC);
CREATE INDEX idx_request_log_status_received_at ON request_log (overall_status, request_received_at DESC);
CREATE INDEX idx_request_log_downstream_protocol_received_at
    ON request_log (downstream_protocol, request_received_at DESC);
