CREATE TABLE IF NOT EXISTS custom_field (
    id INTEGER PRIMARY KEY NOT NULL,
    provider_id INTEGER NOT NULL,
    field_name TEXT NOT NULL,
    field_type TEXT NOT NULL, -- e.g., 'text', 'integer', 'float', 'boolean', 'json'
    text_value TEXT,
    integer_value INTEGER,
    float_value REAL,
    boolean_value BOOLEAN,
    description TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);