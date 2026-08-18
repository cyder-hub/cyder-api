use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cyder_tools::log::{error, info};
use dashmap::DashMap;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, header::CONTENT_TYPE};
use serde::Deserialize;
use std::sync::LazyLock;

use crate::config::NonStreamResponseConfig;

#[cfg(test)]
use super::upstream_response::read_complete_response_body;
use super::{
    auxiliary_http::{
        AuxiliaryHttpError, parse_auxiliary_json, read_auxiliary_response_body,
        send_auxiliary_request,
    },
    provider_http::validate_vertex_token_uri,
    upstream_response::apply_upstream_accept_encoding,
};

#[derive(Clone)]
struct CachedToken {
    access_token: String,
    expiry_time: u64, // Store expiry time as Unix timestamp
}

static VERTEX_TOKEN_CACHE: LazyLock<DashMap<i64, CachedToken>> = LazyLock::new(DashMap::new);

#[derive(serde::Serialize)]
struct Payload<'a> {
    grant_type: &'a str,
    assertion: &'a str,
}

fn issued_at() -> u64 {
    SystemTime::UNIX_EPOCH
        .elapsed()
        .map(|d| d.as_secs())
        .unwrap_or_else(|_| {
            error!("SystemTime::UNIX_EPOCH.elapsed() failed");
            0
        })
        .saturating_sub(10)
}

#[derive(serde::Serialize)]
struct Claims<'a> {
    iss: &'a str,
    scope: &'a str,
    aud: &'a str,
    iat: u64,
    exp: u64,
}

#[derive(Deserialize)]
struct VertexServiceAccount {
    client_email: String,
    token_uri: String,
    private_key: String,
    private_key_id: String,
}

pub fn validate_vertex_service_account(service_account_str: &str) -> Result<(), String> {
    let account: VertexServiceAccount = serde_json::from_str(service_account_str)
        .map_err(|_| "Vertex credential must be a valid service account JSON".to_string())?;
    if account.client_email.trim().is_empty()
        || account.token_uri.trim().is_empty()
        || account.private_key_id.trim().is_empty()
    {
        return Err("Vertex credential is missing required service account fields".to_string());
    }
    validate_vertex_token_uri(&account.token_uri)
        .map_err(|error| format!("Vertex credential token_uri {error}"))?;
    EncodingKey::from_rsa_pem(account.private_key.as_bytes())
        .map_err(|_| "Vertex credential contains an invalid RSA private key".to_string())?;
    Ok(())
}

pub fn invalidate_vertex_token(provider_key_id: i64) {
    VERTEX_TOKEN_CACHE.remove(&provider_key_id);
}

#[cfg(test)]
pub(crate) fn cache_vertex_token_for_test(provider_key_id: i64, access_token: &str) {
    VERTEX_TOKEN_CACHE.insert(
        provider_key_id,
        CachedToken {
            access_token: access_token.to_string(),
            expiry_time: u64::MAX,
        },
    );
}

#[cfg(test)]
pub(crate) fn vertex_token_is_cached_for_test(provider_key_id: i64) -> bool {
    VERTEX_TOKEN_CACHE.contains_key(&provider_key_id)
}

#[derive(Deserialize, Clone)]
pub struct VertexTokenResult {
    pub access_token: String,
    pub expires_in: u32,
}

fn get_current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| {
            error!("Time went backwards");
            SystemTime::UNIX_EPOCH.duration_since(UNIX_EPOCH).unwrap()
        })
        .as_secs()
}

fn get_token_from_cache(key: &i64) -> Option<String> {
    let now = get_current_timestamp();

    // Check cache first
    if let Some(cached) = VERTEX_TOKEN_CACHE.get(key) {
        // Check if token is still valid (add a small buffer, e.g., 60 seconds)
        if cached.expiry_time > now + 60 {
            return Some(cached.access_token.clone());
        }
    }
    None
}

pub async fn get_vertex_token(
    client: &Client,
    provider_key_id: i64,
    service_account_str: &str,
    limits: &NonStreamResponseConfig,
    auxiliary_total_timeout: Duration,
) -> Result<String, String> {
    let account: VertexServiceAccount = serde_json::from_str(service_account_str)
        .map_err(|_| "Vertex credential must be a valid service account JSON".to_string())?;
    validate_vertex_token_uri(&account.token_uri)
        .map_err(|error| format!("Vertex credential token_uri {error}"))?;

    if let Some(token) = get_token_from_cache(&provider_key_id) {
        return Ok(token);
    }

    // If not in cache or expired, request a new token
    let now = get_current_timestamp();
    info!("{provider_key_id} vertex token not in cache or expired, regenerate token");
    let vertex_token_result =
        request_google_token(client, service_account_str, limits, auxiliary_total_timeout).await?;
    Ok(cache_vertex_token_result(
        provider_key_id,
        vertex_token_result,
        now,
    ))
}

