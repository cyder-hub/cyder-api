DROP INDEX IF EXISTS idx_request_replay_run_created_at;
DROP INDEX IF EXISTS idx_request_replay_run_status;
DROP INDEX IF EXISTS idx_request_replay_run_source_attempt_id;
DROP INDEX IF EXISTS idx_request_replay_run_source_request_log_id;

DROP TABLE IF EXISTS request_replay_run;

DROP TYPE IF EXISTS request_replay_status_enum;
DROP TYPE IF EXISTS request_replay_semantic_basis_enum;
DROP TYPE IF EXISTS request_replay_mode_enum;
DROP TYPE IF EXISTS request_replay_kind_enum;

DROP INDEX IF EXISTS idx_request_log_estimated_cost_nanos;
DROP INDEX IF EXISTS idx_request_log_total_tokens;
DROP INDEX IF EXISTS idx_request_log_fallback_count;
DROP INDEX IF EXISTS idx_request_log_retry_count;
DROP INDEX IF EXISTS idx_request_log_final_error_code;
DROP INDEX IF EXISTS idx_request_log_resolved_name_scope;
DROP INDEX IF EXISTS idx_request_log_has_transform_diagnostics;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS transform_diagnostic_max_loss_level;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS transform_diagnostic_count;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS has_transform_diagnostics;
