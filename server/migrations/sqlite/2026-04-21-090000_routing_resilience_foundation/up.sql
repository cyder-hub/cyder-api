-- SQLite has no standalone enum/domain type support.
-- This consolidated migration keeps enum work and request_log rebuild in one
-- place. `request_attempt` specific `TEXT + CHECK` constraints will still be
-- attached when that table is introduced.

ALTER TABLE model ADD COLUMN supports_streaming BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE model ADD COLUMN supports_tools BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE model ADD COLUMN supports_reasoning BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE model ADD COLUMN supports_image_input BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE model ADD COLUMN supports_embeddings BOOLEAN NOT NULL DEFAULT 1;
ALTER TABLE model ADD COLUMN supports_rerank BOOLEAN NOT NULL DEFAULT 1;

CREATE TABLE request_log_new (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT,
    resolved_name_scope TEXT,
    resolved_route_id BIGINT,
    resolved_route_name TEXT,
    user_api_type TEXT NOT NULL,
    overall_status TEXT NOT NULL,
    final_error_code TEXT,
    final_error_message TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    fallback_count INTEGER NOT NULL DEFAULT 0,
    request_received_at BIGINT NOT NULL,
    first_attempt_started_at BIGINT,
    response_started_to_client_at BIGINT,
    completed_at BIGINT,
    client_ip TEXT,
    final_attempt_id BIGINT,
    final_provider_id BIGINT,
    final_provider_api_key_id BIGINT,
    final_model_id BIGINT,
    final_provider_key_snapshot TEXT,
    final_provider_name_snapshot TEXT,
    final_model_name_snapshot TEXT,
    final_real_model_name_snapshot TEXT,
    final_llm_api_type TEXT,
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
    bundle_version INTEGER,
    bundle_storage_type TEXT,
    bundle_storage_key TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_log_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
        ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_id
        FOREIGN KEY (final_provider_id) REFERENCES provider(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_model_id
        FOREIGN KEY (final_model_id) REFERENCES model(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_api_key_id
        FOREIGN KEY (final_provider_api_key_id) REFERENCES provider_api_key(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_overall_status
        CHECK (overall_status IN ('SUCCESS', 'ERROR', 'CANCELLED')),
    CONSTRAINT chk_request_log_user_api_type
        CHECK (user_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI')),
    CONSTRAINT chk_request_log_final_llm_api_type
        CHECK (
            final_llm_api_type IS NULL
            OR final_llm_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI')
        ),
    CONSTRAINT chk_request_log_bundle_storage_type
        CHECK (bundle_storage_type IS NULL OR bundle_storage_type IN ('FILE_SYSTEM', 'S3')),
    CONSTRAINT chk_request_log_bundle_version
        CHECK (bundle_version IS NULL OR bundle_version IN (1, 2)),
    CONSTRAINT chk_request_log_attempt_counts_non_negative
        CHECK (attempt_count >= 0 AND retry_count >= 0 AND fallback_count >= 0),
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
        AND (first_attempt_started_at IS NULL OR first_attempt_started_at >= request_received_at)
        AND (response_started_to_client_at IS NULL OR response_started_to_client_at >= request_received_at)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
    )
);

CREATE TEMP TABLE request_attempt_legacy_source AS
SELECT
    rl.id AS id,
    rl.id AS request_log_id,
    1 AS attempt_index,
    1 AS candidate_position,
    rl.provider_id,
    rl.provider_api_key_id,
    rl.model_id,
    p.provider_key AS provider_key_snapshot,
    p.name AS provider_name_snapshot,
    rl.model_name AS model_name_snapshot,
    rl.real_model_name AS real_model_name_snapshot,
    rl.llm_api_type,
    CASE
        WHEN rl.status = 'SUCCESS' THEN 'SUCCESS'
        WHEN rl.status = 'CANCELLED' THEN 'CANCELLED'
        ELSE 'ERROR'
    END AS attempt_status,
    CASE
        WHEN rl.status = 'SUCCESS' THEN 'RETURN_SUCCESS'
        ELSE 'FAIL_FAST'
    END AS scheduler_action,
    CASE
        WHEN rl.status = 'SUCCESS' THEN NULL
        WHEN rl.status = 'CANCELLED' THEN 'client_cancelled_error'
        WHEN rl.status = 'PENDING' THEN 'legacy_pending_request_log_error'
        ELSE 'legacy_request_log_error'
    END AS error_code,
    CASE
        WHEN rl.status = 'SUCCESS' THEN NULL
        WHEN rl.status = 'CANCELLED' THEN 'migrated from legacy request_log: cancelled'
        WHEN rl.status = 'PENDING' THEN 'migrated from legacy request_log: pending state is not valid in final log model'
        ELSE 'migrated from legacy request_log: original error classification unavailable'
    END AS error_message,
    rl.llm_request_uri AS request_uri,
    NULL AS request_headers_json,
    NULL AS response_headers_json,
    rl.llm_response_status AS http_status,
    CASE
        WHEN rl.llm_request_sent_at IS NULL THEN NULL
        WHEN rl.llm_request_sent_at < rl.created_at THEN rl.created_at
        ELSE rl.llm_request_sent_at
    END AS started_at,
    CASE
        WHEN rl.llm_response_first_chunk_at IS NULL THEN NULL
        WHEN rl.llm_request_sent_at IS NULL THEN rl.llm_response_first_chunk_at
        WHEN rl.llm_response_first_chunk_at < CASE
            WHEN rl.llm_request_sent_at < rl.created_at THEN rl.created_at
            ELSE rl.llm_request_sent_at
        END THEN CASE
            WHEN rl.llm_request_sent_at < rl.created_at THEN rl.created_at
            ELSE rl.llm_request_sent_at
        END
        ELSE rl.llm_response_first_chunk_at
    END AS first_byte_at,
    CASE
        WHEN COALESCE(rl.llm_response_completed_at, rl.updated_at) < CASE
            WHEN rl.llm_request_sent_at IS NULL THEN COALESCE(rl.llm_response_completed_at, rl.updated_at)
            WHEN rl.llm_request_sent_at < rl.created_at THEN rl.created_at
            ELSE rl.llm_request_sent_at
        END THEN CASE
            WHEN rl.llm_request_sent_at < rl.created_at THEN rl.created_at
            ELSE rl.llm_request_sent_at
        END
        ELSE COALESCE(rl.llm_response_completed_at, rl.updated_at)
    END AS completed_at,
    rl.llm_response_first_chunk_at IS NOT NULL AS response_started_to_client,
    NULL AS backoff_ms,
    rl.applied_request_patch_ids_json,
    rl.request_patch_summary_json,
    rl.estimated_cost_nanos,
    rl.estimated_cost_currency,
    rl.cost_catalog_version_id,
    rl.total_input_tokens,
    rl.total_output_tokens,
    rl.input_text_tokens,
    rl.output_text_tokens,
    rl.input_image_tokens,
    rl.output_image_tokens,
    rl.cache_read_tokens,
    rl.cache_write_tokens,
    rl.reasoning_tokens,
    rl.total_tokens,
    NULL AS llm_request_blob_id,
    NULL AS llm_request_patch_id,
    NULL AS llm_response_blob_id,
    NULL AS llm_response_capture_state,
    rl.created_at,
    rl.updated_at
FROM request_log AS rl
JOIN api_key AS ak
    ON ak.id = rl.system_api_key_id
LEFT JOIN provider AS p
    ON p.id = rl.provider_id;

INSERT INTO request_log_new (
    id,
    api_key_id,
    requested_model_name,
    resolved_name_scope,
    resolved_route_id,
    resolved_route_name,
    user_api_type,
    overall_status,
    final_error_code,
    final_error_message,
    attempt_count,
    retry_count,
    fallback_count,
    request_received_at,
    first_attempt_started_at,
    response_started_to_client_at,
    completed_at,
    client_ip,
    final_attempt_id,
    final_provider_id,
    final_provider_api_key_id,
    final_model_id,
    final_provider_key_snapshot,
    final_provider_name_snapshot,
    final_model_name_snapshot,
    final_real_model_name_snapshot,
    final_llm_api_type,
    estimated_cost_nanos,
    estimated_cost_currency,
    cost_catalog_id,
    cost_catalog_version_id,
    cost_snapshot_json,
    total_input_tokens,
    total_output_tokens,
    input_text_tokens,
    output_text_tokens,
    input_image_tokens,
    output_image_tokens,
    cache_read_tokens,
    cache_write_tokens,
    reasoning_tokens,
    total_tokens,
    bundle_version,
    bundle_storage_type,
    bundle_storage_key,
    created_at,
    updated_at
)
SELECT
    rl.id,
    rl.system_api_key_id,
    rl.requested_model_name,
    rl.resolved_name_scope,
    rl.resolved_route_id,
    rl.resolved_route_name,
    rl.user_api_type,
    CASE
        WHEN rl.status = 'SUCCESS' THEN 'SUCCESS'
        WHEN rl.status = 'CANCELLED' THEN 'CANCELLED'
        ELSE 'ERROR'
    END,
    CASE
        WHEN rl.status = 'SUCCESS' THEN NULL
        WHEN rl.status = 'CANCELLED' THEN 'client_cancelled_error'
        WHEN rl.status = 'PENDING' THEN 'legacy_pending_request_log_error'
        ELSE 'legacy_request_log_error'
    END,
    CASE
        WHEN rl.status = 'SUCCESS' THEN NULL
        WHEN rl.status = 'CANCELLED' THEN 'migrated from legacy request_log: cancelled'
        WHEN rl.status = 'PENDING' THEN 'migrated from legacy request_log: pending state is not valid in final log model'
        ELSE 'migrated from legacy request_log: original error classification unavailable'
    END,
    1,
    0,
    0,
    rl.request_received_at,
    rl.llm_request_sent_at,
    rl.llm_response_first_chunk_at,
    COALESCE(rl.llm_response_completed_at, rl.updated_at),
    rl.client_ip,
    rl.id,
    rl.provider_id,
    rl.provider_api_key_id,
    rl.model_id,
    p.provider_key,
    p.name,
    rl.model_name,
    rl.real_model_name,
    rl.llm_api_type,
    rl.estimated_cost_nanos,
    rl.estimated_cost_currency,
    rl.cost_catalog_id,
    rl.cost_catalog_version_id,
    rl.cost_snapshot_json,
    rl.total_input_tokens,
    rl.total_output_tokens,
    rl.input_text_tokens,
    rl.output_text_tokens,
    rl.input_image_tokens,
    rl.output_image_tokens,
    rl.cache_read_tokens,
    rl.cache_write_tokens,
    rl.reasoning_tokens,
    rl.total_tokens,
    CASE
        WHEN rl.storage_type IS NOT NULL THEN 1
        ELSE NULL
    END,
    rl.storage_type,
    CASE
        WHEN rl.storage_type = 'FILE_SYSTEM' THEN
            strftime('%Y/%m/%d', rl.created_at / 1000, 'unixepoch')
            || '/'
            || CASE
                WHEN length(CAST(rl.id AS TEXT)) >= 6
                    THEN substr(CAST(rl.id AS TEXT), length(CAST(rl.id AS TEXT)) - 5, 2) || '/'
                ELSE ''
            END
            || CAST(rl.id AS TEXT)
            || '.mp.gz'
        WHEN rl.storage_type = 'S3' THEN
            'logs/'
            || strftime('%Y/%m/%d', rl.created_at / 1000, 'unixepoch')
            || '/'
            || CAST(rl.id AS TEXT)
            || '.mp.gz'
        ELSE NULL
    END,
    rl.created_at,
    rl.updated_at
FROM request_log AS rl
JOIN api_key AS ak
    ON ak.id = rl.system_api_key_id
LEFT JOIN provider AS p
    ON p.id = rl.provider_id;

DROP INDEX IF EXISTS idx_request_log_cost_catalog_id;
DROP INDEX IF EXISTS idx_request_log_cost_catalog_version_id;
DROP INDEX IF EXISTS idx_request_log_received_at;
DROP INDEX IF EXISTS idx_request_log_channel;
DROP INDEX IF EXISTS idx_request_log_external_id;
DROP TABLE request_log;
ALTER TABLE request_log_new RENAME TO request_log;

CREATE INDEX IF NOT EXISTS idx_request_log_api_key_id
    ON request_log (api_key_id);
CREATE INDEX IF NOT EXISTS idx_request_log_final_provider_id
    ON request_log (final_provider_id);
CREATE INDEX IF NOT EXISTS idx_request_log_final_model_id
    ON request_log (final_model_id);
CREATE INDEX IF NOT EXISTS idx_request_log_request_received_at
    ON request_log (request_received_at);
CREATE INDEX IF NOT EXISTS idx_request_log_overall_status
    ON request_log (overall_status);
CREATE INDEX IF NOT EXISTS idx_request_log_resolved_route_id
    ON request_log (resolved_route_id);
CREATE INDEX IF NOT EXISTS idx_request_log_final_attempt_id
    ON request_log (final_attempt_id);
CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_id
    ON request_log (cost_catalog_id);
CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_version_id
    ON request_log (cost_catalog_version_id);
CREATE INDEX IF NOT EXISTS idx_request_log_bundle_storage_type
    ON request_log (bundle_storage_type);

CREATE TABLE request_attempt (
    id BIGINT PRIMARY KEY NOT NULL,
    request_log_id BIGINT NOT NULL,
    attempt_index INTEGER NOT NULL,
    candidate_position INTEGER NOT NULL,
    provider_id BIGINT,
    provider_api_key_id BIGINT,
    model_id BIGINT,
    provider_key_snapshot TEXT,
    provider_name_snapshot TEXT,
    model_name_snapshot TEXT,
    real_model_name_snapshot TEXT,
    llm_api_type TEXT,
    attempt_status TEXT NOT NULL,
    scheduler_action TEXT NOT NULL,
    error_code TEXT,
    error_message TEXT,
    request_uri TEXT,
    request_headers_json TEXT,
    response_headers_json TEXT,
    http_status INTEGER,
    started_at BIGINT,
    first_byte_at BIGINT,
    completed_at BIGINT,
    response_started_to_client BOOLEAN NOT NULL DEFAULT FALSE,
    backoff_ms INTEGER,
    applied_request_patch_ids_json TEXT,
    request_patch_summary_json TEXT,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    cost_catalog_version_id BIGINT,
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
    llm_request_blob_id INTEGER,
    llm_request_patch_id INTEGER,
    llm_response_blob_id INTEGER,
    llm_response_capture_state TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_attempt_request_log_id
        FOREIGN KEY (request_log_id) REFERENCES request_log(id)
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_provider_api_key_id
        FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT uq_request_attempt_request_log_attempt_index
        UNIQUE (request_log_id, attempt_index),
    CONSTRAINT chk_request_attempt_attempt_index_positive
        CHECK (attempt_index >= 1),
    CONSTRAINT chk_request_attempt_candidate_position_positive
        CHECK (candidate_position >= 1),
    CONSTRAINT chk_request_attempt_llm_api_type
        CHECK (
            llm_api_type IS NULL
            OR llm_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI')
        ),
    CONSTRAINT chk_request_attempt_attempt_status
        CHECK (attempt_status IN ('SKIPPED', 'SUCCESS', 'ERROR', 'CANCELLED')),
    CONSTRAINT chk_request_attempt_scheduler_action
        CHECK (scheduler_action IN ('RETURN_SUCCESS', 'FAIL_FAST', 'RETRY_SAME_CANDIDATE', 'FALLBACK_NEXT_CANDIDATE')),
    CONSTRAINT chk_request_attempt_http_status
        CHECK (http_status IS NULL OR (http_status >= 100 AND http_status <= 599)),
    CONSTRAINT chk_request_attempt_backoff_ms_non_negative
        CHECK (backoff_ms IS NULL OR backoff_ms >= 0),
    CONSTRAINT chk_request_attempt_tokens_non_negative CHECK (
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
    CONSTRAINT chk_request_attempt_bundle_ids_non_negative CHECK (
        (llm_request_blob_id IS NULL OR llm_request_blob_id >= 0)
        AND (llm_request_patch_id IS NULL OR llm_request_patch_id >= 0)
        AND (llm_response_blob_id IS NULL OR llm_response_blob_id >= 0)
    ),
    CONSTRAINT chk_request_attempt_timestamps_order CHECK (
        updated_at >= created_at
        AND (started_at IS NULL OR started_at >= created_at)
        AND (first_byte_at IS NULL OR started_at IS NULL OR first_byte_at >= started_at)
        AND (completed_at IS NULL OR started_at IS NULL OR completed_at >= started_at)
    )
);

CREATE INDEX IF NOT EXISTS idx_request_attempt_request_log_id
    ON request_attempt (request_log_id);
CREATE INDEX IF NOT EXISTS idx_request_attempt_provider_id
    ON request_attempt (provider_id);
CREATE INDEX IF NOT EXISTS idx_request_attempt_model_id
    ON request_attempt (model_id);
CREATE INDEX IF NOT EXISTS idx_request_attempt_started_at
    ON request_attempt (started_at);

INSERT INTO request_attempt (
    id,
    request_log_id,
    attempt_index,
    candidate_position,
    provider_id,
    provider_api_key_id,
    model_id,
    provider_key_snapshot,
    provider_name_snapshot,
    model_name_snapshot,
    real_model_name_snapshot,
    llm_api_type,
    attempt_status,
    scheduler_action,
    error_code,
    error_message,
    request_uri,
    request_headers_json,
    response_headers_json,
    http_status,
    started_at,
    first_byte_at,
    completed_at,
    response_started_to_client,
    backoff_ms,
    applied_request_patch_ids_json,
    request_patch_summary_json,
    estimated_cost_nanos,
    estimated_cost_currency,
    cost_catalog_version_id,
    total_input_tokens,
    total_output_tokens,
    input_text_tokens,
    output_text_tokens,
    input_image_tokens,
    output_image_tokens,
    cache_read_tokens,
    cache_write_tokens,
    reasoning_tokens,
    total_tokens,
    llm_request_blob_id,
    llm_request_patch_id,
    llm_response_blob_id,
    llm_response_capture_state,
    created_at,
    updated_at
)
SELECT
    id,
    request_log_id,
    attempt_index,
    candidate_position,
    provider_id,
    provider_api_key_id,
    model_id,
    provider_key_snapshot,
    provider_name_snapshot,
    model_name_snapshot,
    real_model_name_snapshot,
    llm_api_type,
    attempt_status,
    scheduler_action,
    error_code,
    error_message,
    request_uri,
    request_headers_json,
    response_headers_json,
    http_status,
    started_at,
    first_byte_at,
    completed_at,
    response_started_to_client,
    backoff_ms,
    applied_request_patch_ids_json,
    request_patch_summary_json,
    estimated_cost_nanos,
    estimated_cost_currency,
    cost_catalog_version_id,
    total_input_tokens,
    total_output_tokens,
    input_text_tokens,
    output_text_tokens,
    input_image_tokens,
    output_image_tokens,
    cache_read_tokens,
    cache_write_tokens,
    reasoning_tokens,
    total_tokens,
    llm_request_blob_id,
    llm_request_patch_id,
    llm_response_blob_id,
    llm_response_capture_state,
    created_at,
    updated_at
FROM request_attempt_legacy_source;

DROP TABLE request_attempt_legacy_source;
