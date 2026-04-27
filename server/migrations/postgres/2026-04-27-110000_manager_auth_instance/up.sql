CREATE TABLE manager_auth_instance (
    id BIGINT PRIMARY KEY,
    manager_id BIGINT NOT NULL CHECK (manager_id = 0),
    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
    current_refresh_jti TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    last_rotated_at BIGINT NOT NULL,
    expires_at BIGINT NOT NULL,
    revoked_at BIGINT NULL,
    revoked_reason TEXT NULL
);

CREATE INDEX idx_manager_auth_instance_manager_id
    ON manager_auth_instance (manager_id);

CREATE INDEX idx_manager_auth_instance_expires_at
    ON manager_auth_instance (expires_at);

CREATE INDEX idx_manager_auth_instance_revoked_at
    ON manager_auth_instance (revoked_at);
