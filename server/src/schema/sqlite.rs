// @generated automatically by Diesel CLI.

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use diesel::sql_types::{Integer, BigInt, Binary, Bool, Text, Nullable};

    api_key (id) {
        id -> BigInt,
        api_key_hash -> Text,
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
        secret_ciphertext -> Nullable<Binary>,
        secret_nonce -> Nullable<Binary>,
        secret_format_version -> Nullable<Integer>,
        secret_key_fingerprint -> Nullable<Text>,
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
    use diesel::sql_types::{BigInt, Text, Nullable};

    manager_auth_instance (id) {
        id -> BigInt,
        manager_id -> BigInt,
        manager_subject -> Text,
        current_refresh_jti -> Text,
        session_version -> BigInt,
        created_at -> BigInt,
        last_rotated_at -> BigInt,
        expires_at -> BigInt,
        revoked_at -> Nullable<BigInt>,
        revoked_reason -> Nullable<Text>,
    }
}

diesel::table! {
    manager_credential (manager_id) {
        manager_id -> BigInt,
        manager_subject -> Text,
        password_verifier -> Text,
        credential_epoch -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    reasoning_config (id) {
        id -> BigInt,
        scope_kind -> Text,
        provider_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        mode -> Text,
        family_key -> Nullable<Text>,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    reasoning_config_preset (id) {
        id -> BigInt,
        config_id -> BigInt,
        preset_key -> Text,
        expose_in_models -> Bool,
        is_enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    runtime_feature_config (id) {
        id -> BigInt,
        scope_kind -> Text,
        provider_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        feature_key -> Text,
        enabled -> Bool,
        deleted_at -> Nullable<BigInt>,
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
    use crate::schema::enum_def::LlmApiTypeMapping;
    use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text};

    request_log (id) {
        id -> BigInt,
        api_key_id -> BigInt,
        requested_model_name -> Nullable<Text>,
        base_requested_model_name -> Nullable<Text>,
        resolved_reasoning_suffix -> Nullable<Text>,
        resolved_reasoning_preset -> Nullable<Text>,
        request_received_at -> BigInt,
        upstream_request_sent_at -> Nullable<BigInt>,
        #[sql_name = "response_started_to_client_at"]
        llm_response_first_chunk_at -> Nullable<BigInt>,
        #[sql_name = "completed_at"]
        llm_response_completed_at -> Nullable<BigInt>,
        is_stream -> Bool,
        client_ip -> Nullable<Text>,
        provider_id -> Nullable<BigInt>,
        provider_api_key_id -> Nullable<BigInt>,
        model_id -> Nullable<BigInt>,
        provider_key_snapshot -> Nullable<Text>,
        provider_name_snapshot -> Nullable<Text>,
        model_name_snapshot -> Nullable<Text>,
        real_model_name_snapshot -> Nullable<Text>,
        llm_api_type -> Nullable<LlmApiTypeMapping>,
        upstream_http_status -> Nullable<Integer>,
        #[sql_name = "overall_status"]
        status -> RequestStatusMapping,
        final_error_code -> Nullable<Text>,
        final_error_message -> Nullable<Text>,
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
        user_api_type -> LlmApiTypeMapping,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Nullable};

    metric_ingested_request_log (request_log_id) {
        request_log_id -> BigInt,
        request_received_at -> BigInt,
        completed_at -> Nullable<BigInt>,
        ingested_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Nullable, Text};

    metric_request_rollup_minute (bucket_start_ms, scope_type, scope_id) {
        bucket_start_ms -> BigInt,
        scope_type -> Text,
        scope_id -> Text,
        scope_label -> Nullable<Text>,
        request_count -> BigInt,
        success_count -> BigInt,
        error_count -> BigInt,
        cancelled_count -> BigInt,
        first_byte_latency_sum_ms -> BigInt,
        first_byte_latency_count -> BigInt,
        total_latency_sum_ms -> BigInt,
        total_latency_count -> BigInt,
        input_tokens -> BigInt,
        output_tokens -> BigInt,
        reasoning_tokens -> BigInt,
        total_tokens -> BigInt,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Integer, Text};

    metric_http_status_rollup_minute (bucket_start_ms, scope_type, scope_id, http_status) {
        bucket_start_ms -> BigInt,
        scope_type -> Text,
        scope_id -> Text,
        http_status -> Integer,
        count -> BigInt,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    use diesel::sql_types::{BigInt, Text};

    metric_cost_rollup_minute (bucket_start_ms, scope_type, scope_id, currency) {
        bucket_start_ms -> BigInt,
        scope_type -> Text,
        scope_id -> Text,
        currency -> Text,
        amount_nanos -> BigInt,
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

diesel::joinable!(api_key_acl_rule -> api_key (api_key_id));
diesel::joinable!(api_key_acl_rule -> model (model_id));
diesel::joinable!(api_key_acl_rule -> provider (provider_id));
diesel::joinable!(api_key_rollup_daily -> api_key (api_key_id));
diesel::joinable!(api_key_rollup_monthly -> api_key (api_key_id));
diesel::joinable!(cost_catalog_versions -> cost_catalogs (catalog_id));
diesel::joinable!(cost_components -> cost_catalog_versions (catalog_version_id));
diesel::joinable!(model -> cost_catalogs (cost_catalog_id));
diesel::joinable!(model -> provider (provider_id));
diesel::joinable!(provider_api_key -> provider (provider_id));
diesel::joinable!(reasoning_config_preset -> reasoning_config (config_id));
diesel::joinable!(runtime_feature_config -> model (model_id));
diesel::joinable!(runtime_feature_config -> provider (provider_id));
diesel::joinable!(request_log -> api_key (api_key_id));
diesel::joinable!(request_log -> cost_catalog_versions (cost_catalog_version_id));
diesel::joinable!(request_log -> cost_catalogs (cost_catalog_id));
diesel::joinable!(request_log -> model (model_id));
diesel::joinable!(request_log -> provider (provider_id));
diesel::joinable!(request_log -> provider_api_key (provider_api_key_id));
diesel::joinable!(request_patch_rule -> model (model_id));
diesel::joinable!(request_patch_rule -> provider (provider_id));

diesel::allow_tables_to_appear_in_same_query!(
    api_key,
    api_key_acl_rule,
    api_key_rollup_daily,
    api_key_rollup_monthly,
    cost_catalogs,
    cost_catalog_versions,
    cost_components,
    manager_auth_instance,
    manager_credential,
    metric_cost_rollup_minute,
    metric_http_status_rollup_minute,
    metric_ingested_request_log,
    metric_request_rollup_minute,
    model,
    provider,
    provider_api_key,
    reasoning_config,
    reasoning_config_preset,
    runtime_feature_config,
    request_log,
    request_patch_rule,
);
