use std::fmt;

use reqwest::{
    Url,
    header::{
        ACCEPT_ENCODING, AUTHORIZATION, CONTENT_LENGTH, COOKIE, HOST, HeaderMap, HeaderValue,
        PROXY_AUTHORIZATION, TRANSFER_ENCODING,
    },
};
use serde_json::{Value, json};

use crate::schema::enum_def::UpstreamProfileType;

pub(crate) const VERTEX_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub(crate) const OPENAI_DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";
pub(crate) const GEMINI_OPENAI_DEFAULT_BASE_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/openai";
pub(crate) const ANTHROPIC_MESSAGES_OPERATION: &str = "messages";
pub(crate) const ANTHROPIC_VERSION_HEADER: &str = "anthropic-version";
pub(crate) const ANTHROPIC_VERSION: &str = "2023-06-01";
pub(crate) const ANTHROPIC_BETA_HEADER: &str = "anthropic-beta";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeminiModelOperation {
    GenerateContent,
    StreamGenerateContent,
    CountTokens,
}

impl GeminiModelOperation {
    pub(crate) const fn action(self) -> &'static str {
        match self {
            Self::GenerateContent => "generateContent",
            Self::StreamGenerateContent => "streamGenerateContent",
            Self::CountTokens => "countTokens",
        }
    }
}

pub(crate) fn gemini_operation_target_url(
    base_url: &str,
    model_id: &str,
    operation: GeminiModelOperation,
) -> Result<Url, ProviderHttpUrlError> {
    let target = gemini_model_operation_url(base_url, model_id, operation)?;
    let mut url = Url::parse(&target).map_err(|_| ProviderHttpUrlError::InvalidUrl)?;
    if operation == GeminiModelOperation::StreamGenerateContent {
        url.query_pairs_mut().append_pair("alt", "sse");
    }
    Ok(url)
}

pub(crate) fn sanitize_gemini_request_headers(original: &HeaderMap) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in original {
        if name != HOST
            && name != CONTENT_LENGTH
            && name != ACCEPT_ENCODING
            && name != TRANSFER_ENCODING
            && name != COOKIE
            && name != PROXY_AUTHORIZATION
            && name != "api-key"
            && name != "x-api-key"
            && name != "x-goog-api-key"
            && name != AUTHORIZATION
            && name != "x-request-id"
            && name != "x-client-request-id"
        {
            headers.insert(name.clone(), value.clone());
        }
    }
    headers
}

pub(crate) fn gemini_source_check_body() -> Value {
    json!({
        "contents": [{"role":"user","parts":[{"text":"hi"}]}],
        "generationConfig": {"candidateCount":1,"maxOutputTokens":1}
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeminiSourceCheckResponseError {
    ObjectRequired,
    ErrorEnvelope,
    PromptBlocked,
    SingleCandidateRequired,
    CandidateObjectRequired,
    CandidateIndexInvalid,
    ContentObjectRequired,
    ModelRoleRequired,
    PartsRequired,
    PartInvalid,
}

impl fmt::Display for GeminiSourceCheckResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::ObjectRequired => "Gemini check response must be a JSON object",
            Self::ErrorEnvelope => "Gemini check response contains an application error",
            Self::PromptBlocked => "Gemini check response reports a blocked prompt",
            Self::SingleCandidateRequired => {
                "Gemini check response must contain exactly one candidate"
            }
            Self::CandidateObjectRequired => "Gemini check candidate must be an object",
            Self::CandidateIndexInvalid => "Gemini check candidate index must be zero",
            Self::ContentObjectRequired => "Gemini check candidate content must be an object",
            Self::ModelRoleRequired => "Gemini check candidate content role must be model",
            Self::PartsRequired => "Gemini check candidate content must contain non-empty parts",
            Self::PartInvalid => "Gemini check candidate contains no valid output part",
        })
    }
}

