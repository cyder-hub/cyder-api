-- Drop the old body columns
ALTER TABLE request_log DROP COLUMN llm_request_body;
ALTER TABLE request_log DROP COLUMN llm_response_body;

-- Add new columns for metadata and external storage paths
ALTER TABLE request_log ADD COLUMN metadata JSONB;
ALTER TABLE request_log ADD COLUMN storage_type TEXT;
ALTER TABLE request_log ADD COLUMN user_request_body TEXT;
ALTER TABLE request_log ADD COLUMN llm_request_body TEXT;
ALTER TABLE request_log ADD COLUMN llm_response_body TEXT;
ALTER TABLE request_log ADD COLUMN user_response_body TEXT;

-- Add a CHECK constraint for the new storage_type column
-- Note: SQLite does not allow adding a CHECK constraint to an existing table directly
-- without a table rebuild. For this migration, we are omitting the CHECK constraint
-- to avoid the complexity of a rebuild, assuming application-level validation.
-- If the constraint is critical, a table rebuild would be necessary.