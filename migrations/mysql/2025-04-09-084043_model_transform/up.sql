CREATE TABLE IF NOT EXISTS model_transform (
    id INTEGER PRIMARY KEY,
    model_name TEXT NOT NULL,
    map_model_name TEXT NOT NULL,
    is_enabled BOOLEAN NOT NULL DEFAULT true,
    is_deleted BOOLEAN NOT NULL DEFAULT false,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
