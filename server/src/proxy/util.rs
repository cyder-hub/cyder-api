use std::{collections::BTreeMap, sync::Arc};

use axum::{body::Body, extract::Request};
use cyder_tools::log::debug;
use serde_json::Value;

use super::ProxyError;
use crate::{
    config::CONFIG,
    cost::UsageNormalization,
    schema::enum_def::LlmApiType,
    schema::enum_def::ProviderType,
    service::app_state::AppState,
    service::cache::types::{CacheCostCatalogVersion, CacheModel, CacheProvider},
};

fn serialize_headers_for_log(
    headers: &reqwest::header::HeaderMap,
    redacted_names: &[&str],
) -> Option<String> {
    let mut header_map_simplified = BTreeMap::new();
    for (name, value) in headers.iter() {
        let normalized_name = name.as_str().to_ascii_lowercase();
        if redacted_names.contains(&normalized_name.as_str()) {
            continue;
        }

        header_map_simplified.insert(normalized_name, value.to_str().unwrap_or("").to_string());
    }
    serde_json::to_string(&header_map_simplified).ok()
}

pub(super) fn serialize_reqwest_headers_for_debug(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    serialize_headers_for_log(
        headers,
        &["authorization", "x-api-key", "x-goog-api-key", "cookie"],
    )
}

pub(super) fn serialize_downstream_request_headers_for_log(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    serialize_headers_for_log(
        headers,
        &["authorization", "x-api-key", "x-goog-api-key", "cookie"],
    )
}

pub(super) fn serialize_upstream_response_headers_for_log(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    serialize_headers_for_log(
        headers,
        &["set-cookie", "transfer-encoding", "content-length"],
    )
}

pub(super) async fn get_cost_catalog_version(
    model: &CacheModel,
    app_state: &Arc<AppState>,
) -> Option<CacheCostCatalogVersion> {
    debug!(
        "Fetching active cost catalog version for model: {}, cost_catalog_id: {:?}",
        model.model_name, model.cost_catalog_id
    );
    if model.cost_catalog_id.is_some() {
        app_state
            .get_cost_catalog_version_by_model(model.id, chrono::Utc::now().timestamp_millis())
            .await
            .ok()
            .flatten()
            .map(|version| (*version).clone())
    } else {
        None
    }
}

// Parses the request body into a JSON Value.
pub(super) async fn parse_request_body(request: Request<Body>) -> Result<Value, ProxyError> {
    let body = axum::body::to_bytes(request.into_body(), CONFIG.max_body_size)
        .await
        .map_err(|e| ProxyError::BadRequest(format!("Failed to read body: {}", e)))?;

    if body.is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_slice(&body)
        .map_err(|e| ProxyError::BadRequest(format!("Failed to parse JSON body: {}", e)))
}

pub(super) fn parse_utility_usage_normalization(
    response_body: &Value,
) -> Option<UsageNormalization> {
    let tokens = response_body
        .get("usage")
        .and_then(|u| u.get("total_tokens"))
        .and_then(|t| t.as_i64())
        .or_else(|| response_body.get("totalTokens").and_then(|t| t.as_i64()))
        .or_else(|| {
            response_body
                .get("meta")
                .and_then(|m| m.get("tokens"))
                .and_then(|t| t.get("input_tokens"))
                .and_then(|it| it.as_i64())
        });

    tokens.map(|t| UsageNormalization {
        total_input_tokens: t,
        total_output_tokens: 0,
        input_text_tokens: t,
        output_text_tokens: 0,
        input_image_tokens: 0,
        output_image_tokens: 0,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        warnings: vec![
            "utility usage only reported aggregate token totals; normalized as input_text_tokens"
                .to_string(),
        ],
    })
}

pub(super) fn determine_target_api_type(provider: &CacheProvider) -> LlmApiType {
    match provider.provider_type {
        ProviderType::Vertex | ProviderType::Gemini => LlmApiType::Gemini,
        ProviderType::Ollama => LlmApiType::Ollama,
        ProviderType::Anthropic => LlmApiType::Anthropic,
        ProviderType::Responses => LlmApiType::Responses,
        ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
        ProviderType::Openai | ProviderType::VertexOpenai => LlmApiType::Openai,
    }
}

// Formats a model string for logging purposes.
// Returns "provider/model" if model_name == real_model_name, otherwise "provider/model(real_model_name)".
pub(super) fn format_model_str(provider: &CacheProvider, model: &CacheModel) -> String {
    let real_model_name = model
        .real_model_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&model.model_name);

    if model.model_name == real_model_name {
        format!("{}/{}", &provider.provider_key, &model.model_name)
    } else {
        format!(
            "{}/{}({})",
            &provider.provider_key, &model.model_name, real_model_name
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        determine_target_api_type, parse_utility_usage_normalization,
        serialize_downstream_request_headers_for_log, serialize_upstream_response_headers_for_log,
    };
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::CacheProvider;
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde_json::Value;

    #[test]
    fn determine_target_api_type_maps_gemini_openai_separately() {
        let provider = CacheProvider {
            id: 1,
            provider_key: "provider".to_string(),
            name: "provider".to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::GeminiOpenai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        };

        assert_eq!(
            determine_target_api_type(&provider),
            crate::schema::enum_def::LlmApiType::GeminiOpenai
        );
    }

    #[test]
    fn serialize_downstream_request_headers_for_log_redacts_sensitive_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
        headers.insert("x-api-key", HeaderValue::from_static("secret"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("secret"));
        headers.insert("cookie", HeaderValue::from_static("session=secret"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        let serialized = serialize_downstream_request_headers_for_log(&headers).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();

        assert!(parsed.get("authorization").is_none());
        assert!(parsed.get("x-api-key").is_none());
        assert!(parsed.get("x-goog-api-key").is_none());
        assert!(parsed.get("cookie").is_none());
        assert_eq!(parsed["content-type"], "application/json");
    }

    #[test]
    fn serialize_upstream_response_headers_for_log_redacts_transport_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("set-cookie", HeaderValue::from_static("session=secret"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        headers.insert("content-length", HeaderValue::from_static("42"));
        headers.insert("content-type", HeaderValue::from_static("application/json"));

        let serialized = serialize_upstream_response_headers_for_log(&headers).unwrap();
        let parsed: Value = serde_json::from_str(&serialized).unwrap();

        assert!(parsed.get("set-cookie").is_none());
        assert!(parsed.get("transfer-encoding").is_none());
        assert!(parsed.get("content-length").is_none());
        assert_eq!(parsed["content-type"], "application/json");
    }

    #[test]
    fn parse_utility_usage_normalization_supports_openai_and_gemini_shapes() {
        let openai_usage =
            parse_utility_usage_normalization(&serde_json::json!({"usage": {"total_tokens": 4}}))
                .unwrap();
        let gemini_usage =
            parse_utility_usage_normalization(&serde_json::json!({"totalTokens": 9})).unwrap();

        assert_eq!(openai_usage.total_input_tokens, 4);
        assert_eq!(openai_usage.total_output_tokens, 0);
        assert_eq!(gemini_usage.total_input_tokens, 9);
        assert_eq!(gemini_usage.total_output_tokens, 0);
    }
}
