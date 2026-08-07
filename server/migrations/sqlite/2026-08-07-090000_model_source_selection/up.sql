-- R3.11 makes Model Source availability explicit and removes the retired
-- Model-owned upstream capability flags. This migration has no down migration.

PRAGMA foreign_keys = OFF;

DROP INDEX IF EXISTS idx_model_pid_name_uq_active;
DROP INDEX IF EXISTS idx_model_provider_id;
DROP INDEX IF EXISTS idx_model_cost_catalog_id;

CREATE TABLE model_r311 (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    cost_catalog_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    source_selection_mode TEXT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_model_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs (id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_model_name_not_empty CHECK (model_name <> ''),
    CONSTRAINT chk_model_real_model_name_not_empty CHECK (
        real_model_name IS NULL OR real_model_name <> ''
    ),
    CONSTRAINT chk_model_source_selection_mode CHECK (
        source_selection_mode IN ('INHERIT_ALL', 'EXPLICIT')
    ),
    CONSTRAINT chk_model_timestamps CHECK (updated_at >= created_at)
);

INSERT INTO model_r311 (
    id, provider_id, cost_catalog_id, model_name, real_model_name,
    source_selection_mode, is_enabled, deleted_at, created_at, updated_at
)
SELECT
    id, provider_id, cost_catalog_id, model_name, real_model_name,
    'INHERIT_ALL', is_enabled, deleted_at, created_at, updated_at
FROM model;

DROP TABLE model;
ALTER TABLE model_r311 RENAME TO model;

CREATE UNIQUE INDEX idx_model_pid_name_uq_active
    ON model (provider_id, model_name)
    WHERE deleted_at IS NULL AND is_enabled = 1;
CREATE INDEX idx_model_provider_id ON model (provider_id);
CREATE INDEX idx_model_cost_catalog_id ON model (cost_catalog_id);

CREATE TABLE model_source_binding (
    model_id BIGINT NOT NULL,
    source_id BIGINT NOT NULL,
    is_default BOOLEAN NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (model_id, source_id),
    CONSTRAINT fk_model_source_binding_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_model_source_binding_source_id
        FOREIGN KEY (source_id) REFERENCES upstream_source (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_model_source_binding_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX idx_model_source_binding_source_id
    ON model_source_binding (source_id);
CREATE UNIQUE INDEX idx_model_source_binding_model_default_unique
    ON model_source_binding (model_id)
    WHERE is_default = 1;

ALTER TABLE request_log
    ADD COLUMN source_selection_reason TEXT NULL
        CONSTRAINT chk_request_log_source_selection_reason CHECK (
            source_selection_reason IS NULL
            OR source_selection_reason IN (
                'protocol_match',
                'provider_default_transform',
                'model_default_transform'
            )
        );

PRAGMA foreign_key_check;
PRAGMA foreign_keys = ON;
