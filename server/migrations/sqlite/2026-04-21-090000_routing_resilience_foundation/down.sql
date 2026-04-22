DROP INDEX IF EXISTS idx_request_attempt_started_at;
DROP INDEX IF EXISTS idx_request_attempt_model_id;
DROP INDEX IF EXISTS idx_request_attempt_provider_id;
DROP INDEX IF EXISTS idx_request_attempt_request_log_id;
DROP TABLE IF EXISTS request_attempt;

CREATE TABLE request_log_old (
    id BIGINT PRIMARY KEY NOT NULL,
    system_api_key_id BIGINT NOT NULL,
    provider_id BIGINT,
    model_id BIGINT,
    provider_api_key_id BIGINT,
    requested_model_name TEXT,
    resolved_name_scope TEXT,
    resolved_route_id BIGINT,
    resolved_route_name TEXT,
    model_name TEXT,
    real_model_name TEXT,
    request_received_at BIGINT NOT NULL,
    llm_request_sent_at BIGINT,
    llm_response_first_chunk_at BIGINT,
    llm_response_completed_at BIGINT,
    client_ip TEXT,
    llm_request_uri TEXT,
    llm_response_status INTEGER,
    status TEXT,
    is_stream BOOLEAN NOT NULL DEFAULT 0,
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
    storage_type TEXT,
    user_request_body TEXT,
    llm_request_body TEXT,
    llm_response_body TEXT,
    user_response_body TEXT,
    applied_request_patch_ids_json TEXT,
    request_patch_summary_json TEXT,
    user_api_type TEXT NOT NULL,
    llm_api_type TEXT NOT NULL,
    CONSTRAINT fk_request_log_system_api_key_id
        FOREIGN KEY (system_api_key_id) REFERENCES system_api_key(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
        ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id
        FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id)
        ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
        (total_input_tokens IS NULL OR total_input_tokens >= 0)
        AND (total_output_tokens IS NULL OR total_output_tokens >= 0)
        AND (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
        AND (total_tokens IS NULL OR total_tokens >= 0)
    ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (updated_at >= created_at),
    CONSTRAINT chk_request_log_status CHECK (
        status IS NULL OR status IN ('PENDING', 'SUCCESS', 'ERROR', 'CANCELLED')
    )
);

INSERT INTO request_log_old (
    id,
    system_api_key_id,
    provider_id,
    model_id,
    provider_api_key_id,
    requested_model_name,
    resolved_name_scope,
    resolved_route_id,
    resolved_route_name,
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
    storage_type,
    user_request_body,
    llm_request_body,
    llm_response_body,
    user_response_body,
    applied_request_patch_ids_json,
    request_patch_summary_json,
    user_api_type,
    llm_api_type
)
SELECT
    rl.id,
    rl.api_key_id,
    rl.final_provider_id,
    rl.final_model_id,
    rl.final_provider_api_key_id,
    rl.requested_model_name,
    rl.resolved_name_scope,
    rl.resolved_route_id,
    rl.resolved_route_name,
    rl.final_model_name_snapshot,
    rl.final_real_model_name_snapshot,
    rl.request_received_at,
    rl.first_attempt_started_at,
    rl.response_started_to_client_at,
    rl.completed_at,
    rl.client_ip,
    ra.request_uri,
    ra.http_status,
    rl.overall_status,
    0,
    rl.estimated_cost_nanos,
    rl.estimated_cost_currency,
    rl.cost_catalog_id,
    rl.cost_catalog_version_id,
    rl.cost_snapshot_json,
    rl.created_at,
    rl.updated_at,
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
    rl.bundle_storage_type,
    NULL,
    NULL,
    NULL,
    NULL,
    ra.applied_request_patch_ids_json,
    ra.request_patch_summary_json,
    rl.user_api_type,
    COALESCE(rl.final_llm_api_type, rl.user_api_type)
FROM request_log AS rl
LEFT JOIN request_attempt AS ra
    ON ra.id = rl.final_attempt_id;

DROP INDEX IF EXISTS idx_request_log_api_key_id;
DROP INDEX IF EXISTS idx_request_log_final_provider_id;
DROP INDEX IF EXISTS idx_request_log_final_model_id;
DROP INDEX IF EXISTS idx_request_log_request_received_at;
DROP INDEX IF EXISTS idx_request_log_overall_status;
DROP INDEX IF EXISTS idx_request_log_resolved_route_id;
DROP INDEX IF EXISTS idx_request_log_final_attempt_id;
DROP INDEX IF EXISTS idx_request_log_cost_catalog_id;
DROP INDEX IF EXISTS idx_request_log_cost_catalog_version_id;
DROP INDEX IF EXISTS idx_request_log_bundle_storage_type;
DROP TABLE request_log;
ALTER TABLE request_log_old RENAME TO request_log;

ALTER TABLE model DROP COLUMN supports_rerank;
ALTER TABLE model DROP COLUMN supports_embeddings;
ALTER TABLE model DROP COLUMN supports_image_input;
ALTER TABLE model DROP COLUMN supports_reasoning;
ALTER TABLE model DROP COLUMN supports_tools;
ALTER TABLE model DROP COLUMN supports_streaming;

CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_id
    ON request_log (cost_catalog_id);
CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_version_id
    ON request_log (cost_catalog_version_id);
CREATE INDEX IF NOT EXISTS idx_request_log_received_at
    ON request_log (request_received_at);
