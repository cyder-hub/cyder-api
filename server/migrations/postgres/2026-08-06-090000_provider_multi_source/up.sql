-- R3.10 preserves R3.9 rows while replacing source_key with lifecycle flags.
-- This migration has no down migration.

DROP INDEX idx_upstream_source_provider_active_unique;

ALTER TABLE upstream_source
    ADD COLUMN is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT FALSE;

UPDATE upstream_source
SET is_enabled = (deleted_at IS NULL),
    is_default = (deleted_at IS NULL);

ALTER TABLE upstream_source
    DROP CONSTRAINT chk_upstream_source_key,
    DROP COLUMN source_key,
    ADD CONSTRAINT chk_upstream_source_flags CHECK (
        (is_default = FALSE OR (is_enabled = TRUE AND deleted_at IS NULL))
        AND (deleted_at IS NULL OR (is_enabled = FALSE AND is_default = FALSE))
    );

CREATE UNIQUE INDEX idx_upstream_source_provider_default_unique
    ON upstream_source (provider_id)
    WHERE deleted_at IS NULL AND is_default = TRUE;

CREATE UNIQUE INDEX idx_upstream_source_provider_wire_family_unique
    ON upstream_source (
        provider_id,
        (CASE profile_type
            WHEN 'OPENAI'::upstream_profile_type_enum THEN 'OPENAI'
            WHEN 'VERTEX_OPENAI'::upstream_profile_type_enum THEN 'OPENAI'
            WHEN 'GEMINI_OPENAI'::upstream_profile_type_enum THEN 'OPENAI'
            WHEN 'RESPONSES'::upstream_profile_type_enum THEN 'RESPONSES'
            WHEN 'ANTHROPIC'::upstream_profile_type_enum THEN 'ANTHROPIC'
            WHEN 'GEMINI'::upstream_profile_type_enum THEN 'GEMINI'
            WHEN 'VERTEX'::upstream_profile_type_enum THEN 'GEMINI'
            WHEN 'OLLAMA'::upstream_profile_type_enum THEN 'OLLAMA'
        END)
    )
    WHERE deleted_at IS NULL;

ALTER TABLE request_log
    DROP CONSTRAINT chk_request_log_source_snapshot,
    DROP COLUMN source_key_snapshot,
    ADD CONSTRAINT chk_request_log_source_snapshot CHECK (
        (
            source_id IS NULL
            AND source_profile_type_snapshot IS NULL
            AND source_endpoint_snapshot IS NULL
        )
        OR (
            source_id IS NOT NULL
            AND source_profile_type_snapshot IS NOT NULL
            AND source_endpoint_snapshot <> ''
        )
    );

UPDATE metric_request_rollup_minute AS rollup
SET scope_label = source.profile_type::text
FROM upstream_source AS source
WHERE rollup.scope_type = 'source'
  AND rollup.scope_label = 'primary'
  AND rollup.scope_id = source.id::text;
