CREATE TABLE manager_credential (
    manager_id BIGINT PRIMARY KEY CHECK (manager_id = 0),
    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
    password_verifier TEXT NOT NULL,
    credential_epoch TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
