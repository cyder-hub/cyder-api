use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::body::Body;
use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cyder_tools::auth::{DecodingKey, EncodingKey, JwtError, JwtValidation, decode_jwt, issue_jwt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::LazyLock;
use uuid::Uuid;

use crate::config::CONFIG;
use crate::database::manager_auth_instance::{MANAGER_ID, MANAGER_SUBJECT};

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

static KEYS: LazyLock<Keys> = LazyLock::new(|| Keys::new(CONFIG.jwt_secret.as_bytes()));

const ISSUER: &str = "cyder-api";
const REFRESH_TOKEN_SUBJECT: &str = "MANAGER_REFRESH_TOKEN";
pub const REFRESH_TOKEN_ISSUE_SEC: i64 = 30 * 24 * 3600;
pub const ACCESS_TOKEN_ISSUE_SEC: i64 = 10 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerRefreshClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    iid: i64,
    jti: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerAccessClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    iid: i64,
    jti: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefreshJwtResult {
    pub manager_id: i64,
    pub login_instance_id: i64,
    pub jwt_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub token: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManagerAuthContext {
    pub manager_id: i64,
    pub manager_subject: String,
    pub login_instance_id: i64,
    pub access_jti: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub token: String,
}

pub fn get_current_timestamp() -> i64 {
    let now = SystemTime::now();
    now.duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64
}

pub fn generate_token_jti() -> String {
    Uuid::new_v4().to_string()
}

impl ManagerRefreshClaims {
    fn new(
        manager_id: i64,
        login_instance_id: i64,
        refresh_jti: &str,
        issued_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            aud: manager_id.to_string(),
            exp: expires_at as u64,
            iat: issued_at as u64,
            iss: ISSUER.to_string(),
            sub: REFRESH_TOKEN_SUBJECT.to_string(),
            iid: login_instance_id,
            jti: refresh_jti.to_string(),
        }
    }
}

pub fn issue_refresh_token(
    manager_id: i64,
    login_instance_id: i64,
    refresh_jti: &str,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerRefreshClaims::new(
        manager_id,
        login_instance_id,
        refresh_jti,
        issued_at,
        expires_at,
    );
    issue_jwt(&KEYS.encoding, &claims)
}

pub fn decode_refresh_token(token: &str) -> Result<RefreshJwtResult, JwtError> {
    let validate = JwtValidation {
        validate_aud: false,
        issuer: ISSUER,
        required_spec: &["aud", "jti", "sub", "iat", "exp", "iss", "iid"],
    };
    let result = decode_jwt::<ManagerRefreshClaims>(&KEYS.decoding, token, validate)?;
    if result.sub != REFRESH_TOKEN_SUBJECT {
        return Err(JwtError::Invalid);
    }
    let manager_id = result.aud.parse::<i64>().map_err(|_| JwtError::Parse)?;
    if manager_id != MANAGER_ID {
        return Err(JwtError::Invalid);
    }
    Ok(RefreshJwtResult {
        manager_id,
        login_instance_id: result.iid,
        token: token.to_string(),
        jwt_id: result.jti,
        issued_at: result.iat as i64,
        expires_at: result.exp as i64,
    })
}

impl ManagerAccessClaims {
    fn new(
        manager_id: i64,
        login_instance_id: i64,
        access_jti: &str,
        issued_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            aud: manager_id.to_string(),
            exp: expires_at as u64,
            iat: issued_at as u64,
            iss: ISSUER.to_string(),
            sub: MANAGER_SUBJECT.to_string(),
            iid: login_instance_id,
            jti: access_jti.to_string(),
        }
    }
}

pub fn issue_access_token(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    issued_at: i64,
) -> String {
    issue_access_token_with_expiration(
        manager_id,
        login_instance_id,
        access_jti,
        issued_at,
        issued_at + ACCESS_TOKEN_ISSUE_SEC,
    )
}

fn issue_access_token_with_expiration(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerAccessClaims::new(
        manager_id,
        login_instance_id,
        access_jti,
        issued_at,
        expires_at,
    );
    issue_jwt(&KEYS.encoding, &claims)
}

#[cfg(test)]
pub(crate) fn issue_access_token_with_expiration_for_test(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    issued_at: i64,
    expires_at: i64,
) -> String {
    issue_access_token_with_expiration(
        manager_id,
        login_instance_id,
        access_jti,
        issued_at,
        expires_at,
    )
}

pub fn decode_access_token(token: &str) -> Result<ManagerAuthContext, JwtError> {
    let validate = JwtValidation {
        validate_aud: false,
        issuer: ISSUER,
        required_spec: &["aud", "jti", "sub", "iat", "exp", "iss", "iid"],
    };
    let result = decode_jwt::<ManagerAccessClaims>(&KEYS.decoding, token, validate)?;
    if result.sub != MANAGER_SUBJECT {
        return Err(JwtError::Invalid);
    }
    let manager_id = result.aud.parse::<i64>().map_err(|_| JwtError::Parse)?;
    if manager_id != MANAGER_ID {
        return Err(JwtError::Invalid);
    }
    Ok(ManagerAuthContext {
        manager_id,
        manager_subject: result.sub,
        login_instance_id: result.iid,
        token: token.to_string(),
        access_jti: result.jti,
        issued_at: result.iat as i64,
        expires_at: result.exp as i64,
    })
}

