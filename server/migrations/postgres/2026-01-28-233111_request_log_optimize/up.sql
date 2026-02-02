-- Add storage type enum
CREATE TYPE storage_type_enum AS ENUM ('FILE_SYSTEM', 'S3');

-- Drop old columns
ALTER TABLE request_log DROP COLUMN llm_request_body;
ALTER TABLE request_log DROP COLUMN llm_response_body;

-- Add new columns
ALTER TABLE request_log ADD COLUMN metadata JSONB NULL;
ALTER TABLE request_log ADD COLUMN storage_type storage_type_enum NULL;
ALTER TABLE request_log ADD COLUMN user_request_body TEXT NULL;
ALTER TABLE request_log ADD COLUMN llm_request_body TEXT NULL;
ALTER TABLE request_log ADD COLUMN llm_response_body TEXT NULL;
ALTER TABLE request_log ADD COLUMN user_response_body TEXT NULL;