fn cache_vertex_token_result(
    provider_key_id: i64,
    vertex_token_result: VertexTokenResult,
    observed_at: u64,
) -> String {
    let expiry_time = observed_at + u64::from(vertex_token_result.expires_in);
    let access_token = vertex_token_result.access_token;
    VERTEX_TOKEN_CACHE.insert(
        provider_key_id,
        CachedToken {
            access_token: access_token.clone(),
            expiry_time,
        },
    );
    access_token
}

pub async fn request_google_token(
    client: &Client,
    service_account_str: &str,
    limits: &NonStreamResponseConfig,
    auxiliary_total_timeout: Duration,
) -> Result<VertexTokenResult, String> {
    let vertex_account: VertexServiceAccount = serde_json::from_str(service_account_str)
        .map_err(|_| "Vertex credential must be a valid service account JSON".to_string())?;
    let client_email = &vertex_account.client_email;
    let token_uri = &vertex_account.token_uri;
    let private_key_str = &vertex_account.private_key;
    let private_key_id = &vertex_account.private_key_id;
    validate_vertex_token_uri(token_uri)
        .map_err(|error| format!("Vertex credential token_uri {error}"))?;

    let scope = "https://www.googleapis.com/auth/cloud-platform";

    const EXPIRE: u64 = 60 * 60;
    let iat = issued_at();

    let private_key = EncodingKey::from_rsa_pem(private_key_str.as_bytes())
        .map_err(|_| "Vertex credential contains an invalid RSA private key".to_string())?;

    let claims = Claims {
        iss: client_email,
        scope,
        aud: token_uri,
        iat,
        exp: iat + EXPIRE,
    };

    let mut header = Header::new(Algorithm::HS512);
    header.typ = Some("JWT".to_string());
    header.alg = Algorithm::RS256;
    header.kid = Some(private_key_id.to_string());

    let assertion = encode(&header, &claims, &private_key)
        .map_err(|_| "Failed to sign Vertex OAuth assertion".to_string())?;

    let body_str = serde_urlencoded::to_string(&Payload {
        grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
        assertion: &assertion,
    })
    .map_err(|_| "Failed to encode Vertex OAuth request".to_string())?;

    let headers = vertex_oauth_request_headers();
    let auxiliary_response = send_auxiliary_request(
        client.post(token_uri).headers(headers).body(body_str),
        auxiliary_total_timeout,
    )
    .await
    .map_err(|error| auxiliary_error_message("Failed to send Vertex OAuth request", &error))?;
    let (response, deadline) = auxiliary_response.into_parts();

    read_vertex_token_response_with_deadline(response, limits, deadline).await
}

fn auxiliary_error_message(context: &'static str, error: &AuxiliaryHttpError) -> String {
    format!("{context} ({})", error.safe_kind())
}

fn vertex_oauth_request_headers() -> reqwest::header::HeaderMap {
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(
        CONTENT_TYPE,
        reqwest::header::HeaderValue::from_static("application/x-www-form-urlencoded"),
    );
    apply_upstream_accept_encoding(&mut headers, false);
    headers
}

#[cfg(test)]
async fn read_vertex_token_response(
    response: reqwest::Response,
    limits: &NonStreamResponseConfig,
) -> Result<VertexTokenResult, String> {
    let status = response.status();
    if status.is_success() {
        let body = read_complete_response_body(response, limits)
            .await
            .map_err(|error| format!("Vertex OAuth response was invalid ({error})"))?;
        serde_json::from_slice(&body.bytes)
            .map_err(|_| "Vertex OAuth response was invalid (invalid_json)".to_string())
    } else {
        error!("Vertex token request failed with status {}", status);
        Err(format!(
            "Vertex token request failed with status {}",
            status
        ))
    }
}

