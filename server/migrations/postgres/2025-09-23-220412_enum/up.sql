-- api key mode
CREATE TYPE provider_api_key_mode_enum AS ENUM ('QUEUE', 'RANDOM');

ALTER TABLE provider
    DROP CONSTRAINT IF EXISTS provider_api_key_mode_check;

ALTER TABLE provider
    ALTER COLUMN provider_api_key_mode DROP DEFAULT,
    ALTER COLUMN provider_api_key_mode TYPE provider_api_key_mode_enum USING (provider_api_key_mode::text::provider_api_key_mode_enum),
    ALTER COLUMN provider_api_key_mode SET DEFAULT 'QUEUE'::provider_api_key_mode_enum;

-- action
CREATE TYPE action_enum AS ENUM ('ALLOW', 'DENY');

ALTER TABLE access_control_policy
    DROP CONSTRAINT IF EXISTS access_control_policy_default_action_check;

ALTER TABLE access_control_policy
    ALTER COLUMN default_action DROP DEFAULT,
    ALTER COLUMN default_action TYPE action_enum USING (default_action::text::action_enum),
    ALTER COLUMN default_action SET DEFAULT 'DENY'::action_enum;

ALTER TABLE access_control_rule
    DROP CONSTRAINT IF EXISTS chk_acr_rule_type;

ALTER TABLE access_control_rule
    ALTER COLUMN rule_type TYPE action_enum USING (rule_type::text::action_enum);

-- scope
CREATE TYPE rule_scope_enum AS ENUM ('PROVIDER', 'MODEL');

ALTER TABLE access_control_rule
    DROP CONSTRAINT IF EXISTS chk_acr_scope,
    DROP CONSTRAINT IF EXISTS chk_acr_scope_ids_consistency;

ALTER TABLE access_control_rule
    ALTER COLUMN scope TYPE rule_scope_enum USING (scope::text::rule_scope_enum),
    ADD CONSTRAINT chk_acr_scope_ids_consistency CHECK (
        (scope = 'PROVIDER'::rule_scope_enum AND provider_id IS NOT NULL AND model_id IS NULL)
            OR
        (scope = 'MODEL'::rule_scope_enum AND provider_id IS NOT NULL AND model_id IS NOT NULL)
        );

-- custom_field_definition
CREATE TYPE field_placement_enum AS ENUM ('HEADER', 'QUERY', 'BODY');
CREATE TYPE field_type_enum AS ENUM ('STRING', 'INTEGER', 'NUMBER', 'BOOLEAN', 'JSON_STRING', 'UNSET');

ALTER TABLE custom_field_definition
    DROP CONSTRAINT IF EXISTS chk_cfd_field_placement,
    DROP CONSTRAINT IF EXISTS chk_cfd_field_type,
    DROP CONSTRAINT IF EXISTS chk_cfd_value_type_coherence;
ALTER TABLE custom_field_definition
    ALTER COLUMN field_type TYPE field_type_enum USING (field_type::text::field_type_enum);
ALTER TABLE custom_field_definition
    ALTER COLUMN field_placement TYPE field_placement_enum USING (field_placement::text::field_placement_enum);
ALTER TABLE custom_field_definition
    ADD CONSTRAINT chk_cfd_value_type_coherence CHECK (
        (field_type = 'STRING'::field_type_enum AND string_value IS NOT NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NULL) OR
        (field_type = 'INTEGER'::field_type_enum AND string_value IS NULL AND integer_value IS NOT NULL AND
         number_value IS NULL AND
         boolean_value IS NULL) OR
        (field_type = 'NUMBER'::field_type_enum AND string_value IS NULL AND integer_value IS NULL AND
         number_value IS NOT NULL AND
         boolean_value IS NULL) OR
        (field_type = 'BOOLEAN'::field_type_enum AND string_value IS NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NOT NULL) OR
        (field_type = 'JSON_STRING'::field_type_enum AND string_value IS NOT NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NULL) OR
        (field_type = 'UNSET'::field_type_enum AND string_value IS NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NULL)
        );

-- request_log
CREATE TYPE request_status_enum AS ENUM ('PENDING', 'SUCCESS', 'ERROR', 'CANCELLED');

ALTER TABLE request_log
    DROP CONSTRAINT IF EXISTS chk_request_log_status,
    ALTER COLUMN status DROP DEFAULT,
    ALTER COLUMN status TYPE request_status_enum USING (status::text::request_status_enum),
    ALTER COLUMN status SET DEFAULT 'PENDING'::request_status_enum;


