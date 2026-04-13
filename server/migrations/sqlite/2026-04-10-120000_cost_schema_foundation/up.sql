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

PRAGMA foreign_keys=off;

CREATE TABLE model_new (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    cost_catalog_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
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
    CONSTRAINT chk_model_real_model_name_format CHECK (
        real_model_name IS NULL OR real_model_name <> ''
    ),
    CONSTRAINT chk_model_timestamps CHECK (updated_at >= created_at)
);

INSERT INTO model_new (
    id,
    provider_id,
    cost_catalog_id,
    model_name,
    real_model_name,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
)
SELECT
    id,
    provider_id,
    NULL,
    model_name,
    real_model_name,
    is_enabled,
    deleted_at,
    created_at,
    updated_at
FROM model
WHERE EXISTS (
    SELECT 1
    FROM provider
    WHERE provider.id = model.provider_id
);

DROP TABLE model;
ALTER TABLE model_new RENAME TO model;

CREATE UNIQUE INDEX IF NOT EXISTS idx_model_pid_name_uq_active
    ON model (provider_id, model_name)
    WHERE deleted_at IS NULL AND is_enabled = true;

CREATE INDEX IF NOT EXISTS idx_model_provider_id ON model(provider_id);
CREATE INDEX IF NOT EXISTS idx_model_cost_catalog_id ON model(cost_catalog_id);

CREATE TABLE request_log_new (
    id BIGINT PRIMARY KEY NOT NULL,
    system_api_key_id BIGINT NOT NULL,
    provider_id BIGINT NOT NULL,
    model_id BIGINT NOT NULL,
    provider_api_key_id BIGINT NOT NULL,
    model_name TEXT NOT NULL,
    real_model_name TEXT NOT NULL,
    request_received_at BIGINT NOT NULL,
    llm_request_sent_at BIGINT NOT NULL,
    llm_response_first_chunk_at BIGINT,
    llm_response_completed_at BIGINT,
    client_ip TEXT,
    llm_request_uri TEXT,
    llm_response_status INTEGER,
    status TEXT DEFAULT 'PENDING',
    is_stream BOOLEAN NOT NULL DEFAULT false,
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
    storage_type TEXT,
    user_request_body TEXT,
    llm_request_body TEXT,
    llm_response_body TEXT,
    user_response_body TEXT,
    user_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(user_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES')),
    llm_api_type TEXT NOT NULL DEFAULT 'OPENAI' CHECK(llm_api_type IN ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES')),
    CONSTRAINT fk_request_log_system_api_key_id
        FOREIGN KEY (system_api_key_id) REFERENCES system_api_key(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE RESTRICT
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
            ON DELETE SET NULL
            ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id
        FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id)
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
    CONSTRAINT chk_request_log_timestamps_order CHECK (updated_at >= created_at),
    CONSTRAINT chk_request_log_status CHECK (status IN ('PENDING', 'SUCCESS', 'ERROR', 'CANCELLED'))
);

INSERT INTO request_log_new (
    id,
    system_api_key_id,
    provider_id,
    model_id,
    provider_api_key_id,
    model_name,
    real_model_name,
    request_received_at,
    llm_request_sent_at,
    llm_response_first_chunk_at,
    llm_response_completed_at,
    client_ip,
    llm_request_uri,
    llm_response_status,
    status,
    is_stream,
    estimated_cost_nanos,
    estimated_cost_currency,
    created_at,
    updated_at,
    total_input_tokens,
    total_output_tokens,
    input_text_tokens,
    output_text_tokens,
    input_image_tokens,
    output_image_tokens,
    cache_read_tokens,
    cache_write_tokens,
    reasoning_tokens,
    total_tokens,
    storage_type,
    user_request_body,
    llm_request_body,
    llm_response_body,
    user_response_body,
    user_api_type,
    llm_api_type
)
SELECT
    id,
    system_api_key_id,
    provider_id,
    model_id,
    provider_api_key_id,
    model_name,
    real_model_name,
    request_received_at,
    llm_request_sent_at,
    llm_response_first_chunk_at,
    llm_response_completed_at,
    client_ip,
    llm_request_uri,
    llm_response_status,
    status,
    is_stream,
    calculated_cost,
    cost_currency,
    created_at,
    updated_at,
    input_tokens,
    output_tokens,
    NULL,
    NULL,
    input_image_tokens,
    output_image_tokens,
    cached_tokens,
    0,
    reasoning_tokens,
    total_tokens,
    storage_type,
    user_request_body,
    llm_request_body,
    llm_response_body,
    user_response_body,
    user_api_type,
    llm_api_type
FROM request_log
WHERE EXISTS (
    SELECT 1
    FROM system_api_key
    WHERE system_api_key.id = request_log.system_api_key_id
)
AND EXISTS (
    SELECT 1
    FROM provider
    WHERE provider.id = request_log.provider_id
)
AND EXISTS (
    SELECT 1
    FROM model
    WHERE model.id = request_log.model_id
)
AND EXISTS (
    SELECT 1
    FROM provider_api_key
    WHERE provider_api_key.id = request_log.provider_api_key_id
);

DROP TABLE request_log;
ALTER TABLE request_log_new RENAME TO request_log;

CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_id
    ON request_log (cost_catalog_id);
CREATE INDEX IF NOT EXISTS idx_request_log_cost_catalog_version_id
    ON request_log (cost_catalog_version_id);
CREATE INDEX IF NOT EXISTS idx_request_log_received_at
    ON request_log (request_received_at);

PRAGMA foreign_keys=on;
