ALTER TABLE request_log
    ADD COLUMN has_transform_diagnostics BOOLEAN NOT NULL DEFAULT FALSE;

ALTER TABLE request_log
    ADD COLUMN transform_diagnostic_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE request_log
    ADD COLUMN transform_diagnostic_max_loss_level TEXT NULL;

CREATE INDEX IF NOT EXISTS idx_request_log_has_transform_diagnostics
    ON request_log (has_transform_diagnostics);

CREATE INDEX IF NOT EXISTS idx_request_log_resolved_name_scope
    ON request_log (resolved_name_scope);

CREATE INDEX IF NOT EXISTS idx_request_log_final_error_code
    ON request_log (final_error_code);

CREATE INDEX IF NOT EXISTS idx_request_log_retry_count
    ON request_log (retry_count);

CREATE INDEX IF NOT EXISTS idx_request_log_fallback_count
    ON request_log (fallback_count);

CREATE INDEX IF NOT EXISTS idx_request_log_total_tokens
    ON request_log (total_tokens);

CREATE INDEX IF NOT EXISTS idx_request_log_estimated_cost_nanos
    ON request_log (estimated_cost_nanos);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'request_replay_kind_enum'
    ) THEN
        CREATE TYPE request_replay_kind_enum AS ENUM (
            'ATTEMPT_UPSTREAM',
            'GATEWAY_REQUEST'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'request_replay_mode_enum'
    ) THEN
        CREATE TYPE request_replay_mode_enum AS ENUM (
            'DRY_RUN',
            'LIVE'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'request_replay_semantic_basis_enum'
    ) THEN
        CREATE TYPE request_replay_semantic_basis_enum AS ENUM (
            'HISTORICAL_ATTEMPT_SNAPSHOT',
            'HISTORICAL_REQUEST_SNAPSHOT_WITH_CURRENT_CONFIG'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_type
        WHERE typname = 'request_replay_status_enum'
    ) THEN
        CREATE TYPE request_replay_status_enum AS ENUM (
            'PENDING',
            'RUNNING',
            'SUCCESS',
            'ERROR',
            'CANCELLED',
            'REJECTED'
        );
    END IF;
END
$$;

CREATE TABLE request_replay_run (
    id BIGINT PRIMARY KEY,
    source_request_log_id BIGINT NOT NULL,
    source_attempt_id BIGINT NULL,
    replay_kind request_replay_kind_enum NOT NULL,
    replay_mode request_replay_mode_enum NOT NULL,
    semantic_basis request_replay_semantic_basis_enum NOT NULL,
    status request_replay_status_enum NOT NULL,
    executed_route_id BIGINT NULL,
    executed_route_name TEXT NULL,
    executed_provider_id BIGINT NULL,
    executed_provider_api_key_id BIGINT NULL,
    executed_model_id BIGINT NULL,
    executed_llm_api_type llm_api_type_enum NULL,
    downstream_request_uri TEXT NULL,
    http_status INTEGER NULL,
    error_code TEXT NULL,
    error_message TEXT NULL,
    total_input_tokens INTEGER NULL,
    total_output_tokens INTEGER NULL,
    reasoning_tokens INTEGER NULL,
    total_tokens INTEGER NULL,
    estimated_cost_nanos BIGINT NULL,
    estimated_cost_currency TEXT NULL,
    diff_summary_json TEXT NULL,
    artifact_version INTEGER NULL,
    artifact_storage_type storage_type_enum NULL,
    artifact_storage_key TEXT NULL,
    started_at BIGINT NULL,
    first_byte_at BIGINT NULL,
    completed_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_replay_run_source_request_log_id
        FOREIGN KEY (source_request_log_id) REFERENCES request_log(id)
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_request_replay_run_source_attempt_id
        FOREIGN KEY (source_attempt_id) REFERENCES request_attempt(id)
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_request_replay_run_source_attempt
        CHECK (
            (replay_kind = 'ATTEMPT_UPSTREAM'::request_replay_kind_enum AND source_attempt_id IS NOT NULL)
            OR (replay_kind = 'GATEWAY_REQUEST'::request_replay_kind_enum AND source_attempt_id IS NULL)
        ),
    CONSTRAINT chk_request_replay_run_artifact_locator
        CHECK (
            (artifact_storage_type IS NULL AND artifact_storage_key IS NULL AND artifact_version IS NULL)
            OR (artifact_storage_type IS NOT NULL AND artifact_storage_key IS NOT NULL AND artifact_version IS NOT NULL)
        ),
    CONSTRAINT chk_request_replay_run_tokens_non_negative
        CHECK (
            (total_input_tokens IS NULL OR total_input_tokens >= 0)
            AND (total_output_tokens IS NULL OR total_output_tokens >= 0)
            AND (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
            AND (total_tokens IS NULL OR total_tokens >= 0)
        ),
    CONSTRAINT chk_request_replay_run_cost_non_negative
        CHECK (estimated_cost_nanos IS NULL OR estimated_cost_nanos >= 0)
);

CREATE INDEX idx_request_replay_run_source_request_log_id
    ON request_replay_run (source_request_log_id);

CREATE INDEX idx_request_replay_run_source_attempt_id
    ON request_replay_run (source_attempt_id);

CREATE INDEX idx_request_replay_run_status
    ON request_replay_run (status);

CREATE INDEX idx_request_replay_run_created_at
    ON request_replay_run (created_at);
