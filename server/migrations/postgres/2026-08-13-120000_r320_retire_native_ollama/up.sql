-- R3.20 retires the native Ollama upstream wire family. This is a
-- destructive pre-1.0 migration with no down migration and no automatic
-- Source conversion. Provider, Provider Key, Model, non-Ollama Source,
-- downstream API key governance, Cost Catalog, and Manager data remain.

DELETE FROM metric_ingested_request_log;
DELETE FROM metric_request_rollup_minute;
DELETE FROM metric_http_status_rollup_minute;
DELETE FROM metric_cost_rollup_minute;
DELETE FROM request_log;

DELETE FROM request_patch_rule
WHERE variant_id IN (
    SELECT id
    FROM request_patch_variant
    WHERE source_id IN (
        SELECT id FROM upstream_source WHERE profile_type = 'OLLAMA'
    )
);
DELETE FROM request_patch_variant
WHERE source_id IN (
    SELECT id FROM upstream_source WHERE profile_type = 'OLLAMA'
);
DELETE FROM model_source_binding
WHERE source_id IN (
    SELECT id FROM upstream_source WHERE profile_type = 'OLLAMA'
);
DELETE FROM upstream_source
WHERE profile_type = 'OLLAMA';

DROP INDEX idx_upstream_source_provider_wire_family_unique;
ALTER TABLE upstream_source
    DROP CONSTRAINT chk_upstream_source_operation_shape;
ALTER TABLE request_log
    DROP CONSTRAINT chk_request_log_source_snapshot;

CREATE TYPE upstream_profile_type_enum_r320 AS ENUM (
    'OPENAI',
    'OPENAI_COMPATIBLE',
    'GEMINI',
    'VERTEX',
    'ANTHROPIC',
    'RESPONSES',
    'GEMINI_OPENAI'
);

ALTER TABLE upstream_source
    ALTER COLUMN profile_type TYPE upstream_profile_type_enum_r320
        USING profile_type::text::upstream_profile_type_enum_r320;
ALTER TABLE request_log
    ALTER COLUMN source_profile_type_snapshot TYPE upstream_profile_type_enum_r320
        USING source_profile_type_snapshot::text::upstream_profile_type_enum_r320;

DROP TYPE upstream_profile_type_enum;
ALTER TYPE upstream_profile_type_enum_r320 RENAME TO upstream_profile_type_enum;

CREATE TYPE upstream_protocol_enum_r320 AS ENUM (
    'OPENAI',
    'RESPONSES',
    'ANTHROPIC',
    'GEMINI'
);

ALTER TABLE request_log
    ALTER COLUMN upstream_protocol TYPE upstream_protocol_enum_r320
        USING upstream_protocol::text::upstream_protocol_enum_r320;

DROP TYPE upstream_protocol_enum;
ALTER TYPE upstream_protocol_enum_r320 RENAME TO upstream_protocol_enum;

ALTER TABLE upstream_source
    ADD CONSTRAINT chk_upstream_source_operation_shape CHECK (
        (
            profile_type IN (
                'OPENAI'::upstream_profile_type_enum,
                'OPENAI_COMPATIBLE'::upstream_profile_type_enum,
                'GEMINI_OPENAI'::upstream_profile_type_enum
            )
            AND chat_completions_enabled IS NOT NULL
            AND embeddings_enabled IS NOT NULL
            AND rerank_enabled IS NOT NULL
            AND (
                profile_type = 'OPENAI_COMPATIBLE'::upstream_profile_type_enum
                OR (rerank_enabled = FALSE AND rerank_path_override IS NULL)
            )
        )
        OR
        (
            profile_type NOT IN (
                'OPENAI'::upstream_profile_type_enum,
                'OPENAI_COMPATIBLE'::upstream_profile_type_enum,
                'GEMINI_OPENAI'::upstream_profile_type_enum
            )
            AND chat_completions_enabled IS NULL
            AND chat_completions_path_override IS NULL
            AND embeddings_enabled IS NULL
            AND embeddings_path_override IS NULL
            AND rerank_enabled IS NULL
            AND rerank_path_override IS NULL
        )
    );

CREATE UNIQUE INDEX idx_upstream_source_provider_wire_family_unique
    ON upstream_source (
        provider_id,
        (CASE profile_type
            WHEN 'OPENAI'::upstream_profile_type_enum THEN 'OPENAI'
            WHEN 'OPENAI_COMPATIBLE'::upstream_profile_type_enum THEN 'OPENAI'
            WHEN 'GEMINI_OPENAI'::upstream_profile_type_enum THEN 'OPENAI'
            WHEN 'RESPONSES'::upstream_profile_type_enum THEN 'RESPONSES'
            WHEN 'ANTHROPIC'::upstream_profile_type_enum THEN 'ANTHROPIC'
            WHEN 'GEMINI'::upstream_profile_type_enum THEN 'GEMINI'
            WHEN 'VERTEX'::upstream_profile_type_enum THEN 'GEMINI'
        END)
    )
    WHERE deleted_at IS NULL;

ALTER TABLE request_log
    ADD CONSTRAINT chk_request_log_upstream_protocol CHECK (
        upstream_protocol IS NULL
        OR upstream_protocol IN (
            'OPENAI'::upstream_protocol_enum,
            'RESPONSES'::upstream_protocol_enum,
            'ANTHROPIC'::upstream_protocol_enum,
            'GEMINI'::upstream_protocol_enum
        )
    ),
    ADD CONSTRAINT chk_request_log_source_snapshot CHECK (
        (
            source_id IS NULL
            AND source_profile_type_snapshot IS NULL
            AND source_base_url_snapshot IS NULL
        )
        OR (
            source_id IS NOT NULL
            AND source_profile_type_snapshot IN (
                'OPENAI'::upstream_profile_type_enum,
                'OPENAI_COMPATIBLE'::upstream_profile_type_enum,
                'GEMINI'::upstream_profile_type_enum,
                'VERTEX'::upstream_profile_type_enum,
                'ANTHROPIC'::upstream_profile_type_enum,
                'RESPONSES'::upstream_profile_type_enum,
                'GEMINI_OPENAI'::upstream_profile_type_enum
            )
            AND source_base_url_snapshot <> ''
        )
    );
