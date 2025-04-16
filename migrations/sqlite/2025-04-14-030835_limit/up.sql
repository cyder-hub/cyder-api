CREATE TABLE IF NOT EXISTS limit_strategy (
  id BIGINT PRIMARY KEY NOT NULL,
    main_strategy TEXT NOT NULL DEFAULT 'default', -- unlimited, default
    name TEXT NOT NULL,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS limit_strategy_item (
    id BIGINT PRIMARY KEY NOT NULL,
    limit_strategy_id BIGINT NOT NULL,
    limit_strategy_type TEXT NOT NULL, -- white, black, limit
    resource_type TEXT NOT NULL, -- global, provider, model
    resource_id BIGINT, -- provider_id or model_id, NULL for global
    limit_type TEXT NOT NULL, -- fee, request
    limit_value INTEGER, -- request for times, fee for 1000 * per millian tokens
    duration TEXT -- minute, hour, day
);

ALTER TABLE api_keys ADD COLUMN limit_strategy_id BIGINT NULL;

