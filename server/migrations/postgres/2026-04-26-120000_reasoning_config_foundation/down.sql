DROP INDEX IF EXISTS idx_reasoning_config_preset_config_id;
DROP INDEX IF EXISTS idx_reasoning_config_preset_uq_active;
DROP TABLE IF EXISTS reasoning_config_preset;

DROP INDEX IF EXISTS idx_reasoning_config_scope_owner;
DROP INDEX IF EXISTS idx_reasoning_config_model_uq_active;
DROP INDEX IF EXISTS idx_reasoning_config_provider_uq_active;
DROP TABLE IF EXISTS reasoning_config;
