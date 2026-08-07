-- R3.10 changes the R3.9 single-source identity into a non-reusable source id,
-- preserves every R3.9 row, and adds lifecycle/default/family constraints.
-- This migration has no down migration.

PRAGMA foreign_keys = OFF;

ALTER TABLE upstream_source RENAME TO upstream_source_r39;
DROP INDEX idx_upstream_source_provider_active_unique;
DROP INDEX idx_upstream_source_provider_id;

CREATE TABLE upstream_source (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    profile_type TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT false,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    is_default BOOLEAN NOT NULL DEFAULT false,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_upstream_source_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_upstream_source_profile_type CHECK (
        profile_type IN (
            'OPENAI',
            'GEMINI',
            'VERTEX',
            'VERTEX_OPENAI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_upstream_source_endpoint_not_empty CHECK (endpoint <> ''),
    CONSTRAINT chk_upstream_source_flags CHECK (
        (is_default = 0 OR (is_enabled = 1 AND deleted_at IS NULL))
        AND (deleted_at IS NULL OR (is_enabled = 0 AND is_default = 0))
    ),
    CONSTRAINT chk_upstream_source_timestamps CHECK (updated_at >= created_at)
);

INSERT INTO upstream_source (
    id, provider_id, profile_type, endpoint, use_proxy,
    is_enabled, is_default, deleted_at, created_at, updated_at
)
SELECT
    id,
    provider_id,
    profile_type,
    endpoint,
    use_proxy,
    CASE WHEN deleted_at IS NULL THEN 1 ELSE 0 END,
    CASE WHEN deleted_at IS NULL THEN 1 ELSE 0 END,
    deleted_at,
    created_at,
    updated_at
FROM upstream_source_r39;

CREATE UNIQUE INDEX idx_upstream_source_provider_default_unique
    ON upstream_source (provider_id)
    WHERE deleted_at IS NULL AND is_default = 1;

CREATE UNIQUE INDEX idx_upstream_source_provider_wire_family_unique
    ON upstream_source (
        provider_id,
        CASE
            WHEN profile_type IN ('OPENAI', 'VERTEX_OPENAI', 'GEMINI_OPENAI') THEN 'OPENAI'
            WHEN profile_type = 'RESPONSES' THEN 'RESPONSES'
            WHEN profile_type = 'ANTHROPIC' THEN 'ANTHROPIC'
            WHEN profile_type IN ('GEMINI', 'VERTEX') THEN 'GEMINI'
            WHEN profile_type = 'OLLAMA' THEN 'OLLAMA'
        END
    )
    WHERE deleted_at IS NULL;

CREATE INDEX idx_upstream_source_provider_id
    ON upstream_source (provider_id);

ALTER TABLE request_log RENAME TO request_log_r39;
DROP INDEX idx_request_log_received_at;
DROP INDEX idx_request_log_api_key_received_at;
DROP INDEX idx_request_log_provider_received_at;
DROP INDEX idx_request_log_model_received_at;
DROP INDEX idx_request_log_source_received_at;
DROP INDEX idx_request_log_status_received_at;
DROP INDEX idx_request_log_downstream_protocol_received_at;
DROP INDEX idx_request_log_request_id;
DROP INDEX idx_request_log_client_request_id;

CREATE TABLE request_log (
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
    upstream_response_headers_at BIGINT,
    upstream_first_body_chunk_at BIGINT,
    first_response_body_at BIGINT,
    first_token_at BIGINT,
    max_upstream_response_idle_ms BIGINT,
    completed_at BIGINT,
    is_stream BOOLEAN NOT NULL DEFAULT 0,
    client_ip TEXT,
    provider_id BIGINT,
    provider_api_key_id BIGINT,
    model_id BIGINT,
    source_id BIGINT,
    provider_key_snapshot TEXT,
    provider_name_snapshot TEXT,
    model_name_snapshot TEXT,
    real_model_name_snapshot TEXT,
    source_profile_type_snapshot TEXT,
    source_endpoint_snapshot TEXT,
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
    CONSTRAINT fk_request_log_source_id FOREIGN KEY (source_id) REFERENCES upstream_source(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id) ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_overall_status CHECK (overall_status IN ('SUCCESS', 'ERROR', 'CANCELLED')),
    CONSTRAINT chk_request_log_downstream_protocol CHECK (downstream_protocol IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI')),
    CONSTRAINT chk_request_log_upstream_protocol CHECK (upstream_protocol IS NULL OR upstream_protocol IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI', 'OLLAMA')),
    CONSTRAINT chk_request_log_upstream_http_status CHECK (upstream_http_status IS NULL OR upstream_http_status BETWEEN 100 AND 599),
    CONSTRAINT chk_request_log_source_snapshot CHECK (
        (
            source_id IS NULL
            AND source_profile_type_snapshot IS NULL
            AND source_endpoint_snapshot IS NULL
        )
        OR (
            source_id IS NOT NULL
            AND source_profile_type_snapshot IN (
                'OPENAI', 'GEMINI', 'VERTEX', 'VERTEX_OPENAI',
                'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI'
            )
            AND source_endpoint_snapshot <> ''
        )
    ),
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
    CONSTRAINT chk_request_log_timing_contract CHECK (
        updated_at >= created_at
        AND (upstream_request_sent_at IS NULL OR upstream_request_sent_at >= request_received_at)
        AND (
            upstream_response_headers_at IS NULL
            OR (upstream_request_sent_at IS NOT NULL AND upstream_response_headers_at >= upstream_request_sent_at)
        )
        AND (
            upstream_first_body_chunk_at IS NULL
            OR (upstream_response_headers_at IS NOT NULL AND upstream_first_body_chunk_at >= upstream_response_headers_at)
        )
        AND (
            first_response_body_at IS NULL
            OR (upstream_request_sent_at IS NOT NULL AND first_response_body_at >= upstream_request_sent_at)
        )
        AND (
            first_response_body_at IS NULL
            OR upstream_first_body_chunk_at IS NULL
            OR first_response_body_at >= upstream_first_body_chunk_at
        )
        AND (
            first_token_at IS NULL
            OR (is_stream = 1 AND upstream_first_body_chunk_at IS NOT NULL AND first_token_at >= upstream_first_body_chunk_at)
        )
        AND (max_upstream_response_idle_ms IS NULL OR max_upstream_response_idle_ms >= 0)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
        AND (completed_at IS NULL OR upstream_request_sent_at IS NULL OR completed_at >= upstream_request_sent_at)
        AND (completed_at IS NULL OR upstream_response_headers_at IS NULL OR completed_at >= upstream_response_headers_at)
        AND (completed_at IS NULL OR upstream_first_body_chunk_at IS NULL OR completed_at >= upstream_first_body_chunk_at)
        AND (completed_at IS NULL OR first_response_body_at IS NULL OR completed_at >= first_response_body_at)
        AND (completed_at IS NULL OR first_token_at IS NULL OR completed_at >= first_token_at)
    )
);

INSERT INTO request_log (
    id, request_id, client_request_id, api_key_id, requested_model_name,
    base_requested_model_name, resolved_reasoning_suffix, resolved_reasoning_preset,
    downstream_protocol, overall_status, final_error_code, final_error_message,
    request_received_at, upstream_request_sent_at, upstream_response_headers_at,
    upstream_first_body_chunk_at, first_response_body_at, first_token_at,
    max_upstream_response_idle_ms, completed_at, is_stream, client_ip,
    provider_id, provider_api_key_id, model_id, source_id,
    provider_key_snapshot, provider_name_snapshot, model_name_snapshot,
    real_model_name_snapshot, source_profile_type_snapshot, source_endpoint_snapshot,
    upstream_protocol, upstream_http_status, estimated_cost_nanos,
    estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
    cost_snapshot_json, total_input_tokens, total_output_tokens, input_text_tokens,
    output_text_tokens, input_image_tokens, output_image_tokens, cache_read_tokens,
    cache_write_tokens, reasoning_tokens, total_tokens, created_at, updated_at
)
SELECT
    id, request_id, client_request_id, api_key_id, requested_model_name,
    base_requested_model_name, resolved_reasoning_suffix, resolved_reasoning_preset,
    downstream_protocol, overall_status, final_error_code, final_error_message,
    request_received_at, upstream_request_sent_at, upstream_response_headers_at,
    upstream_first_body_chunk_at, first_response_body_at, first_token_at,
    max_upstream_response_idle_ms, completed_at, is_stream, client_ip,
    provider_id, provider_api_key_id, model_id, source_id,
    provider_key_snapshot, provider_name_snapshot, model_name_snapshot,
    real_model_name_snapshot, source_profile_type_snapshot, source_endpoint_snapshot,
    upstream_protocol, upstream_http_status, estimated_cost_nanos,
    estimated_cost_currency, cost_catalog_id, cost_catalog_version_id,
    cost_snapshot_json, total_input_tokens, total_output_tokens, input_text_tokens,
    output_text_tokens, input_image_tokens, output_image_tokens, cache_read_tokens,
    cache_write_tokens, reasoning_tokens, total_tokens, created_at, updated_at
FROM request_log_r39;

DROP TABLE request_log_r39;
DROP TABLE upstream_source_r39;

CREATE INDEX idx_request_log_received_at ON request_log (request_received_at DESC);
CREATE INDEX idx_request_log_api_key_received_at ON request_log (api_key_id, request_received_at DESC);
CREATE INDEX idx_request_log_provider_received_at ON request_log (provider_id, request_received_at DESC);
CREATE INDEX idx_request_log_model_received_at ON request_log (model_id, request_received_at DESC);
CREATE INDEX idx_request_log_source_received_at ON request_log (source_id, request_received_at DESC);
CREATE INDEX idx_request_log_status_received_at ON request_log (overall_status, request_received_at DESC);
CREATE INDEX idx_request_log_downstream_protocol_received_at ON request_log (downstream_protocol, request_received_at DESC);
CREATE UNIQUE INDEX idx_request_log_request_id ON request_log (request_id);
CREATE INDEX idx_request_log_client_request_id ON request_log (client_request_id);

UPDATE metric_request_rollup_minute
SET scope_label = (
    SELECT profile_type
    FROM upstream_source
    WHERE CAST(upstream_source.id AS TEXT) = metric_request_rollup_minute.scope_id
)
WHERE scope_type = 'source'
  AND scope_label = 'primary';

PRAGMA foreign_key_check;
PRAGMA foreign_keys = ON;
