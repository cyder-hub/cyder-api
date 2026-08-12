-- R3.16 is an intentionally destructive pre-1.0 Provider execution-domain
-- cutover. No Provider-domain row is migrated or kept as a compatibility
-- source. Manager auth, downstream API keys and their daily/monthly rollups,
-- cost catalogs, and non-Provider startup configuration remain untouched.
-- This migration has no down migration.

PRAGMA foreign_keys = OFF;

DELETE FROM metric_ingested_request_log;
DELETE FROM metric_request_rollup_minute;
DELETE FROM metric_http_status_rollup_minute;
DELETE FROM metric_cost_rollup_minute;
DELETE FROM request_log;
DELETE FROM request_patch_rule;
DELETE FROM request_patch_variant;
DELETE FROM model_source_binding;
-- RuleScope has only PROVIDER and MODEL, so every ACL row is Provider-domain.
DELETE FROM api_key_acl_rule;
DELETE FROM model;
DELETE FROM upstream_source;
DELETE FROM provider_api_key;
DELETE FROM provider;

DROP TABLE request_log;
DROP TABLE model;
DROP TABLE upstream_source;

CREATE TABLE upstream_source (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    profile_type TEXT NOT NULL,
    base_url TEXT NOT NULL,
    use_proxy BOOLEAN NOT NULL DEFAULT 0,
    chat_completions_enabled BOOLEAN,
    chat_completions_path_override TEXT,
    embeddings_enabled BOOLEAN,
    embeddings_path_override TEXT,
    rerank_enabled BOOLEAN,
    rerank_path_override TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    is_default BOOLEAN NOT NULL DEFAULT 0,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_upstream_source_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT chk_upstream_source_profile_type CHECK (
        profile_type IN (
            'OPENAI',
            'OPENAI_COMPATIBLE',
            'GEMINI',
            'VERTEX',
            'OLLAMA',
            'ANTHROPIC',
            'RESPONSES',
            'GEMINI_OPENAI'
        )
    ),
    CONSTRAINT chk_upstream_source_base_url_not_empty CHECK (base_url <> ''),
    CONSTRAINT chk_upstream_source_operation_paths CHECK (
        (chat_completions_path_override IS NULL OR chat_completions_path_override <> '')
        AND (embeddings_path_override IS NULL OR embeddings_path_override <> '')
        AND (rerank_path_override IS NULL OR rerank_path_override <> '')
    ),
    CONSTRAINT chk_upstream_source_operation_shape CHECK (
        (
            profile_type IN ('OPENAI', 'OPENAI_COMPATIBLE', 'GEMINI_OPENAI')
            AND chat_completions_enabled IS NOT NULL
            AND embeddings_enabled IS NOT NULL
            AND rerank_enabled IS NOT NULL
            AND (
                profile_type = 'OPENAI_COMPATIBLE'
                OR (rerank_enabled = 0 AND rerank_path_override IS NULL)
            )
        )
        OR
        (
            profile_type NOT IN ('OPENAI', 'OPENAI_COMPATIBLE', 'GEMINI_OPENAI')
            AND chat_completions_enabled IS NULL
            AND chat_completions_path_override IS NULL
            AND embeddings_enabled IS NULL
            AND embeddings_path_override IS NULL
            AND rerank_enabled IS NULL
            AND rerank_path_override IS NULL
        )
    ),
    CONSTRAINT chk_upstream_source_flags CHECK (
        (is_default = 0 OR (is_enabled = 1 AND deleted_at IS NULL))
        AND (deleted_at IS NULL OR (is_enabled = 0 AND is_default = 0))
    ),
    CONSTRAINT chk_upstream_source_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX idx_upstream_source_provider_default_unique
    ON upstream_source (provider_id)
    WHERE deleted_at IS NULL AND is_default = 1;

CREATE UNIQUE INDEX idx_upstream_source_provider_wire_family_unique
    ON upstream_source (
        provider_id,
        CASE
            WHEN profile_type IN ('OPENAI', 'OPENAI_COMPATIBLE', 'GEMINI_OPENAI') THEN 'OPENAI'
            WHEN profile_type = 'RESPONSES' THEN 'RESPONSES'
            WHEN profile_type = 'ANTHROPIC' THEN 'ANTHROPIC'
            WHEN profile_type IN ('GEMINI', 'VERTEX') THEN 'GEMINI'
            WHEN profile_type = 'OLLAMA' THEN 'OLLAMA'
        END
    )
    WHERE deleted_at IS NULL;

CREATE INDEX idx_upstream_source_provider_id
    ON upstream_source (provider_id);

CREATE TABLE model (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    cost_catalog_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    model_kind TEXT NOT NULL,
    source_selection_mode TEXT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    deleted_at BIGINT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_model_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider (id)
            ON DELETE CASCADE ON UPDATE CASCADE,
    CONSTRAINT fk_model_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs (id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_model_name_not_empty CHECK (model_name <> ''),
    CONSTRAINT chk_model_real_model_name_not_empty CHECK (
        real_model_name IS NULL OR real_model_name <> ''
    ),
    CONSTRAINT chk_model_kind CHECK (model_kind IN ('CHAT', 'EMBEDDING', 'RERANK')),
    CONSTRAINT chk_model_source_selection_mode CHECK (
        source_selection_mode IN ('INHERIT_ALL', 'EXPLICIT')
    ),
    CONSTRAINT chk_model_timestamps CHECK (updated_at >= created_at)
);

CREATE UNIQUE INDEX idx_model_pid_name_uq_active
    ON model (provider_id, model_name)
    WHERE deleted_at IS NULL AND is_enabled = 1;
CREATE INDEX idx_model_provider_id ON model (provider_id);
CREATE INDEX idx_model_cost_catalog_id ON model (cost_catalog_id);

CREATE TABLE request_log (
    id BIGINT PRIMARY KEY NOT NULL,
    request_id TEXT NOT NULL
        CONSTRAINT chk_request_log_request_id CHECK (
            length(request_id) = 36
            AND request_id = lower(request_id)
            AND substr(request_id, 9, 1) = '-'
            AND substr(request_id, 14, 1) = '-'
            AND substr(request_id, 15, 1) = '4'
            AND substr(request_id, 19, 1) = '-'
            AND substr(request_id, 20, 1) GLOB '[89ab]'
            AND substr(request_id, 24, 1) = '-'
            AND length(replace(request_id, '-', '')) = 32
            AND replace(request_id, '-', '') NOT GLOB '*[^0-9a-f]*'
        ),
    client_request_id TEXT
        CONSTRAINT chk_request_log_client_request_id CHECK (
            client_request_id IS NULL
            OR (
                length(client_request_id) BETWEEN 1 AND 64
                AND client_request_id NOT GLOB '*[^A-Za-z0-9._:-]*'
            )
        ),
    api_key_id BIGINT NOT NULL,
    requested_model_name TEXT,
    base_requested_model_name TEXT,
    resolved_patch_suffix TEXT,
    downstream_protocol TEXT NOT NULL,
    overall_status TEXT NOT NULL,
    final_error_code TEXT,
    final_error_message TEXT,
    request_received_at BIGINT NOT NULL,
    upstream_request_sent_at BIGINT,
    upstream_response_headers_at BIGINT,
    upstream_first_body_chunk_at BIGINT,
    first_response_body_at BIGINT,
    first_token_at BIGINT,
    max_upstream_response_idle_ms BIGINT,
    completed_at BIGINT,
    is_stream BOOLEAN NOT NULL DEFAULT 0,
    client_ip TEXT,
    provider_id BIGINT,
    provider_api_key_id BIGINT,
    model_id BIGINT,
    source_id BIGINT,
    source_selection_reason TEXT
        CONSTRAINT chk_request_log_source_selection_reason CHECK (
            source_selection_reason IS NULL
            OR source_selection_reason IN (
                'protocol_match',
                'provider_default_transform',
                'model_default_transform'
            )
        ),
    provider_key_snapshot TEXT,
    provider_name_snapshot TEXT,
    model_name_snapshot TEXT,
    real_model_name_snapshot TEXT,
    model_kind_snapshot TEXT,
    source_profile_type_snapshot TEXT,
    source_base_url_snapshot TEXT,
    upstream_protocol TEXT,
    upstream_http_status INTEGER,
    estimated_cost_nanos BIGINT,
    estimated_cost_currency TEXT,
    cost_catalog_id BIGINT,
    cost_catalog_version_id BIGINT,
    cost_snapshot_json TEXT,
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
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    CONSTRAINT fk_request_log_api_key_id
        FOREIGN KEY (api_key_id) REFERENCES api_key(id)
            ON DELETE RESTRICT ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_id
        FOREIGN KEY (provider_id) REFERENCES provider(id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_provider_api_key_id
        FOREIGN KEY (provider_api_key_id) REFERENCES provider_api_key(id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_model_id
        FOREIGN KEY (model_id) REFERENCES model(id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_source_id
        FOREIGN KEY (source_id) REFERENCES upstream_source(id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_id
        FOREIGN KEY (cost_catalog_id) REFERENCES cost_catalogs(id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT fk_request_log_cost_catalog_version_id
        FOREIGN KEY (cost_catalog_version_id) REFERENCES cost_catalog_versions(id)
            ON DELETE SET NULL ON UPDATE CASCADE,
    CONSTRAINT chk_request_log_overall_status CHECK (
        overall_status IN ('SUCCESS', 'ERROR', 'CANCELLED')
    ),
    CONSTRAINT chk_request_log_downstream_protocol CHECK (
        downstream_protocol IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI')
    ),
    CONSTRAINT chk_request_log_upstream_protocol CHECK (
        upstream_protocol IS NULL
        OR upstream_protocol IN ('OPENAI', 'RESPONSES', 'ANTHROPIC', 'GEMINI', 'OLLAMA')
    ),
    CONSTRAINT chk_request_log_upstream_http_status CHECK (
        upstream_http_status IS NULL OR upstream_http_status BETWEEN 100 AND 599
    ),
    CONSTRAINT chk_request_log_model_snapshot CHECK (
        (model_id IS NULL AND model_kind_snapshot IS NULL)
        OR (model_id IS NOT NULL AND model_kind_snapshot IN ('CHAT', 'EMBEDDING', 'RERANK'))
    ),
    CONSTRAINT chk_request_log_source_snapshot CHECK (
        (
            source_id IS NULL
            AND source_profile_type_snapshot IS NULL
            AND source_base_url_snapshot IS NULL
        )
        OR (
            source_id IS NOT NULL
            AND source_profile_type_snapshot IN (
                'OPENAI', 'OPENAI_COMPATIBLE', 'GEMINI', 'VERTEX',
                'OLLAMA', 'ANTHROPIC', 'RESPONSES', 'GEMINI_OPENAI'
            )
            AND source_base_url_snapshot <> ''
        )
    ),
    CONSTRAINT chk_request_log_tokens_non_negative CHECK (
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
    CONSTRAINT chk_request_log_timing_contract CHECK (
        updated_at >= created_at
        AND (upstream_request_sent_at IS NULL OR upstream_request_sent_at >= request_received_at)
        AND (
            upstream_response_headers_at IS NULL
            OR (upstream_request_sent_at IS NOT NULL AND upstream_response_headers_at >= upstream_request_sent_at)
        )
        AND (
            upstream_first_body_chunk_at IS NULL
            OR (upstream_response_headers_at IS NOT NULL AND upstream_first_body_chunk_at >= upstream_response_headers_at)
        )
        AND (
            first_response_body_at IS NULL
            OR (upstream_request_sent_at IS NOT NULL AND first_response_body_at >= upstream_request_sent_at)
        )
        AND (
            first_response_body_at IS NULL
            OR upstream_first_body_chunk_at IS NULL
            OR first_response_body_at >= upstream_first_body_chunk_at
        )
        AND (
            first_token_at IS NULL
            OR (is_stream = 1 AND upstream_first_body_chunk_at IS NOT NULL AND first_token_at >= upstream_first_body_chunk_at)
        )
        AND (max_upstream_response_idle_ms IS NULL OR max_upstream_response_idle_ms >= 0)
        AND (completed_at IS NULL OR completed_at >= request_received_at)
        AND (completed_at IS NULL OR upstream_request_sent_at IS NULL OR completed_at >= upstream_request_sent_at)
        AND (completed_at IS NULL OR upstream_response_headers_at IS NULL OR completed_at >= upstream_response_headers_at)
        AND (completed_at IS NULL OR upstream_first_body_chunk_at IS NULL OR completed_at >= upstream_first_body_chunk_at)
        AND (completed_at IS NULL OR first_response_body_at IS NULL OR completed_at >= first_response_body_at)
        AND (completed_at IS NULL OR first_token_at IS NULL OR completed_at >= first_token_at)
    )
);

CREATE INDEX idx_request_log_received_at ON request_log (request_received_at DESC);
CREATE INDEX idx_request_log_api_key_received_at ON request_log (api_key_id, request_received_at DESC);
CREATE INDEX idx_request_log_provider_received_at ON request_log (provider_id, request_received_at DESC);
CREATE INDEX idx_request_log_model_received_at ON request_log (model_id, request_received_at DESC);
CREATE INDEX idx_request_log_source_received_at ON request_log (source_id, request_received_at DESC);
CREATE INDEX idx_request_log_status_received_at ON request_log (overall_status, request_received_at DESC);
CREATE INDEX idx_request_log_downstream_protocol_received_at
    ON request_log (downstream_protocol, request_received_at DESC);
CREATE UNIQUE INDEX idx_request_log_request_id ON request_log (request_id);
CREATE INDEX idx_request_log_client_request_id ON request_log (client_request_id);

PRAGMA foreign_key_check;
PRAGMA foreign_keys = ON;
