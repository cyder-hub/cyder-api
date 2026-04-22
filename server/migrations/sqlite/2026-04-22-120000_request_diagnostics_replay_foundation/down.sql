DROP INDEX IF EXISTS idx_request_replay_run_created_at;
DROP INDEX IF EXISTS idx_request_replay_run_status;
DROP INDEX IF EXISTS idx_request_replay_run_source_attempt_id;
DROP INDEX IF EXISTS idx_request_replay_run_source_request_log_id;

DROP TABLE IF EXISTS request_replay_run;

PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS idx_request_log_estimated_cost_nanos;
DROP INDEX IF EXISTS idx_request_log_total_tokens;
DROP INDEX IF EXISTS idx_request_log_fallback_count;
DROP INDEX IF EXISTS idx_request_log_retry_count;
DROP INDEX IF EXISTS idx_request_log_final_error_code;
DROP INDEX IF EXISTS idx_request_log_resolved_name_scope;
DROP INDEX IF EXISTS idx_request_log_has_transform_diagnostics;

CREATE TABLE request_log_old (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT,
    resolved_name_scope TEXT,
    resolved_route_id BIGINT,
    resolved_route_name TEXT,
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
    overall_status TEXT NOT NULL,
    final_error_code TEXT,
    final_error_message TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    fallback_count INTEGER NOT NULL DEFAULT 0,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    cost_catalog_id BIGINT,
    cost_catalog_version_id BIGINT,
    cost_snapshot_json TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
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
    user_api_type TEXT NOT NULL,
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

INSERT INTO request_log_old (
    id,
    api_key_id,
    requested_model_name,
    resolved_name_scope,
    resolved_route_id,
    resolved_route_name,
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
    overall_status,
    final_error_code,
    final_error_message,
    attempt_count,
    retry_count,
    fallback_count,
    estimated_cost_nanos,
    estimated_cost_currency,
    cost_catalog_id,
    cost_catalog_version_id,
    cost_snapshot_json,
    created_at,
    updated_at,
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
    user_api_type
)
SELECT
    id,
    api_key_id,
    requested_model_name,
    resolved_name_scope,
    resolved_route_id,
    resolved_route_name,
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
    overall_status,
    final_error_code,
    final_error_message,
    attempt_count,
    retry_count,
    fallback_count,
    estimated_cost_nanos,
    estimated_cost_currency,
    cost_catalog_id,
    cost_catalog_version_id,
    cost_snapshot_json,
    created_at,
    updated_at,
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
    user_api_type
FROM request_log;

DROP TABLE request_log;
ALTER TABLE request_log_old RENAME TO request_log;

PRAGMA foreign_keys = ON;
