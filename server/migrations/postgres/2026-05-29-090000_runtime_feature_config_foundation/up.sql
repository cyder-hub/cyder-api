CREATE TABLE IF NOT EXISTS runtime_feature_config (
    id BIGINT PRIMARY KEY,
    scope_kind TEXT NOT NULL,
    provider_id BIGINT DEFAULT NULL,
    model_id BIGINT DEFAULT NULL,
    feature_key TEXT NOT NULL,
    enabled BOOLEAN NOT NULL,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT fk_runtime_feature_config_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_runtime_feature_config_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_runtime_feature_config_scope_kind CHECK (scope_kind IN ('provider', 'model')),
    CONSTRAINT chk_runtime_feature_config_feature_key CHECK (
        feature_key IN ('openai_reasoning_content_repair')
    ),
    CONSTRAINT chk_runtime_feature_config_owner CHECK (
        (
            scope_kind = 'provider'
            AND provider_id IS NOT NULL
            AND model_id IS NULL
        )
        OR (
            scope_kind = 'model'
            AND model_id IS NOT NULL
            AND provider_id IS NULL
        )
    ),
    CONSTRAINT chk_runtime_feature_config_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_runtime_feature_config_provider_feature_uq_active
    ON runtime_feature_config (provider_id, feature_key)
    WHERE scope_kind = 'provider' AND deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_runtime_feature_config_model_feature_uq_active
    ON runtime_feature_config (model_id, feature_key)
    WHERE scope_kind = 'model' AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_runtime_feature_config_scope_owner
    ON runtime_feature_config (scope_kind, provider_id, model_id)
    WHERE deleted_at IS NULL;
