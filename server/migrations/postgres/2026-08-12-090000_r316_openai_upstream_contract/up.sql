-- R3.16 is an intentionally destructive pre-1.0 Provider execution-domain
-- cutover. No Provider-domain row is migrated or kept as a compatibility
-- source. Manager auth, downstream API keys and their daily/monthly rollups,
-- cost catalogs, and non-Provider startup configuration remain untouched.
-- This migration has no down migration.

DELETE FROM metric_ingested_request_log;
DELETE FROM metric_request_rollup_minute;
DELETE FROM metric_http_status_rollup_minute;
DELETE FROM metric_cost_rollup_minute;
DELETE FROM request_log;
DELETE FROM request_patch_rule;
DELETE FROM request_patch_variant;
DELETE FROM model_source_binding;
-- RuleScope has only PROVIDER and MODEL, so every ACL row is Provider-domain.
DELETE FROM api_key_acl_rule;
DELETE FROM model;
DELETE FROM upstream_source;
DELETE FROM provider_api_key;
DELETE FROM provider;

DROP INDEX idx_upstream_source_provider_wire_family_unique;

CREATE TYPE upstream_profile_type_enum_r316 AS ENUM (
    'OPENAI',
    'OPENAI_COMPATIBLE',
    'GEMINI',
    'VERTEX',
    'OLLAMA',
    'ANTHROPIC',
    'RESPONSES',
    'GEMINI_OPENAI'
);

ALTER TABLE upstream_source
    ALTER COLUMN profile_type TYPE upstream_profile_type_enum_r316
        USING profile_type::text::upstream_profile_type_enum_r316;

ALTER TABLE request_log
    ALTER COLUMN source_profile_type_snapshot TYPE upstream_profile_type_enum_r316
        USING source_profile_type_snapshot::text::upstream_profile_type_enum_r316;

DROP TYPE upstream_profile_type_enum;
ALTER TYPE upstream_profile_type_enum_r316 RENAME TO upstream_profile_type_enum;

CREATE TYPE model_kind_enum AS ENUM ('CHAT', 'EMBEDDING', 'RERANK');

ALTER TABLE model
    ADD COLUMN model_kind model_kind_enum NOT NULL;

ALTER TABLE upstream_source
    DROP CONSTRAINT chk_upstream_source_endpoint_not_empty;

ALTER TABLE upstream_source
    RENAME COLUMN endpoint TO base_url;

ALTER TABLE upstream_source
    ADD COLUMN chat_completions_enabled BOOLEAN,
    ADD COLUMN chat_completions_path_override TEXT,
    ADD COLUMN embeddings_enabled BOOLEAN,
    ADD COLUMN embeddings_path_override TEXT,
    ADD COLUMN rerank_enabled BOOLEAN,
    ADD COLUMN rerank_path_override TEXT,
    ADD CONSTRAINT chk_upstream_source_base_url_not_empty CHECK (base_url <> ''),
    ADD CONSTRAINT chk_upstream_source_operation_paths CHECK (
        (chat_completions_path_override IS NULL OR chat_completions_path_override <> '')
        AND (embeddings_path_override IS NULL OR embeddings_path_override <> '')
        AND (rerank_path_override IS NULL OR rerank_path_override <> '')
    ),
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
            WHEN 'OLLAMA'::upstream_profile_type_enum THEN 'OLLAMA'
        END)
    )
    WHERE deleted_at IS NULL;

ALTER TABLE request_log
    DROP CONSTRAINT chk_request_log_source_snapshot;

ALTER TABLE request_log
    RENAME COLUMN source_endpoint_snapshot TO source_base_url_snapshot;

ALTER TABLE request_log
    ADD COLUMN model_kind_snapshot model_kind_enum,
    ADD CONSTRAINT chk_request_log_model_snapshot CHECK (
        (model_id IS NULL AND model_kind_snapshot IS NULL)
        OR (model_id IS NOT NULL AND model_kind_snapshot IS NOT NULL)
    ),
    ADD CONSTRAINT chk_request_log_source_snapshot CHECK (
        (
            source_id IS NULL
            AND source_profile_type_snapshot IS NULL
            AND source_base_url_snapshot IS NULL
        )
        OR (
            source_id IS NOT NULL
            AND source_profile_type_snapshot IS NOT NULL
            AND source_base_url_snapshot <> ''
        )
    );
