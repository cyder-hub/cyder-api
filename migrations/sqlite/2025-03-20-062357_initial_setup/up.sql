CREATE TABLE IF NOT EXISTS provider (
    id INTEGER PRIMARY KEY NOT NULL,
    provider_key TEXT NOT NULL,
    name TEXT NOT NULL,
    endpoint TEXT NOT NULL,
    omit_config TEXT,
    limit_model BOOLEAN NOT NULL DEFAULT 0,
    use_proxy BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT 0,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS model (
    id INTEGER PRIMARY KEY NOT NULL,
    provider_id INTEGER NOT NULL,
    model_name TEXT NOT NULL,
    real_model_name TEXT,
    is_enabled BOOLEAN NOT NULL DEFAULT 0,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS record (
    id INTEGER PRIMARY KEY NOT NULL,
    api_key_id INTEGER NOT NULL,
    provider_id INTEGER NOT NULL,
    model_id INTEGER,
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
    request_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS price (
    id INTEGER PRIMARY KEY NOT NULL,
    model_id INTEGER NOT NULL,
    start_time INTEGER NOT NULL,
    currency TEXT NOT NULL DEFAULT 'CNY',
    input_price INTEGER NOT NULL DEFAULT 0,
    output_price INTEGER NOT NULL DEFAULT 0,
    input_cache_price INTEGER NOT NULL DEFAULT 0,
    output_cache_price INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_api_key (
    id INTEGER PRIMARY KEY NOT NULL,
    provider_id INTEGER NOT NULL,
    api_key TEXT NOT NULL,
    description TEXT,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS api_keys (
    id INTEGER PRIMARY KEY NOT NULL,
    api_key TEXT NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    is_deleted BOOLEAN NOT NULL DEFAULT 0,
    is_enabled BOOLEAN NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
