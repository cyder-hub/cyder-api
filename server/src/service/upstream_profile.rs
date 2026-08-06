use crate::schema::enum_def::{UpstreamProfileType, UpstreamProtocol};

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
    VertexOpenAi,
}

impl UpstreamEndpointProfile {
    pub const fn as_key(self) -> &'static str {
        match self {
            Self::BaseUrl => "base_url",
            Self::VertexGemini => "vertex_gemini",
            Self::VertexOpenAi => "vertex_openai",
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
        UpstreamProfileType::VertexOpenai => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Openai,
            dialect: UpstreamDialect::GeminiOpenAiCompatibility,
            auth: UpstreamAuthProfile::VertexOAuth,
            endpoint: UpstreamEndpointProfile::VertexOpenAi,
        },
        UpstreamProfileType::Ollama => UpstreamRuntimeProfile {
            upstream_protocol: UpstreamProtocol::Ollama,
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
                UpstreamProfileType::VertexOpenai,
                (
                    UpstreamProtocol::Openai,
                    UpstreamDialect::GeminiOpenAiCompatibility,
                    UpstreamAuthProfile::VertexOAuth,
                    UpstreamEndpointProfile::VertexOpenAi,
                ),
            ),
            (
                UpstreamProfileType::Ollama,
                (
                    UpstreamProtocol::Ollama,
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
            UpstreamEndpointProfile::VertexOpenAi.as_key(),
            "vertex_openai"
        );
    }
}
