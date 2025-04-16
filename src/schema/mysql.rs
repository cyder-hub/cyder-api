// @generated automatically by Diesel CLI.

diesel::table! {
    api_key (id) {
        id -> Bigint,
        _api_key -> Text,
        name -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    api_keys (id) {
        id -> Bigint,
        api_key -> Text,
        name -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> Bigint,
        updated_at -> Bigint,
        limit_strategy_id -> Nullable<Bigint>,
    }
}

diesel::table! {
    custom_field (id) {
        id -> Bigint,
        provider_id -> Bigint,
        field_name -> Text,
        field_type -> Text,
        text_value -> Nullable<Text>,
        integer_value -> Nullable<Integer>,
        float_value -> Nullable<Float>,
        boolean_value -> Nullable<Bool>,
        description -> Nullable<Text>,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    limit_strategy (id) {
        id -> Bigint,
        main_strategy -> Text,
        name -> Text,
        description -> Nullable<Text>,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    limit_strategy_item (id) {
        id -> Bigint,
        limit_strategy_id -> Bigint,
        limit_strategy_type -> Text,
        resource_type -> Text,
        resource_id -> Nullable<Bigint>,
        limit_type -> Text,
        limit_value -> Nullable<Integer>,
        duration -> Nullable<Text>,
    }
}

diesel::table! {
    model (id) {
        id -> Bigint,
        provider_id -> Bigint,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    model_transform (id) {
        id -> Bigint,
        model_name -> Text,
        map_model_name -> Text,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    price (id) {
        id -> Bigint,
        model_id -> Bigint,
        start_time -> Bigint,
        currency -> Text,
        input_price -> Integer,
        output_price -> Integer,
        input_cache_price -> Integer,
        output_cache_price -> Integer,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    provider (id) {
        id -> Bigint,
        provider_key -> Text,
        name -> Text,
        endpoint -> Text,
        omit_config -> Nullable<Text>,
        limit_model -> Bool,
        use_proxy -> Bool,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    provider_api_key (id) {
        id -> Bigint,
        provider_id -> Bigint,
        api_key -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    provider_key (id) {
        id -> Bigint,
        provider_id -> Bigint,
        api_key -> Text,
        description -> Nullable<Text>,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::table! {
    record (id) {
        id -> Bigint,
        api_key_id -> Bigint,
        provider_id -> Bigint,
        model_id -> Nullable<Bigint>,
        model_name -> Text,
        real_model_name -> Text,
        prompt_tokens -> Integer,
        prompt_cache_tokens -> Integer,
        prompt_audio_tokens -> Integer,
        completion_tokens -> Integer,
        reasoning_tokens -> Integer,
        first_token_time -> Nullable<Integer>,
        response_time -> Integer,
        is_stream -> Bool,
        request_at -> Bigint,
        created_at -> Bigint,
        updated_at -> Bigint,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    api_key,
    api_keys,
    custom_field,
    limit_strategy,
    limit_strategy_item,
    model,
    model_transform,
    price,
    provider,
    provider_api_key,
    provider_key,
    record,
);