async fn read_vertex_token_response_with_deadline(
    response: reqwest::Response,
    limits: &NonStreamResponseConfig,
    deadline: tokio::time::Instant,
) -> Result<VertexTokenResult, String> {
    let status = response.status();
    if status.is_success() {
        let body = read_auxiliary_response_body(
            super::auxiliary_http::AuxiliaryResponse::from_parts(response, deadline),
            limits,
        )
        .await
        .map_err(|error| format!("Vertex OAuth response was invalid ({error})"))?;
        parse_auxiliary_json::<VertexTokenResult>(body.bytes, deadline)
            .await
            .map_err(|error| format!("Vertex OAuth response was invalid ({})", error.safe_kind()))
    } else {
        error!("Vertex token request failed with status {}", status);
        Err(format!(
            "Vertex token request failed with status {}",
            status
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use flate2::{Compression, write::GzEncoder};
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::timeout,
    };

    use super::*;

    fn limits() -> NonStreamResponseConfig {
        crate::config::ProxyRequestConfig::default().non_stream_response
    }

    fn gzip(body: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(body).unwrap();
        encoder.finish().unwrap()
    }

    fn exact_token_json(size: usize) -> Vec<u8> {
        let empty = serde_json::to_vec(&serde_json::json!({
            "access_token": "",
            "expires_in": 3600
        }))
        .unwrap();
        let body = serde_json::to_vec(&serde_json::json!({
            "access_token": "x".repeat(size - empty.len()),
            "expires_in": 3600
        }))
        .unwrap();
        assert_eq!(body.len(), size);
        body
    }

    async fn response_fixture(
        status: reqwest::StatusCode,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> reqwest::Response {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let mut wire = format!(
            "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n",
            status.as_u16(),
            body.len()
        );
        for (name, value) in headers {
            wire.push_str(name);
            wire.push_str(": ");
            wire.push_str(value);
            wire.push_str("\r\n");
        }
        wire.push_str("\r\n");
        let body = body.to_vec();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket.write_all(wire.as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
        });
        reqwest::Client::new()
            .get(format!(
                "http://{address}/token?assertion=query-secret-marker"
            ))
            .send()
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn malformed_service_account_error_never_echoes_input() {
        let marker = "vertex-private-sensitive-marker";
        let error =
            match request_google_token(&Client::new(), marker, &limits(), Duration::from_secs(60))
                .await
            {
                Ok(_) => panic!("malformed credential should fail"),
                Err(error) => error,
            };

        assert_eq!(
            error,
            "Vertex credential must be a valid service account JSON"
        );
        assert!(!error.contains(marker));
    }

    #[test]
    fn vertex_oauth_request_owns_content_and_accept_encoding_headers() {
        let headers = vertex_oauth_request_headers();
        assert_eq!(
            headers.get(CONTENT_TYPE).unwrap(),
            "application/x-www-form-urlencoded"
        );
        assert_eq!(
            headers.get(reqwest::header::ACCEPT_ENCODING).unwrap(),
            "gzip, identity"
        );
    }

    #[tokio::test]
    async fn cached_oauth_token_avoids_reusing_service_account_material() {
        let key_id = 98_765;
        cache_vertex_token_for_test(key_id, "cached-oauth-token");

        let token = get_vertex_token(
            &Client::new(),
            key_id,
            r#"{"client_email":"svc@example.com","token_uri":"https://oauth2.googleapis.com/token","private_key_id":"kid","private_key":"not-a-key"}"#,
            &limits(),
            Duration::from_secs(60),
        )
        .await
        .expect("cached token should be returned without parsing the private key");

        assert_eq!(token, "cached-oauth-token");
        invalidate_vertex_token(key_id);
    }

    #[tokio::test]
    async fn cached_oauth_token_does_not_bypass_token_uri_validation() {
        let key_id = 98_766;
        cache_vertex_token_for_test(key_id, "cached-oauth-token");

        let error = get_vertex_token(
            &Client::new(),
            key_id,
            r#"{"client_email":"svc@example.com","token_uri":"https://oauth.example.com/token","private_key_id":"kid","private_key":"not-a-key"}"#,
            &limits(),
            Duration::from_secs(60),
        )
        .await
        .expect_err("legacy unsupported token URI must fail even when a token was cached");

        assert_eq!(
            error,
            "Vertex credential token_uri must exactly match https://oauth2.googleapis.com/token"
        );
        invalidate_vertex_token(key_id);
    }

    #[tokio::test]
    async fn unsupported_vertex_token_uri_fails_before_signing_or_network_access() {
        let error = match request_google_token(
            &Client::new(),
            r#"{"client_email":"svc@example.com","token_uri":"https://oauth.example.com/token","private_key_id":"kid","private_key":"not-a-key"}"#,
            &limits(),
            Duration::from_secs(60),
        )
        .await
        {
            Ok(_) => panic!("unsupported token URI should fail before key parsing"),
            Err(error) => error,
        };

        assert_eq!(
            error,
            "Vertex credential token_uri must exactly match https://oauth2.googleapis.com/token"
        );
    }

    #[tokio::test]
    async fn vertex_response_reader_covers_encoding_limits_json_and_safe_errors() {
        let raw_limits = NonStreamResponseConfig {
            raw_body_limit_bytes: 64,
            decoded_body_limit_bytes: 128,
        };
        let decoded_limits = NonStreamResponseConfig {
            raw_body_limit_bytes: 128,
            decoded_body_limit_bytes: 64,
        };
        let exact = exact_token_json(64);
        let identity = read_vertex_token_response(
            response_fixture(reqwest::StatusCode::OK, &[], &exact).await,
            &raw_limits,
        )
        .await
        .unwrap();
        assert!(!identity.access_token.is_empty());
        let gzip_result = read_vertex_token_response(
            response_fixture(
                reqwest::StatusCode::OK,
                &[("Content-Encoding", "gzip")],
                &gzip(&exact),
            )
            .await,
            &decoded_limits,
        )
        .await
        .unwrap();
        assert_eq!(gzip_result.access_token, identity.access_token);

        let raw_error = read_vertex_token_response(
            response_fixture(reqwest::StatusCode::OK, &[], &exact_token_json(65)).await,
            &raw_limits,
        )
        .await
        .err()
        .expect("raw +1 must fail");
        assert!(raw_error.contains("raw response body exceeded"));
        let decoded_error = read_vertex_token_response(
            response_fixture(
                reqwest::StatusCode::OK,
                &[("Content-Encoding", "gzip")],
                &gzip(&exact_token_json(65)),
            )
            .await,
            &decoded_limits,
        )
        .await
        .err()
        .expect("decoded +1 must fail");
        assert!(decoded_error.contains("decoded response body exceeded"));

        for (status, headers, body, category) in [
            (
                reqwest::StatusCode::OK,
                vec![
                    ("Content-Encoding", "br"),
                    ("X-Private", "header-secret-marker"),
                ],
                b"provider-body-secret-marker".as_slice(),
                "Content-Encoding",
            ),
            (
                reqwest::StatusCode::OK,
                vec![("Content-Encoding", "gzip")],
                b"invalid-gzip-body-secret".as_slice(),
                "gzip",
            ),
            (
                reqwest::StatusCode::OK,
                vec![],
                b"invalid-json-body-secret".as_slice(),
                "invalid_json",
            ),
            (
                reqwest::StatusCode::BAD_REQUEST,
                vec![("X-Private", "header-secret-marker")],
                b"oauth-error-body-secret".as_slice(),
                "status 400",
            ),
        ] {
            let error = read_vertex_token_response(
                response_fixture(status, &headers, body).await,
                &decoded_limits,
            )
            .await
            .err()
            .expect("invalid OAuth response must fail");
            assert!(error.contains(category), "{error}");
            for secret in [
                "query-secret-marker",
                "header-secret-marker",
                "provider-body-secret-marker",
                "invalid-gzip-body-secret",
                "invalid-json-body-secret",
                "oauth-error-body-secret",
            ] {
                assert!(!error.contains(secret), "{error}");
            }
        }
    }

    #[tokio::test]
    async fn vertex_auxiliary_body_timeout_is_reported_as_a_safe_credential_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nfirst\r\n",
                )
                .await
                .unwrap();
            let mut byte = [0u8; 1];
            let _ = socket.read(&mut byte).await;
        });

        let response = reqwest::Client::new()
            .get(format!("http://{address}/token"))
            .send()
            .await
            .unwrap();
        let result = timeout(
            Duration::from_secs(2),
            read_vertex_token_response_with_deadline(
                response,
                &limits(),
                tokio::time::Instant::now() + Duration::from_secs(1),
            ),
        )
        .await
        .expect("Vertex auxiliary body read should be bounded");
        let error = match result {
            Ok(_) => panic!("stalled Vertex body should fail"),
            Err(error) => error,
        };

        assert!(error.contains("timed out"));
        assert!(!error.contains("token"));
    }

    #[tokio::test]
    async fn only_complete_valid_vertex_token_results_enter_the_cache() {
        let success_key = 98_767;
        let failed_key = 98_768;
        invalidate_vertex_token(success_key);
        invalidate_vertex_token(failed_key);
        let parsed = read_vertex_token_response(
            response_fixture(
                reqwest::StatusCode::OK,
                &[],
                br#"{"access_token":"bounded-token","expires_in":3600}"#,
            )
            .await,
            &limits(),
        )
        .await
        .unwrap();
        assert_eq!(
            cache_vertex_token_result(success_key, parsed, get_current_timestamp()),
            "bounded-token"
        );
        assert!(vertex_token_is_cached_for_test(success_key));

        let failed = read_vertex_token_response(
            response_fixture(reqwest::StatusCode::OK, &[], b"invalid-json").await,
            &limits(),
        )
        .await;
        assert!(failed.is_err());
        assert!(!vertex_token_is_cached_for_test(failed_key));
        invalidate_vertex_token(success_key);
    }
}
