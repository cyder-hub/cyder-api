use std::{
    collections::HashMap,
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

use cyder_tools::log::info;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use once_cell::sync::Lazy;
use reqwest::{header::CONTENT_TYPE, Proxy};
use serde::Deserialize;

use crate::config::CONFIG;

#[derive(Clone, Debug)]
struct CachedToken {
    access_token: String,
    expiry_time: u64, // Store expiry time as Unix timestamp
}

static VERTEX_TOKEN_CACHE: Lazy<Mutex<HashMap<i64, CachedToken>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(serde::Serialize)]
struct Payload<'a> {
    grant_type: &'a str,
    assertion: &'a str,
}

fn issued_at() -> u64 {
    SystemTime::UNIX_EPOCH.elapsed().unwrap().as_secs() - 10
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
        .expect("Time went backwards")
        .as_secs()
}

fn get_token_from_cache(key: &i64) -> Option<String> {
    let cache = VERTEX_TOKEN_CACHE.lock().unwrap();
    let now = get_current_timestamp();

    // Check cache first
    if let Some(cached) = cache.get(key) {
        // Check if token is still valid (add a small buffer, e.g., 60 seconds)
        if cached.expiry_time > now + 60 {
            return Some(cached.access_token.clone());
        }
    }
    None
}

pub async fn get_vertex_token(
    provider_key_id: i64,
    service_account_str: &str,
) -> Result<String, String> {
    if let Some(token) = get_token_from_cache(&provider_key_id) {
        return Ok(token);
    }

    // If not in cache or expired, request a new token
    let now = get_current_timestamp();
    info!("{provider_key_id} vertex token not in cache or expired, regenerate token");
    let vertex_token_result = request_google_token(service_account_str).await?;
    let expiry_time = now + vertex_token_result.expires_in as u64;

    let new_cached_token = CachedToken {
        access_token: vertex_token_result.access_token.clone(),
        expiry_time,
    };
    let mut cache = VERTEX_TOKEN_CACHE.lock().unwrap();
    cache.insert(provider_key_id, new_cached_token);

    Ok(vertex_token_result.access_token)
}

pub async fn request_google_token(service_account_str: &str) -> Result<VertexTokenResult, String> {
    let vertex_account: VertexServiceAccount = serde_json::from_str(service_account_str).unwrap();
    let client_email = &vertex_account.client_email;
    let token_uri = &vertex_account.token_uri;
    let private_key = &vertex_account.private_key;
    let private_key_id = &vertex_account.private_key_id;

    let scope = "https://www.googleapis.com/auth/cloud-platform";

    const EXPIRE: u64 = 60 * 60;
    let iat = issued_at();

    let private_key =
        EncodingKey::from_rsa_pem(private_key.as_bytes()).map_err(|e| e.to_string())?;

    let proxy = Proxy::https(&CONFIG.proxy.url).unwrap();
    let client = reqwest::Client::builder().proxy(proxy).build().unwrap();
    let claims = Claims {
        iss: client_email,
        scope: scope,
        aud: token_uri,
        iat,
        exp: iat + EXPIRE,
    };

    let header = Header {
        typ: Some("JWT".to_string()),
        alg: Algorithm::RS256,
        kid: Some(private_key_id.to_string()),
        cty: None,
        jku: None,
        jwk: None,
        x5u: None,
        x5c: None,
        x5t: None,
        x5t_s256: None,
    };
    let body_str = serde_urlencoded::to_string(&Payload {
        grant_type: "urn:ietf:params:oauth:grant-type:jwt-bearer",
        assertion: &encode(&header, &claims, &private_key).unwrap(),
    })
    .unwrap();

    let response = client
        .post(token_uri)
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(body_str)
        .send()
        .await
        .unwrap();

    match response.status() {
        reqwest::StatusCode::OK => {
            let token_result: VertexTokenResult = response.json().await.unwrap();
            Ok(token_result)
        }
        _ => Err("fail".to_string()),
    }
}
