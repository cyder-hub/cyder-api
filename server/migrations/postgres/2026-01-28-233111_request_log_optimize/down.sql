-- Drop new columns
ALTER TABLE request_log DROP COLUMN metadata;
ALTER TABLE request_log DROP COLUMN storage_type;
ALTER TABLE request_log DROP COLUMN user_request_body;
ALTER TABLE request_log DROP COLUMN llm_request_body;
ALTER TABLE request_log DROP COLUMN llm_response_body;
ALTER TABLE request_log DROP COLUMN user_response_body;

-- Drop storage type enum
DROP TYPE storage_type_enum;

-- Add back old columns
ALTER TABLE request_log ADD COLUMN llm_request_body TEXT NULL;
ALTER TABLE request_log ADD COLUMN llm_response_body TEXT NULL;