pub(crate) fn validate_gemini_source_check_response(
    response: &Value,
) -> Result<(), GeminiSourceCheckResponseError> {
    let Some(response) = response.as_object() else {
        return Err(GeminiSourceCheckResponseError::ObjectRequired);
    };
    if response.contains_key("error") {
        return Err(GeminiSourceCheckResponseError::ErrorEnvelope);
    }
    if response
        .get("promptFeedback")
        .and_then(Value::as_object)
        .and_then(|feedback| feedback.get("blockReason"))
        .is_some_and(|reason| !reason.is_null())
    {
        return Err(GeminiSourceCheckResponseError::PromptBlocked);
    }
    let Some(candidates) = response.get("candidates").and_then(Value::as_array) else {
        return Err(GeminiSourceCheckResponseError::SingleCandidateRequired);
    };
    if candidates.len() != 1 {
        return Err(GeminiSourceCheckResponseError::SingleCandidateRequired);
    }
    let Some(candidate) = candidates[0].as_object() else {
        return Err(GeminiSourceCheckResponseError::CandidateObjectRequired);
    };
    if candidate
        .get("index")
        .is_some_and(|index| index.as_u64() != Some(0))
    {
        return Err(GeminiSourceCheckResponseError::CandidateIndexInvalid);
    }
    let Some(content) = candidate.get("content").and_then(Value::as_object) else {
        return Err(GeminiSourceCheckResponseError::ContentObjectRequired);
    };
    if content.get("role").and_then(Value::as_str) != Some("model") {
        return Err(GeminiSourceCheckResponseError::ModelRoleRequired);
    }
    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
        return Err(GeminiSourceCheckResponseError::PartsRequired);
    };
    if parts.is_empty() {
        return Err(GeminiSourceCheckResponseError::PartsRequired);
    }
    let has_valid_part = parts.iter().any(|part| {
        let Some(part) = part.as_object() else {
            return false;
        };
        part.get("text").is_some_and(Value::is_string)
            || part.get("functionCall").is_some_and(Value::is_object)
            || part.get("executableCode").is_some_and(Value::is_object)
            || part.get("inlineData").is_some_and(Value::is_object)
    });
    if !has_valid_part {
        return Err(GeminiSourceCheckResponseError::PartInvalid);
    }
    Ok(())
}

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
    GeminiModelCollectionRequired,
    GeminiEncodedPathSeparatorNotAllowed,
    GeminiModelIdRequired,
    GeminiModelIdWhitespaceNotAllowed,
    GeminiModelIdInvalid,
    GeminiOperationTargetMismatch,
    GeminiApiKeyQueryNotAllowed,
    GeminiAltQueryInvalid,
    GeminiSensitiveHeaderNotAllowed,
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
            Self::GeminiModelCollectionRequired => {
                "Gemini base URL must identify a model collection ending in /models"
            }
            Self::GeminiEncodedPathSeparatorNotAllowed => {
                "Gemini base URL and model ID must not contain encoded path separators"
            }
            Self::GeminiModelIdRequired => "Gemini model ID must not be empty",
            Self::GeminiModelIdWhitespaceNotAllowed => {
                "Gemini model ID must not contain leading or trailing whitespace"
            }
            Self::GeminiModelIdInvalid => {
                "Gemini model ID must be one terminal path segment"
            }
            Self::GeminiOperationTargetMismatch => {
                "Gemini target URL does not match the selected operation"
            }
            Self::GeminiApiKeyQueryNotAllowed => {
                "Gemini target URL must not contain an API key query"
            }
            Self::GeminiAltQueryInvalid => {
                "Gemini target URL has an invalid streaming query contract"
            }
            Self::GeminiSensitiveHeaderNotAllowed => {
                "Gemini target headers must not contain downstream credentials or identity"
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

pub(crate) fn anthropic_messages_url(base_url: &str) -> Result<String, ProviderHttpUrlError> {
    join_base_url_and_operation_path(base_url, ANTHROPIC_MESSAGES_OPERATION)
}

fn contains_encoded_path_separator(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.contains("%2f") || lower.contains("%5c")
}

pub(crate) fn validate_gemini_model_id(model_id: &str) -> Result<(), ProviderHttpUrlError> {
    if model_id.is_empty() {
        return Err(ProviderHttpUrlError::GeminiModelIdRequired);
    }
    if model_id.trim() != model_id {
        return Err(ProviderHttpUrlError::GeminiModelIdWhitespaceNotAllowed);
    }
    if model_id.starts_with("models/")
        || model_id.chars().any(|character| {
            character.is_control() || matches!(character, '/' | '\\' | ':' | '?' | '#')
        })
    {
        return Err(ProviderHttpUrlError::GeminiModelIdInvalid);
    }
    if contains_encoded_path_separator(model_id) {
        return Err(ProviderHttpUrlError::GeminiEncodedPathSeparatorNotAllowed);
    }
    Ok(())
}

pub(crate) fn gemini_model_operation_url(
    model_collection_base_url: &str,
    model_id: &str,
    operation: GeminiModelOperation,
) -> Result<String, ProviderHttpUrlError> {
    let normalized_base_url = normalize_provider_base_url(model_collection_base_url)?;
    let parsed = Url::parse(&normalized_base_url).map_err(|_| ProviderHttpUrlError::InvalidUrl)?;
    if contains_encoded_path_separator(parsed.path()) {
        return Err(ProviderHttpUrlError::GeminiEncodedPathSeparatorNotAllowed);
    }
    if parsed.path_segments().and_then(Iterator::last) != Some("models") {
        return Err(ProviderHttpUrlError::GeminiModelCollectionRequired);
    }
    validate_gemini_model_id(model_id)?;
    Ok(format!(
        "{normalized_base_url}/{model_id}:{}",
        operation.action()
    ))
}

pub(crate) fn validate_gemini_pre_auth_target(
    url: &Url,
    headers: &HeaderMap,
    operation: GeminiModelOperation,
) -> Result<(), ProviderHttpUrlError> {
    let expected_suffix = format!(":{}", operation.action());
    if url.fragment().is_some()
        || !url
            .path_segments()
            .and_then(Iterator::last)
            .is_some_and(|segment| segment.ends_with(&expected_suffix))
    {
        return Err(ProviderHttpUrlError::GeminiOperationTargetMismatch);
    }

    let mut alt_values = Vec::new();
    for (key, value) in url.query_pairs() {
        if key.eq_ignore_ascii_case("key") {
            return Err(ProviderHttpUrlError::GeminiApiKeyQueryNotAllowed);
        }
        if key.eq_ignore_ascii_case("alt") {
            alt_values.push(value.into_owned());
        }
    }
    match operation {
        GeminiModelOperation::StreamGenerateContent if alt_values.as_slice() == ["sse"] => {}
        GeminiModelOperation::StreamGenerateContent => {
            return Err(ProviderHttpUrlError::GeminiAltQueryInvalid);
        }
        GeminiModelOperation::GenerateContent | GeminiModelOperation::CountTokens
            if alt_values.is_empty() => {}
        GeminiModelOperation::GenerateContent | GeminiModelOperation::CountTokens => {
            return Err(ProviderHttpUrlError::GeminiAltQueryInvalid);
        }
    }

    for name in [
        "authorization",
        "proxy-authorization",
        "api-key",
        "x-api-key",
        "x-goog-api-key",
        "cookie",
        "x-request-id",
        "x-client-request-id",
    ] {
        if headers.contains_key(name) {
            return Err(ProviderHttpUrlError::GeminiSensitiveHeaderNotAllowed);
        }
    }
    Ok(())
}

pub(crate) fn enforce_anthropic_version_header(headers: &mut HeaderMap) {
    headers.remove(ANTHROPIC_VERSION_HEADER);
    headers.insert(
        ANTHROPIC_VERSION_HEADER,
        HeaderValue::from_static(ANTHROPIC_VERSION),
    );
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
            ("http://api.example.com/api/", "http://api.example.com/api"),
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
        assert_eq!(
            join_base_url_and_operation_path(
                "https://relay.example/proxy/openai/v1///",
                "responses/",
            ),
            Ok("https://relay.example/proxy/openai/v1/responses".to_string())
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
    fn anthropic_request_contract_normalizes_messages_url_and_fixed_version() {
        for base_url in [
            "https://api.anthropic.com/v1",
            "https://api.anthropic.com/v1/",
        ] {
            assert_eq!(
                anthropic_messages_url(base_url),
                Ok("https://api.anthropic.com/v1/messages".to_string())
            );
        }

        let mut headers = HeaderMap::new();
        headers.append(
            ANTHROPIC_VERSION_HEADER,
            HeaderValue::from_static("2020-01-01"),
        );
        headers.append(
            ANTHROPIC_VERSION_HEADER,
            HeaderValue::from_static("2099-01-01"),
        );
        enforce_anthropic_version_header(&mut headers);
        assert_eq!(
            headers
                .get(ANTHROPIC_VERSION_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(ANTHROPIC_VERSION)
        );
        assert_eq!(headers.get_all(ANTHROPIC_VERSION_HEADER).iter().count(), 1);
    }

    #[test]
    fn gemini_model_operation_builder_supports_both_profiles_and_safe_proxy_collections() {
        let bases = [
            (
                "https://generativelanguage.googleapis.com/v1beta/models/",
                "https://generativelanguage.googleapis.com/v1beta/models",
            ),
            (
                "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/us-central1/publishers/google/models",
                "https://us-central1-aiplatform.googleapis.com/v1/projects/p/locations/us-central1/publishers/google/models",
            ),
            (
                "https://proxy.example/prefix/google/v1/models///",
                "https://proxy.example/prefix/google/v1/models",
            ),
            (
                "http://127.0.0.1:8080/v1beta/models",
                "http://127.0.0.1:8080/v1beta/models",
            ),
            (
                "http://[::1]:8080/v1beta/models/",
                "http://[::1]:8080/v1beta/models",
            ),
            (
                "http://10.0.0.8:8080/custom/models",
                "http://10.0.0.8:8080/custom/models",
            ),
        ];
        let operations = [
            (GeminiModelOperation::GenerateContent, "generateContent"),
            (
                GeminiModelOperation::StreamGenerateContent,
                "streamGenerateContent",
            ),
            (GeminiModelOperation::CountTokens, "countTokens"),
        ];

        for (base, normalized) in bases {
            for (operation, action) in operations {
                assert_eq!(
                    gemini_model_operation_url(base, "gemini-2.5-flash", operation),
                    Ok(format!("{normalized}/gemini-2.5-flash:{action}")),
                    "base={base} operation={operation:?}"
                );
            }
        }
    }

    #[test]
    fn gemini_model_operation_builder_rejects_ambiguous_collections_and_model_segments() {
        let invalid_bases = [
            "https://user:secret@api.example/v1beta/models",
            "https://api.example/v1beta/models?key=private",
            "https://api.example/v1beta/models#fragment",
            "https://api.example/v1beta/model",
            "https://api.example/v1beta/models/%2f/private/models",
            "https://api.example/v1beta/models/%5C/private/models",
        ];
        for base in invalid_bases {
            assert!(
                gemini_model_operation_url(
                    base,
                    "gemini-2.5-flash",
                    GeminiModelOperation::GenerateContent,
                )
                .is_err(),
                "base={base}"
            );
        }

        let base = "https://api.example/v1beta/models";
        let invalid_models = [
            "",
            " gemini-2.5-flash",
            "gemini-2.5-flash ",
            "models/gemini-2.5-flash",
            "projects/p/locations/l/publishers/google/models/gemini-2.5-flash",
            "gemini/2.5",
            "gemini\\2.5",
            "gemini:2.5",
            "gemini?key=private",
            "gemini#fragment",
            "gemini\nprivate",
            "gemini%2Fprivate",
            "gemini%5cprivate",
        ];
        for model in invalid_models {
            assert!(
                gemini_model_operation_url(base, model, GeminiModelOperation::GenerateContent,)
                    .is_err(),
                "model={model:?}"
            );
        }

        let error = gemini_model_operation_url(
            "https://user:sentinel-secret@api.example/v1beta/models?key=sentinel-key",
            "models/sentinel-model",
            GeminiModelOperation::GenerateContent,
        )
        .expect_err("secret-bearing URL must fail")
        .to_string();
        assert!(!error.contains("sentinel"));
        assert!(!error.contains("https://"));
    }

    #[test]
    fn gemini_pre_auth_target_enforces_query_operation_and_sensitive_header_cardinality() {
        let headers = HeaderMap::new();
        for (url, operation) in [
            (
                "https://api.example/v1beta/models/model:generateContent?trace=safe",
                GeminiModelOperation::GenerateContent,
            ),
            (
                "https://api.example/v1beta/models/model:streamGenerateContent?trace=safe&alt=sse",
                GeminiModelOperation::StreamGenerateContent,
            ),
            (
                "https://api.example/v1beta/models/model:countTokens?trace=safe",
                GeminiModelOperation::CountTokens,
            ),
        ] {
            validate_gemini_pre_auth_target(&Url::parse(url).unwrap(), &headers, operation)
                .expect("safe target");
        }

        for (url, operation, expected) in [
            (
                "https://api.example/v1beta/models/model:generateContent?key=sentinel",
                GeminiModelOperation::GenerateContent,
                ProviderHttpUrlError::GeminiApiKeyQueryNotAllowed,
            ),
            (
                "https://api.example/v1beta/models/model:generateContent?Key=sentinel",
                GeminiModelOperation::GenerateContent,
                ProviderHttpUrlError::GeminiApiKeyQueryNotAllowed,
            ),
            (
                "https://api.example/v1beta/models/model:generateContent?alt=sse",
                GeminiModelOperation::GenerateContent,
                ProviderHttpUrlError::GeminiAltQueryInvalid,
            ),
            (
                "https://api.example/v1beta/models/model:streamGenerateContent",
                GeminiModelOperation::StreamGenerateContent,
                ProviderHttpUrlError::GeminiAltQueryInvalid,
            ),
            (
                "https://api.example/v1beta/models/model:streamGenerateContent?alt=sse&alt=sse",
                GeminiModelOperation::StreamGenerateContent,
                ProviderHttpUrlError::GeminiAltQueryInvalid,
            ),
            (
                "https://api.example/v1beta/models/model:streamGenerateContent?alt=json",
                GeminiModelOperation::StreamGenerateContent,
                ProviderHttpUrlError::GeminiAltQueryInvalid,
            ),
            (
                "https://api.example/v1beta/models/model:countTokens?alt=sse",
                GeminiModelOperation::CountTokens,
                ProviderHttpUrlError::GeminiAltQueryInvalid,
            ),
            (
                "https://api.example/v1beta/models/model:countTokens",
                GeminiModelOperation::GenerateContent,
                ProviderHttpUrlError::GeminiOperationTargetMismatch,
            ),
        ] {
            assert_eq!(
                validate_gemini_pre_auth_target(&Url::parse(url).unwrap(), &headers, operation),
                Err(expected),
                "url={url}"
            );
        }

        for name in [
            "authorization",
            "proxy-authorization",
            "api-key",
            "x-api-key",
            "x-goog-api-key",
            "cookie",
            "x-request-id",
            "x-client-request-id",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(name, HeaderValue::from_static("sentinel-secret"));
            assert_eq!(
                validate_gemini_pre_auth_target(
                    &Url::parse(
                        "https://api.example/v1beta/models/model:generateContent?trace=safe"
                    )
                    .unwrap(),
                    &headers,
                    GeminiModelOperation::GenerateContent,
                ),
                Err(ProviderHttpUrlError::GeminiSensitiveHeaderNotAllowed),
                "header={name}"
            );
        }
    }

    #[test]
    fn gemini_shared_request_primitives_cover_runtime_and_source_check() {
        assert_eq!(
            gemini_operation_target_url(
                "https://api.example/v1beta/models/",
                "gemini-fixture",
                GeminiModelOperation::GenerateContent,
            )
            .expect("non-stream Gemini target")
            .as_str(),
            "https://api.example/v1beta/models/gemini-fixture:generateContent"
        );
        assert_eq!(
            gemini_operation_target_url(
                "https://api.example/v1beta/models",
                "gemini-fixture",
                GeminiModelOperation::StreamGenerateContent,
            )
            .expect("stream Gemini target")
            .as_str(),
            "https://api.example/v1beta/models/gemini-fixture:streamGenerateContent?alt=sse"
        );

        let mut incoming = HeaderMap::new();
        for name in [
            "authorization",
            "x-api-key",
            "x-goog-api-key",
            "cookie",
            "proxy-authorization",
            "x-request-id",
            "x-client-request-id",
        ] {
            incoming.insert(name, HeaderValue::from_static("private"));
        }
        incoming.insert("content-type", HeaderValue::from_static("application/json"));
        incoming.insert("x-safe", HeaderValue::from_static("yes"));
        let sanitized = sanitize_gemini_request_headers(&incoming);
        assert_eq!(sanitized.len(), 2);
        assert_eq!(sanitized.get("x-safe").unwrap(), "yes");
        assert_eq!(
            gemini_source_check_body(),
            json!({
                "contents":[{"role":"user","parts":[{"text":"hi"}]}],
                "generationConfig":{"candidateCount":1,"maxOutputTokens":1}
            })
        );
    }

    #[test]
    fn gemini_source_check_response_requires_one_valid_model_candidate() {
        validate_gemini_source_check_response(&json!({
            "responseId":"response-fixture",
            "candidates":[{
                "index":0,
                "content":{"role":"model","parts":[{"text":"ok"}]},
                "finishReason":"STOP"
            }]
        }))
        .expect("minimal Gemini check response");

        for (response, expected) in [
            (json!(null), GeminiSourceCheckResponseError::ObjectRequired),
            (
                json!({}),
                GeminiSourceCheckResponseError::SingleCandidateRequired,
            ),
            (
                json!({"error":{"message":"private"}}),
                GeminiSourceCheckResponseError::ErrorEnvelope,
            ),
            (
                json!({"promptFeedback":{"blockReason":"SAFETY"}}),
                GeminiSourceCheckResponseError::PromptBlocked,
            ),
            (
                json!({"candidates":[]}),
                GeminiSourceCheckResponseError::SingleCandidateRequired,
            ),
            (
                json!({"candidates":[{"index":1,"content":{"role":"model","parts":[{"text":"x"}]}}]}),
                GeminiSourceCheckResponseError::CandidateIndexInvalid,
            ),
            (
                json!({"candidates":[{"index":0,"content":{"role":"user","parts":[{"text":"x"}]}}]}),
                GeminiSourceCheckResponseError::ModelRoleRequired,
            ),
            (
                json!({"candidates":[{"index":0,"content":{"role":"model","parts":[]}}]}),
                GeminiSourceCheckResponseError::PartsRequired,
            ),
            (
                json!({"candidates":[{"index":0,"content":{"role":"model","parts":[{"future":"private"}]}}]}),
                GeminiSourceCheckResponseError::PartInvalid,
            ),
        ] {
            assert_eq!(
                validate_gemini_source_check_response(&response),
                Err(expected)
            );
            assert!(!expected.to_string().contains("private"));
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
