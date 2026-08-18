-- R3.8 keeps request-log, ingest-marker, and rollup history. The old
-- response_started column is renamed to its lossless first-response-body
-- meaning; all newly observable stages start as NULL for existing rows.

ALTER TABLE request_log
    RENAME COLUMN response_started_to_client_at TO first_response_body_at;

ALTER TABLE request_log
    ADD COLUMN upstream_response_headers_at BIGINT NULL,
    ADD COLUMN upstream_first_body_chunk_at BIGINT NULL,
    ADD COLUMN first_token_at BIGINT NULL,
    ADD COLUMN max_upstream_response_idle_ms BIGINT NULL;

ALTER TABLE request_log
    DROP CONSTRAINT IF EXISTS chk_request_log_timestamps_order,
    ADD CONSTRAINT chk_request_log_timing_contract CHECK (
        updated_at >= created_at
        AND (upstream_request_sent_at IS NULL OR upstream_request_sent_at >= request_received_at)
        AND (
            upstream_response_headers_at IS NULL
            OR (
                upstream_request_sent_at IS NOT NULL
                AND upstream_response_headers_at >= upstream_request_sent_at
            )
        )
        AND (
            upstream_first_body_chunk_at IS NULL
            OR (
                upstream_response_headers_at IS NOT NULL
                AND upstream_first_body_chunk_at >= upstream_response_headers_at
            )
        )
        AND (
            first_response_body_at IS NULL
            OR (
                upstream_request_sent_at IS NOT NULL
                AND first_response_body_at >= upstream_request_sent_at
            )
        )
        AND (
            first_response_body_at IS NULL
            OR upstream_first_body_chunk_at IS NULL
            OR first_response_body_at >= upstream_first_body_chunk_at
        )
        AND (
            first_token_at IS NULL
            OR (
                is_stream = TRUE
                AND upstream_first_body_chunk_at IS NOT NULL
                AND first_token_at >= upstream_first_body_chunk_at
            )
        )
        AND (max_upstream_response_idle_ms IS NULL OR max_upstream_response_idle_ms >= 0)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
        AND (
            completed_at IS NULL
            OR upstream_request_sent_at IS NULL
            OR completed_at >= upstream_request_sent_at
        )
        AND (
            completed_at IS NULL
            OR upstream_response_headers_at IS NULL
            OR completed_at >= upstream_response_headers_at
        )
        AND (
            completed_at IS NULL
            OR upstream_first_body_chunk_at IS NULL
            OR completed_at >= upstream_first_body_chunk_at
        )
        AND (
            completed_at IS NULL
            OR first_response_body_at IS NULL
            OR completed_at >= first_response_body_at
        )
        AND (
            completed_at IS NULL
            OR first_token_at IS NULL
            OR completed_at >= first_token_at
        )
    );

-- Preserve historical rollups while replacing the retired latency names.
-- TTFT has no lossless historical source and therefore starts at 0.
ALTER TABLE metric_request_rollup_minute
    RENAME COLUMN first_byte_latency_sum_ms TO time_to_first_response_body_sum_ms;

ALTER TABLE metric_request_rollup_minute
    RENAME COLUMN first_byte_latency_count TO time_to_first_response_body_count;

ALTER TABLE metric_request_rollup_minute
    ADD COLUMN ttft_sum_ms BIGINT NOT NULL DEFAULT 0,
    ADD COLUMN ttft_count BIGINT NOT NULL DEFAULT 0;

ALTER TABLE metric_request_rollup_minute
    ADD CONSTRAINT chk_metric_request_rollup_latency_samples CHECK (
        time_to_first_response_body_sum_ms >= 0
        AND time_to_first_response_body_count >= 0
        AND ttft_sum_ms >= 0
        AND ttft_count >= 0
        AND total_latency_sum_ms >= 0
        AND total_latency_count >= 0
    );
