use crate::schema::enum_def::{ProviderType, UpstreamProtocol};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderDialect {
    Standard,
    GeminiOpenAiCompatibility,
}

impl ProviderDialect {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::GeminiOpenAiCompatibility => "gemini_openai_compatibility",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderAuthProfile {
    BearerApiKey,
    GeminiApiKey,
    AnthropicApiKey,
    VertexOAuth,
}

impl ProviderAuthProfile {
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
pub enum ProviderEndpointProfile {
    BaseUrl,
    VertexGemini,
    VertexOpenAi,
}

impl ProviderEndpointProfile {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::BaseUrl => "base_url",
            Self::VertexGemini => "vertex_gemini",
            Self::VertexOpenAi => "vertex_openai",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProviderRuntimeProfile {
    pub upstream_protocol: UpstreamProtocol,
    pub dialect: ProviderDialect,
    pub auth: ProviderAuthProfile,
    pub endpoint: ProviderEndpointProfile,
}

/// Resolves the persisted provider identity into its complete runtime wire
/// profile. This is the only provider-to-protocol mapping in active code.
pub const fn provider_runtime_profile(provider_type: &ProviderType) -> ProviderRuntimeProfile {
    match provider_type {
        ProviderType::Openai => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: ProviderDialect::Standard,
            auth: ProviderAuthProfile::BearerApiKey,
            endpoint: ProviderEndpointProfile::BaseUrl,
        },
        ProviderType::Gemini => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Gemini,
            dialect: ProviderDialect::Standard,
            auth: ProviderAuthProfile::GeminiApiKey,
            endpoint: ProviderEndpointProfile::BaseUrl,
        },
        ProviderType::Vertex => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Gemini,
            dialect: ProviderDialect::Standard,
            auth: ProviderAuthProfile::VertexOAuth,
            endpoint: ProviderEndpointProfile::VertexGemini,
        },
        ProviderType::VertexOpenai => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: ProviderDialect::GeminiOpenAiCompatibility,
            auth: ProviderAuthProfile::VertexOAuth,
            endpoint: ProviderEndpointProfile::VertexOpenAi,
        },
        ProviderType::Ollama => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Ollama,
            dialect: ProviderDialect::Standard,
            auth: ProviderAuthProfile::BearerApiKey,
            endpoint: ProviderEndpointProfile::BaseUrl,
        },
        ProviderType::Anthropic => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Anthropic,
            dialect: ProviderDialect::Standard,
            auth: ProviderAuthProfile::AnthropicApiKey,
            endpoint: ProviderEndpointProfile::BaseUrl,
        },
        ProviderType::Responses => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Responses,
            dialect: ProviderDialect::Standard,
            auth: ProviderAuthProfile::BearerApiKey,
            endpoint: ProviderEndpointProfile::BaseUrl,
        },
        ProviderType::GeminiOpenai => ProviderRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: ProviderDialect::GeminiOpenAiCompatibility,
            auth: ProviderAuthProfile::BearerApiKey,
            endpoint: ProviderEndpointProfile::BaseUrl,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_type_has_the_expected_runtime_profile() {
        for (provider_type, expected) in [
            (
                ProviderType::Openai,
                (
                    UpstreamProtocol::Openai,
                    ProviderDialect::Standard,
                    ProviderAuthProfile::BearerApiKey,
                    ProviderEndpointProfile::BaseUrl,
                ),
            ),
            (
                ProviderType::Gemini,
                (
                    UpstreamProtocol::Gemini,
                    ProviderDialect::Standard,
                    ProviderAuthProfile::GeminiApiKey,
                    ProviderEndpointProfile::BaseUrl,
                ),
            ),
            (
                ProviderType::Vertex,
                (
                    UpstreamProtocol::Gemini,
                    ProviderDialect::Standard,
                    ProviderAuthProfile::VertexOAuth,
                    ProviderEndpointProfile::VertexGemini,
                ),
            ),
            (
                ProviderType::VertexOpenai,
                (
                    UpstreamProtocol::Openai,
                    ProviderDialect::GeminiOpenAiCompatibility,
                    ProviderAuthProfile::VertexOAuth,
                    ProviderEndpointProfile::VertexOpenAi,
                ),
            ),
            (
                ProviderType::Ollama,
                (
                    UpstreamProtocol::Ollama,
                    ProviderDialect::Standard,
                    ProviderAuthProfile::BearerApiKey,
                    ProviderEndpointProfile::BaseUrl,
                ),
            ),
            (
                ProviderType::Anthropic,
                (
                    UpstreamProtocol::Anthropic,
                    ProviderDialect::Standard,
                    ProviderAuthProfile::AnthropicApiKey,
                    ProviderEndpointProfile::BaseUrl,
                ),
            ),
            (
                ProviderType::Responses,
                (
                    UpstreamProtocol::Responses,
                    ProviderDialect::Standard,
                    ProviderAuthProfile::BearerApiKey,
                    ProviderEndpointProfile::BaseUrl,
                ),
            ),
            (
                ProviderType::GeminiOpenai,
                (
                    UpstreamProtocol::Openai,
                    ProviderDialect::GeminiOpenAiCompatibility,
                    ProviderAuthProfile::BearerApiKey,
                    ProviderEndpointProfile::BaseUrl,
                ),
            ),
        ] {
            let profile = provider_runtime_profile(&provider_type);
            assert_eq!(
                (
                    profile.upstream_protocol,
                    profile.dialect,
                    profile.auth,
                    profile.endpoint,
                ),
                expected,
                "unexpected runtime profile for {provider_type:?}"
            );
        }
    }

    #[test]
    fn profile_keys_are_stable_for_matrix_and_diagnostics() {
        assert_eq!(ProviderDialect::Standard.as_key(), "standard");
        assert_eq!(
            ProviderDialect::GeminiOpenAiCompatibility.as_key(),
            "gemini_openai_compatibility"
        );
        assert_eq!(ProviderAuthProfile::VertexOAuth.as_key(), "vertex_oauth");
        assert_eq!(
            ProviderEndpointProfile::VertexOpenAi.as_key(),
            "vertex_openai"
        );
    }
}
