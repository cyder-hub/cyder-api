-- R3.9 is an intentionally destructive pre-1.0 ownership cutover.
-- Provider execution-domain data and request/minute-metric history are cleared;
-- manager identity/session, downstream API keys and their daily/monthly rollups,
-- and cost catalogs remain untouched. This migration has no down migration.

DELETE FROM metric_ingested_request_log;
DELETE FROM metric_request_rollup_minute;
DELETE FROM metric_http_status_rollup_minute;
DELETE FROM metric_cost_rollup_minute;
DELETE FROM request_log;
DELETE FROM reasoning_config_preset;
DELETE FROM reasoning_config;
DELETE FROM runtime_feature_config;
DELETE FROM request_patch_rule;
-- RuleScope has only PROVIDER and MODEL; no global ACL rows exist to preserve.
DELETE FROM api_key_acl_rule;
DELETE FROM model;
DELETE FROM provider_api_key;
DELETE FROM provider;

ALTER TYPE provider_type_enum RENAME TO upstream_profile_type_enum;

ALTER TABLE provider
    DROP COLUMN endpoint,
    DROP COLUMN use_proxy,
    DROP COLUMN provider_type;

CREATE TABLE upstream_source (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NOT NULL,
    source_key TEXT NOT NULL,
    profile_type upstream_profile_type_enum NOT NULL,
    endpoint TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT FALSE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_upstream_source_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_upstream_source_key CHECK (source_key = 'primary'),
    CONSTRAINT chk_upstream_source_endpoint_not_empty CHECK (endpoint <> ''),
    CONSTRAINT chk_upstream_source_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX idx_upstream_source_provider_active_unique
    ON upstream_source (provider_id)
    WHERE deleted_at IS NULL;

CREATE INDEX idx_upstream_source_provider_id
    ON upstream_source (provider_id);

ALTER TABLE request_log
    ADD COLUMN source_id BIGINT NULL,
    ADD COLUMN source_key_snapshot TEXT NULL,
    ADD COLUMN source_profile_type_snapshot upstream_profile_type_enum NULL,
    ADD COLUMN source_endpoint_snapshot TEXT NULL,
    ADD CONSTRAINT fk_request_log_source_id
        FOREIGN KEY (source_id) REFERENCES upstream_source(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    ADD CONSTRAINT chk_request_log_source_snapshot CHECK (
        (
            source_id IS NULL
            AND source_key_snapshot IS NULL
            AND source_profile_type_snapshot IS NULL
            AND source_endpoint_snapshot IS NULL
        )
        OR (
            source_key_snapshot = 'primary'
            AND source_profile_type_snapshot IS NOT NULL
            AND source_endpoint_snapshot <> ''
        )
    );

CREATE INDEX idx_request_log_source_received_at
    ON request_log (source_id, request_received_at DESC);
