DELETE FROM manager_auth_instance;

ALTER TABLE manager_auth_instance
ADD COLUMN session_version BIGINT NOT NULL DEFAULT 1
CHECK (session_version >= 1);
