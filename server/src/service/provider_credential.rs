use std::{fmt, sync::Arc};

use axum::http::HeaderMap;
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue};

use crate::{
    database::provider::{Provider, ProviderApiKeyRepository},
    schema::enum_def::{ProviderType, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::CacheProvider,
        provider_profile::{ProviderAuthProfile, provider_runtime_profile},
        runtime::GroupItemSelectionStrategy,
        secret_encryption::{SecretDomain, SensitiveSecret},
        vertex::get_vertex_token,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderCredentialError {
    RuntimeStateUnavailable,
    NoEnabledCredential,
    CredentialUnavailable,
    VertexTokenUnavailable,
    ProxyRequiredButNotConfigured,
    UnsupportedProtocol,
    InvalidAuthHeader,
}

impl fmt::Display for ProviderCredentialError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::RuntimeStateUnavailable => "provider credential runtime state is unavailable",
            Self::NoEnabledCredential => "provider has no enabled credential",
            Self::CredentialUnavailable => {
                "provider credential is unavailable; replace the credential"
            }
            Self::VertexTokenUnavailable => "Vertex OAuth token is unavailable",
            Self::ProxyRequiredButNotConfigured => {
                "provider requires the global proxy, but no proxy is configured"
            }
            Self::UnsupportedProtocol => "provider does not support the requested protocol",
            Self::InvalidAuthHeader => "provider credential cannot be used as an auth header",
        })
    }
}

impl std::error::Error for ProviderCredentialError {}

/// A request-scoped upstream credential. The contained secret is zeroized on
/// drop, cannot be cloned, and never appears in Debug output.
pub struct ProviderCredential {
    key_id: i64,
    request_secret: SensitiveSecret,
}

impl ProviderCredential {
    pub fn key_id(&self) -> i64 {
        self.key_id
    }

    pub(crate) fn expose_for_request(&self) -> &str {
        self.request_secret.expose()
    }

    #[cfg(test)]
    pub(crate) fn for_test(key_id: i64, secret: &str) -> Self {
        Self {
            key_id,
            request_secret: SensitiveSecret::new(secret.to_string()),
        }
    }
}

impl fmt::Debug for ProviderCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "ProviderCredential {{ key_id: {}, request_secret: <redacted> }}",
            self.key_id
        )
    }
}

async fn materialize_provider_credential(
    client: &reqwest::Client,
    provider_type: &ProviderType,
    key_id: i64,
    secret: SensitiveSecret,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let request_secret = match provider_runtime_profile(provider_type).auth {
        ProviderAuthProfile::VertexOAuth => SensitiveSecret::new(
            get_vertex_token(client, key_id, secret.expose())
                .await
                .map_err(|_| ProviderCredentialError::VertexTokenUnavailable)?,
        ),
        _ => secret,
    };

    Ok(ProviderCredential {
        key_id,
        request_secret,
    })
}

async fn provider_client(
    app_state: &AppState,
    use_proxy: bool,
) -> Result<Arc<reqwest::Client>, ProviderCredentialError> {
    app_state
        .infra
        .provider_client(use_proxy)
        .await
        .map_err(|_| ProviderCredentialError::ProxyRequiredButNotConfigured)
}

/// Selects one enabled encrypted key, decrypts it, and materializes the
/// provider-specific request credential at the last possible boundary.
pub async fn resolve_selected_provider_credential(
    provider: &CacheProvider,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let client = provider_client(app_state, provider.use_proxy).await?;
    let strategy = GroupItemSelectionStrategy::from(provider.provider_api_key_mode.clone());
    let selected_key = app_state
        .provider_key_selector
        .get_one_provider_api_key_by_provider(provider.id, strategy)
        .await
        .map_err(|_| ProviderCredentialError::RuntimeStateUnavailable)?
        .ok_or(ProviderCredentialError::NoEnabledCredential)?;

    let encrypted = selected_key
        .encrypted_secret()
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    let secret = app_state
        .secret_encryption
        .decrypt_current(SecretDomain::ProviderApiKey(selected_key.id), &encrypted)
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    materialize_provider_credential(
        client.as_ref(),
        &provider.provider_type,
        selected_key.id,
        secret,
    )
    .await
}

