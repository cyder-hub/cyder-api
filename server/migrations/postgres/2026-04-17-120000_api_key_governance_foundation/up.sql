CREATE TABLE IF NOT EXISTS api_key (
    id BIGINT PRIMARY KEY,
    api_key TEXT NOT NULL,
    api_key_hash TEXT NULL,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    default_action action_enum NOT NULL DEFAULT 'ALLOW',
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    expires_at BIGINT NULL,
    rate_limit_rpm INTEGER NULL,
    max_concurrent_requests INTEGER NULL,
    quota_daily_requests BIGINT NULL,
    quota_daily_tokens BIGINT NULL,
    quota_monthly_tokens BIGINT NULL,
    budget_daily_nanos BIGINT NULL,
    budget_daily_currency TEXT NULL,
    budget_monthly_nanos BIGINT NULL,
    budget_monthly_currency TEXT NULL,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_api_key_api_key_not_empty CHECK (api_key <> ''),
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
    CONSTRAINT chk_api_key_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_key_uq_active
    ON api_key (api_key)
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_hash_uq_active
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL AND is_enabled = true AND api_key_hash IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_api_key_name ON api_key (name);
CREATE INDEX IF NOT EXISTS idx_api_key_deleted_at ON api_key (deleted_at);
CREATE INDEX IF NOT EXISTS idx_api_key_expires_at ON api_key (expires_at);

INSERT INTO api_key (
    id,
    api_key,
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
    deleted_at,
    created_at,
    updated_at
)
SELECT
    sak.id,
    sak.api_key,
    NULL,
    SUBSTRING(sak.api_key FROM 1 FOR 12),
    CASE
        WHEN LENGTH(sak.api_key) <= 4 THEN sak.api_key
        ELSE RIGHT(sak.api_key, 4)
    END,
    sak.name,
    sak.description,
    COALESCE(acp.default_action, 'ALLOW'::action_enum),
    sak.is_enabled,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    NULL,
    sak.deleted_at,
    sak.created_at,
    sak.updated_at
FROM system_api_key AS sak
LEFT JOIN access_control_policy AS acp
    ON acp.id = sak.access_control_policy_id
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS api_key_acl_rule (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL,
    effect action_enum NOT NULL,
    scope rule_scope_enum NOT NULL,
    provider_id BIGINT NULL,
    model_id BIGINT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    description TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    deleted_at BIGINT NULL,
    CONSTRAINT fk_api_key_acl_rule_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_acl_rule_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_acl_rule_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT chk_api_key_acl_rule_scope_ids CHECK (
        (scope = 'PROVIDER' AND provider_id IS NOT NULL AND model_id IS NULL) OR
        (scope = 'MODEL' AND model_id IS NOT NULL)
    ),
    CONSTRAINT chk_api_key_acl_rule_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_acl_rule_logical_key
    ON api_key_acl_rule (api_key_id, effect, scope, provider_id, model_id)
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE INDEX IF NOT EXISTS idx_api_key_acl_rule_api_key_id
    ON api_key_acl_rule (api_key_id, priority);

WITH migrated_rules AS (
    SELECT
        ROW_NUMBER() OVER (ORDER BY sak.id, acr.priority, acr.id) AS seq,
        sak.id AS api_key_id,
        acr.rule_type AS effect,
        acr.scope AS scope,
        acr.provider_id AS provider_id,
        acr.model_id AS model_id,
        acr.priority AS priority,
        acr.is_enabled AS is_enabled,
        acr.description AS description,
        acr.created_at AS created_at,
        acr.updated_at AS updated_at,
        acr.deleted_at AS deleted_at
    FROM system_api_key AS sak
    JOIN access_control_rule AS acr
        ON acr.policy_id = sak.access_control_policy_id
)
INSERT INTO api_key_acl_rule (
    id,
    api_key_id,
    effect,
    scope,
    provider_id,
    model_id,
    priority,
    is_enabled,
    description,
    created_at,
    updated_at,
    deleted_at
)
SELECT
    -seq::BIGINT,
    api_key_id,
    effect,
    scope,
    provider_id,
    model_id,
    priority,
    is_enabled,
    description,
    created_at,
    updated_at,
    deleted_at
FROM migrated_rules
ON CONFLICT (id) DO NOTHING;

CREATE TABLE IF NOT EXISTS api_key_rollup_daily (
    api_key_id BIGINT NOT NULL,
    day_bucket BIGINT NOT NULL,
    currency TEXT NOT NULL,
    request_count BIGINT NOT NULL DEFAULT 0,
    total_input_tokens BIGINT NOT NULL DEFAULT 0,
    total_output_tokens BIGINT NOT NULL DEFAULT 0,
    total_reasoning_tokens BIGINT NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    billed_amount_nanos BIGINT NOT NULL DEFAULT 0,
    last_request_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (api_key_id, day_bucket, currency),
    CONSTRAINT fk_api_key_rollup_daily_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_api_key_rollup_daily_currency_len CHECK (LENGTH(currency) = 3),
    CONSTRAINT chk_api_key_rollup_daily_non_negative CHECK (
        request_count >= 0 AND
        total_input_tokens >= 0 AND
        total_output_tokens >= 0 AND
        total_reasoning_tokens >= 0 AND
        total_tokens >= 0 AND
        billed_amount_nanos >= 0
    ),
    CONSTRAINT chk_api_key_rollup_daily_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_api_key_rollup_daily_bucket
    ON api_key_rollup_daily (day_bucket, api_key_id);

CREATE TABLE IF NOT EXISTS api_key_rollup_monthly (
    api_key_id BIGINT NOT NULL,
    month_bucket BIGINT NOT NULL,
    currency TEXT NOT NULL,
    request_count BIGINT NOT NULL DEFAULT 0,
    total_input_tokens BIGINT NOT NULL DEFAULT 0,
    total_output_tokens BIGINT NOT NULL DEFAULT 0,
    total_reasoning_tokens BIGINT NOT NULL DEFAULT 0,
    total_tokens BIGINT NOT NULL DEFAULT 0,
    billed_amount_nanos BIGINT NOT NULL DEFAULT 0,
    last_request_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (api_key_id, month_bucket, currency),
    CONSTRAINT fk_api_key_rollup_monthly_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_api_key_rollup_monthly_currency_len CHECK (LENGTH(currency) = 3),
    CONSTRAINT chk_api_key_rollup_monthly_non_negative CHECK (
        request_count >= 0 AND
        total_input_tokens >= 0 AND
        total_output_tokens >= 0 AND
        total_reasoning_tokens >= 0 AND
        total_tokens >= 0 AND
        billed_amount_nanos >= 0
    ),
    CONSTRAINT chk_api_key_rollup_monthly_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_api_key_rollup_monthly_bucket
    ON api_key_rollup_monthly (month_bucket, api_key_id);
