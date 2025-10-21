// @generated automatically by Diesel CLI.

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use diesel::sql_types::{Int8, Text, Nullable};

    access_control_policy (id) {
        id -> Int8,
        name -> Text,
        description -> Nullable<Text>,
        default_action -> ActionMapping,
        created_at -> Int8,
        updated_at -> Int8,
        deleted_at -> Nullable<Int8>,
    }
}

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use crate::schema::enum_def::RuleScopeMapping;
    use diesel::sql_types::{Int4, Int8, Bool, Text, Nullable};

    access_control_rule (id) {
        id -> Int8,
        policy_id -> Int8,
        rule_type -> ActionMapping,
        priority -> Int4,
        scope -> RuleScopeMapping,
        provider_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        is_enabled -> Bool,
        description -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
        deleted_at -> Nullable<Int8>,
    }
}

diesel::table! {
    billing_plans (id) {
        id -> Int8,
        name -> Text,
        description -> Nullable<Text>,
        currency -> Text,
        created_at -> Int8,
        updated_at -> Int8,
        deleted_at -> Nullable<Int8>,
    }
}

diesel::table! {
    use crate::schema::enum_def::FieldPlacementMapping;
    use crate::schema::enum_def::FieldTypeMapping;
    use diesel::sql_types::{Int8, Float4, Text, Bool, Nullable};

    custom_field_definition (id) {
        id -> Int8,
        definition_name -> Nullable<Text>,
        definition_description -> Nullable<Text>,
        field_name -> Text,
        field_placement -> FieldPlacementMapping,
        field_type -> FieldTypeMapping,
        string_value -> Nullable<Text>,
        integer_value -> Nullable<Int8>,
        number_value -> Nullable<Float4>,
        boolean_value -> Nullable<Bool>,
        is_definition_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    model (id) {
        id -> Int8,
        provider_id -> Int8,
        billing_plan_id -> Nullable<Int8>,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    model_alias (id) {
        id -> Int8,
        alias_name -> Text,
        target_model_id -> Int8,
        description -> Nullable<Text>,
        priority -> Nullable<Int4>,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    model_custom_field_assignment (model_id, custom_field_definition_id) {
        model_id -> Int8,
        custom_field_definition_id -> Int8,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    price_rules (id) {
        id -> Int8,
        plan_id -> Int8,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        effective_from -> Int8,
        effective_until -> Nullable<Int8>,
        period_start_seconds_utc -> Nullable<Int4>,
        period_end_seconds_utc -> Nullable<Int4>,
        usage_type -> Text,
        media_type -> Nullable<Text>,
        condition_had_reasoning -> Nullable<Int4>,
        tier_from_tokens -> Nullable<Int4>,
        tier_to_tokens -> Nullable<Int4>,
        price_in_micro_units -> Int8,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use crate::schema::enum_def::ProviderTypeMapping;
    use crate::schema::enum_def::ProviderApiKeyModeMapping;
    use diesel::sql_types::{Int8, Text, Bool, Nullable};

    provider (id) {
        id -> Int8,
        provider_key -> Text,
        name -> Text,
        endpoint -> Text,
        use_proxy -> Bool,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
        provider_type -> ProviderTypeMapping,
        provider_api_key_mode -> ProviderApiKeyModeMapping,
    }
}

diesel::table! {
    provider_api_key (id) {
        id -> Int8,
        provider_id -> Int8,
        api_key -> Text,
        description -> Nullable<Text>,
        deleted_at -> Nullable<Int8>,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    provider_custom_field_assignment (provider_id, custom_field_definition_id) {
        provider_id -> Int8,
        custom_field_definition_id -> Int8,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use crate::schema::enum_def::RequestStatusMapping;
    use diesel::sql_types::{Bool, Int4, Int8, Text, Nullable};

    request_log (id) {
        id -> Int8,
        system_api_key_id -> Nullable<Int8>,
        provider_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        provider_api_key_id -> Nullable<Int8>,
        model_name -> Nullable<Text>,
        real_model_name -> Nullable<Text>,
        request_received_at -> Int8,
        llm_request_sent_at -> Nullable<Int8>,
        llm_response_first_chunk_at -> Nullable<Int8>,
        llm_response_completed_at -> Nullable<Int8>,
        response_sent_to_client_at -> Nullable<Int8>,
        client_ip -> Nullable<Text>,
        external_request_uri -> Nullable<Text>,
        llm_request_uri -> Nullable<Text>,
        llm_response_status -> Nullable<Int4>,
        llm_request_body -> Nullable<Text>,
        llm_response_body -> Nullable<Text>,
        status -> Nullable<RequestStatusMapping>,
        is_stream -> Bool,
        calculated_cost -> Nullable<Int8>,
        cost_currency -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
        prompt_tokens -> Nullable<Int4>,
        completion_tokens -> Nullable<Int4>,
        reasoning_tokens -> Nullable<Int4>,
        total_tokens -> Nullable<Int4>,
        channel -> Nullable<Text>,
        external_id -> Nullable<Text>,
    }
}

diesel::table! {
    system_api_key (id) {
        id -> Int8,
        api_key -> Text,
        name -> Text,
        description -> Nullable<Text>,
        access_control_policy_id -> Nullable<Int8>,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
        #[sql_name = "ref"]
        ref_ -> Nullable<Text>,
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
