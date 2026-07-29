ALTER TABLE api_key
    ALTER COLUMN api_key_hash SET NOT NULL;

DROP INDEX IF EXISTS idx_api_key_hash_uq_active;

ALTER TABLE api_key
    DROP COLUMN api_key,
    ADD COLUMN secret_ciphertext BYTEA NULL,
    ADD COLUMN secret_nonce BYTEA NULL,
    ADD COLUMN secret_format_version INTEGER NULL,
    ADD COLUMN secret_key_fingerprint TEXT NULL,
    ADD CONSTRAINT chk_api_key_secret_all_or_none CHECK (
        (
            secret_ciphertext IS NULL AND
            secret_nonce IS NULL AND
            secret_format_version IS NULL AND
            secret_key_fingerprint IS NULL
        ) OR (
            secret_ciphertext IS NOT NULL AND
            secret_nonce IS NOT NULL AND
            secret_format_version IS NOT NULL AND
            secret_key_fingerprint IS NOT NULL
        )
    );

CREATE UNIQUE INDEX idx_api_key_hash_uq_not_deleted
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL;
