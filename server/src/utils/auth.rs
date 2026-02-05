use std::time::{SystemTime, UNIX_EPOCH};

use axum::body::Body;
use axum::extract::Request;
use axum::http::{self, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use cyder_tools::auth::{
    decode_jwt, issue_jwt, DecodingKey,
    EncodingKey, JwtError, JwtValidation
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::config::CONFIG;

struct Keys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl Keys {
    fn new(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

static KEYS: Lazy<Keys> =
    Lazy::new(|| Keys::new(CONFIG.jwt_secret.as_bytes()));

const ISSUER: &str = "cyder-api";
const REFRESH_TOKEN_SUBJECT: &str = "REFRESH_TOKEN";
const REFRESH_TOKEN_ISSUE_SEC: u64 = 30 * 24 * 3600;
const ACCESS_TOKEN_ISSUE_SEC: u64 = 3600;

#[derive(Debug, Serialize, Deserialize)]
struct RefreshClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    jti: String,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct JwtResult {
    pub id: i64,
    pub token: String,
}

#[allow(dead_code)]
#[derive(Clone)]
pub struct RefreshJwtResult {
    pub id: i64,
    pub jwt_id: String,
    pub token: String,
}

fn get_current_timestamp() -> u64 {
    let now = SystemTime::now();
    now.duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

impl RefreshClaims {
    fn new(id: i64) -> Self {
        let now = get_current_timestamp();
        RefreshClaims {
            aud: id.to_string(),
            exp: now + REFRESH_TOKEN_ISSUE_SEC,
            iat: now,
            iss: ISSUER.to_string(),
            sub: REFRESH_TOKEN_SUBJECT.to_string(),
            jti: Uuid::new_v4().to_string(),
        }
    }
}

pub fn issue_refresh_token(id: i64) -> String {
    let claims = RefreshClaims::new(id);
    issue_jwt(&KEYS.encoding, &claims)
}

fn decode_refresh_token(token: &str) -> Result<RefreshJwtResult, JwtError> {
    let validate = JwtValidation {
        validate_aud: false,
        issuer: ISSUER,
        required_spec: &["jti", "sub", "iat", "exp"],
    };
    let result = decode_jwt::<RefreshClaims>(&KEYS.decoding, token, validate)?;
    if !REFRESH_TOKEN_SUBJECT.eq(&result.sub) {
        return Err(JwtError::Invalid);
    }
    let user_id = result.aud.parse::<i64>().map_err(|_| JwtError::Parse)?;
    Ok(RefreshJwtResult {
        id: user_id,
        token: token.to_string(),
        jwt_id: result.jti,
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct AccessClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
}

impl AccessClaims {
    fn new(id: i64, sub: String) -> Self {
        let now = get_current_timestamp();
        AccessClaims {
            aud: id.to_string(),
            exp: now + ACCESS_TOKEN_ISSUE_SEC,
            iat: now,
            iss: ISSUER.to_string(),
            sub,
        }
    }
}

pub fn issue_access_token(id: i64, sub: String) -> String {
    let claims = AccessClaims::new(id, sub);
    issue_jwt(&KEYS.encoding, &claims)
}

fn decode_access_token(token: &str) -> Result<JwtResult, JwtError> {
    let validate = JwtValidation {
        validate_aud: false,
        issuer: ISSUER,
        required_spec: &["sub", "iat", "exp"],
    };
    let result = decode_jwt::<AccessClaims>(&KEYS.decoding, &token, validate)?;
    let user_id = result.aud.parse::<i64>().map_err(|_| JwtError::Parse)?;
    Ok(JwtResult {
        id: user_id,
        token: token.to_string(),
    })
}

#[derive(Debug)]
pub enum AuthError {
    Empty,
    Invalid,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let (status, error_code, error_message) = match self {
            AuthError::Empty => (
                StatusCode::UNAUTHORIZED,
                1001,
                "header Authorization is needed",
            ),
            AuthError::Invalid => (StatusCode::UNAUTHORIZED, 1002, "token invalid or expired"),
        };
        let body = Json(json!({
            "code": error_code,
            "msg": error_message,
        }));
        (status, body).into_response()
    }
}

pub async fn authorization_refresh_middleware(
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AuthError> {
    let auth_header = req.headers_mut().get(http::header::AUTHORIZATION);

    let auth_header = match auth_header {
        Some(header) => header.to_str().unwrap(),
        None => return Err(AuthError::Empty),
    };
    let mut header = auth_header.split_whitespace();
    let (_, token) = (header.next(), header.next());
    let token = token.unwrap();
    let token_data = match decode_refresh_token(token) {
        Ok(data) => data,
        Err(_) => {
            return Err(AuthError::Invalid);
        }
    };
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

pub async fn authorization_access_middleware(
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AuthError> {
    let auth_header = req.headers_mut().get(http::header::AUTHORIZATION);

    let auth_header = match auth_header {
        Some(header) => header.to_str().unwrap(),
        None => return Err(AuthError::Empty),
    };
    let mut header = auth_header.split_whitespace();
    let (_, token) = (header.next(), header.next());
    let token = token.unwrap();
    let token_data = match decode_access_token(token) {
        Ok(data) => data,
        Err(_) => {
            return Err(AuthError::Invalid);
        }
    };
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}
