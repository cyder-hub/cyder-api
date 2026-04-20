-- Table: model_route
-- Shared logical entry names. A route may resolve to one or more concrete provider models.
CREATE TABLE IF NOT EXISTS model_route (
    id BIGINT PRIMARY KEY,
    route_name TEXT NOT NULL,
    description TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    expose_in_models BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT chk_model_route_name_not_empty CHECK (route_name <> ''),
    CONSTRAINT chk_model_route_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_route_name_uq_active
ON model_route (route_name)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_model_route_deleted_at ON model_route (deleted_at);
CREATE INDEX IF NOT EXISTS idx_model_route_enabled ON model_route (is_enabled);

-- Table: model_route_candidate
-- Ordered concrete candidates for a logical route.
CREATE TABLE IF NOT EXISTS model_route_candidate (
    id BIGINT PRIMARY KEY,
    route_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT fk_model_route_candidate_route_id
        FOREIGN KEY (route_id) REFERENCES model_route(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE,
    CONSTRAINT fk_model_route_candidate_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
        ON DELETE RESTRICT
        ON UPDATE CASCADE,
    CONSTRAINT chk_model_route_candidate_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_route_candidate_route_model_uq_active
ON model_route_candidate (route_id, model_id)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_model_route_candidate_route_id
ON model_route_candidate (route_id);

CREATE INDEX IF NOT EXISTS idx_model_route_candidate_route_priority
ON model_route_candidate (route_id, priority, id);

-- Table: api_key_model_override
-- Optional caller-scoped name overrides that target shared logical routes.
CREATE TABLE IF NOT EXISTS api_key_model_override (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL,
    source_name TEXT NOT NULL,
    target_route_id BIGINT NOT NULL,
    description TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,

    CONSTRAINT fk_api_key_model_override_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
        ON DELETE CASCADE
        ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_model_override_target_route_id
        FOREIGN KEY (target_route_id) REFERENCES model_route(id)
        ON DELETE RESTRICT
        ON UPDATE CASCADE,
    CONSTRAINT chk_api_key_model_override_name_not_empty CHECK (source_name <> ''),
    CONSTRAINT chk_api_key_model_override_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_model_override_name_uq_active
ON api_key_model_override (api_key_id, source_name)
WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_api_key_model_override_api_key_id
ON api_key_model_override (api_key_id);

CREATE INDEX IF NOT EXISTS idx_api_key_model_override_target_route_id
ON api_key_model_override (target_route_id);
