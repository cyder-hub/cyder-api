CREATE TABLE IF NOT EXISTS custom_field (
    id BIGINT PRIMARY KEY,
    provider_id BIGINT NOT NULL,
    field_name TEXT NOT NULL,
    field_type TEXT NOT NULL, -- e.g., 'text', 'integer', 'float', 'boolean', 'json'
    text_value TEXT,
    integer_value INTEGER,
    float_value FLOAT,
    boolean_value BOOLEAN,
    description TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL
);