PRAGMA foreign_keys = OFF;

CREATE TABLE IF NOT EXISTS reasoning_profile (
    id BIGINT PRIMARY KEY NOT NULL,
    profile_key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    family_key TEXT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_reasoning_profile_key_not_empty CHECK (profile_key <> ''),
    CONSTRAINT chk_reasoning_profile_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_reasoning_profile_family_key_not_empty CHECK (family_key <> ''),
    CONSTRAINT chk_reasoning_profile_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reasoning_profile_key_uq_active
    ON reasoning_profile (profile_key)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_reasoning_profile_enabled
    ON reasoning_profile (is_enabled)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS reasoning_profile_preset (
    id BIGINT PRIMARY KEY NOT NULL,
    profile_id BIGINT NOT NULL,
    preset_key TEXT NOT NULL,
    expose_in_models BOOLEAN NOT NULL DEFAULT true,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_reasoning_profile_preset_profile_id
        FOREIGN KEY (profile_id) REFERENCES reasoning_profile (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_reasoning_profile_preset_key_not_empty CHECK (preset_key <> ''),
    CONSTRAINT chk_reasoning_profile_preset_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reasoning_profile_preset_uq_active
    ON reasoning_profile_preset (profile_id, preset_key)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_reasoning_profile_preset_profile_id
    ON reasoning_profile_preset (profile_id)
    WHERE deleted_at IS NULL;

ALTER TABLE provider
    ADD COLUMN default_reasoning_profile_id BIGINT DEFAULT NULL
        REFERENCES reasoning_profile (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE;

CREATE INDEX IF NOT EXISTS idx_provider_default_reasoning_profile_id
    ON provider (default_reasoning_profile_id)
    WHERE deleted_at IS NULL AND default_reasoning_profile_id IS NOT NULL;

ALTER TABLE model
    ADD COLUMN reasoning_profile_override_id BIGINT DEFAULT NULL
        REFERENCES reasoning_profile (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE;

CREATE INDEX IF NOT EXISTS idx_model_reasoning_profile_override_id
    ON model (reasoning_profile_override_id)
    WHERE deleted_at IS NULL AND reasoning_profile_override_id IS NOT NULL;

PRAGMA foreign_keys = ON;
