CREATE TYPE provider_type_enum AS ENUM ('OPENAI', 'GEMINI', 'VERTEX', 'VERTEX_OPENAI', 'OLLAMA');

ALTER TABLE provider DROP CONSTRAINT IF EXISTS provider_type_check;

ALTER TABLE provider
ALTER COLUMN provider_type DROP DEFAULT;

ALTER TABLE provider
ALTER COLUMN provider_type TYPE provider_type_enum
USING provider_type::provider_type_enum;

ALTER TABLE provider
ALTER COLUMN provider_type SET DEFAULT 'OPENAI'::provider_type_enum;
