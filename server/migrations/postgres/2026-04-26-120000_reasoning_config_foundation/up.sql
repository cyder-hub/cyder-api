CREATE TABLE IF NOT EXISTS reasoning_config (
    id BIGINT PRIMARY KEY,
    scope_kind TEXT NOT NULL,
    provider_id BIGINT DEFAULT NULL,
    model_id BIGINT DEFAULT NULL,
    mode TEXT NOT NULL,
    family_key TEXT DEFAULT NULL,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT fk_reasoning_config_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_reasoning_config_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_reasoning_config_scope_kind CHECK (scope_kind IN ('provider', 'model')),
    CONSTRAINT chk_reasoning_config_mode CHECK (mode IN ('custom', 'disabled')),
    CONSTRAINT chk_reasoning_config_owner CHECK (
        (
            scope_kind = 'provider'
            AND provider_id IS NOT NULL
            AND model_id IS NULL
            AND mode = 'custom'
        )
        OR (
            scope_kind = 'model'
            AND model_id IS NOT NULL
            AND provider_id IS NULL
            AND mode IN ('custom', 'disabled')
        )
    ),
    CONSTRAINT chk_reasoning_config_mode_family CHECK (
        (
            mode = 'custom'
            AND family_key IS NOT NULL
            AND family_key <> ''
        )
        OR (
            mode = 'disabled'
            AND family_key IS NULL
        )
    ),
    CONSTRAINT chk_reasoning_config_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reasoning_config_provider_uq_active
    ON reasoning_config (provider_id)
    WHERE scope_kind = 'provider' AND deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_reasoning_config_model_uq_active
    ON reasoning_config (model_id)
    WHERE scope_kind = 'model' AND deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_reasoning_config_scope_owner
    ON reasoning_config (scope_kind, provider_id, model_id)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS reasoning_config_preset (
    id BIGINT PRIMARY KEY,
    config_id BIGINT NOT NULL,
    preset_key TEXT NOT NULL,
    expose_in_models BOOLEAN NOT NULL DEFAULT TRUE,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT fk_reasoning_config_preset_config_id
        FOREIGN KEY (config_id) REFERENCES reasoning_config (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_reasoning_config_preset_key_not_empty CHECK (preset_key <> ''),
    CONSTRAINT chk_reasoning_config_preset_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reasoning_config_preset_uq_active
    ON reasoning_config_preset (config_id, preset_key)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_reasoning_config_preset_config_id
    ON reasoning_config_preset (config_id)
    WHERE deleted_at IS NULL;

WITH provider_bindings AS (
    SELECT
        (-1000000000000 - ROW_NUMBER() OVER (ORDER BY p.id, rp.id)) AS config_id,
        p.id AS provider_id,
        rp.id AS profile_id,
        rp.family_key AS family_key,
        rp.created_at AS created_at,
        rp.updated_at AS updated_at
    FROM provider p
    INNER JOIN reasoning_profile rp
        ON rp.id = p.default_reasoning_profile_id
    WHERE p.deleted_at IS NULL
      AND rp.deleted_at IS NULL
      AND rp.is_enabled = TRUE
      AND p.default_reasoning_profile_id IS NOT NULL
)
INSERT INTO reasoning_config (
    id,
    scope_kind,
    provider_id,
    model_id,
    mode,
    family_key,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    config_id,
    'provider',
    provider_id,
    NULL,
    'custom',
    family_key,
    NULL,
    created_at,
    updated_at
FROM provider_bindings
WHERE NOT EXISTS (
    SELECT 1
    FROM reasoning_config rc
    WHERE rc.scope_kind = 'provider'
      AND rc.provider_id = provider_bindings.provider_id
      AND rc.deleted_at IS NULL
);

WITH provider_bindings AS (
    SELECT
        (-1000000000000 - ROW_NUMBER() OVER (ORDER BY p.id, rp.id)) AS config_id,
        p.id AS provider_id,
        rp.id AS profile_id
    FROM provider p
    INNER JOIN reasoning_profile rp
        ON rp.id = p.default_reasoning_profile_id
    WHERE p.deleted_at IS NULL
      AND rp.deleted_at IS NULL
      AND rp.is_enabled = TRUE
      AND p.default_reasoning_profile_id IS NOT NULL
),
provider_preset_rows AS (
    SELECT
        (-3000000000000 - ROW_NUMBER() OVER (ORDER BY pb.provider_id, rpp.id)) AS preset_id,
        pb.config_id AS config_id,
        rpp.preset_key AS preset_key,
        rpp.expose_in_models AS expose_in_models,
        rpp.is_enabled AS is_enabled,
        rpp.created_at AS created_at,
        rpp.updated_at AS updated_at
    FROM provider_bindings pb
    INNER JOIN reasoning_profile_preset rpp
        ON rpp.profile_id = pb.profile_id
    WHERE rpp.deleted_at IS NULL
)
INSERT INTO reasoning_config_preset (
    id,
    config_id,
    preset_key,
    expose_in_models,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    preset_id,
    config_id,
    preset_key,
    expose_in_models,
    is_enabled,
    NULL,
    created_at,
    updated_at
FROM provider_preset_rows
WHERE EXISTS (
    SELECT 1
    FROM reasoning_config rc
    WHERE rc.id = provider_preset_rows.config_id
      AND rc.deleted_at IS NULL
)
AND NOT EXISTS (
    SELECT 1
    FROM reasoning_config_preset rcp
    WHERE rcp.config_id = provider_preset_rows.config_id
      AND rcp.preset_key = provider_preset_rows.preset_key
      AND rcp.deleted_at IS NULL
);

WITH model_bindings AS (
    SELECT
        (-2000000000000 - ROW_NUMBER() OVER (ORDER BY m.id, rp.id)) AS config_id,
        m.id AS model_id,
        rp.id AS profile_id,
        rp.family_key AS family_key,
        rp.created_at AS created_at,
        rp.updated_at AS updated_at
    FROM model m
    INNER JOIN reasoning_profile rp
        ON rp.id = m.reasoning_profile_override_id
    WHERE m.deleted_at IS NULL
      AND rp.deleted_at IS NULL
      AND rp.is_enabled = TRUE
      AND m.reasoning_profile_override_id IS NOT NULL
)
INSERT INTO reasoning_config (
    id,
    scope_kind,
    provider_id,
    model_id,
    mode,
    family_key,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    config_id,
    'model',
    NULL,
    model_id,
    'custom',
    family_key,
    NULL,
    created_at,
    updated_at
FROM model_bindings
WHERE NOT EXISTS (
    SELECT 1
    FROM reasoning_config rc
    WHERE rc.scope_kind = 'model'
      AND rc.model_id = model_bindings.model_id
      AND rc.deleted_at IS NULL
);

WITH model_bindings AS (
    SELECT
        (-2000000000000 - ROW_NUMBER() OVER (ORDER BY m.id, rp.id)) AS config_id,
        m.id AS model_id,
        rp.id AS profile_id
    FROM model m
    INNER JOIN reasoning_profile rp
        ON rp.id = m.reasoning_profile_override_id
    WHERE m.deleted_at IS NULL
      AND rp.deleted_at IS NULL
      AND rp.is_enabled = TRUE
      AND m.reasoning_profile_override_id IS NOT NULL
),
model_preset_rows AS (
    SELECT
        (-4000000000000 - ROW_NUMBER() OVER (ORDER BY mb.model_id, rpp.id)) AS preset_id,
        mb.config_id AS config_id,
        rpp.preset_key AS preset_key,
        rpp.expose_in_models AS expose_in_models,
        rpp.is_enabled AS is_enabled,
        rpp.created_at AS created_at,
        rpp.updated_at AS updated_at
    FROM model_bindings mb
    INNER JOIN reasoning_profile_preset rpp
        ON rpp.profile_id = mb.profile_id
    WHERE rpp.deleted_at IS NULL
)
INSERT INTO reasoning_config_preset (
    id,
    config_id,
    preset_key,
    expose_in_models,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    preset_id,
    config_id,
    preset_key,
    expose_in_models,
    is_enabled,
    NULL,
    created_at,
    updated_at
FROM model_preset_rows
WHERE EXISTS (
    SELECT 1
    FROM reasoning_config rc
    WHERE rc.id = model_preset_rows.config_id
      AND rc.deleted_at IS NULL
)
AND NOT EXISTS (
    SELECT 1
    FROM reasoning_config_preset rcp
    WHERE rcp.config_id = model_preset_rows.config_id
      AND rcp.preset_key = model_preset_rows.preset_key
      AND rcp.deleted_at IS NULL
);
