use std::sync::Arc;

use axum::{body::Body, http::HeaderMap, response::Response};
use bytes::Bytes;
use cyder_tools::log::{error, info, warn};
use serde_json::Value;

use super::{
    ProxyError,
    auth::{admit_api_key_request, check_access_control},
    cancellation::ProxyCancellationContext,
    core::proxy_request,
    logging::{LoggedBody, RequestLogContext},
    prepare::{get_provider_and_model, prepare_generation_request},
    protocol_transform_error,
    request::ParsedProxyRequest,
    util::{
        calculate_llm_request_body_for_log, determine_target_api_type, format_model_str,
        get_cost_catalog_version,
    },
};
use crate::{
    schema::enum_def::LlmApiType,
    service::{
        app_state::AppState,
        cache::types::{CacheCostCatalogVersion, CacheModel, CacheProvider, CacheSystemApiKey},
        transform::transform_request_data,
    },
};

pub(super) struct ResolvedProxyTarget {
    pub provider: Arc<CacheProvider>,
    pub model: Arc<CacheModel>,
    pub target_api_type: LlmApiType,
    pub cost_catalog_version: Option<CacheCostCatalogVersion>,
}

pub(super) struct PreparedLogSeed {
    pub log_context: RequestLogContext,
    pub model_str: String,
}

pub(super) struct GenerationExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub system_api_key: Arc<CacheSystemApiKey>,
    pub api_type: LlmApiType,
    pub requested_model: String,
    pub is_stream: bool,
    pub query_params: std::collections::HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
    pub parsed_request: ParsedProxyRequest,
}

pub(super) fn extract_model_from_request(data: &Value) -> Result<&str, ProxyError> {
    data.get("model")
        .and_then(Value::as_str)
        .ok_or_else(|| ProxyError::BadRequest("'model' field must be a string".to_string()))
}

pub(super) async fn resolve_proxy_target(
    app_state: &Arc<AppState>,
    requested_model: &str,
) -> Result<ResolvedProxyTarget, ProxyError> {
    let (provider, model) = get_provider_and_model(app_state, requested_model)
        .await
        .map_err(ProxyError::BadRequest)?;
    let target_api_type = determine_target_api_type(&provider);
    let cost_catalog_version = get_cost_catalog_version(&model, app_state).await;

    Ok(ResolvedProxyTarget {
        provider,
        model,
        target_api_type,
        cost_catalog_version,
    })
}

pub(super) fn prepare_generation_log_seed(
    system_api_key: &CacheSystemApiKey,
    resolved_target: &ResolvedProxyTarget,
    provider_api_key_id: i64,
    start_time: i64,
    client_ip_addr: &Option<String>,
    api_type: LlmApiType,
    original_request_value: &Value,
    original_request_body: Bytes,
    final_body_value: &Value,
    final_body: &Bytes,
) -> Result<PreparedLogSeed, ProxyError> {
    let llm_request_body = calculate_llm_request_body_for_log(
        api_type,
        resolved_target.target_api_type,
        original_request_value,
        final_body_value,
        final_body,
    )?;

    let mut log_context = RequestLogContext::new(
        system_api_key,
        &resolved_target.provider,
        &resolved_target.model,
        provider_api_key_id,
        start_time,
        client_ip_addr,
        api_type,
        resolved_target.target_api_type,
    );
    log_context.llm_request_body = llm_request_body.map(LoggedBody::from_bytes);
    log_context.user_request_body = Some(LoggedBody::from_bytes(original_request_body));

    Ok(PreparedLogSeed {
        model_str: format_model_str(&resolved_target.provider, &resolved_target.model),
        log_context,
    })
}

