use std::fmt;

use reqwest::Url;

use crate::schema::enum_def::UpstreamProfileType;

pub(crate) const VERTEX_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub(crate) const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub(crate) const GEMINI_OPENAI_DEFAULT_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/openai";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderHttpUrlError {
    InvalidUrl,
    UnsupportedScheme,
    MissingHost,
    EmbeddedCredentials,
    QueryNotAllowed,
    FragmentNotAllowed,
    BaseUrlRequired,
    OperationPathEmpty,
    OperationPathMustBeRelative,
    OperationPathBackslashNotAllowed,
    OperationPathDotSegmentNotAllowed,
    ProxyPathNotAllowed,
    UnsupportedVertexTokenUri,
}

impl fmt::Display for ProviderHttpUrlError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::InvalidUrl => "must be a valid absolute URL",
            Self::UnsupportedScheme => "must use the http or https scheme",
            Self::MissingHost => "must include a host",
            Self::EmbeddedCredentials => "must not contain embedded credentials",
            Self::QueryNotAllowed => "must not contain a query string",
            Self::FragmentNotAllowed => "must not contain a fragment",
            Self::BaseUrlRequired => "must provide a base URL for this Profile",
            Self::OperationPathEmpty => "operation path must not be empty",
            Self::OperationPathMustBeRelative => {
                "operation path must be relative and must not contain a scheme, host, query, or fragment"
            }
            Self::OperationPathBackslashNotAllowed => {
                "operation path must not contain a backslash"
            }
            Self::OperationPathDotSegmentNotAllowed => {
                "operation path must not contain dot segments"
            }
            Self::ProxyPathNotAllowed => "must not contain a non-root path",
            Self::UnsupportedVertexTokenUri => {
                "must exactly match https://oauth2.googleapis.com/token"
            }
        })
    }
}

fn parse_http_url(value: &str) -> Result<Url, ProviderHttpUrlError> {
    let url = Url::parse(value).map_err(|_| ProviderHttpUrlError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ProviderHttpUrlError::UnsupportedScheme);
    }
    if url.host_str().is_none() {
        return Err(ProviderHttpUrlError::MissingHost);
    }
    Ok(url)
}

pub(crate) fn normalize_provider_base_url(value: &str) -> Result<String, ProviderHttpUrlError> {
    let mut url = parse_http_url(value.trim())?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ProviderHttpUrlError::EmbeddedCredentials);
    }
    if url.query().is_some() {
        return Err(ProviderHttpUrlError::QueryNotAllowed);
    }
    if url.fragment().is_some() {
        return Err(ProviderHttpUrlError::FragmentNotAllowed);
    }

    let normalized_path = url.path().trim_end_matches('/').to_string();
    url.set_path(&normalized_path);
    let mut normalized = url.to_string();
    while normalized.ends_with('/') {
        normalized.pop();
    }
    Ok(normalized)
}

pub(crate) fn default_base_url(profile_type: &UpstreamProfileType) -> Option<&'static str> {
    match profile_type {
        UpstreamProfileType::Openai => Some(OPENAI_DEFAULT_BASE_URL),
        UpstreamProfileType::GeminiOpenai => Some(GEMINI_OPENAI_DEFAULT_BASE_URL),
        _ => None,
    }
}

pub(crate) fn normalize_source_base_url(
    profile_type: &UpstreamProfileType,
    value: Option<&str>,
) -> Result<String, ProviderHttpUrlError> {
    let trimmed = value.map(str::trim).filter(|value| !value.is_empty());
    let effective = match (trimmed, default_base_url(profile_type)) {
        (Some(value), _) => value,
        (None, Some(default)) => default,
        (None, None) => return Err(ProviderHttpUrlError::BaseUrlRequired),
    };
    normalize_provider_base_url(effective)
}

pub(crate) fn base_url_is_default(
    profile_type: &UpstreamProfileType,
    normalized_base_url: &str,
) -> bool {
    default_base_url(profile_type).is_some_and(|default| default == normalized_base_url)
}

fn is_dot_segment(segment: &str) -> bool {
    let mut rest = segment;
    let mut dot_count = 0usize;
    while !rest.is_empty() {
        if let Some(tail) = rest.strip_prefix('.') {
            dot_count += 1;
            rest = tail;
        } else if rest
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("%2e"))
        {
            dot_count += 1;
            rest = &rest[3..];
        } else {
            return false;
        }
    }
    matches!(dot_count, 1 | 2)
}

pub(crate) fn normalize_operation_path(value: &str) -> Result<String, ProviderHttpUrlError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ProviderHttpUrlError::OperationPathEmpty);
    }
    if trimmed.starts_with('/')
        || trimmed.starts_with("//")
        || trimmed.contains("://")
        || trimmed.contains('?')
        || trimmed.contains('#')
    {
        return Err(ProviderHttpUrlError::OperationPathMustBeRelative);
    }
    if trimmed.contains('\\') {
        return Err(ProviderHttpUrlError::OperationPathBackslashNotAllowed);
    }

    let normalized = trimmed.trim_end_matches('/');
    if normalized.is_empty() {
        return Err(ProviderHttpUrlError::OperationPathEmpty);
    }
    if normalized.split('/').any(is_dot_segment) {
        return Err(ProviderHttpUrlError::OperationPathDotSegmentNotAllowed);
    }
    Ok(normalized.to_string())
}

