use std::fmt;

use reqwest::Url;

pub(crate) const VERTEX_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProviderHttpUrlError {
    InvalidUrl,
    UnsupportedScheme,
    MissingHost,
    EmbeddedCredentials,
    QueryNotAllowed,
    FragmentNotAllowed,
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

pub(crate) fn normalize_provider_endpoint(value: &str) -> Result<String, ProviderHttpUrlError> {
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
    fn provider_endpoint_normalizes_safe_public_private_and_loopback_urls() {
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
                normalize_provider_endpoint(input).expect("endpoint should normalize"),
                expected
            );
        }
    }

    #[test]
    fn provider_endpoint_rejects_ambiguous_or_credential_bearing_urls() {
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
            assert_eq!(normalize_provider_endpoint(input), Err(expected));
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
