// @generated automatically by Diesel CLI.

diesel::table! {
    api_keys (id) {
        id -> Int8,
        api_key -> Text,
        name -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
        limit_strategy_id -> Nullable<Int8>,
    }
}

diesel::table! {
    custom_field (id) {
        id -> Int8,
        provider_id -> Int8,
        field_name -> Text,
        field_type -> Text,
        text_value -> Nullable<Text>,
        integer_value -> Nullable<Int4>,
        float_value -> Nullable<Float>,
        boolean_value -> Nullable<Bool>,
        description -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    limit_strategy (id) {
        id -> Int8,
        main_strategy -> Text,
        name -> Text,
        description -> Nullable<Text>,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    limit_strategy_item (id) {
        id -> Int8,
        limit_strategy_id -> Int8,
        limit_strategy_type -> Text,
        resource_type -> Text,
        resource_id -> Nullable<Int8>,
        limit_type -> Text,
        limit_value -> Nullable<Int4>,
        duration -> Nullable<Text>,
    }
}

diesel::table! {
    model (id) {
        id -> Int8,
        provider_id -> Int8,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    model_transform (id) {
        id -> Int8,
        model_name -> Text,
        map_model_name -> Text,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    price (id) {
        id -> Int8,
        model_id -> Int8,
        start_time -> Int8,
        currency -> Text,
        input_price -> Int4,
        output_price -> Int4,
        input_cache_price -> Int4,
        output_cache_price -> Int4,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    provider (id) {
        id -> Int8,
        provider_key -> Text,
        name -> Text,
        endpoint -> Text,
        omit_config -> Nullable<Text>,
        limit_model -> Bool,
        use_proxy -> Bool,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    provider_api_key (id) {
        id -> Int8,
        provider_id -> Int8,
        api_key -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::table! {
    record (id) {
        id -> Int8,
        api_key_id -> Int8,
        provider_id -> Int8,
        model_id -> Nullable<Int8>,
        model_name -> Text,
        real_model_name -> Text,
        prompt_tokens -> Int4,
        prompt_cache_tokens -> Int4,
        prompt_audio_tokens -> Int4,
        completion_tokens -> Int4,
        reasoning_tokens -> Int4,
        first_token_time -> Nullable<Int4>,
        response_time -> Int4,
        is_stream -> Bool,
        request_at -> Int8,
        created_at -> Int8,
        updated_at -> Int8,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    api_keys,
    custom_field,
    limit_strategy,
    limit_strategy_item,
    model,
    model_transform,
    price,
    provider,
    provider_api_key,
    record,
);
