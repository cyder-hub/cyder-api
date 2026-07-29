PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE manager_credential_r213_new (
    manager_id BIGINT PRIMARY KEY NOT NULL CHECK (manager_id = 0),
    manager_subject TEXT NOT NULL CHECK (manager_subject = 'admin'),
    password_verifier TEXT NOT NULL,
    credential_epoch TEXT NOT NULL UNIQUE,
    totp_secret_ciphertext BLOB NULL,
    totp_secret_nonce BLOB NULL,
    totp_secret_format_version INTEGER NULL,
    totp_secret_key_fingerprint TEXT NULL,
    totp_last_accepted_step BIGINT NULL,
    totp_enabled_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_manager_credential_totp_all_or_none CHECK (
        (
            totp_secret_ciphertext IS NULL
            AND totp_secret_nonce IS NULL
            AND totp_secret_format_version IS NULL
            AND totp_secret_key_fingerprint IS NULL
            AND totp_last_accepted_step IS NULL
            AND totp_enabled_at IS NULL
        ) OR (
            totp_secret_ciphertext IS NOT NULL
            AND totp_secret_nonce IS NOT NULL
            AND totp_secret_format_version IS NOT NULL
            AND totp_secret_key_fingerprint IS NOT NULL
            AND totp_last_accepted_step IS NOT NULL
            AND totp_enabled_at IS NOT NULL
        )
    )
);

INSERT INTO manager_credential_r213_new (
    manager_id,
    manager_subject,
    password_verifier,
    credential_epoch,
    totp_secret_ciphertext,
    totp_secret_nonce,
    totp_secret_format_version,
    totp_secret_key_fingerprint,
    totp_last_accepted_step,
    totp_enabled_at,
    created_at,
    updated_at
)
SELECT
    manager_id,
    manager_subject,
    password_verifier,
    credential_epoch,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    created_at,
    updated_at
FROM manager_credential;

DROP TABLE manager_credential;
ALTER TABLE manager_credential_r213_new RENAME TO manager_credential;

CREATE TABLE manager_totp_recovery_code (
    code_id TEXT PRIMARY KEY NOT NULL CHECK (length(code_id) = 4),
    manager_id BIGINT NOT NULL CHECK (manager_id = 0),
    code_verifier TEXT NOT NULL CHECK (code_verifier <> ''),
    created_at BIGINT NOT NULL,
    FOREIGN KEY (manager_id)
        REFERENCES manager_credential(manager_id)
        ON DELETE CASCADE
);

COMMIT;
PRAGMA foreign_keys = ON;
