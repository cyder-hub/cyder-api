DROP INDEX IF EXISTS idx_model_reasoning_profile_override_id;
DROP INDEX IF EXISTS idx_provider_default_reasoning_profile_id;

ALTER TABLE request_log
DROP COLUMN base_requested_model_name;

ALTER TABLE request_log
DROP COLUMN resolved_reasoning_suffix;

ALTER TABLE request_log
DROP COLUMN resolved_reasoning_preset;

PRAGMA foreign_keys = OFF;

CREATE TABLE provider_old (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_key TEXT NOT NULL,
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT false,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    provider_type TEXT NOT NULL DEFAULT 'OPENAI',
    provider_api_key_mode TEXT NOT NULL DEFAULT 'QUEUE',
    CONSTRAINT chk_provider_type CHECK (
        provider_type IN (
            'OPENAI',
            'GEMINI',
            'VERTEX',
            'VERTEX_OPENAI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_provider_api_key_mode CHECK (
        provider_api_key_mode IN ('QUEUE', 'RANDOM')
    ),
    CONSTRAINT chk_provider_timestamps CHECK (updated_at >= created_at),
    CONSTRAINT chk_provider_key_not_empty CHECK (provider_key <> ''),
    CONSTRAINT chk_provider_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_provider_endpoint_not_empty CHECK (endpoint <> '')
);

INSERT INTO provider_old (
    id,
    provider_key,
    name,
    endpoint,
    use_proxy,
    is_enabled,
    deleted_at,
    created_at,
    updated_at,
    provider_type,
    provider_api_key_mode
)
SELECT
    id,
    provider_key,
    name,
    endpoint,
    use_proxy,
    is_enabled,
    deleted_at,
    created_at,
    updated_at,
    provider_type,
    provider_api_key_mode
FROM provider;

DROP TABLE provider;
ALTER TABLE provider_old RENAME TO provider;

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_key_unique_when_active
    ON provider (provider_key)
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE TABLE model_old (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    cost_catalog_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    supports_streaming BOOLEAN NOT NULL DEFAULT true,
    supports_tools BOOLEAN NOT NULL DEFAULT true,
    supports_reasoning BOOLEAN NOT NULL DEFAULT true,
    supports_image_input BOOLEAN NOT NULL DEFAULT true,
    supports_embeddings BOOLEAN NOT NULL DEFAULT true,
    supports_rerank BOOLEAN NOT NULL DEFAULT true,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_model_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT chk_model_name_not_empty CHECK (model_name <> ''),
    CONSTRAINT chk_model_real_model_name_not_empty CHECK (
        real_model_name IS NULL OR real_model_name <> ''
    ),
    CONSTRAINT chk_model_timestamps CHECK (updated_at >= created_at)
);

INSERT INTO model_old (
    id,
    provider_id,
    cost_catalog_id,
    model_name,
    real_model_name,
    supports_streaming,
    supports_tools,
    supports_reasoning,
    supports_image_input,
    supports_embeddings,
    supports_rerank,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    id,
    provider_id,
    cost_catalog_id,
    model_name,
    real_model_name,
    supports_streaming,
    supports_tools,
    supports_reasoning,
    supports_image_input,
    supports_embeddings,
    supports_rerank,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
FROM model;

DROP TABLE model;
ALTER TABLE model_old RENAME TO model;

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_pid_name_uq_active
    ON model (provider_id, model_name)
    WHERE deleted_at IS NULL AND is_enabled = true;
CREATE INDEX IF NOT EXISTS idx_model_provider_id
    ON model (provider_id);
CREATE INDEX IF NOT EXISTS idx_model_cost_catalog_id
    ON model (cost_catalog_id);

DROP INDEX IF EXISTS idx_reasoning_profile_preset_profile_id;
DROP INDEX IF EXISTS idx_reasoning_profile_preset_uq_active;
DROP TABLE IF EXISTS reasoning_profile_preset;

DROP INDEX IF EXISTS idx_reasoning_profile_enabled;
DROP INDEX IF EXISTS idx_reasoning_profile_key_uq_active;
DROP TABLE IF EXISTS reasoning_profile;

PRAGMA foreign_keys = ON;
