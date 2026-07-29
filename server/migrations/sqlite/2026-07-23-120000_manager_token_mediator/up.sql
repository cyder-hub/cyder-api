DROP TABLE manager_auth_instance;

CREATE TABLE manager_auth_instance (
    id BIGINT PRIMARY KEY NOT NULL,
    manager_id BIGINT NOT NULL CHECK (manager_id = 0),
    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
    current_refresh_jti TEXT NOT NULL UNIQUE,
    refresh_generation BIGINT NOT NULL DEFAULT 1 CHECK (refresh_generation >= 1),
    session_version BIGINT NOT NULL DEFAULT 1 CHECK (session_version >= 1),
    signing_key_id TEXT NOT NULL CHECK (length(signing_key_id) = 64),
    credential_epoch TEXT NOT NULL CHECK (length(credential_epoch) = 36),
    created_at BIGINT NOT NULL,
    last_rotated_at BIGINT NOT NULL,
    idle_expires_at BIGINT NOT NULL,
    absolute_expires_at BIGINT NOT NULL,
    revoked_at BIGINT NULL,
    revoked_reason TEXT NULL,
    CHECK (created_at <= last_rotated_at),
    CHECK (last_rotated_at < idle_expires_at),
    CHECK (idle_expires_at <= absolute_expires_at),
    CHECK (
        (revoked_at IS NULL AND revoked_reason IS NULL)
        OR (revoked_at IS NOT NULL AND revoked_reason IS NOT NULL)
    )
);

CREATE INDEX idx_manager_auth_instance_manager_id
    ON manager_auth_instance (manager_id);

CREATE INDEX idx_manager_auth_instance_active_deadlines
    ON manager_auth_instance (revoked_at, idle_expires_at, absolute_expires_at);

CREATE INDEX idx_manager_auth_instance_signing_key_id
    ON manager_auth_instance (signing_key_id);
