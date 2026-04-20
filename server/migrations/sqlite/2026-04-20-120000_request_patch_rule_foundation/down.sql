ALTER TABLE request_log
    DROP COLUMN request_patch_summary_json;

ALTER TABLE request_log
    DROP COLUMN applied_request_patch_ids_json;

DROP INDEX IF EXISTS idx_request_patch_rule_model_identity_active;
DROP INDEX IF EXISTS idx_request_patch_rule_provider_identity_active;
DROP INDEX IF EXISTS idx_request_patch_rule_model_id;
DROP INDEX IF EXISTS idx_request_patch_rule_provider_id;
DROP TABLE IF EXISTS request_patch_rule;

CREATE TABLE IF NOT EXISTS custom_field_definition (
    id BIGINT PRIMARY KEY NOT NULL,
    definition_name TEXT,
    definition_description TEXT,
    field_name TEXT NOT NULL,
    field_placement TEXT NOT NULL,
    field_type TEXT NOT NULL,
    string_value TEXT,
    integer_value BIGINT,
    number_value REAL,
    boolean_value BOOLEAN,
    is_definition_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT chk_cfd_field_name_not_empty CHECK (field_name <> ''),
    CONSTRAINT chk_cfd_field_placement CHECK (field_placement IN ('HEADER', 'QUERY', 'BODY')),
    CONSTRAINT chk_cfd_field_type CHECK (
        field_type IN ('STRING', 'INTEGER', 'NUMBER', 'BOOLEAN', 'JSON_STRING', 'UNSET')
    ),
    CONSTRAINT chk_cfd_value_type_coherence CHECK (
        (field_type = 'STRING' AND string_value IS NOT NULL AND integer_value IS NULL AND number_value IS NULL AND boolean_value IS NULL)
            OR
        (field_type = 'INTEGER' AND string_value IS NULL AND integer_value IS NOT NULL AND number_value IS NULL AND boolean_value IS NULL)
            OR
        (field_type = 'NUMBER' AND string_value IS NULL AND integer_value IS NULL AND number_value IS NOT NULL AND boolean_value IS NULL)
            OR
        (field_type = 'BOOLEAN' AND string_value IS NULL AND integer_value IS NULL AND number_value IS NULL AND boolean_value IS NOT NULL)
            OR
        (field_type = 'JSON_STRING' AND string_value IS NOT NULL AND integer_value IS NULL AND number_value IS NULL AND boolean_value IS NULL)
            OR
        (field_type = 'UNSET' AND string_value IS NULL AND integer_value IS NULL AND number_value IS NULL AND boolean_value IS NULL)
    ),
    CONSTRAINT chk_cfd_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cfd_content_uq_not_deleted
    ON custom_field_definition (
        field_name,
        field_placement,
        field_type,
        string_value,
        integer_value,
        number_value,
        boolean_value
    )
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_cfd_definition_name
    ON custom_field_definition(definition_name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_cfd_is_definition_enabled
    ON custom_field_definition(is_definition_enabled);

CREATE INDEX IF NOT EXISTS idx_cfd_deleted_at
    ON custom_field_definition(deleted_at);

CREATE TABLE IF NOT EXISTS provider_custom_field_assignment (
    provider_id BIGINT NOT NULL,
    custom_field_definition_id BIGINT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    PRIMARY KEY (provider_id, custom_field_definition_id),
    CONSTRAINT fk_pcfa_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_pcfa_definition_id
        FOREIGN KEY (custom_field_definition_id) REFERENCES custom_field_definition(id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT chk_pcfa_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_pcfa_provider_id_is_enabled
    ON provider_custom_field_assignment(provider_id, is_enabled);

CREATE INDEX IF NOT EXISTS idx_pcfa_definition_id
    ON provider_custom_field_assignment(custom_field_definition_id);

CREATE TABLE IF NOT EXISTS model_custom_field_assignment (
    model_id BIGINT NOT NULL,
    custom_field_definition_id BIGINT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    PRIMARY KEY (model_id, custom_field_definition_id),
    CONSTRAINT fk_mcfa_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_mcfa_definition_id
        FOREIGN KEY (custom_field_definition_id) REFERENCES custom_field_definition(id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT chk_mcfa_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_mcfa_model_id_is_enabled
    ON model_custom_field_assignment(model_id, is_enabled);

CREATE INDEX IF NOT EXISTS idx_mcfa_definition_id
    ON model_custom_field_assignment(custom_field_definition_id);
