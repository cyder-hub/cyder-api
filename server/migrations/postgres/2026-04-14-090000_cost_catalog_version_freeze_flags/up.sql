ALTER TABLE cost_catalog_versions
    ADD COLUMN first_used_at BIGINT NULL,
    ADD COLUMN is_archived BOOLEAN NOT NULL DEFAULT false;

UPDATE cost_catalog_versions AS versions
SET first_used_at = usage.first_used_at
FROM (
    SELECT
        cost_catalog_version_id,
        MIN(request_received_at) AS first_used_at
    FROM request_log
    WHERE cost_catalog_version_id IS NOT NULL
    GROUP BY cost_catalog_version_id
) AS usage
WHERE versions.id = usage.cost_catalog_version_id
  AND versions.first_used_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_cost_catalog_versions_first_used_at
    ON cost_catalog_versions (first_used_at);
