// @generated automatically by Diesel CLI.

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use diesel::sql_types::{Int4, Int8, Binary, Bool, Text, Nullable};

    api_key (id) {
        id -> Int8,
        api_key_hash -> Text,
        key_prefix -> Text,
        key_last4 -> Text,
        name -> Text,
        description -> Nullable<Text>,
        default_action -> ActionMapping,
        is_enabled -> Bool,
        expires_at -> Nullable<Int8>,
        rate_limit_rpm -> Nullable<Int4>,
        max_concurrent_requests -> Nullable<Int4>,
        quota_daily_requests -> Nullable<Int8>,
        quota_daily_tokens -> Nullable<Int8>,
        quota_monthly_tokens -> Nullable<Int8>,
        budget_daily_nanos -> Nullable<Int8>,
        budget_daily_currency -> Nullable<Text>,
        budget_monthly_nanos -> Nullable<Int8>,
        budget_monthly_currency -> Nullable<Text>,
        secret_ciphertext -> Nullable<Binary>,
        secret_nonce -> Nullable<Binary>,
        secret_format_version -> Nullable<Int4>,
        secret_key_fingerprint -> Nullable<Text>,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use crate::schema::enum_def::ActionMapping;
    use crate::schema::enum_def::RuleScopeMapping;
    use diesel::sql_types::{Int4, Int8, Bool, Text, Nullable};

    api_key_acl_rule (id) {
        id -> Int8,
        api_key_id -> Int8,
        effect -> ActionMapping,
        scope -> RuleScopeMapping,
        provider_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        priority -> Int4,
        is_enabled -> Bool,
        description -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
        deleted_at -> Nullable<Int8>,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Text, Nullable};

    api_key_rollup_daily (api_key_id, day_bucket, currency) {
        api_key_id -> Int8,
        day_bucket -> Int8,
        currency -> Text,
        request_count -> Int8,
        total_input_tokens -> Int8,
        total_output_tokens -> Int8,
        total_reasoning_tokens -> Int8,
        total_tokens -> Int8,
        billed_amount_nanos -> Int8,
        last_request_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Text, Nullable};

    api_key_rollup_monthly (api_key_id, month_bucket, currency) {
        api_key_id -> Int8,
        month_bucket -> Int8,
        currency -> Text,
        request_count -> Int8,
        total_input_tokens -> Int8,
        total_output_tokens -> Int8,
        total_reasoning_tokens -> Int8,
        total_tokens -> Int8,
        billed_amount_nanos -> Int8,
        last_request_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    cost_catalogs (id) {
        id -> Int8,
        name -> Text,
        description -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
        deleted_at -> Nullable<Int8>,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Bool, Text, Nullable};

    cost_catalog_versions (id) {
        id -> Int8,
        catalog_id -> Int8,
        version -> Text,
        currency -> Text,
        source -> Nullable<Text>,
        effective_from -> Int8,
        effective_until -> Nullable<Int8>,
        first_used_at -> Nullable<Int8>,
        is_archived -> Bool,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int4, Int8, Text, Nullable};

    cost_components (id) {
        id -> Int8,
        catalog_version_id -> Int8,
        meter_key -> Text,
        charge_kind -> Text,
        unit_price_nanos -> Nullable<Int8>,
        flat_fee_nanos -> Nullable<Int8>,
        tier_config_json -> Nullable<Text>,
        match_attributes_json -> Nullable<Text>,
        priority -> Int4,
        description -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Text, Nullable};

    manager_auth_instance (id) {
        id -> Int8,
        manager_id -> Int8,
        manager_subject -> Text,
        current_refresh_jti -> Text,
        session_version -> Int8,
        created_at -> Int8,
        last_rotated_at -> Int8,
        expires_at -> Int8,
        revoked_at -> Nullable<Int8>,
        revoked_reason -> Nullable<Text>,
    }
}

diesel::table! {
    manager_credential (manager_id) {
        manager_id -> Int8,
        manager_subject -> Text,
        password_verifier -> Text,
        credential_epoch -> Text,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    reasoning_config (id) {
        id -> Int8,
        scope_kind -> Text,
        provider_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        mode -> Text,
        family_key -> Nullable<Text>,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    reasoning_config_preset (id) {
        id -> Int8,
        config_id -> Int8,
        preset_key -> Text,
        expose_in_models -> Bool,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    runtime_feature_config (id) {
        id -> Int8,
        scope_kind -> Text,
        provider_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        feature_key -> Text,
        enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    model (id) {
        id -> Int8,
        provider_id -> Int8,
        cost_catalog_id -> Nullable<Int8>,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        supports_streaming -> Bool,
        supports_tools -> Bool,
        supports_reasoning -> Bool,
        supports_image_input -> Bool,
        supports_embeddings -> Bool,
        supports_rerank -> Bool,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
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
    use diesel::sql_types::{Binary, Bool, Int4, Int8, Nullable, Text};

    provider_api_key (id) {
        id -> Int8,
        provider_id -> Int8,
        description -> Nullable<Text>,
        key_prefix -> Text,
        key_last4 -> Text,
        secret_ciphertext -> Nullable<Binary>,
        secret_nonce -> Nullable<Binary>,
        secret_format_version -> Nullable<Int4>,
        secret_key_fingerprint -> Nullable<Text>,
        secret_hmac -> Nullable<Text>,
        deleted_at -> Nullable<Int8>,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use crate::schema::enum_def::RequestStatusMapping;
    use crate::schema::enum_def::LlmApiTypeMapping;
    use diesel::sql_types::{Bool, Int4, Int8, Nullable, Text};

    request_log (id) {
        id -> Int8,
        api_key_id -> Int8,
        requested_model_name -> Nullable<Text>,
        base_requested_model_name -> Nullable<Text>,
        resolved_reasoning_suffix -> Nullable<Text>,
        resolved_reasoning_preset -> Nullable<Text>,
        request_received_at -> Int8,
        upstream_request_sent_at -> Nullable<Int8>,
        #[sql_name = "response_started_to_client_at"]
        llm_response_first_chunk_at -> Nullable<Int8>,
        #[sql_name = "completed_at"]
        llm_response_completed_at -> Nullable<Int8>,
        is_stream -> Bool,
        client_ip -> Nullable<Text>,
        provider_id -> Nullable<Int8>,
        provider_api_key_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        provider_key_snapshot -> Nullable<Text>,
        provider_name_snapshot -> Nullable<Text>,
        model_name_snapshot -> Nullable<Text>,
        real_model_name_snapshot -> Nullable<Text>,
        llm_api_type -> Nullable<LlmApiTypeMapping>,
        upstream_http_status -> Nullable<Int4>,
        #[sql_name = "overall_status"]
        status -> RequestStatusMapping,
        final_error_code -> Nullable<Text>,
        final_error_message -> Nullable<Text>,
        estimated_cost_nanos -> Nullable<Int8>,
        estimated_cost_currency -> Nullable<Text>,
        cost_catalog_id -> Nullable<Int8>,
        cost_catalog_version_id -> Nullable<Int8>,
        cost_snapshot_json -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
        total_input_tokens -> Nullable<Int4>,
        total_output_tokens -> Nullable<Int4>,
        input_text_tokens -> Nullable<Int4>,
        output_text_tokens -> Nullable<Int4>,
        input_image_tokens -> Nullable<Int4>,
        output_image_tokens -> Nullable<Int4>,
        cache_read_tokens -> Nullable<Int4>,
        cache_write_tokens -> Nullable<Int4>,
        reasoning_tokens -> Nullable<Int4>,
        total_tokens -> Nullable<Int4>,
        user_api_type -> LlmApiTypeMapping,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Nullable};

    metric_ingested_request_log (request_log_id) {
        request_log_id -> Int8,
        request_received_at -> Int8,
        completed_at -> Nullable<Int8>,
        ingested_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Nullable, Text};

    metric_request_rollup_minute (bucket_start_ms, scope_type, scope_id) {
        bucket_start_ms -> Int8,
        scope_type -> Text,
        scope_id -> Text,
        scope_label -> Nullable<Text>,
        request_count -> Int8,
        success_count -> Int8,
        error_count -> Int8,
        cancelled_count -> Int8,
        first_byte_latency_sum_ms -> Int8,
        first_byte_latency_count -> Int8,
        total_latency_sum_ms -> Int8,
        total_latency_count -> Int8,
        input_tokens -> Int8,
        output_tokens -> Int8,
        reasoning_tokens -> Int8,
        total_tokens -> Int8,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int4, Int8, Text};

    metric_http_status_rollup_minute (bucket_start_ms, scope_type, scope_id, http_status) {
        bucket_start_ms -> Int8,
        scope_type -> Text,
        scope_id -> Text,
        http_status -> Int4,
        count -> Int8,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int8, Text};

    metric_cost_rollup_minute (bucket_start_ms, scope_type, scope_id, currency) {
        bucket_start_ms -> Int8,
        scope_type -> Text,
        scope_id -> Text,
        currency -> Text,
        amount_nanos -> Int8,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use crate::schema::enum_def::RequestPatchOperationMapping;
    use crate::schema::enum_def::RequestPatchPlacementMapping;
    use diesel::sql_types::{Bool, Int8, Nullable, Text};

    request_patch_rule (id) {
        id -> Int8,
        provider_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
        placement -> RequestPatchPlacementMapping,
        target -> Text,
        operation -> RequestPatchOperationMapping,
        value_json -> Nullable<Text>,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
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
