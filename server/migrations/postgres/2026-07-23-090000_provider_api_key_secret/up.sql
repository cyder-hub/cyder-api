DELETE FROM provider_api_key;

DROP INDEX IF EXISTS idx_provider_api_key_pid_apikey_uq_active;

ALTER TABLE provider_api_key
    DROP COLUMN api_key,
    ADD COLUMN key_prefix TEXT NOT NULL,
    ADD COLUMN key_last4 TEXT NOT NULL,
    ADD COLUMN secret_ciphertext BYTEA NULL,
    ADD COLUMN secret_nonce BYTEA NULL,
    ADD COLUMN secret_format_version INTEGER NULL,
    ADD COLUMN secret_key_fingerprint TEXT NULL,
    ADD COLUMN secret_hmac TEXT NULL,
    ADD CONSTRAINT chk_provider_api_key_secret_state CHECK (
        (
            deleted_at IS NULL AND
            secret_ciphertext IS NOT NULL AND
            secret_nonce IS NOT NULL AND
            secret_format_version IS NOT NULL AND
            secret_key_fingerprint IS NOT NULL AND
            secret_hmac IS NOT NULL
        ) OR (
            deleted_at IS NOT NULL AND
            is_enabled = false AND
            secret_ciphertext IS NULL AND
            secret_nonce IS NULL AND
            secret_format_version IS NULL AND
            secret_key_fingerprint IS NULL AND
            secret_hmac IS NULL
        )
    ),
    ADD CONSTRAINT chk_provider_api_key_nonce_len CHECK (
        secret_nonce IS NULL OR OCTET_LENGTH(secret_nonce) = 24
    ),
    ADD CONSTRAINT chk_provider_api_key_format_version CHECK (
        secret_format_version IS NULL OR secret_format_version = 1
    ),
    ADD CONSTRAINT chk_provider_api_key_fingerprint_format CHECK (
        secret_key_fingerprint IS NULL OR secret_key_fingerprint ~ '^[0-9a-f]{64}$'
    ),
    ADD CONSTRAINT chk_provider_api_key_hmac_format CHECK (
        secret_hmac IS NULL OR secret_hmac ~ '^[0-9a-f]{64}$'
    );

CREATE UNIQUE INDEX idx_provider_api_key_provider_hmac_uq_not_deleted
    ON provider_api_key (provider_id, secret_hmac)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_provider_api_key_provider_id ON provider_api_key (provider_id);
CREATE INDEX idx_provider_api_key_deleted_at ON provider_api_key (deleted_at);
