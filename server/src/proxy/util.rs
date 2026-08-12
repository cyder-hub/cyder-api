use std::sync::Arc;

use cyder_tools::log::debug;
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{
    cost::UsageNormalization,
    schema::enum_def::UpstreamProtocol,
    service::app_state::AppState,
    service::cache::types::{
        CacheCostCatalogVersion, CacheModel, CacheProvider, CacheUpstreamSource,
    },
    service::upstream_profile::upstream_runtime_profile,
};

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
        .and_then(|u| u.get("prompt_tokens").or_else(|| u.get("total_tokens")))
        .and_then(|t| t.as_i64())
        .or_else(|| response_body.get("totalTokens").and_then(|t| t.as_i64()))
        .or_else(|| {
            response_body
                .get("meta")
                .and_then(|m| m.get("tokens"))
                .and_then(|t| t.get("input_tokens"))
                .and_then(|it| it.as_i64())
        });

    tokens
        .filter(|tokens| (0..=i64::from(i32::MAX)).contains(tokens))
        .map(|tokens| {
            UsageNormalization::input_only(
                tokens,
                "utility usage was normalized as input_text_tokens; output tokens are not applicable",
            )
        })
}

pub(crate) fn determine_upstream_protocol(source: &CacheUpstreamSource) -> UpstreamProtocol {
    upstream_runtime_profile(&source.profile_type).upstream_protocol
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
        parse_utility_usage_normalization, sha256_hex, top_level_json_field_count,
    };
    use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
    use crate::service::cache::types::{CacheProvider, CacheUpstreamSource};
    use serde_json::Value;

    #[test]
    fn determine_upstream_protocol_maps_gemini_openai_to_openai_wire_protocol() {
        let provider = CacheProvider {
            id: 1,
            provider_key: "provider".to_string(),
            name: "provider".to_string(),
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
            upstream_sources: vec![CacheUpstreamSource {
                id: 2,
                profile_type: UpstreamProfileType::GeminiOpenai,
                base_url: "https://example.com".to_string(),
                use_proxy: false,
                chat_completions_enabled: Some(true),
                chat_completions_path_override: None,
                embeddings_enabled: Some(true),
                embeddings_path_override: None,
                rerank_enabled: Some(false),
                rerank_path_override: None,
                is_enabled: true,
                is_default: true,
            }],
        };

        assert_eq!(
            determine_upstream_protocol(&provider.upstream_sources[0]),
            crate::schema::enum_def::UpstreamProtocol::Openai
        );
    }

    #[test]
    fn parse_utility_usage_normalization_supports_openai_and_gemini_shapes() {
        let openai_usage = parse_utility_usage_normalization(&serde_json::json!({
            "usage": {"prompt_tokens": 4, "total_tokens": 7}
        }))
        .unwrap();
        let gemini_usage =
            parse_utility_usage_normalization(&serde_json::json!({"totalTokens": 9})).unwrap();

        assert_eq!(openai_usage.total_input_tokens, 4);
        assert_eq!(openai_usage.total_output_tokens, 0);
        assert!(!openai_usage.output_tokens_applicable);
        assert_eq!(gemini_usage.total_input_tokens, 9);
        assert_eq!(gemini_usage.total_output_tokens, 0);
        assert!(!gemini_usage.output_tokens_applicable);
        assert!(
            parse_utility_usage_normalization(&serde_json::json!({
                "usage": {"prompt_tokens": -1}
            }))
            .is_none()
        );
        assert!(
            parse_utility_usage_normalization(&serde_json::json!({
                "usage": {"prompt_tokens": i64::from(i32::MAX) + 1}
            }))
            .is_none()
        );
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
