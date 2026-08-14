use std::fmt;

use crate::{
    schema::enum_def::{UpstreamProfileType, UpstreamProtocol},
    service::{
        cache::types::CacheUpstreamSource,
        provider_http::{ProviderHttpUrlError, join_base_url_and_operation_path},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UpstreamOperation {
    ChatCompletions,
    Embeddings,
    Rerank,
}

impl UpstreamOperation {
    pub(crate) const fn as_key(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat_completions",
            Self::Embeddings => "embeddings",
            Self::Rerank => "rerank",
        }
    }

    pub(crate) const fn default_path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "chat/completions",
            Self::Embeddings => "embeddings",
            Self::Rerank => "rerank",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SourceOperationError {
    SourceDisabled,
    UnsupportedProfile,
    MissingOperationConfiguration,
    OperationDisabled,
    InvalidTargetUrl(ProviderHttpUrlError),
}

impl fmt::Display for SourceOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SourceDisabled => formatter.write_str("the selected Source is disabled"),
            Self::UnsupportedProfile => {
                formatter.write_str("the selected Source Profile does not support this operation")
            }
            Self::MissingOperationConfiguration => formatter
                .write_str("the selected Source is missing its required operation configuration"),
            Self::OperationDisabled => {
                formatter.write_str("the operation is disabled for the selected Source")
            }
            Self::InvalidTargetUrl(error) => {
                write!(formatter, "the Source operation target is invalid: {error}")
            }
        }
    }
}

fn operation_configuration(
    source: &CacheUpstreamSource,
    operation: UpstreamOperation,
) -> Result<(bool, Option<&str>), SourceOperationError> {
    use UpstreamOperation::{ChatCompletions, Embeddings, Rerank};
    use UpstreamProfileType::{GeminiOpenai, Openai, OpenaiCompatible};

    let supported = match (source.profile_type, operation) {
        (Openai | GeminiOpenai, ChatCompletions | Embeddings)
        | (OpenaiCompatible, ChatCompletions | Embeddings | Rerank) => true,
        _ => false,
    };
    if !supported {
        return Err(SourceOperationError::UnsupportedProfile);
    }

    let (enabled, path) = match operation {
        ChatCompletions => (
            source.chat_completions_enabled,
            source.chat_completions_path_override.as_deref(),
        ),
        Embeddings => (
            source.embeddings_enabled,
            source.embeddings_path_override.as_deref(),
        ),
        Rerank => (
            source.rerank_enabled,
            source.rerank_path_override.as_deref(),
        ),
    };
    enabled
        .map(|enabled| (enabled, path))
        .ok_or(SourceOperationError::MissingOperationConfiguration)
}

