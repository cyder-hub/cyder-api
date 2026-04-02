use std::time::{SystemTime, UNIX_EPOCH};

use cyder_tools::log::{error, info};
use dashmap::DashMap;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use reqwest::{Client, header::CONTENT_TYPE};
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Clone, Debug)]
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

#[derive(Debug, Deserialize)]
struct VertexServiceAccount {
    client_email: String,
    token_uri: String,
    private_key: String,
    private_key_id: String,
}

#[derive(Debug, Deserialize, Clone)]
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
) -> Result<String, String> {
    if let Some(token) = get_token_from_cache(&provider_key_id) {
        return Ok(token);
    }

    // If not in cache or expired, request a new token
    let now = get_current_timestamp();
    info!("{provider_key_id} vertex token not in cache or expired, regenerate token");
    let vertex_token_result = request_google_token(client, service_account_str).await?;
    let expiry_time = now + vertex_token_result.expires_in as u64;

    let new_cached_token = CachedToken {
        access_token: vertex_token_result.access_token.clone(),
        expiry_time,
    };
    VERTEX_TOKEN_CACHE.insert(provider_key_id, new_cached_token);

    Ok(vertex_token_result.access_token)
}

pub async fn request_google_token(
    client: &Client,
    service_account_str: &str,
) -> Result<VertexTokenResult, String> {
    let vertex_account: VertexServiceAccount =
        serde_json::from_str(service_account_str).map_err(|e| e.to_string())?;
    let client_email = &vertex_account.client_email;
    let token_uri = &vertex_account.token_uri;
    let private_key_str = &vertex_account.private_key;
    let private_key_id = &vertex_account.private_key_id;

    let scope = "https://www.googleapis.com/auth/cloud-platform";

    const EXPIRE: u64 = 60 * 60;
    let iat = issued_at();

    let private_key = EncodingKey::from_rsa_pem(private_key_str.as_bytes())
        .map_err(|e| format!("Failed to load private key: {}", e))?;

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

    let assertion = encode(&header, &claims, &private_key).map_err(|e| e.to_string())?;

    let body_str = serde_urlencoded::to_string(&Payload {
        grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
        assertion: &assertion,
    })
    .map_err(|e| e.to_string())?;

    let response = client
        .post(token_uri)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body_str)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let status = response.status();
    if status.is_success() {
        let token_result: VertexTokenResult = response.json().await.map_err(|e| e.to_string())?;
        Ok(token_result)
    } else {
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());
        error!(
            "Vertex token request failed with status {}: {}",
            status, error_text
        );
        Err(format!(
            "Vertex token request failed with status {}: {}",
            status, error_text
        ))
    }
}
