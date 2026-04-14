ALTER TABLE cost_catalog_versions
    ADD COLUMN first_used_at BIGINT;

ALTER TABLE cost_catalog_versions
    ADD COLUMN is_archived BOOLEAN NOT NULL DEFAULT 0;

UPDATE cost_catalog_versions
SET first_used_at = (
    SELECT MIN(request_log.request_received_at)
    FROM request_log
    WHERE request_log.cost_catalog_version_id = cost_catalog_versions.id
)
WHERE EXISTS (
    SELECT 1
    FROM request_log
    WHERE request_log.cost_catalog_version_id = cost_catalog_versions.id
);

CREATE INDEX IF NOT EXISTS idx_cost_catalog_versions_first_used_at
    ON cost_catalog_versions (first_used_at);