/// Resolves a saved key by identity. This is used by explicit manager checks;
/// runtime selection continues to use `resolve_selected_provider_credential`.
pub async fn resolve_saved_provider_credential(
    provider: &Provider,
    key_id: i64,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let client = provider_client(app_state, provider.use_proxy).await?;
    let stored = ProviderApiKeyRepository::get_stored_by_id(provider.id, key_id)
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    let encrypted = stored
        .encrypted_secret()
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    let secret = app_state
        .secret_encryption
        .decrypt_current(SecretDomain::ProviderApiKey(key_id), &encrypted)
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    materialize_provider_credential(client.as_ref(), &provider.provider_type, key_id, secret).await
}

/// Wraps a request-local draft secret without persisting or cloning it.
pub async fn resolve_draft_provider_credential(
    provider: &Provider,
    key_id: i64,
    secret: SensitiveSecret,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let client = provider_client(app_state, provider.use_proxy).await?;
    materialize_provider_credential(client.as_ref(), &provider.provider_type, key_id, secret).await
}

pub fn apply_provider_request_auth_header(
    headers: &mut HeaderMap,
    provider: &CacheProvider,
    upstream_protocol: UpstreamProtocol,
    credential: &ProviderCredential,
) -> Result<(), ProviderCredentialError> {
    let profile = provider_runtime_profile(&provider.provider_type);
    if profile.upstream_protocol != upstream_protocol {
        return Err(ProviderCredentialError::UnsupportedProtocol);
    }

    let header_name = match profile.auth {
        ProviderAuthProfile::BearerApiKey | ProviderAuthProfile::VertexOAuth => AUTHORIZATION,
        ProviderAuthProfile::GeminiApiKey => HeaderName::from_static("x-goog-api-key"),
        ProviderAuthProfile::AnthropicApiKey => HeaderName::from_static("x-api-key"),
    };

    let secret = credential.expose_for_request();
    let header_value = if header_name == AUTHORIZATION {
        HeaderValue::try_from(format!("Bearer {secret}"))
            .map_err(|_| ProviderCredentialError::InvalidAuthHeader)?
    } else {
        HeaderValue::try_from(secret).map_err(|_| ProviderCredentialError::InvalidAuthHeader)?
    };

    headers.remove(AUTHORIZATION);
    headers.remove("x-api-key");
    headers.remove("x-goog-api-key");
    headers.insert(header_name, header_value);
    Ok(())
}

pub fn provider_upstream_protocol(provider_type: &ProviderType) -> UpstreamProtocol {
    provider_runtime_profile(provider_type).upstream_protocol
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::schema::enum_def::ProviderApiKeyMode;

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

    fn credential() -> ProviderCredential {
        ProviderCredential {
            key_id: 7,
            request_secret: SensitiveSecret::new("provider-secret".to_string()),
        }
    }

    #[test]
    fn auth_header_replaces_all_legacy_auth_headers() {
        let provider = provider(ProviderType::Gemini);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));
        headers.insert("x-api-key", HeaderValue::from_static("stale"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("stale"));

        apply_provider_request_auth_header(
            &mut headers,
            &provider,
            UpstreamProtocol::Gemini,
            &credential(),
        )
        .expect("gemini auth should apply");

        assert!(headers.get(AUTHORIZATION).is_none());
        assert!(headers.get("x-api-key").is_none());
        assert_eq!(headers.get("x-goog-api-key").unwrap(), "provider-secret");
    }

    #[test]
    fn auth_header_contract_covers_supported_protocols() {
        for (provider_type, api_type, name, expected) in [
            (
                ProviderType::Openai,
                UpstreamProtocol::Openai,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                ProviderType::VertexOpenai,
                UpstreamProtocol::Openai,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                ProviderType::GeminiOpenai,
                UpstreamProtocol::Openai,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                ProviderType::Responses,
                UpstreamProtocol::Responses,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                ProviderType::Ollama,
                UpstreamProtocol::Ollama,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                ProviderType::Gemini,
                UpstreamProtocol::Gemini,
                "x-goog-api-key",
                "provider-secret",
            ),
            (
                ProviderType::Vertex,
                UpstreamProtocol::Gemini,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                ProviderType::Anthropic,
                UpstreamProtocol::Anthropic,
                "x-api-key",
                "provider-secret",
            ),
        ] {
            let provider = provider(provider_type);
            let mut headers = HeaderMap::new();
            apply_provider_request_auth_header(&mut headers, &provider, api_type, &credential())
                .expect("supported auth should apply");
            assert_eq!(headers.get(name).unwrap(), expected);
        }
    }

    #[test]
    fn credential_debug_redacts_secret() {
        let rendered = format!("{:?}", credential());
        assert!(!rendered.contains("provider-secret"));
        assert!(rendered.contains("<redacted>"));
    }
}
