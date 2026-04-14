DROP INDEX IF EXISTS idx_cost_catalog_versions_first_used_at;

ALTER TABLE cost_catalog_versions
    DROP COLUMN IF EXISTS is_archived,
    DROP COLUMN IF EXISTS first_used_at;
