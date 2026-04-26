DROP INDEX IF EXISTS idx_model_reasoning_profile_override_id;
DROP INDEX IF EXISTS idx_provider_default_reasoning_profile_id;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS base_requested_model_name;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS resolved_reasoning_suffix;

ALTER TABLE request_log
    DROP COLUMN IF EXISTS resolved_reasoning_preset;

ALTER TABLE model
    DROP COLUMN IF EXISTS reasoning_profile_override_id;

ALTER TABLE provider
    DROP COLUMN IF EXISTS default_reasoning_profile_id;

DROP INDEX IF EXISTS idx_reasoning_profile_preset_profile_id;
DROP INDEX IF EXISTS idx_reasoning_profile_preset_uq_active;
DROP TABLE IF EXISTS reasoning_profile_preset;

DROP INDEX IF EXISTS idx_reasoning_profile_enabled;
DROP INDEX IF EXISTS idx_reasoning_profile_key_uq_active;
DROP TABLE IF EXISTS reasoning_profile;
