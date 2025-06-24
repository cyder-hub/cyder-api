-- Drop indexes and tables in reverse order of creation,
-- handling dependencies.

-- Model Alias
DROP INDEX IF EXISTS idx_model_alias_deleted_at;
DROP INDEX IF EXISTS idx_model_alias_is_enabled;
DROP INDEX IF EXISTS idx_model_alias_target_model_id;
DROP INDEX IF EXISTS idx_model_alias_name_uq_active;
DROP TABLE IF EXISTS model_alias;

-- Request Log
DROP INDEX IF EXISTS idx_request_log_created_at;
DROP INDEX IF EXISTS idx_request_log_client_ip;
DROP INDEX IF EXISTS idx_request_log_status;
DROP INDEX IF EXISTS idx_request_log_request_received_at;
DROP INDEX IF EXISTS idx_request_log_real_model_name;
DROP INDEX IF EXISTS idx_request_log_model_name;
DROP INDEX IF EXISTS idx_request_log_model_id;
DROP INDEX IF EXISTS idx_request_log_provider_id;
DROP INDEX IF EXISTS idx_request_log_system_api_key_id;
DROP TABLE IF EXISTS request_log;

-- Model Custom Field Assignment
DROP INDEX IF EXISTS idx_mcfa_definition_id;
DROP INDEX IF EXISTS idx_mcfa_model_id_is_enabled;
DROP TABLE IF EXISTS model_custom_field_assignment;

-- Provider Custom Field Assignment
DROP INDEX IF EXISTS idx_pcfa_definition_id;
DROP INDEX IF EXISTS idx_pcfa_provider_id_is_enabled;
DROP TABLE IF EXISTS provider_custom_field_assignment;

-- Custom Field Definition
DROP INDEX IF EXISTS idx_cfd_deleted_at;
DROP INDEX IF EXISTS idx_cfd_is_definition_enabled;
DROP INDEX IF EXISTS idx_cfd_definition_name;
DROP INDEX IF EXISTS idx_cfd_content_uq_not_deleted;
DROP TABLE IF EXISTS custom_field_definition;

-- System API Key (references access_control_policy)
DROP INDEX IF EXISTS idx_system_api_key_name;
DROP INDEX IF EXISTS idx_system_api_key_deleted_at;
DROP INDEX IF EXISTS idx_system_api_key_is_enabled;
DROP INDEX IF EXISTS idx_system_api_key_ac_policy_id;
DROP INDEX IF EXISTS idx_system_api_key_key_uq_active;
DROP TABLE IF EXISTS system_api_key;

-- Access Control Rule (references access_control_policy, provider, model)
DROP INDEX IF EXISTS idx_acr_enabled_deleted_at;
DROP INDEX IF EXISTS idx_acr_policy_id;
DROP INDEX IF EXISTS idx_acr_enabled_logical_key;
DROP TABLE IF EXISTS access_control_rule;

-- Access Control Policy
DROP INDEX IF EXISTS idx_ac_policy_deleted_at;
DROP INDEX IF EXISTS idx_ac_policy_name_uq_not_deleted;
DROP TABLE IF EXISTS access_control_policy;

-- Model (references provider, billing_plans)
DROP INDEX IF EXISTS idx_model_provider_id;
DROP INDEX IF EXISTS idx_model_pid_name_uq_active;
DROP TABLE IF EXISTS model;

-- Price Rules (references billing_plans)
DROP INDEX IF EXISTS idx_pr_lookup_by_plan;
DROP TABLE IF EXISTS price_rules;

-- Billing Plans
DROP INDEX IF EXISTS idx_bp_unique_name;
DROP TABLE IF EXISTS billing_plans;

-- Provider API Key (references provider)
DROP INDEX IF EXISTS idx_pak_provider_id;
DROP INDEX IF EXISTS idx_provider_api_key_pid_apikey_uq_active;
DROP TABLE IF EXISTS provider_api_key;

-- Provider
DROP INDEX IF EXISTS idx_provider_key_unique_when_active;
DROP TABLE IF EXISTS provider;
