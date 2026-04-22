DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'request_attempt_status_enum'
    ) THEN
        CREATE TYPE request_attempt_status_enum AS ENUM (
            'SKIPPED',
            'SUCCESS',
            'ERROR',
            'CANCELLED'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'scheduler_action_enum'
    ) THEN
        CREATE TYPE scheduler_action_enum AS ENUM (
            'RETURN_SUCCESS',
            'FAIL_FAST',
            'RETRY_SAME_CANDIDATE',
            'FALLBACK_NEXT_CANDIDATE'
        );
    END IF;
END
$$;

ALTER TABLE model ADD COLUMN supports_streaming BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE model ADD COLUMN supports_tools BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE model ADD COLUMN supports_reasoning BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE model ADD COLUMN supports_image_input BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE model ADD COLUMN supports_embeddings BOOLEAN NOT NULL DEFAULT TRUE;
ALTER TABLE model ADD COLUMN supports_rerank BOOLEAN NOT NULL DEFAULT TRUE;

CREATE TABLE request_log_new (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT NULL,
    resolved_name_scope TEXT NULL,
    resolved_route_id BIGINT NULL,
    resolved_route_name TEXT NULL,
    user_api_type llm_api_type_enum NOT NULL,
    overall_status request_status_enum NOT NULL,
    final_error_code TEXT NULL,
    final_error_message TEXT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    fallback_count INTEGER NOT NULL DEFAULT 0,
    request_received_at BIGINT NOT NULL,
    first_attempt_started_at BIGINT NULL,
    response_started_to_client_at BIGINT NULL,
    completed_at BIGINT NULL,
    client_ip TEXT NULL,
    final_attempt_id BIGINT NULL,
    final_provider_id BIGINT NULL,
    final_provider_api_key_id BIGINT NULL,
    final_model_id BIGINT NULL,
    final_provider_key_snapshot TEXT NULL,
    final_provider_name_snapshot TEXT NULL,
    final_model_name_snapshot TEXT NULL,
    final_real_model_name_snapshot TEXT NULL,
    final_llm_api_type llm_api_type_enum NULL,
    estimated_cost_nanos BIGINT NULL,
    estimated_cost_currency TEXT NULL,
    cost_catalog_id BIGINT NULL,
    cost_catalog_version_id BIGINT NULL,
    cost_snapshot_json TEXT NULL,
    total_input_tokens INTEGER NULL,
    total_output_tokens INTEGER NULL,
    input_text_tokens INTEGER NULL,
    output_text_tokens INTEGER NULL,
    input_image_tokens INTEGER NULL,
    output_image_tokens INTEGER NULL,
    cache_read_tokens INTEGER NULL,
    cache_write_tokens INTEGER NULL,
    reasoning_tokens INTEGER NULL,
    total_tokens INTEGER NULL,
    bundle_version INTEGER NULL,
    bundle_storage_type storage_type_enum NULL,
    bundle_storage_key TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_log_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
        ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_id
        FOREIGN KEY (final_provider_id) REFERENCES provider(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_api_key_id
        FOREIGN KEY (final_provider_api_key_id) REFERENCES provider_api_key(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_model_id
        FOREIGN KEY (final_model_id) REFERENCES model(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_bundle_version
        CHECK (bundle_version IS NULL OR bundle_version IN (1, 2)),
    CONSTRAINT chk_request_log_attempt_counts_non_negative
        CHECK (attempt_count >= 0 AND retry_count >= 0 AND fallback_count >= 0),
    CONSTRAINT chk_request_log_tokens_non_negative
        CHECK (
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
    1::INTEGER AS attempt_index,
    1::INTEGER AS candidate_position,
    rl.provider_id,
    rl.provider_api_key_id,
    rl.model_id,
    p.provider_key AS provider_key_snapshot,
    p.name AS provider_name_snapshot,
    rl.model_name AS model_name_snapshot,
    rl.real_model_name AS real_model_name_snapshot,
    rl.llm_api_type,
    CASE
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN 'SUCCESS'::request_attempt_status_enum
        WHEN rl.status = 'CANCELLED'::request_status_enum THEN 'CANCELLED'::request_attempt_status_enum
        ELSE 'ERROR'::request_attempt_status_enum
    END AS attempt_status,
    CASE
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN 'RETURN_SUCCESS'::scheduler_action_enum
        ELSE 'FAIL_FAST'::scheduler_action_enum
    END AS scheduler_action,
    CASE
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN NULL
        WHEN rl.status = 'CANCELLED'::request_status_enum THEN 'client_cancelled_error'
        WHEN rl.status = 'PENDING'::request_status_enum THEN 'legacy_pending_request_log_error'
        ELSE 'legacy_request_log_error'
    END AS error_code,
    CASE
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN NULL
        WHEN rl.status = 'CANCELLED'::request_status_enum THEN 'migrated from legacy request_log: cancelled'
        WHEN rl.status = 'PENDING'::request_status_enum THEN 'migrated from legacy request_log: pending state is not valid in final log model'
        ELSE 'migrated from legacy request_log: original error classification unavailable'
    END AS error_message,
    rl.llm_request_uri AS request_uri,
    NULL::TEXT AS request_headers_json,
    NULL::TEXT AS response_headers_json,
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
    (rl.llm_response_first_chunk_at IS NOT NULL) AS response_started_to_client,
    NULL::INTEGER AS backoff_ms,
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
    NULL::INTEGER AS llm_request_blob_id,
    NULL::INTEGER AS llm_request_patch_id,
    NULL::INTEGER AS llm_response_blob_id,
    NULL::TEXT AS llm_response_capture_state,
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
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN 'SUCCESS'::request_status_enum
        WHEN rl.status = 'CANCELLED'::request_status_enum THEN 'CANCELLED'::request_status_enum
        ELSE 'ERROR'::request_status_enum
    END,
    CASE
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN NULL
        WHEN rl.status = 'CANCELLED'::request_status_enum THEN 'client_cancelled_error'
        WHEN rl.status = 'PENDING'::request_status_enum THEN 'legacy_pending_request_log_error'
        ELSE 'legacy_request_log_error'
    END,
    CASE
        WHEN rl.status = 'SUCCESS'::request_status_enum THEN NULL
        WHEN rl.status = 'CANCELLED'::request_status_enum THEN 'migrated from legacy request_log: cancelled'
        WHEN rl.status = 'PENDING'::request_status_enum THEN 'migrated from legacy request_log: pending state is not valid in final log model'
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
        WHEN rl.storage_type = 'FILE_SYSTEM'::storage_type_enum THEN
            to_char(to_timestamp(rl.created_at / 1000.0), 'YYYY/MM/DD')
            || '/'
            || CASE
                WHEN length(rl.id::text) >= 6
                    THEN substr(rl.id::text, length(rl.id::text) - 5, 2) || '/'
                ELSE ''
            END
            || rl.id::text
            || '.mp.gz'
        WHEN rl.storage_type = 'S3'::storage_type_enum THEN
            'logs/'
            || to_char(to_timestamp(rl.created_at / 1000.0), 'YYYY/MM/DD')
            || '/'
            || rl.id::text
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
    id BIGINT PRIMARY KEY,
    request_log_id BIGINT NOT NULL,
    attempt_index INTEGER NOT NULL,
    candidate_position INTEGER NOT NULL,
    provider_id BIGINT NULL,
    provider_api_key_id BIGINT NULL,
    model_id BIGINT NULL,
    provider_key_snapshot TEXT NULL,
    provider_name_snapshot TEXT NULL,
    model_name_snapshot TEXT NULL,
    real_model_name_snapshot TEXT NULL,
    llm_api_type llm_api_type_enum NULL,
    attempt_status request_attempt_status_enum NOT NULL,
    scheduler_action scheduler_action_enum NOT NULL,
    error_code TEXT NULL,
    error_message TEXT NULL,
    request_uri TEXT NULL,
    request_headers_json TEXT NULL,
    response_headers_json TEXT NULL,
    http_status INTEGER NULL,
    started_at BIGINT NULL,
    first_byte_at BIGINT NULL,
    completed_at BIGINT NULL,
    response_started_to_client BOOLEAN NOT NULL DEFAULT FALSE,
    backoff_ms INTEGER NULL,
    applied_request_patch_ids_json TEXT NULL,
    request_patch_summary_json TEXT NULL,
    estimated_cost_nanos BIGINT NULL,
    estimated_cost_currency TEXT NULL,
    cost_catalog_version_id BIGINT NULL,
    total_input_tokens INTEGER NULL,
    total_output_tokens INTEGER NULL,
    input_text_tokens INTEGER NULL,
    output_text_tokens INTEGER NULL,
    input_image_tokens INTEGER NULL,
    output_image_tokens INTEGER NULL,
    cache_read_tokens INTEGER NULL,
    cache_write_tokens INTEGER NULL,
    reasoning_tokens INTEGER NULL,
    total_tokens INTEGER NULL,
    llm_request_blob_id INTEGER NULL,
    llm_request_patch_id INTEGER NULL,
    llm_response_blob_id INTEGER NULL,
    llm_response_capture_state TEXT NULL,
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
    CONSTRAINT chk_request_attempt_http_status
        CHECK (http_status IS NULL OR (http_status >= 100 AND http_status <= 599)),
    CONSTRAINT chk_request_attempt_backoff_ms_non_negative
        CHECK (backoff_ms IS NULL OR backoff_ms >= 0),
    CONSTRAINT chk_request_attempt_tokens_non_negative
        CHECK (
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
    CONSTRAINT chk_request_attempt_bundle_ids_non_negative
        CHECK (
            (llm_request_blob_id IS NULL OR llm_request_blob_id >= 0)
            AND (llm_request_patch_id IS NULL OR llm_request_patch_id >= 0)
            AND (llm_response_blob_id IS NULL OR llm_response_blob_id >= 0)
        ),
    CONSTRAINT chk_request_attempt_timestamps_order
        CHECK (
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
