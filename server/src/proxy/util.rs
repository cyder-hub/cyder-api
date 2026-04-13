use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, extract::Request};
use cyder_tools::log::debug;
use serde_json::Value;

use super::{ProxyError, protocol_transform_error};
use crate::{
    config::CONFIG,
    cost::UsageNormalization,
    schema::enum_def::LlmApiType,
    schema::enum_def::ProviderType,
    service::app_state::AppState,
    service::cache::types::{CacheCostCatalogVersion, CacheModel, CacheProvider},
};
use bytes::Bytes;

// Helper to serialize reqwest::header::HeaderMap to JSON String
pub(super) fn serialize_reqwest_headers(headers: &reqwest::header::HeaderMap) -> Option<String> {
    let mut header_map_simplified = HashMap::new();
    for (name, value) in headers.iter() {
        header_map_simplified.insert(
            name.as_str().to_string(),
            value.to_str().unwrap_or("").to_string(),
        );
    }
    serde_json::to_string(&header_map_simplified).ok()
}

// Helper to serialize axum::http::HeaderMap

const IGNORED_AXUM_HEADERS: [&str; 2] = ["authorization", "cookie"];

pub(super) fn _serialize_axum_headers(headers: &axum::http::HeaderMap) -> Option<String> {
    let mut header_map_simplified = HashMap::new();
    for (name, value) in headers.iter() {
        let name_str = name.as_str().to_lowercase();
        if IGNORED_AXUM_HEADERS.contains(&name_str.as_str()) {
            continue;
        }
        header_map_simplified.insert(
            name.as_str().to_string(),
            value.to_str().unwrap_or("").to_string(),
        );
    }
    serde_json::to_string(&header_map_simplified).ok()
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

pub(super) fn calculate_llm_request_body_for_log(
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    original_request_value: &serde_json::Value,
    final_body_value: &serde_json::Value,
    final_body_bytes: &Bytes,
) -> Result<Option<Bytes>, ProxyError> {
    if api_type == target_api_type {
        let patch = json_patch::diff(original_request_value, final_body_value);
        if patch.is_empty() {
            // If there's no difference, we can treat it as a full body
            // that is identical to the user request body, allowing for hash optimization.
            Ok(None)
        } else {
            let patch_bytes = Bytes::from(
                serde_json::to_vec(&patch)
                    .map_err(|e| protocol_transform_error("Failed to serialize json-patch", e))?,
            );
            Ok(Some(patch_bytes))
        }
    } else {
        Ok(Some(final_body_bytes.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::determine_target_api_type;
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::service::cache::types::CacheProvider;

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
}