pub fn constant_time_eq(left: &str, right: &str) -> bool {
    let left = left.as_bytes();
    let right = right.as_bytes();
    let mut diff = left.len() ^ right.len();
    let max_len = left.len().max(right.len());

    for i in 0..max_len {
        let a = left.get(i).copied().unwrap_or(0);
        let b = right.get(i).copied().unwrap_or(0);
        diff |= (a ^ b) as usize;
    }

    diff == 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

pub fn extract_bearer_token_from_headers(headers: &HeaderMap) -> Result<&str, AuthError> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .ok_or(AuthError::Empty)?
        .to_str()
        .map_err(|_| AuthError::Invalid)?;

    let mut parts = auth_header.split_whitespace();
    let scheme = parts.next().ok_or(AuthError::Invalid)?;
    let token = parts.next().ok_or(AuthError::Invalid)?;

    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() || parts.next().is_some() {
        return Err(AuthError::Invalid);
    }

    Ok(token)
}

fn log_access_rejected(reason: &str) {
    cyder_tools::log::debug!(
        "{}",
        crate::logging::event_message_with_fields(
            "manager.auth.access_rejected",
            &[("reason", Some(reason.to_string()))],
        )
    );
}

pub async fn authorization_refresh_middleware(
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AuthError> {
    let token = extract_bearer_token_from_headers(req.headers())?.to_string();
    let token_data = decode_refresh_token(&token).map_err(|_| AuthError::Invalid)?;
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

pub async fn authorization_access_middleware(
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AuthError> {
    let token = match extract_bearer_token_from_headers(req.headers()) {
        Ok(token) => token.to_string(),
        Err(err) => {
            log_access_rejected("header");
            return Err(err);
        }
    };
    let token_data = match decode_access_token(&token) {
        Ok(data) => data,
        Err(_) => {
            log_access_rejected("token");
            return Err(AuthError::Invalid);
        }
    };
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};
    use cyder_tools::auth::issue_jwt;
    use serde::Serialize;

    use super::{
        ACCESS_TOKEN_ISSUE_SEC, AuthError, ISSUER, KEYS, MANAGER_ID, MANAGER_SUBJECT,
        REFRESH_TOKEN_ISSUE_SEC, constant_time_eq, decode_access_token, decode_refresh_token,
        extract_bearer_token_from_headers, get_current_timestamp, issue_access_token,
        issue_refresh_token,
    };

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_str(value).expect("header value should build"),
        );
        headers
    }

    #[test]
    fn manager_refresh_and_access_claims_roundtrip_instance_and_jti() {
        let now = get_current_timestamp();
        let refresh_token = issue_refresh_token(
            MANAGER_ID,
            42,
            "refresh-jti",
            now,
            now + REFRESH_TOKEN_ISSUE_SEC,
        );
        let access_token = issue_access_token(MANAGER_ID, 42, "access-jti", now);

        let refresh = decode_refresh_token(&refresh_token).expect("refresh should decode");
        assert_eq!(refresh.manager_id, MANAGER_ID);
        assert_eq!(refresh.login_instance_id, 42);
        assert_eq!(refresh.jwt_id, "refresh-jti");
        assert_eq!(refresh.issued_at, now);
        assert_eq!(refresh.expires_at, now + REFRESH_TOKEN_ISSUE_SEC);

        let access = decode_access_token(&access_token).expect("access should decode");
        assert_eq!(access.manager_id, MANAGER_ID);
        assert_eq!(access.manager_subject, MANAGER_SUBJECT);
        assert_eq!(access.login_instance_id, 42);
        assert_eq!(access.access_jti, "access-jti");
        assert_eq!(access.issued_at, now);
        assert_eq!(access.expires_at, now + ACCESS_TOKEN_ISSUE_SEC);
    }

    #[test]
    fn legacy_access_token_without_instance_id_is_rejected() {
        #[derive(Debug, Serialize)]
        struct LegacyAccessClaims {
            aud: String,
            exp: u64,
            iat: u64,
            iss: String,
            sub: String,
        }

        let now = get_current_timestamp();
        let token = issue_jwt(
            &KEYS.encoding,
            &LegacyAccessClaims {
                aud: MANAGER_ID.to_string(),
                exp: (now + ACCESS_TOKEN_ISSUE_SEC) as u64,
                iat: now as u64,
                iss: ISSUER.to_string(),
                sub: MANAGER_SUBJECT.to_string(),
            },
        );

        assert!(decode_access_token(&token).is_err());
    }

    #[test]
    fn authorization_header_requires_single_bearer_token() {
        assert_eq!(
            extract_bearer_token_from_headers(&HeaderMap::new()).unwrap_err(),
            AuthError::Empty
        );
        assert_eq!(
            extract_bearer_token_from_headers(&headers("Token abc")).unwrap_err(),
            AuthError::Invalid
        );
        assert_eq!(
            extract_bearer_token_from_headers(&headers("Bearer abc extra")).unwrap_err(),
            AuthError::Invalid
        );
        assert_eq!(
            extract_bearer_token_from_headers(&headers("Bearer abc")).unwrap(),
            "abc"
        );
        assert_eq!(
            extract_bearer_token_from_headers(&headers("bearer abc")).unwrap(),
            "abc"
        );
    }

    #[test]
    fn constant_time_string_compare_preserves_equality_semantics() {
        assert!(constant_time_eq("secret", "secret"));
        assert!(!constant_time_eq("secret", "secreu"));
        assert!(!constant_time_eq("secret", "secret-extra"));
        assert!(!constant_time_eq("secret-extra", "secret"));
    }
}