pub(super) async fn execute_generation_proxy(
    app_state: Arc<AppState>,
    input: GenerationExecutionInput,
) -> Result<Response<Body>, ProxyError> {
    let GenerationExecutionInput {
        cancellation,
        system_api_key,
        api_type,
        requested_model,
        is_stream,
        query_params,
        original_headers,
        client_ip_addr,
        start_time,
        parsed_request,
    } = input;
    let ParsedProxyRequest {
        mut data,
        original_request_value,
        original_request_body,
    } = parsed_request;

    info!(
        "Processing {:?} request for model: {}",
        api_type, requested_model
    );

    let resolved_target = resolve_proxy_target(&app_state, &requested_model)
        .await
        .map_err(|e| {
            warn!("Failed to resolve model '{}': {}", requested_model, e);
            e
        })?;
    let target_api_type = resolved_target.target_api_type;

    data = transform_request_data(data, api_type, target_api_type, is_stream);

    check_access_control(
        &system_api_key,
        &resolved_target.provider,
        &resolved_target.model,
        &app_state,
    )
    .await
    .map_err(|e| {
        warn!("Access control check failed: {:?}", e);
        e
    })?;

    let prepared_request = prepare_generation_request(
        &resolved_target.provider,
        &resolved_target.model,
        data,
        &original_headers,
        &app_state,
        target_api_type,
        is_stream,
        &query_params,
    )
    .await
    .map_err(|e| {
        error!(
            "Failed to prepare generation request for target {:?}: {:?}",
            target_api_type, e
        );
        e
    })?;

    let final_body = Bytes::from(
        serde_json::to_vec(&prepared_request.final_body_value)
            .map_err(|e| protocol_transform_error("Failed to serialize final request body", e))?,
    );

    let log_seed = prepare_generation_log_seed(
        &system_api_key,
        &resolved_target,
        prepared_request.provider_api_key_id,
        start_time,
        &client_ip_addr,
        api_type,
        &original_request_value,
        original_request_body,
        &prepared_request.final_body_value,
        &final_body,
    )?;

    let api_key_concurrency_guard = admit_api_key_request(&app_state, &system_api_key)
        .await
        .map_err(|e| {
            warn!("API key request admission failed: {:?}", e);
            e
        })?;

    proxy_request(
        app_state,
        cancellation,
        log_seed.log_context,
        prepared_request.final_url,
        final_body,
        prepared_request.final_headers,
        log_seed.model_str,
        resolved_target.provider.use_proxy,
        resolved_target.cost_catalog_version,
        api_key_concurrency_guard,
        api_type,
        target_api_type,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::{
        PreparedLogSeed, ResolvedProxyTarget, extract_model_from_request,
        prepare_generation_log_seed,
    };
    use crate::proxy::logging::LoggedBody;
    use crate::{
        proxy::ProxyError,
        schema::enum_def::{LlmApiType, ProviderApiKeyMode, ProviderType},
        service::cache::types::{
            CacheCostCatalogVersion, CacheModel, CacheProvider, CacheSystemApiKey,
        },
    };
    use bytes::Bytes;
    use serde_json::json;
    use std::sync::Arc;

    fn resolved_target(target_api_type: LlmApiType) -> ResolvedProxyTarget {
        ResolvedProxyTarget {
            provider: Arc::new(CacheProvider {
                id: 1,
                provider_key: "provider".to_string(),
                name: "Provider".to_string(),
                endpoint: "https://example.com".to_string(),
                use_proxy: false,
                provider_type: ProviderType::Openai,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                is_enabled: true,
            }),
            model: Arc::new(CacheModel {
                id: 2,
                provider_id: 1,
                model_name: "gpt-test".to_string(),
                real_model_name: Some("real-gpt-test".to_string()),
                cost_catalog_id: Some(3),
                is_enabled: true,
            }),
            target_api_type,
            cost_catalog_version: Some(CacheCostCatalogVersion {
                id: 3,
                catalog_id: 3,
                version: "v1".to_string(),
                currency: "USD".to_string(),
                source: None,
                effective_from: 0,
                effective_until: None,
                is_enabled: true,
                components: vec![],
            }),
        }
    }

    #[test]
    fn extract_model_from_request_reads_string_model() {
        let data = json!({"model":"provider/gpt-test"});

        assert_eq!(
            extract_model_from_request(&data).unwrap(),
            "provider/gpt-test"
        );
    }

    #[test]
    fn extract_model_from_request_rejects_non_string_model() {
        let data = json!({"model":123});

        let err = extract_model_from_request(&data).unwrap_err();

        assert!(matches!(err, ProxyError::BadRequest(_)));
    }

    #[test]
    fn prepare_generation_log_seed_initializes_log_context() {
        let system_api_key = CacheSystemApiKey {
            id: 10,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "system".to_string(),
            description: None,
            default_action: crate::schema::enum_def::Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: vec![],
        };
        let resolved_target = resolved_target(LlmApiType::Gemini);
        let original_request_value = json!({"model":"provider/gpt-test","messages":[]});
        let final_body_value = json!({"contents":[]});
        let original_request_body =
            Bytes::from_static(br#"{"model":"provider/gpt-test","messages":[]}"#);
        let final_body = Bytes::from_static(br#"{"contents":[]}"#);

        let PreparedLogSeed {
            log_context,
            model_str,
        } = prepare_generation_log_seed(
            &system_api_key,
            &resolved_target,
            99,
            1234,
            &Some("127.0.0.1".to_string()),
            LlmApiType::Openai,
            &original_request_value,
            original_request_body.clone(),
            &final_body_value,
            &final_body,
        )
        .unwrap();

        assert_eq!(model_str, "provider/gpt-test(real-gpt-test)");
        assert_eq!(log_context.system_api_key_id, 10);
        assert_eq!(log_context.provider_api_key_id, 99);
        match log_context.user_request_body {
            Some(LoggedBody::InMemory { bytes, .. }) => assert_eq!(bytes, original_request_body),
            other => panic!("unexpected user_request_body: {other:?}"),
        }
        match log_context.llm_request_body {
            Some(LoggedBody::InMemory { bytes, .. }) => assert_eq!(bytes, final_body),
            other => panic!("unexpected llm_request_body: {other:?}"),
        }
        assert_eq!(log_context.model_name, "gpt-test");
        assert_eq!(log_context.real_model_name, "real-gpt-test");
        assert_eq!(log_context.user_api_type, LlmApiType::Openai);
        assert_eq!(log_context.llm_api_type, LlmApiType::Gemini);
    }
}
