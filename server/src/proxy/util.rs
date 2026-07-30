use std::{collections::BTreeMap, sync::Arc};

use cyder_tools::log::debug;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    cost::UsageNormalization,
    schema::enum_def::UpstreamProtocol,
    service::app_state::AppState,
    service::cache::types::{CacheCostCatalogVersion, CacheModel, CacheProvider},
    service::provider_profile::provider_runtime_profile,
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

pub(super) fn serialize_upstream_response_headers_for_log(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    serialize_headers_for_log(
        headers,
        &["set-cookie", "transfer-encoding", "content-length"],
    )
}

pub(super) fn sha256_hex(body: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(body.as_ref()))
}

pub(super) fn top_level_json_field_count(value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len(),
        Value::Array(items) => items.len(),
        Value::Null => 0,
        Value::Bool(_) | Value::Number(_) | Value::String(_) => 1,
    }
}

pub(super) fn json_top_level_field_count_from_bytes(body: impl AsRef<[u8]>) -> Option<usize> {
    serde_json::from_slice::<Value>(body.as_ref())
        .ok()
        .map(|value| top_level_json_field_count(&value))
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
            .catalog
            .get_cost_catalog_version_by_model(model.id, chrono::Utc::now().timestamp_millis())
            .await
            .ok()
            .flatten()
            .map(|version| (*version).clone())
    } else {
        None
    }
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

pub(crate) fn determine_upstream_protocol(provider: &CacheProvider) -> UpstreamProtocol {
    provider_runtime_profile(&provider.provider_type).upstream_protocol
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
        determine_upstream_protocol, json_top_level_field_count_from_bytes,
        parse_utility_usage_normalization, serialize_upstream_response_headers_for_log, sha256_hex,
        top_level_json_field_count,
    };
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::CacheProvider;
    use reqwest::header::{HeaderMap, HeaderValue};
    use serde_json::Value;

    #[test]
    fn determine_upstream_protocol_maps_gemini_openai_to_openai_wire_protocol() {
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
            determine_upstream_protocol(&provider),
            crate::schema::enum_def::UpstreamProtocol::Openai
        );
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

    #[test]
    fn json_top_level_field_count_helpers_handle_objects_arrays_and_scalars() {
        assert_eq!(
            top_level_json_field_count(&serde_json::json!({"a": 1, "b": 2})),
            2
        );
        assert_eq!(
            top_level_json_field_count(&serde_json::json!(["a", "b"])),
            2
        );
        assert_eq!(top_level_json_field_count(&serde_json::json!("value")), 1);
        assert_eq!(top_level_json_field_count(&Value::Null), 0);
        assert_eq!(
            json_top_level_field_count_from_bytes(br#"{"stream":true,"model":"m"}"#),
            Some(2)
        );
    }

    #[test]
    fn sha256_hex_is_stable_for_logged_payload_summaries() {
        assert_eq!(
            sha256_hex(br#"{"hello":"world"}"#),
            "93a23971a914e5eacbf0a8d25154cda309c3c1c72fbb9914d47c60f3cb681588"
        );
    }
}
