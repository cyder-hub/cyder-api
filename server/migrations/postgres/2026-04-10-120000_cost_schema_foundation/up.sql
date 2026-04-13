CREATE TABLE IF NOT EXISTS cost_catalogs (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    deleted_at BIGINT,
    CONSTRAINT chk_cost_catalogs_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_cost_catalogs_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cost_catalogs_name_active
    ON cost_catalogs (name)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS cost_catalog_versions (
    id BIGINT PRIMARY KEY,
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
        FOREIGN KEY (catalog_id) REFERENCES cost_catalogs(id)
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

CREATE UNIQUE INDEX IF NOT EXISTS idx_cost_catalog_versions_catalog_version
    ON cost_catalog_versions (catalog_id, version);

CREATE INDEX IF NOT EXISTS idx_cost_catalog_versions_lookup
    ON cost_catalog_versions (catalog_id, is_enabled, effective_from, effective_until);

CREATE TABLE IF NOT EXISTS cost_components (
    id BIGINT PRIMARY KEY,
    catalog_version_id BIGINT NOT NULL,
    meter_key TEXT NOT NULL,
    charge_kind TEXT NOT NULL,
    unit_price_nanos BIGINT,
    flat_fee_nanos BIGINT,
    tier_config_json TEXT,
    match_attributes_json TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_cost_components_catalog_version_id
        FOREIGN KEY (catalog_version_id) REFERENCES cost_catalog_versions(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_cost_components_meter_key_not_empty CHECK (meter_key <> ''),
    CONSTRAINT chk_cost_components_charge_kind CHECK (
        charge_kind IN ('per_unit', 'flat', 'tiered_per_unit')
    ),
    CONSTRAINT chk_cost_components_unit_price_non_negative CHECK (
        unit_price_nanos IS NULL OR unit_price_nanos >= 0
    ),
    CONSTRAINT chk_cost_components_flat_fee_non_negative CHECK (
        flat_fee_nanos IS NULL OR flat_fee_nanos >= 0
    ),
    CONSTRAINT chk_cost_components_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_cost_components_version_priority
    ON cost_components (catalog_version_id, priority, meter_key);

ALTER TABLE model
    ADD COLUMN IF NOT EXISTS cost_catalog_id BIGINT NULL;

ALTER TABLE model
    ADD CONSTRAINT fk_model_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE;

CREATE INDEX IF NOT EXISTS idx_model_cost_catalog_id
    ON model (cost_catalog_id);

ALTER TABLE request_log
    RENAME COLUMN calculated_cost TO estimated_cost_nanos;

ALTER TABLE request_log
    RENAME COLUMN cost_currency TO estimated_cost_currency;

ALTER TABLE request_log
    RENAME COLUMN input_tokens TO total_input_tokens;

ALTER TABLE request_log
    RENAME COLUMN output_tokens TO total_output_tokens;

ALTER TABLE request_log
    RENAME COLUMN cached_tokens TO cache_read_tokens;

ALTER TABLE request_log
    ADD COLUMN cost_catalog_id BIGINT NULL,
    ADD COLUMN cost_catalog_version_id BIGINT NULL,
    ADD COLUMN cost_snapshot_json TEXT NULL,
    ADD COLUMN input_text_tokens INTEGER NULL,
    ADD COLUMN output_text_tokens INTEGER NULL,
    ADD COLUMN cache_write_tokens INTEGER NULL;

ALTER TABLE request_log
    ADD CONSTRAINT fk_request_log_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    ADD CONSTRAINT fk_request_log_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE;

CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_id
    ON request_log (cost_catalog_id);

CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_version_id
    ON request_log (cost_catalog_version_id);