pub(crate) fn join_base_url_and_operation_path(
    base_url: &str,
    operation_path: &str,
) -> Result<String, ProviderHttpUrlError> {
    let base_url = normalize_provider_base_url(base_url)?;
    let operation_path = normalize_operation_path(operation_path)?;
    Ok(format!("{base_url}/{operation_path}"))
}

pub(crate) fn parse_proxy_url(value: &str) -> Result<Url, ProviderHttpUrlError> {
    let url = parse_http_url(value)?;
    if !matches!(url.path(), "" | "/") {
        return Err(ProviderHttpUrlError::ProxyPathNotAllowed);
    }
    if url.query().is_some() {
        return Err(ProviderHttpUrlError::QueryNotAllowed);
    }
    if url.fragment().is_some() {
        return Err(ProviderHttpUrlError::FragmentNotAllowed);
    }
    Ok(url)
}

pub(crate) fn validate_vertex_token_uri(value: &str) -> Result<(), ProviderHttpUrlError> {
    if value != VERTEX_TOKEN_URI {
        return Err(ProviderHttpUrlError::UnsupportedVertexTokenUri);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_base_url_normalizes_safe_public_private_and_loopback_urls() {
        let cases = [
            (
                "  HTTPS://API.EXAMPLE.COM:443/v1///  ",
                "https://api.example.com/v1",
            ),
            ("http://127.0.0.1:11434/", "http://127.0.0.1:11434"),
            ("http://ollama:11434/api/", "http://ollama:11434/api"),
            ("http://[::1]:11434/v1/", "http://[::1]:11434/v1"),
        ];

        for (input, expected) in cases {
            assert_eq!(
                normalize_provider_base_url(input).expect("base URL should normalize"),
                expected
            );
        }
    }

    #[test]
    fn provider_base_url_rejects_ambiguous_or_credential_bearing_urls() {
        let cases = [
            (
                "ftp://api.example.com/v1",
                ProviderHttpUrlError::UnsupportedScheme,
            ),
            ("https://", ProviderHttpUrlError::InvalidUrl),
            (
                "https://user:secret@api.example.com/v1",
                ProviderHttpUrlError::EmbeddedCredentials,
            ),
            (
                "https://api.example.com/v1?tenant=one",
                ProviderHttpUrlError::QueryNotAllowed,
            ),
            (
                "https://api.example.com/v1#section",
                ProviderHttpUrlError::FragmentNotAllowed,
            ),
        ];

        for (input, expected) in cases {
            assert_eq!(normalize_provider_base_url(input), Err(expected));
        }
    }

    #[test]
    fn source_base_url_applies_only_the_two_official_defaults() {
        assert_eq!(
            normalize_source_base_url(&UpstreamProfileType::Openai, None),
            Ok(OPENAI_DEFAULT_BASE_URL.to_string())
        );
        assert_eq!(
            normalize_source_base_url(&UpstreamProfileType::GeminiOpenai, Some("  ")),
            Ok(GEMINI_OPENAI_DEFAULT_BASE_URL.to_string())
        );
        assert_eq!(
            normalize_source_base_url(&UpstreamProfileType::OpenaiCompatible, None),
            Err(ProviderHttpUrlError::BaseUrlRequired)
        );
        assert!(base_url_is_default(
            &UpstreamProfileType::Openai,
            OPENAI_DEFAULT_BASE_URL
        ));
    }

    #[test]
    fn operation_path_is_relative_normalized_and_cannot_escape_the_base_path() {
        assert_eq!(
            normalize_operation_path(" chat/completions/ "),
            Ok("chat/completions".to_string())
        );
        assert_eq!(
            join_base_url_and_operation_path("https://relay.example/prefix/", "chat/completions/"),
            Ok("https://relay.example/prefix/chat/completions".to_string())
        );
        for value in [
            "",
            "/chat/completions",
            "//other.example/path",
            "https://other.example/path",
            "chat\\completions",
            "chat?mode=one",
            "chat#fragment",
            ".",
            "..",
            "%2e",
            "%2E%2e",
            "chat/%2e%2e/completions",
        ] {
            assert!(
                normalize_operation_path(value).is_err(),
                "operation path {value:?} must be rejected"
            );
        }
    }

    #[test]
    fn proxy_url_accepts_credentials_and_private_hosts_but_rejects_extra_components() {
        let proxy = parse_proxy_url("http://user:password@127.0.0.1:8080")
            .expect("authenticated local proxy should be supported");
        assert_eq!(proxy.username(), "user");
        assert_eq!(proxy.password(), Some("password"));

        assert_eq!(
            parse_proxy_url("http://proxy:8080/gateway"),
            Err(ProviderHttpUrlError::ProxyPathNotAllowed)
        );
        assert_eq!(
            parse_proxy_url("http://proxy:8080?region=cn"),
            Err(ProviderHttpUrlError::QueryNotAllowed)
        );
        assert_eq!(
            parse_proxy_url("http://proxy:8080#internal"),
            Err(ProviderHttpUrlError::FragmentNotAllowed)
        );
    }

    #[test]
    fn vertex_token_uri_is_fixed_to_the_google_oauth_endpoint() {
        assert!(validate_vertex_token_uri(VERTEX_TOKEN_URI).is_ok());
        assert_eq!(
            validate_vertex_token_uri("https://oauth.example.com/token"),
            Err(ProviderHttpUrlError::UnsupportedVertexTokenUri)
        );
        assert_eq!(
            validate_vertex_token_uri(" https://oauth2.googleapis.com/token "),
            Err(ProviderHttpUrlError::UnsupportedVertexTokenUri)
        );
    }
}
