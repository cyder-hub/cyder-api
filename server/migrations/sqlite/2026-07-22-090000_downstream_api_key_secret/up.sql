PRAGMA foreign_keys = OFF;
BEGIN IMMEDIATE;

CREATE TABLE api_key_r26_new (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_hash TEXT NOT NULL,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    default_action TEXT NOT NULL DEFAULT 'ALLOW' CHECK (default_action IN ('ALLOW', 'DENY')),
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    expires_at BIGINT,
    rate_limit_rpm INTEGER,
    max_concurrent_requests INTEGER,
    quota_daily_requests BIGINT,
    quota_daily_tokens BIGINT,
    quota_monthly_tokens BIGINT,
    budget_daily_nanos BIGINT,
    budget_daily_currency TEXT,
    budget_monthly_nanos BIGINT,
    budget_monthly_currency TEXT,
    secret_ciphertext BLOB,
    secret_nonce BLOB,
    secret_format_version INTEGER,
    secret_key_fingerprint TEXT,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_api_key_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_api_key_key_prefix_not_empty CHECK (key_prefix <> ''),
    CONSTRAINT chk_api_key_key_last4_not_empty CHECK (key_last4 <> ''),
    CONSTRAINT chk_api_key_budget_daily_currency_len CHECK (
        budget_daily_currency IS NULL OR LENGTH(budget_daily_currency) = 3
    ),
    CONSTRAINT chk_api_key_budget_monthly_currency_len CHECK (
        budget_monthly_currency IS NULL OR LENGTH(budget_monthly_currency) = 3
    ),
    CONSTRAINT chk_api_key_expires_at_order CHECK (
        expires_at IS NULL OR expires_at >= created_at
    ),
    CONSTRAINT chk_api_key_limits_non_negative CHECK (
        (rate_limit_rpm IS NULL OR rate_limit_rpm >= 0) AND
        (max_concurrent_requests IS NULL OR max_concurrent_requests >= 0) AND
        (quota_daily_requests IS NULL OR quota_daily_requests >= 0) AND
        (quota_daily_tokens IS NULL OR quota_daily_tokens >= 0) AND
        (quota_monthly_tokens IS NULL OR quota_monthly_tokens >= 0) AND
        (budget_daily_nanos IS NULL OR budget_daily_nanos >= 0) AND
        (budget_monthly_nanos IS NULL OR budget_monthly_nanos >= 0)
    ),
    CONSTRAINT chk_api_key_secret_all_or_none CHECK (
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
    ),
    CONSTRAINT chk_api_key_timestamps CHECK (updated_at >= created_at)
);

INSERT INTO api_key_r26_new (
    id,
    api_key_hash,
    key_prefix,
    key_last4,
    name,
    description,
    default_action,
    is_enabled,
    expires_at,
    rate_limit_rpm,
    max_concurrent_requests,
    quota_daily_requests,
    quota_daily_tokens,
    quota_monthly_tokens,
    budget_daily_nanos,
    budget_daily_currency,
    budget_monthly_nanos,
    budget_monthly_currency,
    secret_ciphertext,
    secret_nonce,
    secret_format_version,
    secret_key_fingerprint,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    id,
    api_key_hash,
    key_prefix,
    key_last4,
    name,
    description,
    default_action,
    is_enabled,
    expires_at,
    rate_limit_rpm,
    max_concurrent_requests,
    quota_daily_requests,
    quota_daily_tokens,
    quota_monthly_tokens,
    budget_daily_nanos,
    budget_daily_currency,
    budget_monthly_nanos,
    budget_monthly_currency,
    NULL,
    NULL,
    NULL,
    NULL,
    deleted_at,
    created_at,
    updated_at
FROM api_key;

DROP TABLE api_key;
ALTER TABLE api_key_r26_new RENAME TO api_key;

CREATE UNIQUE INDEX idx_api_key_hash_uq_not_deleted
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_api_key_name ON api_key (name);
CREATE INDEX idx_api_key_deleted_at ON api_key (deleted_at);
CREATE INDEX idx_api_key_expires_at ON api_key (expires_at);

COMMIT;
PRAGMA foreign_keys = ON;
