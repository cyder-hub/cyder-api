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
    use crate::schema::enum_def::ActionMapping;
    use diesel::sql_types::{Int4, Int8, Bool, Text, Nullable};

    api_key (id) {
        id -> Int8,
        #[sql_name = "api_key"]
        api_key_value -> Text,
        api_key_hash -> Nullable<Text>,
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
    use diesel::sql_types::{Int8, Bool, Text, Nullable};

    api_key_model_override (id) {
        id -> Int8,
        api_key_id -> Int8,
        source_name -> Text,
        target_route_id -> Int8,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
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
    model_route (id) {
        id -> Int8,
        route_name -> Text,
        description -> Nullable<Text>,
        is_enabled -> Bool,
        expose_in_models -> Bool,
        deleted_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use diesel::sql_types::{Int4, Int8, Bool, Nullable};

    model_route_candidate (id) {
        id -> Int8,
        route_id -> Int8,
        model_id -> Int8,
        priority -> Int4,
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
    use crate::schema::enum_def::RequestStatusMapping;
    use crate::schema::enum_def::StorageTypeMapping;
    use crate::schema::enum_def::LlmApiTypeMapping;
    use diesel::sql_types::{Bool, Int4, Int8, Nullable, Text};

    request_log (id) {
        id -> Int8,
        api_key_id -> Int8,
        requested_model_name -> Nullable<Text>,
        resolved_name_scope -> Nullable<Text>,
        resolved_route_id -> Nullable<Int8>,
        resolved_route_name -> Nullable<Text>,
        request_received_at -> Int8,
        #[sql_name = "first_attempt_started_at"]
        llm_request_sent_at -> Nullable<Int8>,
        #[sql_name = "response_started_to_client_at"]
        llm_response_first_chunk_at -> Nullable<Int8>,
        #[sql_name = "completed_at"]
        llm_response_completed_at -> Nullable<Int8>,
        client_ip -> Nullable<Text>,
        #[sql_name = "final_attempt_id"]
        final_attempt_id -> Nullable<Int8>,
        #[sql_name = "final_provider_id"]
        provider_id -> Nullable<Int8>,
        #[sql_name = "final_provider_api_key_id"]
        provider_api_key_id -> Nullable<Int8>,
        #[sql_name = "final_model_id"]
        model_id -> Nullable<Int8>,
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
        attempt_count -> Int4,
        retry_count -> Int4,
        fallback_count -> Int4,
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
        has_transform_diagnostics -> Bool,
        transform_diagnostic_count -> Int4,
        transform_diagnostic_max_loss_level -> Nullable<Text>,
        bundle_version -> Nullable<Int4>,
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
    use diesel::sql_types::{Bool, Int4, Int8, Nullable, Text};

    request_attempt (id) {
        id -> Int8,
        request_log_id -> Int8,
        attempt_index -> Int4,
        candidate_position -> Int4,
        provider_id -> Nullable<Int8>,
        provider_api_key_id -> Nullable<Int8>,
        model_id -> Nullable<Int8>,
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
        http_status -> Nullable<Int4>,
        started_at -> Nullable<Int8>,
        first_byte_at -> Nullable<Int8>,
        completed_at -> Nullable<Int8>,
        response_started_to_client -> Bool,
        backoff_ms -> Nullable<Int4>,
        applied_request_patch_ids_json -> Nullable<Text>,
        request_patch_summary_json -> Nullable<Text>,
        estimated_cost_nanos -> Nullable<Int8>,
        estimated_cost_currency -> Nullable<Text>,
        cost_catalog_version_id -> Nullable<Int8>,
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
        llm_request_blob_id -> Nullable<Int4>,
        llm_request_patch_id -> Nullable<Int4>,
        llm_response_blob_id -> Nullable<Int4>,
        llm_response_capture_state -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    use crate::schema::enum_def::LlmApiTypeMapping;
    use crate::schema::enum_def::RequestReplayKindMapping;
    use crate::schema::enum_def::RequestReplayModeMapping;
    use crate::schema::enum_def::RequestReplaySemanticBasisMapping;
    use crate::schema::enum_def::RequestReplayStatusMapping;
    use crate::schema::enum_def::StorageTypeMapping;
    use diesel::sql_types::{Int4, Int8, Nullable, Text};

    request_replay_run (id) {
        id -> Int8,
        source_request_log_id -> Int8,
        source_attempt_id -> Nullable<Int8>,
        replay_kind -> RequestReplayKindMapping,
        replay_mode -> RequestReplayModeMapping,
        semantic_basis -> RequestReplaySemanticBasisMapping,
        status -> RequestReplayStatusMapping,
        executed_route_id -> Nullable<Int8>,
        executed_route_name -> Nullable<Text>,
        executed_provider_id -> Nullable<Int8>,
        executed_provider_api_key_id -> Nullable<Int8>,
        executed_model_id -> Nullable<Int8>,
        executed_llm_api_type -> Nullable<LlmApiTypeMapping>,
        downstream_request_uri -> Nullable<Text>,
        http_status -> Nullable<Int4>,
        error_code -> Nullable<Text>,
        error_message -> Nullable<Text>,
        total_input_tokens -> Nullable<Int4>,
        total_output_tokens -> Nullable<Int4>,
        reasoning_tokens -> Nullable<Int4>,
        total_tokens -> Nullable<Int4>,
        estimated_cost_nanos -> Nullable<Int8>,
        estimated_cost_currency -> Nullable<Text>,
        diff_summary_json -> Nullable<Text>,
        artifact_version -> Nullable<Int4>,
        artifact_storage_type -> Nullable<StorageTypeMapping>,
        artifact_storage_key -> Nullable<Text>,
        started_at -> Nullable<Int8>,
        first_byte_at -> Nullable<Int8>,
        completed_at -> Nullable<Int8>,
        created_at -> Int8,
        updated_at -> Int8,
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
diesel::joinable!(request_replay_run -> request_attempt (source_attempt_id));
diesel::joinable!(request_replay_run -> request_log (source_request_log_id));
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
    request_replay_run,
    request_patch_rule,
    system_api_key,
);
