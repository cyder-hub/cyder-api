PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

DROP TABLE manager_totp_recovery_code;

CREATE TABLE manager_credential_r212_new (
    manager_id BIGINT PRIMARY KEY NOT NULL CHECK (manager_id = 0),
    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
    password_verifier TEXT NOT NULL,
    credential_epoch TEXT NOT NULL UNIQUE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

INSERT INTO manager_credential_r212_new (
    manager_id,
    manager_subject,
    password_verifier,
    credential_epoch,
    created_at,
    updated_at
)
SELECT
    manager_id,
    manager_subject,
    password_verifier,
    credential_epoch,
    created_at,
    updated_at
FROM manager_credential;

DROP TABLE manager_credential;
ALTER TABLE manager_credential_r212_new RENAME TO manager_credential;

COMMIT;
PRAGMA foreign_keys = ON;
