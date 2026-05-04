CREATE TABLE metric_ingested_request_log (
    request_log_id BIGINT PRIMARY KEY,
    request_received_at BIGINT NOT NULL,
    completed_at BIGINT NULL,
    ingested_at BIGINT NOT NULL
);

CREATE INDEX idx_metric_ingested_request_log_received_at
    ON metric_ingested_request_log (request_received_at);

CREATE TABLE metric_request_rollup_minute (
    bucket_start_ms BIGINT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    scope_label TEXT NULL,
    request_count BIGINT NOT NULL,
    success_count BIGINT NOT NULL,
    error_count BIGINT NOT NULL,
    cancelled_count BIGINT NOT NULL,
    retry_count BIGINT NOT NULL,
    fallback_count BIGINT NOT NULL,
    first_byte_latency_sum_ms BIGINT NOT NULL,
    first_byte_latency_count BIGINT NOT NULL,
    total_latency_sum_ms BIGINT NOT NULL,
    total_latency_count BIGINT NOT NULL,
    input_tokens BIGINT NOT NULL,
    output_tokens BIGINT NOT NULL,
    reasoning_tokens BIGINT NOT NULL,
    total_tokens BIGINT NOT NULL,
    transform_diagnostic_count BIGINT NOT NULL,
    transform_diagnostic_lossy_major_count BIGINT NOT NULL,
    transform_diagnostic_reject_count BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (bucket_start_ms, scope_type, scope_id)
);

CREATE INDEX idx_metric_request_rollup_scope_time
    ON metric_request_rollup_minute (scope_type, scope_id, bucket_start_ms);

CREATE TABLE metric_attempt_rollup_minute (
    bucket_start_ms BIGINT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    scope_label TEXT NULL,
    attempt_count BIGINT NOT NULL,
    success_count BIGINT NOT NULL,
    error_count BIGINT NOT NULL,
    skipped_count BIGINT NOT NULL,
    retry_same_candidate_count BIGINT NOT NULL,
    fallback_next_candidate_count BIGINT NOT NULL,
    fail_fast_count BIGINT NOT NULL,
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

CREATE INDEX idx_metric_attempt_rollup_scope_time
    ON metric_attempt_rollup_minute (scope_type, scope_id, bucket_start_ms);

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

CREATE INDEX idx_metric_http_status_rollup_scope_time
    ON metric_http_status_rollup_minute (scope_type, scope_id, bucket_start_ms);

CREATE TABLE metric_cost_rollup_minute (
    bucket_start_ms BIGINT NOT NULL,
    metric_kind TEXT NOT NULL,
    scope_type TEXT NOT NULL,
    scope_id TEXT NOT NULL,
    currency TEXT NOT NULL,
    amount_nanos BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (bucket_start_ms, metric_kind, scope_type, scope_id, currency)
);

CREATE INDEX idx_metric_cost_rollup_scope_time
    ON metric_cost_rollup_minute (metric_kind, scope_type, scope_id, bucket_start_ms);
