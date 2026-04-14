DROP INDEX IF EXISTS idx_cost_catalog_versions_first_used_at;

PRAGMA foreign_keys=off;

CREATE TABLE cost_catalog_versions_old (
    id BIGINT PRIMARY KEY NOT NULL,
    catalog_id BIGINT NOT NULL,
    version TEXT NOT NULL,
    currency TEXT NOT NULL,
    source TEXT,
    effective_from BIGINT NOT NULL,
    effective_until BIGINT,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_cost_catalog_versions_catalog_id
        FOREIGN KEY (catalog_id) REFERENCES cost_catalogs (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_cost_catalog_versions_version_not_empty CHECK (version <> ''),
    CONSTRAINT chk_cost_catalog_versions_currency_len CHECK (LENGTH(currency) = 3),
    CONSTRAINT chk_cost_catalog_versions_effective_range CHECK (
        effective_until IS NULL OR effective_until >= effective_from
    ),
    CONSTRAINT chk_cost_catalog_versions_timestamps CHECK (updated_at >= created_at),
    CONSTRAINT chk_cost_catalog_versions_source_not_empty CHECK (
        source IS NULL OR source <> ''
    )
);

INSERT INTO cost_catalog_versions_old (
    id,
    catalog_id,
    version,
    currency,
    source,
    effective_from,
    effective_until,
    is_enabled,
    created_at,
    updated_at
)
SELECT
    id,
    catalog_id,
    version,
    currency,
    source,
    effective_from,
    effective_until,
    is_enabled,
    created_at,
    updated_at
FROM cost_catalog_versions;

DROP TABLE cost_catalog_versions;
ALTER TABLE cost_catalog_versions_old RENAME TO cost_catalog_versions;

CREATE UNIQUE INDEX IF NOT EXISTS idx_cost_catalog_versions_catalog_version
    ON cost_catalog_versions (catalog_id, version);

CREATE INDEX IF NOT EXISTS idx_cost_catalog_versions_lookup
    ON cost_catalog_versions (catalog_id, is_enabled, effective_from, effective_until);

PRAGMA foreign_keys=on;
