CREATE TABLE IF NOT EXISTS provider (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_key TEXT NOT NULL,
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT false,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    provider_type TEXT NOT NULL DEFAULT 'OPENAI',
    provider_api_key_mode TEXT NOT NULL DEFAULT 'QUEUE',
    CONSTRAINT chk_provider_type CHECK (
        provider_type IN (
            'OPENAI',
            'GEMINI',
            'VERTEX',
            'VERTEX_OPENAI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_provider_api_key_mode CHECK (
        provider_api_key_mode IN ('QUEUE', 'RANDOM')
    ),
    CONSTRAINT chk_provider_timestamps CHECK (updated_at >= created_at),
    CONSTRAINT chk_provider_key_not_empty CHECK (provider_key <> ''),
    CONSTRAINT chk_provider_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_provider_endpoint_not_empty CHECK (endpoint <> '')
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_key_unique_when_active
    ON provider (provider_key)
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE TABLE IF NOT EXISTS provider_api_key (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    api_key TEXT NOT NULL,
    description TEXT,
    deleted_at BIGINT DEFAULT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_provider_api_key_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_provider_api_key_not_empty CHECK (api_key <> ''),
    CONSTRAINT chk_provider_api_key_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_provider_api_key_pid_apikey_uq_active
    ON provider_api_key (provider_id, api_key)
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE INDEX IF NOT EXISTS idx_pak_provider_id
    ON provider_api_key (provider_id);

CREATE TABLE IF NOT EXISTS cost_catalogs (
    id BIGINT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    deleted_at BIGINT,
    CONSTRAINT chk_cost_catalogs_name_not_empty CHECK (name <> ''),
    CONSTRAINT chk_cost_catalogs_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_cost_catalogs_name_active
    ON cost_catalogs (name)
    WHERE deleted_at IS NULL;

CREATE TABLE IF NOT EXISTS cost_catalog_versions (
    id BIGINT PRIMARY KEY NOT NULL,
    catalog_id BIGINT NOT NULL,
    version TEXT NOT NULL,
    currency TEXT NOT NULL,
    source TEXT,
    effective_from BIGINT NOT NULL,
    effective_until BIGINT,
    first_used_at BIGINT,
    is_archived BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_cost_catalog_versions_catalog_id
        FOREIGN KEY (catalog_id) REFERENCES cost_catalogs (id)
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
    id BIGINT PRIMARY KEY NOT NULL,
    catalog_version_id BIGINT NOT NULL,
    meter_key TEXT NOT NULL,
    charge_kind TEXT NOT NULL,
    unit_price_nanos BIGINT,
    flat_fee_nanos BIGINT,
    tier_config_json TEXT,
    match_attributes_json TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_cost_components_catalog_version_id
        FOREIGN KEY (catalog_version_id) REFERENCES cost_catalog_versions (id)
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
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    cost_catalog_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    supports_streaming BOOLEAN NOT NULL DEFAULT 1,
    supports_tools BOOLEAN NOT NULL DEFAULT 1,
    supports_reasoning BOOLEAN NOT NULL DEFAULT 1,
    supports_image_input BOOLEAN NOT NULL DEFAULT 1,
    supports_embeddings BOOLEAN NOT NULL DEFAULT 1,
    supports_rerank BOOLEAN NOT NULL DEFAULT 1,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_model_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs (id)
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
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE INDEX IF NOT EXISTS idx_model_provider_id
    ON model (provider_id);

CREATE INDEX IF NOT EXISTS idx_model_cost_catalog_id
    ON model (cost_catalog_id);

CREATE TABLE IF NOT EXISTS model_route (
    id BIGINT PRIMARY KEY NOT NULL,
    route_name TEXT NOT NULL,
    description TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    expose_in_models BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
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
    id BIGINT PRIMARY KEY NOT NULL,
    route_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_route_candidate_route_id
        FOREIGN KEY (route_id) REFERENCES model_route (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_model_route_candidate_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
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
    id BIGINT PRIMARY KEY NOT NULL,
    api_key TEXT NOT NULL,
    api_key_hash TEXT,
    key_prefix TEXT NOT NULL,
    key_last4 TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    default_action TEXT NOT NULL DEFAULT 'ALLOW',
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
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT chk_api_key_default_action CHECK (default_action IN ('ALLOW', 'DENY')),
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
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE UNIQUE INDEX IF NOT EXISTS idx_api_key_hash_uq_active
    ON api_key (api_key_hash)
    WHERE deleted_at IS NULL AND is_enabled = true AND api_key_hash IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_api_key_name
    ON api_key (name);

CREATE INDEX IF NOT EXISTS idx_api_key_deleted_at
    ON api_key (deleted_at);

CREATE INDEX IF NOT EXISTS idx_api_key_expires_at
    ON api_key (expires_at);

CREATE TABLE IF NOT EXISTS api_key_acl_rule (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    effect TEXT NOT NULL,
    scope TEXT NOT NULL,
    provider_id BIGINT,
    model_id BIGINT,
    priority INTEGER NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    deleted_at BIGINT,
    CONSTRAINT fk_api_key_acl_rule_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_acl_rule_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_acl_rule_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT chk_api_key_acl_rule_effect CHECK (effect IN ('ALLOW', 'DENY')),
    CONSTRAINT chk_api_key_acl_rule_scope CHECK (scope IN ('PROVIDER', 'MODEL')),
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
    last_request_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (api_key_id, day_bucket, currency),
    CONSTRAINT fk_api_key_rollup_daily_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key (id)
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
    last_request_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    PRIMARY KEY (api_key_id, month_bucket, currency),
    CONSTRAINT fk_api_key_rollup_monthly_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key (id)
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
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    source_name TEXT NOT NULL,
    target_route_id BIGINT NOT NULL,
    description TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_api_key_model_override_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_api_key_model_override_target_route_id
        FOREIGN KEY (target_route_id) REFERENCES model_route (id)
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
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT DEFAULT NULL,
    model_id BIGINT DEFAULT NULL,
    placement TEXT NOT NULL,
    target TEXT NOT NULL,
    operation TEXT NOT NULL,
    value_json TEXT DEFAULT NULL,
    description TEXT DEFAULT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    deleted_at BIGINT DEFAULT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_patch_rule_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_patch_rule_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_patch_rule_scope_xor CHECK (
        (provider_id IS NOT NULL AND model_id IS NULL)
            OR
        (provider_id IS NULL AND model_id IS NOT NULL)
    ),
    CONSTRAINT chk_request_patch_rule_placement CHECK (
        placement IN ('HEADER', 'QUERY', 'BODY')
    ),
    CONSTRAINT chk_request_patch_rule_operation CHECK (
        operation IN ('SET', 'REMOVE')
    ),
    CONSTRAINT chk_request_patch_rule_target_not_empty CHECK (target <> ''),
    CONSTRAINT chk_request_patch_rule_value_shape CHECK (
        (operation = 'SET' AND value_json IS NOT NULL AND json_valid(value_json))
            OR
        (operation = 'REMOVE' AND value_json IS NULL)
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
    WHERE deleted_at IS NULL AND is_enabled = true AND provider_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_request_patch_rule_model_identity_active
    ON request_patch_rule (model_id, placement, target)
    WHERE deleted_at IS NULL AND is_enabled = true AND model_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS request_log (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT,
    resolved_name_scope TEXT,
    resolved_route_id BIGINT,
    resolved_route_name TEXT,
    request_received_at BIGINT NOT NULL,
    first_attempt_started_at BIGINT,
    response_started_to_client_at BIGINT,
    completed_at BIGINT,
    client_ip TEXT,
    final_attempt_id BIGINT,
    final_provider_id BIGINT,
    final_provider_api_key_id BIGINT,
    final_model_id BIGINT,
    final_provider_key_snapshot TEXT,
    final_provider_name_snapshot TEXT,
    final_model_name_snapshot TEXT,
    final_real_model_name_snapshot TEXT,
    final_llm_api_type TEXT,
    overall_status TEXT NOT NULL,
    final_error_code TEXT,
    final_error_message TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    retry_count INTEGER NOT NULL DEFAULT 0,
    fallback_count INTEGER NOT NULL DEFAULT 0,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    cost_catalog_id BIGINT,
    cost_catalog_version_id BIGINT,
    cost_snapshot_json TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    total_input_tokens INTEGER,
    total_output_tokens INTEGER,
    input_text_tokens INTEGER,
    output_text_tokens INTEGER,
    input_image_tokens INTEGER,
    output_image_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens INTEGER,
    total_tokens INTEGER,
    has_transform_diagnostics BOOLEAN NOT NULL DEFAULT 0,
    transform_diagnostic_count INTEGER NOT NULL DEFAULT 0,
    transform_diagnostic_max_loss_level TEXT,
    bundle_version INTEGER,
    bundle_storage_type TEXT,
    bundle_storage_key TEXT,
    user_api_type TEXT NOT NULL,
    CONSTRAINT fk_request_log_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key (id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_id
        FOREIGN KEY (final_provider_id) REFERENCES provider (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_model_id
        FOREIGN KEY (final_model_id) REFERENCES model (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_final_provider_api_key_id
        FOREIGN KEY (final_provider_api_key_id) REFERENCES provider_api_key (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_overall_status CHECK (
        overall_status IN ('SUCCESS', 'ERROR', 'CANCELLED')
    ),
    CONSTRAINT chk_request_log_user_api_type CHECK (
        user_api_type IN (
            'OPENAI',
            'GEMINI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_request_log_final_llm_api_type CHECK (
        final_llm_api_type IS NULL OR final_llm_api_type IN (
            'OPENAI',
            'GEMINI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_request_log_bundle_storage_type CHECK (
        bundle_storage_type IS NULL OR bundle_storage_type IN ('FILE_SYSTEM', 'S3')
    ),
    CONSTRAINT chk_request_log_bundle_version CHECK (
        bundle_version IS NULL OR bundle_version IN (1, 2)
    ),
    CONSTRAINT chk_request_log_attempt_counts_non_negative CHECK (
        attempt_count >= 0 AND retry_count >= 0 AND fallback_count >= 0
    ),
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
        (total_input_tokens IS NULL OR total_input_tokens >= 0) AND
        (total_output_tokens IS NULL OR total_output_tokens >= 0) AND
        (input_text_tokens IS NULL OR input_text_tokens >= 0) AND
        (output_text_tokens IS NULL OR output_text_tokens >= 0) AND
        (input_image_tokens IS NULL OR input_image_tokens >= 0) AND
        (output_image_tokens IS NULL OR output_image_tokens >= 0) AND
        (cache_read_tokens IS NULL OR cache_read_tokens >= 0) AND
        (cache_write_tokens IS NULL OR cache_write_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0) AND
        (total_tokens IS NULL OR total_tokens >= 0)
    ),
    CONSTRAINT chk_request_log_timestamps_order CHECK (
        updated_at >= created_at
        AND (first_attempt_started_at IS NULL OR first_attempt_started_at >= request_received_at)
        AND (
            response_started_to_client_at IS NULL
            OR response_started_to_client_at >= request_received_at
        )
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
    id BIGINT PRIMARY KEY NOT NULL,
    request_log_id BIGINT NOT NULL,
    attempt_index INTEGER NOT NULL,
    candidate_position INTEGER NOT NULL,
    provider_id BIGINT,
    provider_api_key_id BIGINT,
    model_id BIGINT,
    provider_key_snapshot TEXT,
    provider_name_snapshot TEXT,
    model_name_snapshot TEXT,
    real_model_name_snapshot TEXT,
    llm_api_type TEXT,
    attempt_status TEXT NOT NULL,
    scheduler_action TEXT NOT NULL,
    error_code TEXT,
    error_message TEXT,
    request_uri TEXT,
    request_headers_json TEXT,
    response_headers_json TEXT,
    http_status INTEGER,
    started_at BIGINT,
    first_byte_at BIGINT,
    completed_at BIGINT,
    response_started_to_client BOOLEAN NOT NULL DEFAULT 0,
    backoff_ms INTEGER,
    applied_request_patch_ids_json TEXT,
    request_patch_summary_json TEXT,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    cost_catalog_version_id BIGINT,
    total_input_tokens INTEGER,
    total_output_tokens INTEGER,
    input_text_tokens INTEGER,
    output_text_tokens INTEGER,
    input_image_tokens INTEGER,
    output_image_tokens INTEGER,
    cache_read_tokens INTEGER,
    cache_write_tokens INTEGER,
    reasoning_tokens INTEGER,
    total_tokens INTEGER,
    llm_request_blob_id INTEGER,
    llm_request_patch_id INTEGER,
    llm_response_blob_id INTEGER,
    llm_response_capture_state TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_attempt_request_log_id
        FOREIGN KEY (request_log_id) REFERENCES request_log (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_provider_api_key_id
        FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_model_id
        FOREIGN KEY (model_id) REFERENCES model (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_attempt_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions (id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT uq_request_attempt_request_log_attempt_index
        UNIQUE (request_log_id, attempt_index),
    CONSTRAINT chk_request_attempt_attempt_index_positive
        CHECK (attempt_index >= 1),
    CONSTRAINT chk_request_attempt_candidate_position_positive
        CHECK (candidate_position >= 1),
    CONSTRAINT chk_request_attempt_llm_api_type CHECK (
        llm_api_type IS NULL OR llm_api_type IN (
            'OPENAI',
            'GEMINI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_request_attempt_attempt_status CHECK (
        attempt_status IN ('SKIPPED', 'SUCCESS', 'ERROR', 'CANCELLED')
    ),
    CONSTRAINT chk_request_attempt_scheduler_action CHECK (
        scheduler_action IN (
            'RETURN_SUCCESS',
            'FAIL_FAST',
            'RETRY_SAME_CANDIDATE',
            'FALLBACK_NEXT_CANDIDATE'
        )
    ),
    CONSTRAINT chk_request_attempt_http_status CHECK (
        http_status IS NULL OR (http_status >= 100 AND http_status <= 599)
    ),
    CONSTRAINT chk_request_attempt_backoff_ms_non_negative CHECK (
        backoff_ms IS NULL OR backoff_ms >= 0
    ),
    CONSTRAINT chk_request_attempt_tokens_non_negative CHECK (
        (total_input_tokens IS NULL OR total_input_tokens >= 0) AND
        (total_output_tokens IS NULL OR total_output_tokens >= 0) AND
        (input_text_tokens IS NULL OR input_text_tokens >= 0) AND
        (output_text_tokens IS NULL OR output_text_tokens >= 0) AND
        (input_image_tokens IS NULL OR input_image_tokens >= 0) AND
        (output_image_tokens IS NULL OR output_image_tokens >= 0) AND
        (cache_read_tokens IS NULL OR cache_read_tokens >= 0) AND
        (cache_write_tokens IS NULL OR cache_write_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0) AND
        (total_tokens IS NULL OR total_tokens >= 0)
    ),
    CONSTRAINT chk_request_attempt_bundle_ids_non_negative CHECK (
        (llm_request_blob_id IS NULL OR llm_request_blob_id >= 0) AND
        (llm_request_patch_id IS NULL OR llm_request_patch_id >= 0) AND
        (llm_response_blob_id IS NULL OR llm_response_blob_id >= 0)
    ),
    CONSTRAINT chk_request_attempt_timestamps_order CHECK (
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
    source_attempt_id BIGINT,
    replay_kind TEXT NOT NULL,
    replay_mode TEXT NOT NULL,
    semantic_basis TEXT NOT NULL,
    status TEXT NOT NULL,
    executed_route_id BIGINT,
    executed_route_name TEXT,
    executed_provider_id BIGINT,
    executed_provider_api_key_id BIGINT,
    executed_model_id BIGINT,
    executed_llm_api_type TEXT,
    downstream_request_uri TEXT,
    http_status INTEGER,
    error_code TEXT,
    error_message TEXT,
    total_input_tokens INTEGER,
    total_output_tokens INTEGER,
    reasoning_tokens INTEGER,
    total_tokens INTEGER,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    diff_summary_json TEXT,
    artifact_version INTEGER,
    artifact_storage_type TEXT,
    artifact_storage_key TEXT,
    started_at BIGINT,
    first_byte_at BIGINT,
    completed_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_replay_run_source_request_log_id
        FOREIGN KEY (source_request_log_id) REFERENCES request_log (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_replay_run_source_attempt_id
        FOREIGN KEY (source_attempt_id) REFERENCES request_attempt (id)
            ON DELETE CASCADE
            ON UPDATE CASCADE,
    CONSTRAINT chk_request_replay_run_kind CHECK (
        replay_kind IN ('ATTEMPT_UPSTREAM', 'GATEWAY_REQUEST')
    ),
    CONSTRAINT chk_request_replay_run_mode CHECK (
        replay_mode IN ('DRY_RUN', 'LIVE')
    ),
    CONSTRAINT chk_request_replay_run_semantic_basis CHECK (
        semantic_basis IN (
            'HISTORICAL_ATTEMPT_SNAPSHOT',
            'HISTORICAL_REQUEST_SNAPSHOT_WITH_CURRENT_CONFIG'
        )
    ),
    CONSTRAINT chk_request_replay_run_status CHECK (
        status IN ('PENDING', 'RUNNING', 'SUCCESS', 'ERROR', 'CANCELLED', 'REJECTED')
    ),
    CONSTRAINT chk_request_replay_run_source_attempt CHECK (
        (replay_kind = 'ATTEMPT_UPSTREAM' AND source_attempt_id IS NOT NULL)
        OR
        (replay_kind = 'GATEWAY_REQUEST' AND source_attempt_id IS NULL)
    ),
    CONSTRAINT chk_request_replay_run_executed_llm_api_type CHECK (
        executed_llm_api_type IS NULL OR executed_llm_api_type IN (
            'OPENAI',
            'GEMINI',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_request_replay_run_artifact_storage_type CHECK (
        artifact_storage_type IS NULL OR artifact_storage_type IN ('FILE_SYSTEM', 'S3')
    ),
    CONSTRAINT chk_request_replay_run_artifact_locator CHECK (
        (artifact_storage_type IS NULL AND artifact_storage_key IS NULL AND artifact_version IS NULL)
        OR
        (artifact_storage_type IS NOT NULL AND artifact_storage_key IS NOT NULL AND artifact_version IS NOT NULL)
    ),
    CONSTRAINT chk_request_replay_run_tokens_non_negative CHECK (
        (total_input_tokens IS NULL OR total_input_tokens >= 0) AND
        (total_output_tokens IS NULL OR total_output_tokens >= 0) AND
        (reasoning_tokens IS NULL OR reasoning_tokens >= 0) AND
        (total_tokens IS NULL OR total_tokens >= 0)
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
