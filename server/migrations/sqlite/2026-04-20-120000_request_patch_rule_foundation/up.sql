DROP TABLE IF EXISTS model_custom_field_assignment;
DROP TABLE IF EXISTS provider_custom_field_assignment;
DROP TABLE IF EXISTS custom_field_definition;

CREATE TABLE IF NOT EXISTS request_patch_rule (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT DEFAULT NULL,
    model_id BIGINT DEFAULT NULL,
    placement TEXT NOT NULL,
    target TEXT NOT NULL,
    operation TEXT NOT NULL,
    value_json TEXT DEFAULT NULL,
    description TEXT DEFAULT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT fk_request_patch_rule_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_patch_rule_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_patch_rule_scope_xor CHECK (
        (provider_id IS NOT NULL AND model_id IS NULL)
            OR
        (provider_id IS NULL AND model_id IS NOT NULL)
    ),
    CONSTRAINT chk_request_patch_rule_placement CHECK (
        placement IN ('HEADER', 'QUERY', 'BODY')
    ),
    CONSTRAINT chk_request_patch_rule_operation CHECK (
        operation IN ('SET', 'REMOVE')
    ),
    CONSTRAINT chk_request_patch_rule_target_not_empty CHECK (target <> ''),
    CONSTRAINT chk_request_patch_rule_value_shape CHECK (
        (operation = 'SET' AND value_json IS NOT NULL AND json_valid(value_json))
            OR
        (operation = 'REMOVE' AND value_json IS NULL)
    ),
    CONSTRAINT chk_request_patch_rule_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_request_patch_rule_provider_id
    ON request_patch_rule(provider_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_request_patch_rule_model_id
    ON request_patch_rule(model_id)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_patch_rule_provider_identity_active
    ON request_patch_rule(provider_id, placement, target)
    WHERE deleted_at IS NULL AND is_enabled = true AND provider_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_patch_rule_model_identity_active
    ON request_patch_rule(model_id, placement, target)
    WHERE deleted_at IS NULL AND is_enabled = true AND model_id IS NOT NULL;

ALTER TABLE request_log
    ADD COLUMN applied_request_patch_ids_json TEXT;

ALTER TABLE request_log
    ADD COLUMN request_patch_summary_json TEXT;
