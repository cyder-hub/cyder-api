use std::{fmt, sync::Arc, time::Duration};

use axum::http::HeaderMap;
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue};

use crate::{
    config::NonStreamResponseConfig,
    database::provider::{ProviderAggregate, ProviderApiKeyRepository},
    schema::enum_def::{UpstreamProfileType, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::{CacheProvider, CacheUpstreamSource},
        runtime::GroupItemSelectionStrategy,
        secret_encryption::{SecretDomain, SensitiveSecret},
        upstream_profile::{UpstreamAuthProfile, upstream_runtime_profile},
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
    profile_type: &UpstreamProfileType,
    key_id: i64,
    secret: SensitiveSecret,
    limits: &NonStreamResponseConfig,
    auxiliary_total_timeout: Duration,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let request_secret = match upstream_runtime_profile(profile_type).auth {
        UpstreamAuthProfile::VertexOAuth => SensitiveSecret::new(
            get_vertex_token(
                client,
                key_id,
                secret.expose(),
                limits,
                auxiliary_total_timeout,
            )
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

async fn auxiliary_client(
    app_state: &AppState,
    use_proxy: bool,
) -> Result<Arc<reqwest::Client>, ProviderCredentialError> {
    app_state
        .infra
        .auxiliary_client(use_proxy)
        .await
        .map_err(|_| ProviderCredentialError::ProxyRequiredButNotConfigured)
}

/// Selects one enabled encrypted key, decrypts it, and materializes the
/// provider-specific request credential at the last possible boundary.
pub async fn resolve_selected_provider_credential(
    provider: &CacheProvider,
    source: &CacheUpstreamSource,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let client = auxiliary_client(app_state, source.use_proxy).await?;
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
        &source.profile_type,
        selected_key.id,
        secret,
        &app_state.infra.proxy_request_config().non_stream_response,
        app_state.infra.auxiliary_total_timeout(),
    )
    .await
}

/// Resolves a saved key by identity. This is used by explicit manager checks;
/// runtime selection continues to use `resolve_selected_provider_credential`.
pub async fn resolve_saved_provider_credential(
    provider: &ProviderAggregate,
    source: &CacheUpstreamSource,
    key_id: i64,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let client = auxiliary_client(app_state, source.use_proxy).await?;
    let stored = ProviderApiKeyRepository::get_stored_by_id(provider.id, key_id)
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    let encrypted = stored
        .encrypted_secret()
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    let secret = app_state
        .secret_encryption
        .decrypt_current(SecretDomain::ProviderApiKey(key_id), &encrypted)
        .map_err(|_| ProviderCredentialError::CredentialUnavailable)?;
    materialize_provider_credential(
        client.as_ref(),
        &source.profile_type,
        key_id,
        secret,
        &app_state.infra.proxy_request_config().non_stream_response,
        app_state.infra.auxiliary_total_timeout(),
    )
    .await
}

/// Wraps a request-local draft secret without persisting or cloning it.
pub async fn resolve_draft_provider_credential(
    _provider: &ProviderAggregate,
    source: &CacheUpstreamSource,
    key_id: i64,
    secret: SensitiveSecret,
    app_state: &Arc<AppState>,
) -> Result<ProviderCredential, ProviderCredentialError> {
    let client = auxiliary_client(app_state, source.use_proxy).await?;
    materialize_provider_credential(
        client.as_ref(),
        &source.profile_type,
        key_id,
        secret,
        &app_state.infra.proxy_request_config().non_stream_response,
        app_state.infra.auxiliary_total_timeout(),
    )
    .await
}

pub fn apply_provider_request_auth_header(
    headers: &mut HeaderMap,
    source: &CacheUpstreamSource,
    upstream_protocol: UpstreamProtocol,
    credential: &ProviderCredential,
) -> Result<(), ProviderCredentialError> {
    let profile = upstream_runtime_profile(&source.profile_type);
    if profile.upstream_protocol != upstream_protocol {
        return Err(ProviderCredentialError::UnsupportedProtocol);
    }

    let header_name = match profile.auth {
        UpstreamAuthProfile::BearerApiKey | UpstreamAuthProfile::VertexOAuth => AUTHORIZATION,
        UpstreamAuthProfile::GeminiApiKey => HeaderName::from_static("x-goog-api-key"),
        UpstreamAuthProfile::AnthropicApiKey => HeaderName::from_static("x-api-key"),
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
    let auth_header_count = [AUTHORIZATION.as_str(), "x-api-key", "x-goog-api-key"]
        .into_iter()
        .map(|name| headers.get_all(name).iter().count())
        .sum::<usize>();
    if auth_header_count != 1 {
        return Err(ProviderCredentialError::InvalidAuthHeader);
    }
    Ok(())
}

pub fn upstream_protocol_for_profile(profile_type: &UpstreamProfileType) -> UpstreamProtocol {
    upstream_runtime_profile(profile_type).upstream_protocol
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;
    use crate::schema::enum_def::ProviderApiKeyMode;
    use crate::service::cache::types::CacheUpstreamSource;

    fn provider(profile_type: UpstreamProfileType) -> CacheProvider {
        CacheProvider {
            id: 1,
            provider_key: "provider".to_string(),
            name: "Provider".to_string(),
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
            upstream_sources: vec![CacheUpstreamSource {
                id: 2,
                profile_type,
                base_url: "https://example.com".to_string(),
                use_proxy: false,
                chat_completions_enabled: None,
                chat_completions_path_override: None,
                embeddings_enabled: None,
                embeddings_path_override: None,
                rerank_enabled: None,
                rerank_path_override: None,
                is_enabled: true,
                is_default: true,
            }],
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
        let provider = provider(UpstreamProfileType::Gemini);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));
        headers.insert("x-api-key", HeaderValue::from_static("stale"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("stale"));

        apply_provider_request_auth_header(
            &mut headers,
            &provider.upstream_sources[0],
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
        for (profile_type, api_type, name, expected) in [
            (
                UpstreamProfileType::Openai,
                UpstreamProtocol::Openai,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                UpstreamProfileType::OpenaiCompatible,
                UpstreamProtocol::Openai,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                UpstreamProfileType::GeminiOpenai,
                UpstreamProtocol::Openai,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                UpstreamProfileType::Responses,
                UpstreamProtocol::Responses,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                UpstreamProfileType::Gemini,
                UpstreamProtocol::Gemini,
                "x-goog-api-key",
                "provider-secret",
            ),
            (
                UpstreamProfileType::Vertex,
                UpstreamProtocol::Gemini,
                "authorization",
                "Bearer provider-secret",
            ),
            (
                UpstreamProfileType::Anthropic,
                UpstreamProtocol::Anthropic,
                "x-api-key",
                "provider-secret",
            ),
        ] {
            let provider = provider(profile_type.clone());
            let mut headers = HeaderMap::new();
            apply_provider_request_auth_header(
                &mut headers,
                &provider.upstream_sources[0],
                api_type,
                &credential(),
            )
            .expect("supported auth should apply");
            assert_eq!(headers.get(name).unwrap(), expected);
            assert_eq!(
                [AUTHORIZATION.as_str(), "x-api-key", "x-goog-api-key"]
                    .into_iter()
                    .map(|name| headers.get_all(name).iter().count())
                    .sum::<usize>(),
                1,
                "profile={profile_type:?}"
            );
        }
    }

    #[test]
    fn source_profile_and_protocol_mismatch_rejects_credential_before_header_mutation() {
        let provider = provider(UpstreamProfileType::Gemini);
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, HeaderValue::from_static("Bearer existing"));

        let error = apply_provider_request_auth_header(
            &mut headers,
            &provider.upstream_sources[0],
            UpstreamProtocol::Openai,
            &credential(),
        )
        .expect_err("mismatched source profile must fail closed");

        assert_eq!(error, ProviderCredentialError::UnsupportedProtocol);
        assert_eq!(headers.get(AUTHORIZATION).unwrap(), "Bearer existing");
        assert!(headers.get("x-goog-api-key").is_none());
    }

    #[test]
    fn credential_debug_redacts_secret() {
        let rendered = format!("{:?}", credential());
        assert!(!rendered.contains("provider-secret"));
        assert!(rendered.contains("<redacted>"));
    }
}
