CREATE TABLE IF NOT EXISTS provider (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_key TEXT NOT NULL,
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    omit_config TEXT,
    limit_model BOOLEAN NOT NULL DEFAULT 0,
    use_proxy BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT 0,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS model (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT 0,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS record (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key_id BIGINT NOT NULL,
    provider_id BIGINT NOT NULL,
    model_id BIGINT,
    model_name TEXT NOT NULL,
    real_model_name TEXT NOT NULL,
    prompt_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_cache_tokens INTEGER NOT NULL DEFAULT 0,
    prompt_audio_tokens INTEGER NOT NULL DEFAULT 0,
    completion_tokens INTEGER NOT NULL DEFAULT 0,
    reasoning_tokens INTEGER NOT NULL DEFAULT 0,
    first_token_time INTEGER,
    response_time INTEGER NOT NULL,
    is_stream BOOLEAN NOT NULL DEFAULT 0,
    request_at BIGINT NOT NULL,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS price (
    id BIGINT PRIMARY KEY NOT NULL,
    model_id BIGINT NOT NULL,
    start_time BIGINT NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CNY',
    input_price INTEGER NOT NULL DEFAULT 0,
    output_price INTEGER NOT NULL DEFAULT 0,
    input_cache_price INTEGER NOT NULL DEFAULT 0,
    output_cache_price INTEGER NOT NULL DEFAULT 0,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_api_key (
    id BIGINT PRIMARY KEY NOT NULL,
    provider_id BIGINT NOT NULL,
    api_key TEXT NOT NULL,
    description TEXT,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    id BIGINT PRIMARY KEY NOT NULL,
    api_key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);
