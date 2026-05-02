use std::sync::Arc;

use axum::http::HeaderMap;
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue as ReqwestHeaderValue};

use crate::{
    proxy::ProxyError,
    schema::enum_def::{LlmApiType, ProviderType},
    service::{
        app_state::AppState, cache::types::CacheProvider, runtime::GroupItemSelectionStrategy,
        vertex::get_vertex_token,
    },
};
use cyder_tools::log::error;

/// Resolved API key info for a provider, including the selected key ID and the
/// final request credential (which may be a Vertex AI OAuth token).
pub(in crate::proxy) struct ProviderCredentials {
    /// The database ID of the selected provider API key.
    pub key_id: i64,
    /// The credential to use for the downstream request. For Vertex AI providers,
    /// this is an OAuth token; for others, it's the raw API key.
    pub request_key: String,
}

/// Resolves the API key and authentication credential for a provider.
///
/// This handles: selecting a provider API key via the provider's configured
/// selection strategy, and exchanging it for a Vertex AI OAuth token when the
/// provider type requires it.
pub(in crate::proxy) async fn resolve_provider_credentials(
    provider: &CacheProvider,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredentials, ProxyError> {
    let strategy = GroupItemSelectionStrategy::from(provider.provider_api_key_mode.clone());
    let selected_key = app_state
        .provider_key_selector
        .get_one_provider_api_key_by_provider(provider.id, strategy)
        .await
        .map_err(|e| {
            error!(
                "Failed to get provider API key from cache for provider_id {}: {:?}",
                provider.id, e
            );
            ProxyError::InternalError(format!(
                "Failed to retrieve API key for provider '{}'",
                provider.name
            ))
        })?
        .ok_or_else(|| {
            ProxyError::InternalError(format!(
                "No API keys configured for provider '{}'",
                provider.name
            ))
        })?;

    let request_key = match provider.provider_type {
        ProviderType::Vertex | ProviderType::VertexOpenai => get_vertex_token(
            app_state.infra.proxy_client().await.as_ref(),
            selected_key.id,
            &selected_key.api_key,
        )
        .await
        .map_err(ProxyError::BadRequest)?,
        _ => selected_key.api_key.clone(),
    };

    Ok(ProviderCredentials {
        key_id: selected_key.id,
        request_key,
    })
}

pub(crate) fn apply_provider_request_auth_header(
    headers: &mut HeaderMap,
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    request_key: &str,
) -> Result<(), ProxyError> {
    let (header_name, header_value) =
        provider_request_auth_header(provider, target_api_type, request_key)?;

    headers.remove(AUTHORIZATION);
    headers.remove("x-api-key");
    headers.remove("x-goog-api-key");
    headers.insert(header_name, header_value);

    Ok(())
}

fn provider_request_auth_header(
    provider: &CacheProvider,
    target_api_type: LlmApiType,
    request_key: &str,
) -> Result<(HeaderName, ReqwestHeaderValue), ProxyError> {
    let header_name = match (&provider.provider_type, target_api_type) {
        (ProviderType::Openai, LlmApiType::Openai)
        | (ProviderType::Responses, LlmApiType::Responses)
        | (ProviderType::Vertex, LlmApiType::Gemini)
        | (ProviderType::VertexOpenai, LlmApiType::Openai)
        | (ProviderType::Ollama, LlmApiType::Ollama)
        | (ProviderType::GeminiOpenai, LlmApiType::GeminiOpenai) => AUTHORIZATION,
        (ProviderType::Gemini, LlmApiType::Gemini) => HeaderName::from_static("x-goog-api-key"),
        (ProviderType::Anthropic, LlmApiType::Anthropic) => HeaderName::from_static("x-api-key"),
        _ => {
            return Err(ProxyError::BadRequest(format!(
                "Provider '{}' with type {:?} does not support downstream protocol {:?}",
                provider.name, provider.provider_type, target_api_type
            )));
        }
    };

    let header_value = if header_name == AUTHORIZATION {
        ReqwestHeaderValue::try_from(format!("Bearer {}", request_key)).map_err(|err| {
            ProxyError::BadRequest(format!(
                "Invalid provider credential for replay/auth header '{}': {}",
                header_name, err
            ))
        })?
    } else {
        ReqwestHeaderValue::try_from(request_key).map_err(|err| {
            ProxyError::BadRequest(format!(
                "Invalid provider credential for replay/auth header '{}': {}",
                header_name, err
            ))
        })?
    };

    Ok((header_name, header_value))
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::{
        schema::enum_def::{ProviderApiKeyMode, ProviderType},
        service::cache::types::CacheProvider,
    };

    fn provider(provider_type: ProviderType) -> CacheProvider {
        CacheProvider {
            id: 1,
            provider_key: "provider".to_string(),
            name: "Provider".to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        }
    }

    #[test]
    fn apply_provider_request_auth_header_replaces_existing_auth_headers() {
        let provider = provider(ProviderType::Gemini);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));
        headers.insert("x-api-key", HeaderValue::from_static("stale"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("stale"));

        apply_provider_request_auth_header(
            &mut headers,
            &provider,
            LlmApiType::Gemini,
            "provider-secret",
        )
        .expect("gemini auth header should apply");

        assert!(headers.get(AUTHORIZATION).is_none());
        assert!(headers.get("x-api-key").is_none());
        assert_eq!(
            headers
                .get("x-goog-api-key")
                .and_then(|value| value.to_str().ok()),
            Some("provider-secret")
        );
    }

    #[test]
    fn provider_request_auth_header_uses_bearer_for_openai_compatible_protocols() {
        let provider = provider(ProviderType::VertexOpenai);

        let (name, value) =
            provider_request_auth_header(&provider, LlmApiType::Openai, "provider-secret")
                .expect("vertex openai auth should build");

        assert_eq!(name, AUTHORIZATION);
        assert_eq!(value.to_str().ok(), Some("Bearer provider-secret"));
    }

    #[test]
    fn provider_request_auth_header_rejects_unsupported_protocol_pair() {
        let provider = provider(ProviderType::Gemini);

        let err = provider_request_auth_header(&provider, LlmApiType::Openai, "provider-secret")
            .expect_err("native gemini should reject openai protocol");

        assert!(matches!(err, ProxyError::BadRequest(_)));
    }
}
