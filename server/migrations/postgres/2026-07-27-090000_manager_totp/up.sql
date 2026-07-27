ALTER TABLE manager_credential
    ADD COLUMN totp_secret_ciphertext BYTEA NULL,
    ADD COLUMN totp_secret_nonce BYTEA NULL,
    ADD COLUMN totp_secret_format_version INTEGER NULL,
    ADD COLUMN totp_secret_key_fingerprint TEXT NULL,
    ADD COLUMN totp_last_accepted_step BIGINT NULL,
    ADD COLUMN totp_enabled_at BIGINT NULL,
    ADD CONSTRAINT chk_manager_credential_totp_all_or_none CHECK (
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
    );

CREATE TABLE manager_totp_recovery_code (
    code_id TEXT PRIMARY KEY CHECK (length(code_id) = 4),
    manager_id BIGINT NOT NULL CHECK (manager_id = 0),
    code_verifier TEXT NOT NULL CHECK (code_verifier <> ''),
    created_at BIGINT NOT NULL,
    CONSTRAINT fk_manager_totp_recovery_code_manager
        FOREIGN KEY (manager_id)
        REFERENCES manager_credential(manager_id)
        ON DELETE CASCADE
);
