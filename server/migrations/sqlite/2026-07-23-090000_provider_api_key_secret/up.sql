PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

UPDATE request_log
SET provider_api_key_id = NULL
WHERE provider_api_key_id IS NOT NULL;

DELETE FROM provider_api_key;

CREATE TABLE provider_api_key_r27_new (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    description TEXT,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    secret_ciphertext BLOB,
    secret_nonce BLOB,
    secret_format_version INTEGER,
    secret_key_fingerprint TEXT,
    secret_hmac TEXT,
    deleted_at BIGINT,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_provider_api_key_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
        ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_provider_api_key_secret_state CHECK (
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
    CONSTRAINT chk_provider_api_key_nonce_len CHECK (
        secret_nonce IS NULL OR LENGTH(secret_nonce) = 24
    ),
    CONSTRAINT chk_provider_api_key_format_version CHECK (
        secret_format_version IS NULL OR secret_format_version = 1
    ),
    CONSTRAINT chk_provider_api_key_fingerprint_format CHECK (
        secret_key_fingerprint IS NULL OR (
            LENGTH(secret_key_fingerprint) = 64 AND
            secret_key_fingerprint = LOWER(secret_key_fingerprint) AND
            secret_key_fingerprint NOT GLOB '*[^0-9a-f]*'
        )
    ),
    CONSTRAINT chk_provider_api_key_hmac_format CHECK (
        secret_hmac IS NULL OR (
            LENGTH(secret_hmac) = 64 AND
            secret_hmac = LOWER(secret_hmac) AND
            secret_hmac NOT GLOB '*[^0-9a-f]*'
        )
    ),
    CONSTRAINT chk_provider_api_key_timestamps CHECK (updated_at >= created_at)
);

DROP TABLE provider_api_key;
ALTER TABLE provider_api_key_r27_new RENAME TO provider_api_key;

CREATE UNIQUE INDEX idx_provider_api_key_provider_hmac_uq_not_deleted
    ON provider_api_key (provider_id, secret_hmac)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_provider_api_key_provider_id ON provider_api_key (provider_id);
CREATE INDEX idx_provider_api_key_deleted_at ON provider_api_key (deleted_at);

COMMIT;
PRAGMA foreign_keys = ON;