pub(crate) fn resolve_source_operation_url(
    source: &CacheUpstreamSource,
    operation: UpstreamOperation,
) -> Result<String, SourceOperationError> {
    if !source.is_enabled {
        return Err(SourceOperationError::SourceDisabled);
    }
    let (enabled, path_override) = operation_configuration(source, operation)?;
    if !enabled {
        return Err(SourceOperationError::OperationDisabled);
    }
    join_base_url_and_operation_path(
        &source.base_url,
        path_override.unwrap_or(operation.default_path()),
    )
    .map_err(SourceOperationError::InvalidTargetUrl)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamDialect {
    Standard,
    GeminiOpenAiCompatibility,
}

impl UpstreamDialect {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::GeminiOpenAiCompatibility => "gemini_openai_compatibility",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamAuthProfile {
    BearerApiKey,
    GeminiApiKey,
    AnthropicApiKey,
    VertexOAuth,
}

impl UpstreamAuthProfile {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::BearerApiKey => "bearer_api_key",
            Self::GeminiApiKey => "gemini_api_key",
            Self::AnthropicApiKey => "anthropic_api_key",
            Self::VertexOAuth => "vertex_oauth",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamEndpointProfile {
    BaseUrl,
    VertexGemini,
}

impl UpstreamEndpointProfile {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::BaseUrl => "base_url",
            Self::VertexGemini => "vertex_gemini",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpstreamRuntimeProfile {
    pub upstream_protocol: UpstreamProtocol,
    pub dialect: UpstreamDialect,
    pub auth: UpstreamAuthProfile,
    pub endpoint: UpstreamEndpointProfile,
}

/// Resolves the selected upstream source profile into its complete runtime wire
/// contract. This is the only source-profile-to-protocol mapping in active code.
pub const fn upstream_runtime_profile(
    profile_type: &UpstreamProfileType,
) -> UpstreamRuntimeProfile {
    match profile_type {
        UpstreamProfileType::Openai => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: UpstreamDialect::Standard,
            auth: UpstreamAuthProfile::BearerApiKey,
            endpoint: UpstreamEndpointProfile::BaseUrl,
        },
        UpstreamProfileType::Gemini => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Gemini,
            dialect: UpstreamDialect::Standard,
            auth: UpstreamAuthProfile::GeminiApiKey,
            endpoint: UpstreamEndpointProfile::BaseUrl,
        },
        UpstreamProfileType::Vertex => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Gemini,
            dialect: UpstreamDialect::Standard,
            auth: UpstreamAuthProfile::VertexOAuth,
            endpoint: UpstreamEndpointProfile::VertexGemini,
        },
        UpstreamProfileType::OpenaiCompatible => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: UpstreamDialect::Standard,
            auth: UpstreamAuthProfile::BearerApiKey,
            endpoint: UpstreamEndpointProfile::BaseUrl,
        },
        UpstreamProfileType::Anthropic => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Anthropic,
            dialect: UpstreamDialect::Standard,
            auth: UpstreamAuthProfile::AnthropicApiKey,
            endpoint: UpstreamEndpointProfile::BaseUrl,
        },
        UpstreamProfileType::Responses => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Responses,
            dialect: UpstreamDialect::Standard,
            auth: UpstreamAuthProfile::BearerApiKey,
            endpoint: UpstreamEndpointProfile::BaseUrl,
        },
        UpstreamProfileType::GeminiOpenai => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: UpstreamDialect::GeminiOpenAiCompatibility,
            auth: UpstreamAuthProfile::BearerApiKey,
            endpoint: UpstreamEndpointProfile::BaseUrl,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(profile_type: UpstreamProfileType) -> CacheUpstreamSource {
        CacheUpstreamSource {
            id: 1,
            profile_type,
            base_url: "https://relay.example/prefix/".to_string(),
            use_proxy: false,
            chat_completions_enabled: Some(true),
            chat_completions_path_override: None,
            embeddings_enabled: Some(true),
            embeddings_path_override: None,
            rerank_enabled: Some(false),
            rerank_path_override: None,
            is_enabled: true,
            is_default: true,
        }
    }

    #[test]
    fn openai_wire_operation_urls_use_defaults_and_safe_relative_overrides() {
        let mut openai = source(UpstreamProfileType::Openai);
        assert_eq!(
            resolve_source_operation_url(&openai, UpstreamOperation::ChatCompletions),
            Ok("https://relay.example/prefix/chat/completions".to_string())
        );
        openai.embeddings_path_override = Some("custom/embeddings/".to_string());
        assert_eq!(
            resolve_source_operation_url(&openai, UpstreamOperation::Embeddings),
            Ok("https://relay.example/prefix/custom/embeddings".to_string())
        );

        let mut compatible = source(UpstreamProfileType::OpenaiCompatible);
        compatible.rerank_enabled = Some(true);
        compatible.rerank_path_override = Some("v2/rerank".to_string());
        assert_eq!(
            resolve_source_operation_url(&compatible, UpstreamOperation::Rerank),
            Ok("https://relay.example/prefix/v2/rerank".to_string())
        );
    }

    #[test]
    fn source_operation_resolution_rejects_disabled_unsupported_and_invalid_targets() {
        let mut selected = source(UpstreamProfileType::Openai);
        selected.chat_completions_enabled = Some(false);
        assert_eq!(
            resolve_source_operation_url(&selected, UpstreamOperation::ChatCompletions),
            Err(SourceOperationError::OperationDisabled)
        );

        selected.chat_completions_enabled = Some(true);
        selected.is_enabled = false;
        assert_eq!(
            resolve_source_operation_url(&selected, UpstreamOperation::ChatCompletions),
            Err(SourceOperationError::SourceDisabled)
        );

        let native = source(UpstreamProfileType::Anthropic);
        assert_eq!(
            resolve_source_operation_url(&native, UpstreamOperation::ChatCompletions),
            Err(SourceOperationError::UnsupportedProfile)
        );

        let mut invalid = source(UpstreamProfileType::OpenaiCompatible);
        invalid.rerank_enabled = Some(true);
        invalid.rerank_path_override = Some("../rerank".to_string());
        assert!(matches!(
            resolve_source_operation_url(&invalid, UpstreamOperation::Rerank),
            Err(SourceOperationError::InvalidTargetUrl(_))
        ));
    }

    #[test]
    fn every_upstream_profile_type_has_the_expected_runtime_profile() {
        for (profile_type, expected) in [
            (
                UpstreamProfileType::Openai,
                (
                    UpstreamProtocol::Openai,
                    UpstreamDialect::Standard,
                    UpstreamAuthProfile::BearerApiKey,
                    UpstreamEndpointProfile::BaseUrl,
                ),
            ),
            (
                UpstreamProfileType::Gemini,
                (
                    UpstreamProtocol::Gemini,
                    UpstreamDialect::Standard,
                    UpstreamAuthProfile::GeminiApiKey,
                    UpstreamEndpointProfile::BaseUrl,
                ),
            ),
            (
                UpstreamProfileType::Vertex,
                (
                    UpstreamProtocol::Gemini,
                    UpstreamDialect::Standard,
                    UpstreamAuthProfile::VertexOAuth,
                    UpstreamEndpointProfile::VertexGemini,
                ),
            ),
            (
                UpstreamProfileType::OpenaiCompatible,
                (
                    UpstreamProtocol::Openai,
                    UpstreamDialect::Standard,
                    UpstreamAuthProfile::BearerApiKey,
                    UpstreamEndpointProfile::BaseUrl,
                ),
            ),
            (
                UpstreamProfileType::Anthropic,
                (
                    UpstreamProtocol::Anthropic,
                    UpstreamDialect::Standard,
                    UpstreamAuthProfile::AnthropicApiKey,
                    UpstreamEndpointProfile::BaseUrl,
                ),
            ),
            (
                UpstreamProfileType::Responses,
                (
                    UpstreamProtocol::Responses,
                    UpstreamDialect::Standard,
                    UpstreamAuthProfile::BearerApiKey,
                    UpstreamEndpointProfile::BaseUrl,
                ),
            ),
            (
                UpstreamProfileType::GeminiOpenai,
                (
                    UpstreamProtocol::Openai,
                    UpstreamDialect::GeminiOpenAiCompatibility,
                    UpstreamAuthProfile::BearerApiKey,
                    UpstreamEndpointProfile::BaseUrl,
                ),
            ),
        ] {
            let profile = upstream_runtime_profile(&profile_type);
            assert_eq!(
                (
                    profile.upstream_protocol,
                    profile.dialect,
                    profile.auth,
                    profile.endpoint,
                ),
                expected,
                "unexpected runtime profile for {profile_type:?}"
            );
        }
    }

    #[test]
    fn profile_keys_are_stable_for_matrix_and_diagnostics() {
        assert_eq!(UpstreamDialect::Standard.as_key(), "standard");
        assert_eq!(
            UpstreamDialect::GeminiOpenAiCompatibility.as_key(),
            "gemini_openai_compatibility"
        );
        assert_eq!(UpstreamAuthProfile::VertexOAuth.as_key(), "vertex_oauth");
        assert_eq!(
            UpstreamEndpointProfile::VertexGemini.as_key(),
            "vertex_gemini"
        );
    }
}
