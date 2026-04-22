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
    use crate::schema::enum_def::ActionMapping;
    use diesel::sql_types::{Integer, BigInt, Bool, Text, Nullable};

    api_key (id) {
        id -> BigInt,
        #[sql_name = "api_key"]
        api_key_value -> Text,
        api_key_hash -> Nullable<Text>,
        key_prefix -> Text,
        key_last4 -> Text,
        name -> Text,
        description -> Nullable<Text>,
        default_action -> ActionMapping,
        is_enabled -> Bool,
        expires_at -> Nullable<BigInt>,
        rate_limit_rpm -> Nullable<Integer>,
        max_concurrent_requests -> Nullable<Integer>,
        quota_daily_requests -> Nullable<BigInt>,
        quota_daily_tokens -> Nullable<BigInt>,
        quota_monthly_tokens -> Nullable<BigInt>,
        budget_daily_nanos -> Nullable<BigInt>,
        budget_daily_currency -> Nullable<Text>,
        budget_monthly_nanos -> Nullable<BigInt>,
        budget_monthly_currency -> Nullable<Text>,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use crate::schema::enum_def::RuleScopeMapping;
    use diesel::sql_types::{Integer, BigInt, Bool, Text, Nullable};

    api_key_acl_rule (id) {
        id -> BigInt,
        api_key_id -> BigInt,
        effect -> ActionMapping,
        scope -> RuleScopeMapping,
        provider_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        priority -> Integer,
        is_enabled -> Bool,
        description -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Bool, Text, Nullable};

    api_key_model_override (id) {
        id -> BigInt,
        api_key_id -> BigInt,
        source_name -> Text,
        target_route_id -> BigInt,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Text, Nullable};

    api_key_rollup_daily (api_key_id, day_bucket, currency) {
        api_key_id -> BigInt,
        day_bucket -> BigInt,
        currency -> Text,
        request_count -> BigInt,
        total_input_tokens -> BigInt,
        total_output_tokens -> BigInt,
        total_reasoning_tokens -> BigInt,
        total_tokens -> BigInt,
        billed_amount_nanos -> BigInt,
        last_request_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Text, Nullable};

    api_key_rollup_monthly (api_key_id, month_bucket, currency) {
        api_key_id -> BigInt,
        month_bucket -> BigInt,
        currency -> Text,
        request_count -> BigInt,
        total_input_tokens -> BigInt,
        total_output_tokens -> BigInt,
        total_reasoning_tokens -> BigInt,
        total_tokens -> BigInt,
        billed_amount_nanos -> BigInt,
        last_request_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    cost_catalogs (id) {
        id -> BigInt,
        name -> Text,
        description -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        deleted_at -> Nullable<BigInt>,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Bool, Text, Nullable};

    cost_catalog_versions (id) {
        id -> BigInt,
        catalog_id -> BigInt,
        version -> Text,
        currency -> Text,
        source -> Nullable<Text>,
        effective_from -> BigInt,
        effective_until -> Nullable<BigInt>,
        first_used_at -> Nullable<BigInt>,
        is_archived -> Bool,
        is_enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{Integer, BigInt, Text, Nullable};

    cost_components (id) {
        id -> BigInt,
        catalog_version_id -> BigInt,
        meter_key -> Text,
        charge_kind -> Text,
        unit_price_nanos -> Nullable<BigInt>,
        flat_fee_nanos -> Nullable<BigInt>,
        tier_config_json -> Nullable<Text>,
        match_attributes_json -> Nullable<Text>,
        priority -> Integer,
        description -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    model (id) {
        id -> BigInt,
        provider_id -> BigInt,
        cost_catalog_id -> Nullable<BigInt>,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        supports_streaming -> Bool,
        supports_tools -> Bool,
        supports_reasoning -> Bool,
        supports_image_input -> Bool,
        supports_embeddings -> Bool,
        supports_rerank -> Bool,
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
    model_route (id) {
        id -> BigInt,
        route_name -> Text,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        expose_in_models -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{Integer, BigInt, Bool, Nullable};

    model_route_candidate (id) {
        id -> BigInt,
        route_id -> BigInt,
        model_id -> BigInt,
        priority -> Integer,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
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
    use crate::schema::enum_def::RequestStatusMapping;
    use crate::schema::enum_def::StorageTypeMapping;
    use crate::schema::enum_def::LlmApiTypeMapping;
    use diesel::sql_types::{Integer, BigInt, Text, Nullable};

    request_log (id) {
        id -> BigInt,
        api_key_id -> BigInt,
        requested_model_name -> Nullable<Text>,
        resolved_name_scope -> Nullable<Text>,
        resolved_route_id -> Nullable<BigInt>,
        resolved_route_name -> Nullable<Text>,
        request_received_at -> BigInt,
        #[sql_name = "first_attempt_started_at"]
        llm_request_sent_at -> Nullable<BigInt>,
        #[sql_name = "response_started_to_client_at"]
        llm_response_first_chunk_at -> Nullable<BigInt>,
        #[sql_name = "completed_at"]
        llm_response_completed_at -> Nullable<BigInt>,
        client_ip -> Nullable<Text>,
        #[sql_name = "final_attempt_id"]
        final_attempt_id -> Nullable<BigInt>,
        #[sql_name = "final_provider_id"]
        provider_id -> Nullable<BigInt>,
        #[sql_name = "final_provider_api_key_id"]
        provider_api_key_id -> Nullable<BigInt>,
        #[sql_name = "final_model_id"]
        model_id -> Nullable<BigInt>,
        final_provider_key_snapshot -> Nullable<Text>,
        final_provider_name_snapshot -> Nullable<Text>,
        #[sql_name = "final_model_name_snapshot"]
        model_name -> Nullable<Text>,
        #[sql_name = "final_real_model_name_snapshot"]
        real_model_name -> Nullable<Text>,
        #[sql_name = "final_llm_api_type"]
        llm_api_type -> Nullable<LlmApiTypeMapping>,
        #[sql_name = "overall_status"]
        status -> RequestStatusMapping,
        final_error_code -> Nullable<Text>,
        final_error_message -> Nullable<Text>,
        attempt_count -> Integer,
        retry_count -> Integer,
        fallback_count -> Integer,
        estimated_cost_nanos -> Nullable<BigInt>,
        estimated_cost_currency -> Nullable<Text>,
        cost_catalog_id -> Nullable<BigInt>,
        cost_catalog_version_id -> Nullable<BigInt>,
        cost_snapshot_json -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
        total_input_tokens -> Nullable<Integer>,
        total_output_tokens -> Nullable<Integer>,
        input_text_tokens -> Nullable<Integer>,
        output_text_tokens -> Nullable<Integer>,
        input_image_tokens -> Nullable<Integer>,
        output_image_tokens -> Nullable<Integer>,
        cache_read_tokens -> Nullable<Integer>,
        cache_write_tokens -> Nullable<Integer>,
        reasoning_tokens -> Nullable<Integer>,
        total_tokens -> Nullable<Integer>,
        bundle_version -> Nullable<Integer>,
        #[sql_name = "bundle_storage_type"]
        storage_type -> Nullable<StorageTypeMapping>,
        bundle_storage_key -> Nullable<Text>,
        user_api_type -> LlmApiTypeMapping,
    }
}

diesel::table! {
    use crate::schema::enum_def::LlmApiTypeMapping;
    use crate::schema::enum_def::RequestAttemptStatusMapping;
    use crate::schema::enum_def::SchedulerActionMapping;
    use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text};

    request_attempt (id) {
        id -> BigInt,
        request_log_id -> BigInt,
        attempt_index -> Integer,
        candidate_position -> Integer,
        provider_id -> Nullable<BigInt>,
        provider_api_key_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        provider_key_snapshot -> Nullable<Text>,
        provider_name_snapshot -> Nullable<Text>,
        model_name_snapshot -> Nullable<Text>,
        real_model_name_snapshot -> Nullable<Text>,
        llm_api_type -> Nullable<LlmApiTypeMapping>,
        attempt_status -> RequestAttemptStatusMapping,
        scheduler_action -> SchedulerActionMapping,
        error_code -> Nullable<Text>,
        error_message -> Nullable<Text>,
        request_uri -> Nullable<Text>,
        request_headers_json -> Nullable<Text>,
        response_headers_json -> Nullable<Text>,
        http_status -> Nullable<Integer>,
        started_at -> Nullable<BigInt>,
        first_byte_at -> Nullable<BigInt>,
        completed_at -> Nullable<BigInt>,
        response_started_to_client -> Bool,
        backoff_ms -> Nullable<Integer>,
        applied_request_patch_ids_json -> Nullable<Text>,
        request_patch_summary_json -> Nullable<Text>,
        estimated_cost_nanos -> Nullable<BigInt>,
        estimated_cost_currency -> Nullable<Text>,
        cost_catalog_version_id -> Nullable<BigInt>,
        total_input_tokens -> Nullable<Integer>,
        total_output_tokens -> Nullable<Integer>,
        input_text_tokens -> Nullable<Integer>,
        output_text_tokens -> Nullable<Integer>,
        input_image_tokens -> Nullable<Integer>,
        output_image_tokens -> Nullable<Integer>,
        cache_read_tokens -> Nullable<Integer>,
        cache_write_tokens -> Nullable<Integer>,
        reasoning_tokens -> Nullable<Integer>,
        total_tokens -> Nullable<Integer>,
        llm_request_blob_id -> Nullable<Integer>,
        llm_request_patch_id -> Nullable<Integer>,
        llm_response_blob_id -> Nullable<Integer>,
        llm_response_capture_state -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
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

diesel::table! {
    use crate::schema::enum_def::RequestPatchOperationMapping;
    use crate::schema::enum_def::RequestPatchPlacementMapping;
    use diesel::sql_types::{BigInt, Bool, Nullable, Text};

    request_patch_rule (id) {
        id -> BigInt,
        provider_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        placement -> RequestPatchPlacementMapping,
        target -> Text,
        operation -> RequestPatchOperationMapping,
        value_json -> Nullable<Text>,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::joinable!(access_control_rule -> access_control_policy (policy_id));
diesel::joinable!(access_control_rule -> model (model_id));
diesel::joinable!(access_control_rule -> provider (provider_id));
diesel::joinable!(api_key_acl_rule -> api_key (api_key_id));
diesel::joinable!(api_key_model_override -> api_key (api_key_id));
diesel::joinable!(api_key_model_override -> model_route (target_route_id));
diesel::joinable!(api_key_acl_rule -> model (model_id));
diesel::joinable!(api_key_acl_rule -> provider (provider_id));
diesel::joinable!(api_key_rollup_daily -> api_key (api_key_id));
diesel::joinable!(api_key_rollup_monthly -> api_key (api_key_id));
diesel::joinable!(cost_catalog_versions -> cost_catalogs (catalog_id));
diesel::joinable!(cost_components -> cost_catalog_versions (catalog_version_id));
diesel::joinable!(model -> cost_catalogs (cost_catalog_id));
diesel::joinable!(model -> provider (provider_id));
diesel::joinable!(model_alias -> model (target_model_id));
diesel::joinable!(model_route_candidate -> model (model_id));
diesel::joinable!(model_route_candidate -> model_route (route_id));
diesel::joinable!(provider_api_key -> provider (provider_id));
diesel::joinable!(request_attempt -> cost_catalog_versions (cost_catalog_version_id));
diesel::joinable!(request_attempt -> model (model_id));
diesel::joinable!(request_attempt -> provider (provider_id));
diesel::joinable!(request_attempt -> provider_api_key (provider_api_key_id));
diesel::joinable!(request_attempt -> request_log (request_log_id));
diesel::joinable!(request_log -> api_key (api_key_id));
diesel::joinable!(request_log -> cost_catalog_versions (cost_catalog_version_id));
diesel::joinable!(request_log -> cost_catalogs (cost_catalog_id));
diesel::joinable!(request_log -> model (model_id));
diesel::joinable!(request_log -> provider (provider_id));
diesel::joinable!(request_log -> provider_api_key (provider_api_key_id));
diesel::joinable!(request_patch_rule -> model (model_id));
diesel::joinable!(request_patch_rule -> provider (provider_id));
diesel::joinable!(system_api_key -> access_control_policy (access_control_policy_id));

diesel::allow_tables_to_appear_in_same_query!(
    access_control_policy,
    access_control_rule,
    api_key,
    api_key_acl_rule,
    api_key_model_override,
    api_key_rollup_daily,
    api_key_rollup_monthly,
    cost_catalogs,
    cost_catalog_versions,
    cost_components,
    model,
    model_alias,
    model_route,
    model_route_candidate,
    provider,
    provider_api_key,
    request_attempt,
    request_log,
    request_patch_rule,
    system_api_key,
);
