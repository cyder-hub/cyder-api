-- R3.12 is a pre-1.0 destructive boundary. The old Request Patch,
-- Reasoning Config, and Runtime Feature tables are intentionally deleted;
-- no rows are copied, disabled, or kept as a compatibility source.
-- This migration has no down migration.

PRAGMA foreign_keys = OFF;

DROP TABLE IF EXISTS reasoning_config_preset;
DROP TABLE IF EXISTS reasoning_config;
DROP TABLE IF EXISTS runtime_feature_config;
DROP TABLE IF EXISTS request_patch_rule;

ALTER TABLE request_log
    RENAME COLUMN resolved_reasoning_suffix TO resolved_patch_suffix;
ALTER TABLE request_log
    DROP COLUMN resolved_reasoning_preset;

CREATE TABLE request_patch_variant (
    id BIGINT PRIMARY KEY NOT NULL,
    source_id BIGINT NOT NULL,
    model_id BIGINT,
    suffix TEXT,
    enabled BOOLEAN NOT NULL DEFAULT 1,
    expose_in_models BOOLEAN NOT NULL DEFAULT 0,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_patch_variant_source_id
        FOREIGN KEY (source_id) REFERENCES upstream_source (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_request_patch_variant_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_request_patch_variant_suffix CHECK (
        suffix IS NULL
        OR (
            suffix <> ''
            AND suffix NOT GLOB '-*'
            AND suffix NOT GLOB '*-'
            AND suffix NOT GLOB '*--*'
            AND suffix NOT GLOB '*[^a-z0-9-]*'
        )
    ),
    CONSTRAINT chk_request_patch_variant_flags CHECK (
        enabled IN (0, 1)
        AND expose_in_models IN (0, 1)
        AND (expose_in_models = 0 OR (suffix IS NOT NULL AND enabled = 1))
    ),
    CONSTRAINT chk_request_patch_variant_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX idx_request_patch_variant_source_base_active
    ON request_patch_variant (source_id)
    WHERE deleted_at IS NULL AND model_id IS NULL AND suffix IS NULL;

CREATE UNIQUE INDEX idx_request_patch_variant_source_suffix_active
    ON request_patch_variant (source_id, suffix)
    WHERE deleted_at IS NULL AND model_id IS NULL AND suffix IS NOT NULL;

CREATE UNIQUE INDEX idx_request_patch_variant_model_base_active
    ON request_patch_variant (model_id, source_id)
    WHERE deleted_at IS NULL AND model_id IS NOT NULL AND suffix IS NULL;

CREATE UNIQUE INDEX idx_request_patch_variant_model_suffix_active
    ON request_patch_variant (model_id, source_id, suffix)
    WHERE deleted_at IS NULL AND model_id IS NOT NULL AND suffix IS NOT NULL;

CREATE INDEX idx_request_patch_variant_source_id
    ON request_patch_variant (source_id)
    WHERE deleted_at IS NULL;

CREATE INDEX idx_request_patch_variant_model_source_id
    ON request_patch_variant (model_id, source_id)
    WHERE deleted_at IS NULL;

CREATE TABLE request_patch_rule (
    id BIGINT PRIMARY KEY NOT NULL,
    variant_id BIGINT NOT NULL,
    placement TEXT NOT NULL,
    target TEXT NOT NULL,
    operation TEXT NOT NULL,
    value_json TEXT,
    description TEXT,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_patch_rule_variant_id
        FOREIGN KEY (variant_id) REFERENCES request_patch_variant (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_request_patch_rule_placement CHECK (
        placement IN ('HEADER', 'QUERY', 'BODY')
    ),
    CONSTRAINT chk_request_patch_rule_operation CHECK (
        operation IN ('SET', 'REMOVE')
    ),
    CONSTRAINT chk_request_patch_rule_target_not_empty CHECK (target <> ''),
    CONSTRAINT chk_request_patch_rule_value_shape CHECK (
        (operation = 'SET' AND value_json IS NOT NULL AND json_valid(value_json))
        OR (operation = 'REMOVE' AND value_json IS NULL)
    ),
    CONSTRAINT chk_request_patch_rule_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX idx_request_patch_rule_variant_id
    ON request_patch_rule (variant_id)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX idx_request_patch_rule_variant_identity_active
    ON request_patch_rule (variant_id, placement, target)
    WHERE deleted_at IS NULL;

PRAGMA foreign_key_check;
PRAGMA foreign_keys = ON;
