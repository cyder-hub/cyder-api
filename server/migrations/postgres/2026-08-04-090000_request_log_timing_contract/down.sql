ALTER TABLE request_log
    DROP CONSTRAINT IF EXISTS chk_request_log_timing_contract,
    DROP COLUMN IF EXISTS upstream_response_headers_at,
    DROP COLUMN IF EXISTS upstream_first_body_chunk_at,
    DROP COLUMN IF EXISTS first_token_at,
    DROP COLUMN IF EXISTS max_upstream_response_idle_ms;

ALTER TABLE request_log
    RENAME COLUMN first_response_body_at TO response_started_to_client_at;

ALTER TABLE request_log
    ADD CONSTRAINT chk_request_log_timestamps_order CHECK (
        updated_at >= created_at
        AND (upstream_request_sent_at IS NULL OR upstream_request_sent_at >= request_received_at)
        AND (response_started_to_client_at IS NULL OR response_started_to_client_at >= request_received_at)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
    );

ALTER TABLE metric_request_rollup_minute
    DROP CONSTRAINT IF EXISTS chk_metric_request_rollup_latency_samples,
    DROP COLUMN IF EXISTS ttft_sum_ms,
    DROP COLUMN IF EXISTS ttft_count;

ALTER TABLE metric_request_rollup_minute
    RENAME COLUMN time_to_first_response_body_sum_ms TO first_byte_latency_sum_ms;

ALTER TABLE metric_request_rollup_minute
    RENAME COLUMN time_to_first_response_body_count TO first_byte_latency_count;
