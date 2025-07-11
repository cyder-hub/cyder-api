ALTER TABLE provider ALTER COLUMN provider_type DROP DEFAULT;

ALTER TABLE provider ALTER COLUMN provider_type TYPE TEXT USING provider_type::TEXT;

DROP TYPE IF EXISTS provider_type_enum;

ALTER TABLE provider ALTER COLUMN provider_type SET DEFAULT 'OPENAI';

ALTER TABLE provider
ADD CONSTRAINT provider_type_check
CHECK (provider_type IN ('OPENAI', 'GEMINI', 'VERTEX', 'VERTEX_OPENAI', 'OLLAMA'));
