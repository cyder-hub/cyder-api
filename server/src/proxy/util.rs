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
    utils::storage::{
        RequestLogBundleHttpHeader, RequestLogBundleQueryParam, RequestLogBundleRequestSnapshot,
    },
};

const REDACTED_REQUEST_HEADER_NAMES: &[&str] =
    &["authorization", "x-api-key", "x-goog-api-key", "cookie"];
const REDACTED_REQUEST_QUERY_PARAM_NAMES: &[&str] = &["key"];

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
    serialize_headers_for_log(headers, REDACTED_REQUEST_HEADER_NAMES)
}

pub(super) fn serialize_downstream_request_headers_for_log(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    serialize_headers_for_log(headers, REDACTED_REQUEST_HEADER_NAMES)
}

pub(super) fn serialize_upstream_response_headers_for_log(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    serialize_headers_for_log(
        headers,
        &["set-cookie", "transfer-encoding", "content-length"],
    )
}

pub(super) fn build_request_snapshot(
    request_path: &str,
    operation_kind: &str,
    raw_query: Option<&str>,
    headers: &axum::http::HeaderMap,
) -> RequestLogBundleRequestSnapshot {
    RequestLogBundleRequestSnapshot {
        request_path: request_path.to_string(),
        operation_kind: operation_kind.to_string(),
        query_params: parse_request_query_params_for_snapshot(raw_query),
        sanitized_original_headers: sanitize_original_request_headers_for_snapshot(headers),
    }
}

pub(super) fn sanitize_original_request_headers_for_snapshot(
    headers: &axum::http::HeaderMap,
) -> Vec<RequestLogBundleHttpHeader> {
    let mut items = headers
        .iter()
        .filter_map(|(name, value)| {
            let normalized_name = name.as_str().to_ascii_lowercase();
            if REDACTED_REQUEST_HEADER_NAMES.contains(&normalized_name.as_str()) {
                return None;
            }

            Some(RequestLogBundleHttpHeader {
                name: normalized_name,
                value: value.to_str().unwrap_or("").to_string(),
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then(left.value.cmp(&right.value))
    });
    items
}

pub(super) fn parse_request_query_params_for_snapshot(
    raw_query: Option<&str>,
) -> Vec<RequestLogBundleQueryParam> {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|segment| !segment.is_empty())
        .filter_map(|segment| {
            let (raw_name, raw_value) = match segment.split_once('=') {
                Some((name, value)) => (name, Some(value)),
                None => (segment, None),
            };
            let name = decode_query_component(raw_name);
            let normalized_name = name.to_ascii_lowercase();
            if REDACTED_REQUEST_QUERY_PARAM_NAMES.contains(&normalized_name.as_str()) {
                return None;
            }

            Some(RequestLogBundleQueryParam {
                name,
                value: raw_value.map(decode_query_component),
                value_present: raw_value.is_some(),
                encoded_name: Some(raw_name.to_string()),
                encoded_value: raw_value.map(str::to_string),
            })
        })
        .collect()
}

fn decode_query_component(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                match (hex_value(bytes[index + 1]), hex_value(bytes[index + 2])) {
                    (Some(high), Some(low)) => {
                        decoded.push((high << 4) | low);
                        index += 3;
                    }
                    _ => {
                        decoded.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
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
        build_request_snapshot, determine_target_api_type, parse_request_query_params_for_snapshot,
        parse_utility_usage_normalization, sanitize_original_request_headers_for_snapshot,
        serialize_downstream_request_headers_for_log, serialize_upstream_response_headers_for_log,
    };
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::CacheProvider;
    use axum::http::HeaderMap as AxumHeaderMap;
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
    fn sanitize_original_request_headers_for_snapshot_redacts_sensitive_headers() {
        let mut headers = AxumHeaderMap::new();
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));
        headers.insert("x-api-key", HeaderValue::from_static("secret"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("secret"));
        headers.insert("cookie", HeaderValue::from_static("session=secret"));
        headers.insert("x-trace-id", HeaderValue::from_static("trace-123"));

        let sanitized = sanitize_original_request_headers_for_snapshot(&headers);

        assert_eq!(sanitized.len(), 1);
        assert_eq!(sanitized[0].name, "x-trace-id");
        assert_eq!(sanitized[0].value, "trace-123");
    }

    #[test]
    fn parse_request_query_params_for_snapshot_preserves_flags_and_redacts_auth_key() {
        let params =
            parse_request_query_params_for_snapshot(Some("trace=1&verbose&key=secret&mode="));

        assert_eq!(params.len(), 3);
        assert_eq!(params[0].name, "trace");
        assert_eq!(params[0].value.as_deref(), Some("1"));
        assert!(params[0].value_present);
        assert_eq!(params[0].encoded_name.as_deref(), Some("trace"));
        assert_eq!(params[0].encoded_value.as_deref(), Some("1"));
        assert_eq!(params[1].name, "verbose");
        assert_eq!(params[1].value, None);
        assert!(!params[1].value_present);
        assert_eq!(params[1].encoded_name.as_deref(), Some("verbose"));
        assert_eq!(params[1].encoded_value, None);
        assert_eq!(params[2].name, "mode");
        assert_eq!(params[2].value.as_deref(), Some(""));
        assert!(params[2].value_present);
        assert_eq!(params[2].encoded_name.as_deref(), Some("mode"));
        assert_eq!(params[2].encoded_value.as_deref(), Some(""));
    }

    #[test]
    fn parse_request_query_params_for_snapshot_decodes_before_redacting_preserves_order_and_tracks_original_encoding()
     {
        let params = parse_request_query_params_for_snapshot(Some(
            "k%65y=secret&tag=a&tag=b&flag&mode=&q=a%20b&plus=a+b&literal=a%2Bb&KEY=secret2",
        ));

        assert_eq!(
            params
                .iter()
                .map(|param| (
                    param.name.as_str(),
                    param.value.as_deref(),
                    param.value_present
                ))
                .collect::<Vec<_>>(),
            vec![
                ("tag", Some("a"), true),
                ("tag", Some("b"), true),
                ("flag", None, false),
                ("mode", Some(""), true),
                ("q", Some("a b"), true),
                ("plus", Some("a b"), true),
                ("literal", Some("a+b"), true),
            ]
        );
        assert_eq!(params[4].encoded_value.as_deref(), Some("a%20b"));
        assert_eq!(params[5].encoded_value.as_deref(), Some("a+b"));
        assert_eq!(params[6].encoded_value.as_deref(), Some("a%2Bb"));
    }

    #[test]
    fn build_request_snapshot_combines_path_query_and_header_assets() {
        let mut headers = AxumHeaderMap::new();
        headers.insert("x-trace-id", HeaderValue::from_static("trace-123"));
        headers.insert("authorization", HeaderValue::from_static("Bearer secret"));

        let snapshot = build_request_snapshot(
            "/openai/v1/chat/completions",
            "chat_completions_create",
            Some("trace=1&verbose"),
            &headers,
        );

        assert_eq!(snapshot.request_path, "/openai/v1/chat/completions");
        assert_eq!(snapshot.operation_kind, "chat_completions_create");
        assert_eq!(snapshot.query_params.len(), 2);
        assert_eq!(snapshot.sanitized_original_headers.len(), 1);
        assert_eq!(snapshot.sanitized_original_headers[0].name, "x-trace-id");
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
