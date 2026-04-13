DROP INDEX IF EXISTS idx_request_log_cost_catalog_version_id;
DROP INDEX IF EXISTS idx_request_log_cost_catalog_id;

ALTER TABLE request_log
    DROP CONSTRAINT IF EXISTS fk_request_log_cost_catalog_version_id,
    DROP CONSTRAINT IF EXISTS fk_request_log_cost_catalog_id;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS cache_write_tokens,
    DROP COLUMN IF EXISTS output_text_tokens,
    DROP COLUMN IF EXISTS input_text_tokens,
    DROP COLUMN IF EXISTS cost_snapshot_json,
    DROP COLUMN IF EXISTS cost_catalog_version_id,
    DROP COLUMN IF EXISTS cost_catalog_id;

ALTER TABLE request_log
    RENAME COLUMN cache_read_tokens TO cached_tokens;

ALTER TABLE request_log
    RENAME COLUMN total_output_tokens TO output_tokens;

ALTER TABLE request_log
    RENAME COLUMN total_input_tokens TO input_tokens;

ALTER TABLE request_log
    RENAME COLUMN estimated_cost_currency TO cost_currency;

ALTER TABLE request_log
    RENAME COLUMN estimated_cost_nanos TO calculated_cost;

DROP INDEX IF EXISTS idx_model_cost_catalog_id;

ALTER TABLE model
    DROP CONSTRAINT IF EXISTS fk_model_cost_catalog_id;

ALTER TABLE model
    DROP COLUMN IF EXISTS cost_catalog_id;

DROP INDEX IF EXISTS idx_cost_components_version_priority;
DROP INDEX IF EXISTS idx_cost_catalog_versions_lookup;
DROP INDEX IF EXISTS idx_cost_catalog_versions_catalog_version;
DROP INDEX IF EXISTS idx_cost_catalogs_name_active;

DROP TABLE IF EXISTS cost_components;
DROP TABLE IF EXISTS cost_catalog_versions;
DROP TABLE IF EXISTS cost_catalogs;
