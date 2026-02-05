// @generated automatically by Diesel CLI.

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use diesel::sql_types::{BigInt, Text, Nullable};

    access_control_policy (id) {
        id -> BigInt,
        name -> Text,
        description -> Nullable<Text>,
        default_action -> ActionMapping,
        created_at -> BigInt,
        updated_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use crate::schema::enum_def::RuleScopeMapping;
    use diesel::sql_types::{Integer, BigInt, Bool, Text, Nullable};

    access_control_rule (id) {
        id -> BigInt,
        policy_id -> BigInt,
        rule_type -> ActionMapping,
        priority -> Integer,
        scope -> RuleScopeMapping,
        provider_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        is_enabled -> Bool,
        description -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    billing_plans (id) {
        id -> BigInt,
        name -> Text,
        description -> Nullable<Text>,
        is_default -> Bool,
        currency -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    use crate::schema::enum_def::FieldPlacementMapping;
    use crate::schema::enum_def::FieldTypeMapping;
    use diesel::sql_types::{BigInt, Float, Text, Bool, Nullable};

    custom_field_definition (id) {
        id -> BigInt,
        definition_name -> Nullable<Text>,
        definition_description -> Nullable<Text>,
        field_name -> Text,
        field_placement -> FieldPlacementMapping,
        field_type -> FieldTypeMapping,
        string_value -> Nullable<Text>,
        integer_value -> Nullable<BigInt>,
        number_value -> Nullable<Float>,
        boolean_value -> Nullable<Bool>,
        is_definition_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    model (id) {
        id -> BigInt,
        provider_id -> BigInt,
        billing_plan_id -> Nullable<BigInt>,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    model_alias (id) {
        id -> BigInt,
        alias_name -> Text,
        target_model_id -> BigInt,
        description -> Nullable<Text>,
        priority -> Nullable<Integer>,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    model_custom_field_assignment (model_id, custom_field_definition_id) {
        model_id -> BigInt,
        custom_field_definition_id -> BigInt,
        is_enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    price_rules (id) {
        id -> BigInt,
        plan_id -> BigInt,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        effective_from -> BigInt,
        effective_until -> Nullable<BigInt>,
        period_start_seconds_utc -> Nullable<Integer>,
        period_end_seconds_utc -> Nullable<Integer>,
        usage_type -> Text,
        media_type -> Nullable<Text>,
        condition_had_reasoning -> Nullable<Integer>,
        tier_from_tokens -> Nullable<Integer>,
        tier_to_tokens -> Nullable<Integer>,
        price_in_micro_units -> BigInt,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use crate::schema::enum_def::ProviderTypeMapping;
    use crate::schema::enum_def::ProviderApiKeyModeMapping;
    use diesel::sql_types::{BigInt, Text, Bool, Nullable};

    provider (id) {
        id -> BigInt,
        provider_key -> Text,
        name -> Text,
        endpoint -> Text,
        use_proxy -> Bool,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
        provider_type -> ProviderTypeMapping,
        provider_api_key_mode -> ProviderApiKeyModeMapping,
    }
}

diesel::table! {
    provider_api_key (id) {
        id -> BigInt,
        provider_id -> BigInt,
        api_key -> Text,
        description -> Nullable<Text>,
        deleted_at -> Nullable<BigInt>,
        is_enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    provider_custom_field_assignment (provider_id, custom_field_definition_id) {
        provider_id -> BigInt,
        custom_field_definition_id -> BigInt,
        is_enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use crate::schema::enum_def::RequestStatusMapping;
    use crate::schema::enum_def::StorageTypeMapping;
    use crate::schema::enum_def::LlmApiTypeMapping;
    use diesel::sql_types::{Bool, Integer, BigInt, Text, Nullable, Jsonb};

    request_log (id) {
        id -> BigInt,
        system_api_key_id -> BigInt,
        provider_id -> BigInt,
        model_id -> BigInt,
        provider_api_key_id -> BigInt,
        model_name -> Text,
        real_model_name -> Text,
        request_received_at -> BigInt,
        llm_request_sent_at -> BigInt,
        llm_response_first_chunk_at -> Nullable<BigInt>,
        llm_response_completed_at -> Nullable<BigInt>,
        client_ip -> Nullable<Text>,
        llm_request_uri -> Nullable<Text>,
        llm_response_status -> Nullable<Integer>,
        status -> Nullable<RequestStatusMapping>,
        is_stream -> Bool,
        calculated_cost -> Nullable<BigInt>,
        cost_currency -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        input_tokens -> Nullable<Integer>,
        output_tokens -> Nullable<Integer>,
        input_image_tokens -> Nullable<Integer>,
        output_image_tokens -> Nullable<Integer>,
        cached_tokens -> Nullable<Integer>,
        reasoning_tokens -> Nullable<Integer>,
        total_tokens -> Nullable<Integer>,
        storage_type -> Nullable<StorageTypeMapping>,
        user_request_body -> Nullable<Text>,
        llm_request_body -> Nullable<Text>,
        llm_response_body -> Nullable<Text>,
        user_response_body -> Nullable<Text>,
        user_api_type -> LlmApiTypeMapping,
        llm_api_type -> LlmApiTypeMapping,
    }
}

diesel::table! {
    system_api_key (id) {
        id -> BigInt,
        api_key -> Text,
        name -> Text,
        description -> Nullable<Text>,
        access_control_policy_id -> Nullable<BigInt>,
        usage_limit_policy_id -> Nullable<BigInt>,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::joinable!(access_control_rule -> access_control_policy (policy_id));
diesel::joinable!(access_control_rule -> model (model_id));
diesel::joinable!(access_control_rule -> provider (provider_id));
diesel::joinable!(model -> billing_plans (billing_plan_id));
diesel::joinable!(model -> provider (provider_id));
diesel::joinable!(model_alias -> model (target_model_id));
diesel::joinable!(model_custom_field_assignment -> custom_field_definition (custom_field_definition_id));
diesel::joinable!(model_custom_field_assignment -> model (model_id));
diesel::joinable!(price_rules -> billing_plans (plan_id));
diesel::joinable!(provider_api_key -> provider (provider_id));
diesel::joinable!(provider_custom_field_assignment -> custom_field_definition (custom_field_definition_id));
diesel::joinable!(provider_custom_field_assignment -> provider (provider_id));
diesel::joinable!(request_log -> model (model_id));
diesel::joinable!(request_log -> provider (provider_id));
diesel::joinable!(request_log -> provider_api_key (provider_api_key_id));
diesel::joinable!(request_log -> system_api_key (system_api_key_id));
diesel::joinable!(system_api_key -> access_control_policy (access_control_policy_id));

diesel::allow_tables_to_appear_in_same_query!(
    access_control_policy,
    access_control_rule,
    billing_plans,
    custom_field_definition,
    model,
    model_alias,
    model_custom_field_assignment,
    price_rules,
    provider,
    provider_api_key,
    provider_custom_field_assignment,
    request_log,
    system_api_key,
);
