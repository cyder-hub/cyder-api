// @generated automatically by Diesel CLI.

diesel::table! {
    api_keys (id) {
        id -> BigInt,
        api_key -> Text,
        name -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    custom_field (id) {
        id -> BigInt,
        provider_id -> BigInt,
        field_name -> Text,
        field_type -> Text,
        text_value -> Nullable<Text>,
        integer_value -> Nullable<Integer>,
        float_value -> Nullable<Float>,
        boolean_value -> Nullable<Bool>,
        description -> Nullable<Text>,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    model (id) {
        id -> BigInt,
        provider_id -> BigInt,
        model_name -> Text,
        real_model_name -> Nullable<Text>,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    model_transform (id) {
        id -> BigInt,
        model_name -> Text,
        map_model_name -> Text,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    price (id) {
        id -> BigInt,
        model_id -> BigInt,
        start_time -> BigInt,
        currency -> Text,
        input_price -> Integer,
        output_price -> Integer,
        input_cache_price -> Integer,
        output_cache_price -> Integer,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    provider (id) {
        id -> BigInt,
        provider_key -> Text,
        name -> Text,
        endpoint -> Text,
        omit_config -> Nullable<Text>,
        limit_model -> Bool,
        use_proxy -> Bool,
        is_enabled -> Bool,
        is_deleted -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    provider_api_key (id) {
        id -> BigInt,
        provider_id -> BigInt,
        api_key -> Text,
        description -> Nullable<Text>,
        is_deleted -> Bool,
        is_enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    record (id) {
        id -> BigInt,
        api_key_id -> BigInt,
        provider_id -> BigInt,
        model_id -> Nullable<BigInt>,
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
        request_at -> BigInt,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::allow_tables_to_appear_in_same_query!(
    api_keys,
    custom_field,
    model,
    model_transform,
    price,
    provider,
    provider_api_key,
    record,
);
