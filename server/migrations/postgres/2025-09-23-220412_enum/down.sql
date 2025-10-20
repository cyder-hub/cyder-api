-- api key mode
ALTER TABLE provider
    ALTER COLUMN provider_api_key_mode DROP DEFAULT,
    ALTER COLUMN provider_api_key_mode TYPE TEXT USING (provider_api_key_mode::text),
    ALTER COLUMN provider_api_key_mode SET DEFAULT 'QUEUE',
    ADD CONSTRAINT provider_api_key_mode_check CHECK ( provider_api_key_mode in ('QUEUE', 'RANDOM') );

DROP TYPE IF EXISTS provider_api_key_mode_enum;

-- action
ALTER TABLE access_control_policy
    ALTER COLUMN default_action DROP DEFAULT,
    ALTER COLUMN default_action TYPE TEXT USING (default_action::text),
    ALTER COLUMN default_action SET DEFAULT 'DENY',
    ADD CONSTRAINT access_control_policy_default_action_check CHECK ( default_action in ('ALLOW', 'DENY') );

ALTER TABLE access_control_rule
    ALTER COLUMN rule_type TYPE TEXT USING (rule_type::text),
    ADD CONSTRAINT chk_acr_rule_type CHECK ( rule_type in ('ALLOW', 'DENY') );

DROP TYPE IF EXISTS action_enum;

-- scope
ALTER TABLE access_control_rule
    DROP CONSTRAINT chk_acr_scope_ids_consistency,
    ALTER COLUMN scope TYPE TEXT USING (scope::text),
    ADD CONSTRAINT chk_acr_scope CHECK ( scope in ('PROVIDER', 'MODEL') ),
    ADD CONSTRAINT chk_acr_scope_ids_consistency CHECK (
        (scope = 'PROVIDER' AND provider_id IS NOT NULL AND model_id IS NULL)
            OR
        (scope = 'MODEL' AND provider_id IS NOT NULL AND model_id IS NOT NULL)
        );

DROP TYPE IF EXISTS rule_scope_enum;

-- custom_field_definition
ALTER TABLE custom_field_definition
    DROP CONSTRAINT chk_cfd_value_type_coherence,
    ALTER COLUMN field_type TYPE TEXT USING (field_type::text),
    ALTER COLUMN field_placement TYPE TEXT USING (field_placement::text),
    ADD CONSTRAINT chk_cfd_value_type_coherence CHECK (
        (field_type = 'STRING' AND string_value IS NOT NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NULL) OR
        (field_type = 'INTEGER' AND string_value IS NULL AND integer_value IS NOT NULL AND
         number_value IS NULL AND
         boolean_value IS NULL) OR
        (field_type = 'NUMBER' AND string_value IS NULL AND integer_value IS NULL AND
         number_value IS NOT NULL AND
         boolean_value IS NULL) OR
        (field_type = 'BOOLEAN' AND string_value IS NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NOT NULL) OR
        (field_type = 'JSON_STRING' AND string_value IS NOT NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NULL) OR
        (field_type = 'UNSET' AND string_value IS NULL AND integer_value IS NULL AND
         number_value IS NULL AND
         boolean_value IS NULL)
        ),
    ADD CONSTRAINT chk_cfd_field_type CHECK ( field_type IN ('HEADER', 'QUERY', 'BODY') ),
    ADD CONSTRAINT chk_cfd_field_placement CHECK ( field_placement IN
                                                   ('STRING', 'INTEGER', 'NUMBER', 'BOOLEAN', 'JSON_STRING', 'UNSET') );

DROP TYPE IF EXISTS field_type_enum;
DROP TYPE IF EXISTS field_placement_enum;

-- request_log
ALTER TABLE request_log
    ALTER COLUMN status DROP DEFAULT,
    ALTER COLUMN status TYPE TEXT USING (status::text),
    ALTER COLUMN status SET DEFAULT 'PENDING',
    ADD CONSTRAINT chk_request_log_status CHECK ( status in ('PENDING', 'SUCCESS', 'ERROR', 'CANCELLED') );

DROP TYPE IF EXISTS request_status_enum;

