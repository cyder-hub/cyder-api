DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'provider_type_enum') THEN
        CREATE TYPE provider_type_enum AS ENUM (
            'OPENAI',
            'GEMINI',
            'VERTEX',
            'VERTEX_OPENAI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'provider_api_key_mode_enum') THEN
        CREATE TYPE provider_api_key_mode_enum AS ENUM ('QUEUE', 'RANDOM');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'action_enum') THEN
        CREATE TYPE action_enum AS ENUM ('ALLOW', 'DENY');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'rule_scope_enum') THEN
        CREATE TYPE rule_scope_enum AS ENUM ('PROVIDER', 'MODEL');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'llm_api_type_enum') THEN
        CREATE TYPE llm_api_type_enum AS ENUM (
            'OPENAI',
            'GEMINI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_status_enum') THEN
        CREATE TYPE request_status_enum AS ENUM ('PENDING', 'SUCCESS', 'ERROR', 'CANCELLED');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_attempt_status_enum') THEN
        CREATE TYPE request_attempt_status_enum AS ENUM (
            'SKIPPED',
            'SUCCESS',
            'ERROR',
            'CANCELLED'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'scheduler_action_enum') THEN
        CREATE TYPE scheduler_action_enum AS ENUM (
            'RETURN_SUCCESS',
            'FAIL_FAST',
            'RETRY_SAME_CANDIDATE',
            'FALLBACK_NEXT_CANDIDATE'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'storage_type_enum') THEN
        CREATE TYPE storage_type_enum AS ENUM ('FILE_SYSTEM', 'S3');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_patch_placement_enum') THEN
        CREATE TYPE request_patch_placement_enum AS ENUM ('HEADER', 'QUERY', 'BODY');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_patch_operation_enum') THEN
        CREATE TYPE request_patch_operation_enum AS ENUM ('SET', 'REMOVE');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_replay_kind_enum') THEN
        CREATE TYPE request_replay_kind_enum AS ENUM (
            'ATTEMPT_UPSTREAM',
            'GATEWAY_REQUEST'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_replay_mode_enum') THEN
        CREATE TYPE request_replay_mode_enum AS ENUM ('DRY_RUN', 'LIVE');
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_replay_semantic_basis_enum') THEN
        CREATE TYPE request_replay_semantic_basis_enum AS ENUM (
            'HISTORICAL_ATTEMPT_SNAPSHOT',
            'HISTORICAL_REQUEST_SNAPSHOT_WITH_CURRENT_CONFIG'
        );
    END IF;
END
$$;

DO $$
BEGIN
    IF NOT EXISTS (SELECT 1 FROM pg_type WHERE typname = 'request_replay_status_enum') THEN
        CREATE TYPE request_replay_status_enum AS ENUM (
            'PENDING',
            'RUNNING',
            'SUCCESS',
            'ERROR',
            'CANCELLED',
            'REJECTED'
        );
    END IF;
END
$$;

CREATE TABLE IF NOT EXISTS provider (
    id BIGINT PRIMARY KEY,
    provider_key TEXT NOT NULL,
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT FALSE,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    provider_type provider_type_enum NOT NULL DEFAULT 'OPENAI'::provider_type_enum,
    provider_api_key_mode provider_api_key_mode_enum NOT NULL DEFAULT 'QUEUE'::provider_api_key_mode_enum,
    CONSTRAINT chk_provider_timestamps CHECK (updated_at >= created_at),
    CONSTRAINT chk_provider_key_not_empty CHECK (provider_key <> ''),
    CONSTRAINT chk_provider_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_provider_endpoint_not_empty CHECK (endpoint <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_key_unique_when_active
    ON provider (provider_key)
    WHERE deleted_at IS NULL AND is_enabled = TRUE;

CREATE TABLE IF NOT EXISTS provider_api_key (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NOT NULL,
    api_key TEXT NOT NULL,
    description TEXT NULL,
    deleted_at BIGINT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_provider_api_key_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_provider_api_key_not_empty CHECK (api_key <> ''),
    CONSTRAINT chk_provider_api_key_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_api_key_pid_apikey_uq_active
    ON provider_api_key (provider_id, api_key)
    WHERE deleted_at IS NULL AND is_enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_pak_provider_id
    ON provider_api_key (provider_id);

CREATE TABLE IF NOT EXISTS cost_catalogs (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    deleted_at BIGINT NULL,
    CONSTRAINT chk_cost_catalogs_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_cost_catalogs_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cost_catalogs_name_active
    ON cost_catalogs (name)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS cost_catalog_versions (
    id BIGINT PRIMARY KEY,
    catalog_id BIGINT NOT NULL,
    version TEXT NOT NULL,
    currency TEXT NOT NULL,
    source TEXT NULL,
    effective_from BIGINT NOT NULL,
    effective_until BIGINT NULL,
    first_used_at BIGINT NULL,
    is_archived BOOLEAN NOT NULL DEFAULT FALSE,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_cost_catalog_versions_catalog_id
        FOREIGN KEY (catalog_id) REFERENCES cost_catalogs(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_cost_catalog_versions_version_not_empty CHECK (version <> ''),
    CONSTRAINT chk_cost_catalog_versions_currency_len CHECK (LENGTH(currency) = 3),
    CONSTRAINT chk_cost_catalog_versions_effective_range CHECK (
        effective_until IS NULL OR effective_until >= effective_from
    ),
    CONSTRAINT chk_cost_catalog_versions_timestamps CHECK (updated_at >= created_at),
    CONSTRAINT chk_cost_catalog_versions_source_not_empty CHECK (
        source IS NULL OR source <> ''
    )
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cost_catalog_versions_catalog_version
    ON cost_catalog_versions (catalog_id, version);

CREATE INDEX IF NOT EXISTS idx_cost_catalog_versions_lookup
    ON cost_catalog_versions (catalog_id, is_enabled, effective_from, effective_until);

CREATE INDEX IF NOT EXISTS idx_cost_catalog_versions_first_used_at
    ON cost_catalog_versions (first_used_at);

CREATE TABLE IF NOT EXISTS cost_components (
    id BIGINT PRIMARY KEY,
    catalog_version_id BIGINT NOT NULL,
    meter_key TEXT NOT NULL,
    charge_kind TEXT NOT NULL,
    unit_price_nanos BIGINT NULL,
    flat_fee_nanos BIGINT NULL,
    tier_config_json TEXT NULL,
    match_attributes_json TEXT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    description TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_cost_components_catalog_version_id
        FOREIGN KEY (catalog_version_id) REFERENCES cost_catalog_versions(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_cost_components_meter_key_not_empty CHECK (meter_key <> ''),
    CONSTRAINT chk_cost_components_charge_kind CHECK (
        charge_kind IN ('per_unit', 'flat', 'tiered_per_unit')
    ),
    CONSTRAINT chk_cost_components_unit_price_non_negative CHECK (
        unit_price_nanos IS NULL OR unit_price_nanos >= 0
    ),
    CONSTRAINT chk_cost_components_flat_fee_non_negative CHECK (
        flat_fee_nanos IS NULL OR flat_fee_nanos >= 0
    ),
    CONSTRAINT chk_cost_components_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_cost_components_version_priority
    ON cost_components (catalog_version_id, priority, meter_key);

CREATE TABLE IF NOT EXISTS model (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NOT NULL,
    cost_catalog_id BIGINT NULL,
    model_name TEXT NOT NULL,
    real_model_name TEXT NULL,
    supports_streaming BOOLEAN NOT NULL DEFAULT TRUE,
    supports_tools BOOLEAN NOT NULL DEFAULT TRUE,
    supports_reasoning BOOLEAN NOT NULL DEFAULT TRUE,
    supports_image_input BOOLEAN NOT NULL DEFAULT TRUE,
    supports_embeddings BOOLEAN NOT NULL DEFAULT TRUE,
    supports_rerank BOOLEAN NOT NULL DEFAULT TRUE,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_model_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT chk_model_name_not_empty CHECK (model_name <> ''),
    CONSTRAINT chk_model_real_model_name_not_empty CHECK (
        real_model_name IS NULL OR real_model_name <> ''
    ),
    CONSTRAINT chk_model_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_pid_name_uq_active
    ON model (provider_id, model_name)
    WHERE deleted_at IS NULL AND is_enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_model_provider_id
    ON model (provider_id);

CREATE INDEX IF NOT EXISTS idx_model_cost_catalog_id
    ON model (cost_catalog_id);

CREATE TABLE IF NOT EXISTS model_route (
    id BIGINT PRIMARY KEY,
    route_name TEXT NOT NULL,
    description TEXT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    expose_in_models BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_model_route_name_not_empty CHECK (route_name <> ''),
    CONSTRAINT chk_model_route_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_route_name_uq_active
    ON model_route (route_name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_model_route_deleted_at
    ON model_route (deleted_at);

CREATE INDEX IF NOT EXISTS idx_model_route_enabled
    ON model_route (is_enabled);

CREATE TABLE IF NOT EXISTS model_route_candidate (
    id BIGINT PRIMARY KEY,
    route_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_route_candidate_route_id
        FOREIGN KEY (route_id) REFERENCES model_route(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_model_route_candidate_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT chk_model_route_candidate_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_route_candidate_route_model_uq_active
    ON model_route_candidate (route_id, model_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_model_route_candidate_route_id
    ON model_route_candidate (route_id);

CREATE INDEX IF NOT EXISTS idx_model_route_candidate_route_priority
    ON model_route_candidate (route_id, priority, id);

CREATE TABLE IF NOT EXISTS api_key (
    id BIGINT PRIMARY KEY,
    api_key TEXT NOT NULL,
    api_key_hash TEXT NULL,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT NULL,
    default_action action_enum NOT NULL DEFAULT 'ALLOW'::action_enum,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
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
    CONSTRAINT chk_api_key_not_empty CHECK (api_key <> ''),
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
    WHERE deleted_at IS NULL AND is_enabled = TRUE;

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_hash_uq_active
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL AND is_enabled = TRUE AND api_key_hash IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_api_key_name
    ON api_key (name);

CREATE INDEX IF NOT EXISTS idx_api_key_deleted_at
    ON api_key (deleted_at);

CREATE INDEX IF NOT EXISTS idx_api_key_expires_at
    ON api_key (expires_at);

CREATE TABLE IF NOT EXISTS api_key_acl_rule (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL,
    effect action_enum NOT NULL,
    scope rule_scope_enum NOT NULL,
    provider_id BIGINT NULL,
    model_id BIGINT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
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
        (scope = 'PROVIDER'::rule_scope_enum AND provider_id IS NOT NULL AND model_id IS NULL)
        OR
        (scope = 'MODEL'::rule_scope_enum AND model_id IS NOT NULL)
    ),
    CONSTRAINT chk_api_key_acl_rule_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_acl_rule_logical_key
    ON api_key_acl_rule (api_key_id, effect, scope, provider_id, model_id)
    WHERE deleted_at IS NULL AND is_enabled = TRUE;

CREATE INDEX IF NOT EXISTS idx_api_key_acl_rule_api_key_id
    ON api_key_acl_rule (api_key_id, priority);

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

CREATE TABLE IF NOT EXISTS api_key_model_override (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL,
    source_name TEXT NOT NULL,
    target_route_id BIGINT NOT NULL,
    description TEXT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_api_key_model_override_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_model_override_target_route_id
        FOREIGN KEY (target_route_id) REFERENCES model_route(id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT chk_api_key_model_override_name_not_empty CHECK (source_name <> ''),
    CONSTRAINT chk_api_key_model_override_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_model_override_name_uq_active
    ON api_key_model_override (api_key_id, source_name)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_api_key_model_override_api_key_id
    ON api_key_model_override (api_key_id);

CREATE INDEX IF NOT EXISTS idx_api_key_model_override_target_route_id
    ON api_key_model_override (target_route_id);

CREATE TABLE IF NOT EXISTS request_patch_rule (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NULL,
    model_id BIGINT NULL,
    placement request_patch_placement_enum NOT NULL,
    target TEXT NOT NULL,
    operation request_patch_operation_enum NOT NULL,
    value_json TEXT NULL,
    description TEXT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    deleted_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_patch_rule_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_patch_rule_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_patch_rule_scope_xor CHECK (
        (provider_id IS NOT NULL AND model_id IS NULL)
        OR
        (provider_id IS NULL AND model_id IS NOT NULL)
    ),
    CONSTRAINT chk_request_patch_rule_target_not_empty CHECK (target <> ''),
    CONSTRAINT chk_request_patch_rule_value_shape CHECK (
        (operation = 'SET'::request_patch_operation_enum AND value_json IS NOT NULL AND jsonb_typeof(value_json::jsonb) IS NOT NULL)
        OR
        (operation = 'REMOVE'::request_patch_operation_enum AND value_json IS NULL)
    ),
    CONSTRAINT chk_request_patch_rule_timestamps CHECK (updated_at >= created_at)
);

CREATE INDEX IF NOT EXISTS idx_request_patch_rule_provider_id
    ON request_patch_rule (provider_id)
    WHERE deleted_at IS NULL;

CREATE INDEX IF NOT EXISTS idx_request_patch_rule_model_id
    ON request_patch_rule (model_id)
    WHERE deleted_at IS NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_patch_rule_provider_identity_active
    ON request_patch_rule (provider_id, placement, target)
    WHERE deleted_at IS NULL AND is_enabled = TRUE AND provider_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_patch_rule_model_identity_active
    ON request_patch_rule (model_id, placement, target)
    WHERE deleted_at IS NULL AND is_enabled = TRUE AND model_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS request_log (
    id BIGINT PRIMARY KEY,
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT NULL,
    resolved_name_scope TEXT NULL,
    resolved_route_id BIGINT NULL,
    resolved_route_name TEXT NULL,
    request_received_at BIGINT NOT NULL,
    first_attempt_started_at BIGINT NULL,
    response_started_to_client_at BIGINT NULL,
    completed_at BIGINT NULL,
    client_ip TEXT NULL,
    final_attempt_id BIGINT NULL,
    final_provider_id BIGINT NULL,
    final_provider_api_key_id BIGINT NULL,
    final_model_id BIGINT NULL,
    final_provider_key_snapshot TEXT NULL,
    final_provider_name_snapshot TEXT NULL,
    final_model_name_snapshot TEXT NULL,
    final_real_model_name_snapshot TEXT NULL,
    final_llm_api_type llm_api_type_enum NULL,
    overall_status request_status_enum NOT NULL,
    final_error_code TEXT NULL,
    final_error_message TEXT NULL,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    fallback_count INTEGER NOT NULL DEFAULT 0,
    estimated_cost_nanos BIGINT NULL,
    estimated_cost_currency TEXT NULL,
    cost_catalog_id BIGINT NULL,
    cost_catalog_version_id BIGINT NULL,
    cost_snapshot_json TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    total_input_tokens INTEGER NULL,
    total_output_tokens INTEGER NULL,
    input_text_tokens INTEGER NULL,
    output_text_tokens INTEGER NULL,
    input_image_tokens INTEGER NULL,
    output_image_tokens INTEGER NULL,
    cache_read_tokens INTEGER NULL,
    cache_write_tokens INTEGER NULL,
    reasoning_tokens INTEGER NULL,
    total_tokens INTEGER NULL,
    has_transform_diagnostics BOOLEAN NOT NULL DEFAULT FALSE,
    transform_diagnostic_count INTEGER NOT NULL DEFAULT 0,
    transform_diagnostic_max_loss_level TEXT NULL,
    bundle_version INTEGER NULL,
    bundle_storage_type storage_type_enum NULL,
    bundle_storage_key TEXT NULL,
    user_api_type llm_api_type_enum NOT NULL,
    CONSTRAINT fk_request_log_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_id
        FOREIGN KEY (final_provider_id) REFERENCES provider(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_api_key_id
        FOREIGN KEY (final_provider_api_key_id) REFERENCES provider_api_key(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_model_id
        FOREIGN KEY (final_model_id) REFERENCES model(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_bundle_version
        CHECK (bundle_version IS NULL OR bundle_version IN (1, 2)),
    CONSTRAINT chk_request_log_attempt_counts_non_negative
        CHECK (attempt_count >= 0 AND retry_count >= 0 AND fallback_count >= 0),
    CONSTRAINT chk_request_log_tokens_non_negative
        CHECK (
            (total_input_tokens IS NULL OR total_input_tokens >= 0)
            AND (total_output_tokens IS NULL OR total_output_tokens >= 0)
            AND (input_text_tokens IS NULL OR input_text_tokens >= 0)
            AND (output_text_tokens IS NULL OR output_text_tokens >= 0)
            AND (input_image_tokens IS NULL OR input_image_tokens >= 0)
            AND (output_image_tokens IS NULL OR output_image_tokens >= 0)
            AND (cache_read_tokens IS NULL OR cache_read_tokens >= 0)
            AND (cache_write_tokens IS NULL OR cache_write_tokens >= 0)
            AND (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
            AND (total_tokens IS NULL OR total_tokens >= 0)
        ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (
        updated_at >= created_at
        AND (first_attempt_started_at IS NULL OR first_attempt_started_at >= request_received_at)
        AND (response_started_to_client_at IS NULL OR response_started_to_client_at >= request_received_at)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
    )
);

CREATE INDEX IF NOT EXISTS idx_request_log_api_key_id
    ON request_log (api_key_id);

CREATE INDEX IF NOT EXISTS idx_request_log_final_provider_id
    ON request_log (final_provider_id);

CREATE INDEX IF NOT EXISTS idx_request_log_final_model_id
    ON request_log (final_model_id);

CREATE INDEX IF NOT EXISTS idx_request_log_request_received_at
    ON request_log (request_received_at);

CREATE INDEX IF NOT EXISTS idx_request_log_overall_status
    ON request_log (overall_status);

CREATE INDEX IF NOT EXISTS idx_request_log_resolved_route_id
    ON request_log (resolved_route_id);

CREATE INDEX IF NOT EXISTS idx_request_log_final_attempt_id
    ON request_log (final_attempt_id);

CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_id
    ON request_log (cost_catalog_id);

CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_version_id
    ON request_log (cost_catalog_version_id);

CREATE INDEX IF NOT EXISTS idx_request_log_bundle_storage_type
    ON request_log (bundle_storage_type);

CREATE INDEX IF NOT EXISTS idx_request_log_has_transform_diagnostics
    ON request_log (has_transform_diagnostics);

CREATE INDEX IF NOT EXISTS idx_request_log_resolved_name_scope
    ON request_log (resolved_name_scope);

CREATE INDEX IF NOT EXISTS idx_request_log_final_error_code
    ON request_log (final_error_code);

CREATE INDEX IF NOT EXISTS idx_request_log_retry_count
    ON request_log (retry_count);

CREATE INDEX IF NOT EXISTS idx_request_log_fallback_count
    ON request_log (fallback_count);

CREATE INDEX IF NOT EXISTS idx_request_log_total_tokens
    ON request_log (total_tokens);

CREATE INDEX IF NOT EXISTS idx_request_log_estimated_cost_nanos
    ON request_log (estimated_cost_nanos);

CREATE TABLE IF NOT EXISTS request_attempt (
    id BIGINT PRIMARY KEY,
    request_log_id BIGINT NOT NULL,
    attempt_index INTEGER NOT NULL,
    candidate_position INTEGER NOT NULL,
    provider_id BIGINT NULL,
    provider_api_key_id BIGINT NULL,
    model_id BIGINT NULL,
    provider_key_snapshot TEXT NULL,
    provider_name_snapshot TEXT NULL,
    model_name_snapshot TEXT NULL,
    real_model_name_snapshot TEXT NULL,
    llm_api_type llm_api_type_enum NULL,
    attempt_status request_attempt_status_enum NOT NULL,
    scheduler_action scheduler_action_enum NOT NULL,
    error_code TEXT NULL,
    error_message TEXT NULL,
    request_uri TEXT NULL,
    request_headers_json TEXT NULL,
    response_headers_json TEXT NULL,
    http_status INTEGER NULL,
    started_at BIGINT NULL,
    first_byte_at BIGINT NULL,
    completed_at BIGINT NULL,
    response_started_to_client BOOLEAN NOT NULL DEFAULT FALSE,
    backoff_ms INTEGER NULL,
    applied_request_patch_ids_json TEXT NULL,
    request_patch_summary_json TEXT NULL,
    estimated_cost_nanos BIGINT NULL,
    estimated_cost_currency TEXT NULL,
    cost_catalog_version_id BIGINT NULL,
    total_input_tokens INTEGER NULL,
    total_output_tokens INTEGER NULL,
    input_text_tokens INTEGER NULL,
    output_text_tokens INTEGER NULL,
    input_image_tokens INTEGER NULL,
    output_image_tokens INTEGER NULL,
    cache_read_tokens INTEGER NULL,
    cache_write_tokens INTEGER NULL,
    reasoning_tokens INTEGER NULL,
    total_tokens INTEGER NULL,
    llm_request_blob_id INTEGER NULL,
    llm_request_patch_id INTEGER NULL,
    llm_response_blob_id INTEGER NULL,
    llm_response_capture_state TEXT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_attempt_request_log_id
        FOREIGN KEY (request_log_id) REFERENCES request_log(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_provider_api_key_id
        FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT uq_request_attempt_request_log_attempt_index
        UNIQUE (request_log_id, attempt_index),
    CONSTRAINT chk_request_attempt_attempt_index_positive
        CHECK (attempt_index >= 1),
    CONSTRAINT chk_request_attempt_candidate_position_positive
        CHECK (candidate_position >= 1),
    CONSTRAINT chk_request_attempt_http_status
        CHECK (http_status IS NULL OR (http_status >= 100 AND http_status <= 599)),
    CONSTRAINT chk_request_attempt_backoff_ms_non_negative
        CHECK (backoff_ms IS NULL OR backoff_ms >= 0),
    CONSTRAINT chk_request_attempt_tokens_non_negative
        CHECK (
            (total_input_tokens IS NULL OR total_input_tokens >= 0)
            AND (total_output_tokens IS NULL OR total_output_tokens >= 0)
            AND (input_text_tokens IS NULL OR input_text_tokens >= 0)
            AND (output_text_tokens IS NULL OR output_text_tokens >= 0)
            AND (input_image_tokens IS NULL OR input_image_tokens >= 0)
            AND (output_image_tokens IS NULL OR output_image_tokens >= 0)
            AND (cache_read_tokens IS NULL OR cache_read_tokens >= 0)
            AND (cache_write_tokens IS NULL OR cache_write_tokens >= 0)
            AND (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
            AND (total_tokens IS NULL OR total_tokens >= 0)
        ),
    CONSTRAINT chk_request_attempt_bundle_ids_non_negative
        CHECK (
            (llm_request_blob_id IS NULL OR llm_request_blob_id >= 0)
            AND (llm_request_patch_id IS NULL OR llm_request_patch_id >= 0)
            AND (llm_response_blob_id IS NULL OR llm_response_blob_id >= 0)
        ),
    CONSTRAINT chk_request_attempt_timestamps_order
        CHECK (
            updated_at >= created_at
            AND (started_at IS NULL OR started_at >= created_at)
            AND (first_byte_at IS NULL OR started_at IS NULL OR first_byte_at >= started_at)
            AND (completed_at IS NULL OR started_at IS NULL OR completed_at >= started_at)
        )
);

CREATE INDEX IF NOT EXISTS idx_request_attempt_request_log_id
    ON request_attempt (request_log_id);

CREATE INDEX IF NOT EXISTS idx_request_attempt_provider_id
    ON request_attempt (provider_id);

CREATE INDEX IF NOT EXISTS idx_request_attempt_model_id
    ON request_attempt (model_id);

CREATE INDEX IF NOT EXISTS idx_request_attempt_started_at
    ON request_attempt (started_at);

CREATE TABLE IF NOT EXISTS request_replay_run (
    id BIGINT PRIMARY KEY,
    source_request_log_id BIGINT NOT NULL,
    source_attempt_id BIGINT NULL,
    replay_kind request_replay_kind_enum NOT NULL,
    replay_mode request_replay_mode_enum NOT NULL,
    semantic_basis request_replay_semantic_basis_enum NOT NULL,
    status request_replay_status_enum NOT NULL,
    executed_route_id BIGINT NULL,
    executed_route_name TEXT NULL,
    executed_provider_id BIGINT NULL,
    executed_provider_api_key_id BIGINT NULL,
    executed_model_id BIGINT NULL,
    executed_llm_api_type llm_api_type_enum NULL,
    downstream_request_uri TEXT NULL,
    http_status INTEGER NULL,
    error_code TEXT NULL,
    error_message TEXT NULL,
    total_input_tokens INTEGER NULL,
    total_output_tokens INTEGER NULL,
    reasoning_tokens INTEGER NULL,
    total_tokens INTEGER NULL,
    estimated_cost_nanos BIGINT NULL,
    estimated_cost_currency TEXT NULL,
    diff_summary_json TEXT NULL,
    artifact_version INTEGER NULL,
    artifact_storage_type storage_type_enum NULL,
    artifact_storage_key TEXT NULL,
    started_at BIGINT NULL,
    first_byte_at BIGINT NULL,
    completed_at BIGINT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_replay_run_source_request_log_id
        FOREIGN KEY (source_request_log_id) REFERENCES request_log(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_replay_run_source_attempt_id
        FOREIGN KEY (source_attempt_id) REFERENCES request_attempt(id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_replay_run_source_attempt CHECK (
        (replay_kind = 'ATTEMPT_UPSTREAM'::request_replay_kind_enum AND source_attempt_id IS NOT NULL)
        OR
        (replay_kind = 'GATEWAY_REQUEST'::request_replay_kind_enum AND source_attempt_id IS NULL)
    ),
    CONSTRAINT chk_request_replay_run_artifact_locator CHECK (
        (artifact_storage_type IS NULL AND artifact_storage_key IS NULL AND artifact_version IS NULL)
        OR
        (artifact_storage_type IS NOT NULL AND artifact_storage_key IS NOT NULL AND artifact_version IS NOT NULL)
    ),
    CONSTRAINT chk_request_replay_run_tokens_non_negative CHECK (
        (total_input_tokens IS NULL OR total_input_tokens >= 0)
        AND (total_output_tokens IS NULL OR total_output_tokens >= 0)
        AND (reasoning_tokens IS NULL OR reasoning_tokens >= 0)
        AND (total_tokens IS NULL OR total_tokens >= 0)
    ),
    CONSTRAINT chk_request_replay_run_cost_non_negative CHECK (
        estimated_cost_nanos IS NULL OR estimated_cost_nanos >= 0
    )
);

CREATE INDEX IF NOT EXISTS idx_request_replay_run_source_request_log_id
    ON request_replay_run (source_request_log_id);

CREATE INDEX IF NOT EXISTS idx_request_replay_run_source_attempt_id
    ON request_replay_run (source_attempt_id);

CREATE INDEX IF NOT EXISTS idx_request_replay_run_status
    ON request_replay_run (status);

CREATE INDEX IF NOT EXISTS idx_request_replay_run_created_at
    ON request_replay_run (created_at);
