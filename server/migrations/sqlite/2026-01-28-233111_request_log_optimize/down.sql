-- Drop the new columns
ALTER TABLE request_log DROP COLUMN metadata;
ALTER TABLE request_log DROP COLUMN storage_type;
ALTER TABLE request_log DROP COLUMN user_request_body;
ALTER TABLE request_log DROP COLUMN llm_request_body;
ALTER TABLE request_log DROP COLUMN llm_response_body;
ALTER TABLE request_log DROP COLUMN user_response_body;

-- Add the old text columns back
ALTER TABLE request_log ADD COLUMN llm_request_body TEXT;
ALTER TABLE request_log ADD COLUMN llm_response_body TEXT;