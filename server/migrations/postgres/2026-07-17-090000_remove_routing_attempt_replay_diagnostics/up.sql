DROP TABLE IF EXISTS request_replay_run;
DROP TABLE IF EXISTS request_attempt;
DROP TABLE IF EXISTS api_key_model_override;
DROP TABLE IF EXISTS model_route_candidate;
DROP TABLE IF EXISTS model_route;

DROP TYPE IF EXISTS request_attempt_status_enum;
DROP TYPE IF EXISTS scheduler_action_enum;
DROP TYPE IF EXISTS request_replay_kind_enum;
DROP TYPE IF EXISTS request_replay_mode_enum;
DROP TYPE IF EXISTS request_replay_semantic_basis_enum;
DROP TYPE IF EXISTS request_replay_status_enum;

ALTER TABLE request_log DROP COLUMN IF EXISTS resolved_name_scope;
ALTER TABLE request_log DROP COLUMN IF EXISTS resolved_route_id;
ALTER TABLE request_log DROP COLUMN IF EXISTS resolved_route_name;
ALTER TABLE request_log DROP COLUMN IF EXISTS attempt_count;
ALTER TABLE request_log DROP COLUMN IF EXISTS retry_count;
ALTER TABLE request_log DROP COLUMN IF EXISTS fallback_count;
ALTER TABLE request_log DROP COLUMN IF EXISTS final_attempt_id;
ALTER TABLE request_log DROP COLUMN IF EXISTS has_transform_diagnostics;
ALTER TABLE request_log DROP COLUMN IF EXISTS transform_diagnostic_count;
ALTER TABLE request_log DROP COLUMN IF EXISTS transform_diagnostic_max_loss_level;
ALTER TABLE request_log DROP COLUMN IF EXISTS bundle_version;
ALTER TABLE request_log DROP COLUMN IF EXISTS bundle_storage_type;
ALTER TABLE request_log DROP COLUMN IF EXISTS bundle_storage_key;

DROP TYPE IF EXISTS storage_type_enum;

ALTER TABLE request_log RENAME COLUMN first_attempt_started_at TO upstream_request_sent_at;
ALTER TABLE request_log RENAME COLUMN final_provider_id TO provider_id;
ALTER TABLE request_log RENAME COLUMN final_provider_api_key_id TO provider_api_key_id;
ALTER TABLE request_log RENAME COLUMN final_model_id TO model_id;
ALTER TABLE request_log RENAME COLUMN final_provider_key_snapshot TO provider_key_snapshot;
ALTER TABLE request_log RENAME COLUMN final_provider_name_snapshot TO provider_name_snapshot;
ALTER TABLE request_log RENAME COLUMN final_model_name_snapshot TO model_name_snapshot;
ALTER TABLE request_log RENAME COLUMN final_real_model_name_snapshot TO real_model_name_snapshot;
ALTER TABLE request_log RENAME COLUMN final_llm_api_type TO llm_api_type;
ALTER TABLE request_log ADD COLUMN upstream_http_status INTEGER;
ALTER TABLE request_log ADD CONSTRAINT chk_request_log_upstream_http_status
    CHECK (upstream_http_status IS NULL OR upstream_http_status BETWEEN 100 AND 599);

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

TRUNCATE TABLE metric_ingested_request_log;
