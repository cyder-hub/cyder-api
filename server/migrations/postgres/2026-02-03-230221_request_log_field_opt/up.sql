-- Your SQL goes here
CREATE TYPE llm_api_type_enum AS ENUM ('OPENAI', 'GEMINI', 'OLLAMA', 'ANTHROPIC', 'RESPONSES');

ALTER TABLE request_log
    DROP COLUMN response_sent_to_client_at,
    DROP COLUMN external_request_uri,
    DROP COLUMN channel,
    DROP COLUMN external_id,
    DROP COLUMN metadata;

ALTER TABLE request_log
    ALTER COLUMN system_api_key_id SET NOT NULL,
    ALTER COLUMN provider_id SET NOT NULL,
    ALTER COLUMN model_id SET NOT NULL,
    ALTER COLUMN provider_api_key_id SET NOT NULL,
    ALTER COLUMN model_name SET NOT NULL,
    ALTER COLUMN real_model_name SET NOT NULL,
    ALTER COLUMN llm_request_sent_at SET NOT NULL;

ALTER TABLE request_log RENAME COLUMN prompt_tokens TO input_tokens;
ALTER TABLE request_log RENAME COLUMN completion_tokens TO output_tokens;

ALTER TABLE request_log
    ADD COLUMN cached_tokens INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN input_image_tokens INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN output_image_tokens INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN user_api_type llm_api_type_enum NOT NULL DEFAULT 'OPENAI'::llm_api_type_enum,
    ADD COLUMN llm_api_type llm_api_type_enum NOT NULL DEFAULT 'OPENAI'::llm_api_type_enum;

-- enum add action should not be revert in down.sql
ALTER TYPE provider_type_enum ADD VALUE 'ANTHROPIC';
ALTER TYPE provider_type_enum ADD VALUE 'RESPONSES';

-- drop ref in system_api_key
ALTER TABLE system_api_key DROP COLUMN ref;
