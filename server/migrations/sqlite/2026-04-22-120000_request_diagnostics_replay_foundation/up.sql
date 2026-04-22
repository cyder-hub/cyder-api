ALTER TABLE request_log
    ADD COLUMN has_transform_diagnostics BOOLEAN NOT NULL DEFAULT 0;

ALTER TABLE request_log
    ADD COLUMN transform_diagnostic_count INTEGER NOT NULL DEFAULT 0;

ALTER TABLE request_log
    ADD COLUMN transform_diagnostic_max_loss_level TEXT;

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

CREATE TABLE request_replay_run (
    id BIGINT PRIMARY KEY,
    source_request_log_id BIGINT NOT NULL,
    source_attempt_id BIGINT,
    replay_kind TEXT NOT NULL,
    replay_mode TEXT NOT NULL,
    semantic_basis TEXT NOT NULL,
    status TEXT NOT NULL,
    executed_route_id BIGINT,
    executed_route_name TEXT,
    executed_provider_id BIGINT,
    executed_provider_api_key_id BIGINT,
    executed_model_id BIGINT,
    executed_llm_api_type TEXT,
    downstream_request_uri TEXT,
    http_status INTEGER,
    error_code TEXT,
    error_message TEXT,
    total_input_tokens INTEGER,
    total_output_tokens INTEGER,
    reasoning_tokens INTEGER,
    total_tokens INTEGER,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    diff_summary_json TEXT,
    artifact_version INTEGER,
    artifact_storage_type TEXT,
    artifact_storage_key TEXT,
    started_at BIGINT,
    first_byte_at BIGINT,
    completed_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_replay_run_source_request_log_id
        FOREIGN KEY (source_request_log_id) REFERENCES request_log(id)
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_request_replay_run_source_attempt_id
        FOREIGN KEY (source_attempt_id) REFERENCES request_attempt(id)
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_request_replay_run_kind
        CHECK (replay_kind IN ('ATTEMPT_UPSTREAM', 'GATEWAY_REQUEST')),
    CONSTRAINT chk_request_replay_run_mode
        CHECK (replay_mode IN ('DRY_RUN', 'LIVE')),
    CONSTRAINT chk_request_replay_run_semantic_basis
        CHECK (semantic_basis IN (
            'HISTORICAL_ATTEMPT_SNAPSHOT',
            'HISTORICAL_REQUEST_SNAPSHOT_WITH_CURRENT_CONFIG'
        )),
    CONSTRAINT chk_request_replay_run_status
        CHECK (status IN ('PENDING', 'RUNNING', 'SUCCESS', 'ERROR', 'CANCELLED', 'REJECTED')),
    CONSTRAINT chk_request_replay_run_source_attempt
        CHECK (
            (replay_kind = 'ATTEMPT_UPSTREAM' AND source_attempt_id IS NOT NULL)
            OR (replay_kind = 'GATEWAY_REQUEST' AND source_attempt_id IS NULL)
        ),
    CONSTRAINT chk_request_replay_run_executed_llm_api_type
        CHECK (
            executed_llm_api_type IS NULL
            OR executed_llm_api_type IN (
                'OPENAI',
                'GEMINI',
                'OLLAMA',
                'ANTHROPIC',
                'RESPONSES',
                'GEMINI_OPENAI'
            )
        ),
    CONSTRAINT chk_request_replay_run_artifact_storage_type
        CHECK (artifact_storage_type IS NULL OR artifact_storage_type IN ('FILE_SYSTEM', 'S3')),
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
