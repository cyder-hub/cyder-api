use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, extract::Request, http::StatusCode};
use cyder_tools::log::debug;
use serde_json::Value;

use crate::{
    controller::llm_types::LlmApiType,
    schema::enum_def::ProviderType,
    service::app_state::AppState,
    utils::billing::UsageInfo,
    service::cache::types::{CacheBillingPlan, CacheProvider, CacheModel},
};


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


// Retrieves pricing rules and currency for a given model.
pub(super) async fn get_pricing_info(model: &CacheModel, app_state: &Arc<AppState>) -> Option<CacheBillingPlan> {
    debug!(
        "Fetching pricing info for model: {}, plan_id: {:?}",
        model.model_name, model.billing_plan_id
    );
    if let Some(billing_plan_id) = model.billing_plan_id {
        app_state
            .get_billing_plan_by_id(billing_plan_id)
            .await
            .ok()
            .flatten()
            .map(|bp| (*bp).clone())
    } else {
        None
    }
}


// Parses the request body into a JSON Value.
pub(super) async fn parse_request_body(request: Request<Body>) -> Result<Value, (StatusCode, String)> {
    let body = axum::body::to_bytes(request.into_body(), usize::MAX)
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Failed to read body: {}", e)))?;

    if body.is_empty() {
        return Ok(Value::Null);
    }

    serde_json::from_slice(&body)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Failed to parse JSON body: {}", e)))
}

pub(super) fn parse_utility_usage_info(response_body: &Value) -> Option<UsageInfo> {
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

    tokens.map(|t| UsageInfo {
        prompt_tokens: t as i32,
        completion_tokens: 0,
        total_tokens: t as i32,
        reasoning_tokens: 0,
    })
}

// Determines the target API type based on the provider type.
pub(super) fn determine_target_api_type(provider: &CacheProvider) -> LlmApiType {
    if provider.provider_type == ProviderType::Vertex || provider.provider_type == ProviderType::Gemini {
        LlmApiType::Gemini
    } else if provider.provider_type == ProviderType::Ollama {
        LlmApiType::Ollama
    } else {
        LlmApiType::OpenAI
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
