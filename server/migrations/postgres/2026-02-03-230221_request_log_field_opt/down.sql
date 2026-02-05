-- This file should undo anything in `up.sql`
ALTER TABLE system_api_key ADD COLUMN ref TEXT;

ALTER TABLE request_log
    DROP COLUMN cached_tokens,
    DROP COLUMN input_image_tokens,
    DROP COLUMN output_image_tokens,
    DROP COLUMN user_api_type,
    DROP COLUMN llm_api_type;

ALTER TABLE request_log
    RENAME COLUMN input_tokens TO prompt_tokens,
    RENAME COLUMN output_tokens TO completion_tokens;

DROP TYPE llm_api_type_enum;

ALTER TABLE request_log
    ALTER COLUMN system_api_key_id DROP NOT NULL,
    ALTER COLUMN provider_id DROP NOT NULL,
    ALTER COLUMN model_id DROP NOT NULL,
    ALTER COLUMN provider_api_key_id DROP NOT NULL,
    ALTER COLUMN model_name DROP NOT NULL,
    ALTER COLUMN real_model_name DROP NOT NULL,
    ALTER COLUMN llm_request_sent_at DROP NOT NULL;

ALTER TABLE request_log
    ADD COLUMN response_sent_to_client_at bigint,
    ADD COLUMN external_request_uri text,
    ADD COLUMN channel text,
    ADD COLUMN external_id text;

CREATE INDEX idx_request_log_channel ON request_log (channel);
CREATE INDEX idx_request_log_external_id ON request_log (external_id);
