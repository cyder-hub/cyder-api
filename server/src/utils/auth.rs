use std::time::{SystemTime, UNIX_EPOCH};

use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
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
use crate::service::admin::auth::AccessCredentialError;
use crate::service::app_state::AppState;
use std::sync::Arc;

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

#[derive(Clone, Serialize, Deserialize)]
pub struct ManagerRefreshClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    iid: i64,
    jti: String,
    credential_epoch: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ManagerAccessClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    iid: i64,
    jti: String,
    credential_epoch: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RefreshJwtResult {
    pub manager_id: i64,
    pub login_instance_id: i64,
    pub jwt_id: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub credential_epoch: Uuid,
    pub token: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerAuthContext {
    pub manager_id: i64,
    pub manager_subject: String,
    pub login_instance_id: i64,
    pub access_jti: String,
    pub issued_at: i64,
    pub expires_at: i64,
    pub credential_epoch: Uuid,
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
        credential_epoch: &Uuid,
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
            credential_epoch: credential_epoch.to_string(),
        }
    }
}

pub fn issue_refresh_token(
    manager_id: i64,
    login_instance_id: i64,
    refresh_jti: &str,
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerRefreshClaims::new(
        manager_id,
        login_instance_id,
        refresh_jti,
        credential_epoch,
        issued_at,
        expires_at,
    );
    issue_jwt(&KEYS.encoding, &claims)
}

pub fn decode_refresh_token(token: &str) -> Result<RefreshJwtResult, JwtError> {
    let validate = JwtValidation {
        validate_aud: false,
        issuer: ISSUER,
        required_spec: &[
            "aud",
            "jti",
            "sub",
            "iat",
            "exp",
            "iss",
            "iid",
            "credential_epoch",
        ],
    };
    let result = decode_jwt::<ManagerRefreshClaims>(&KEYS.decoding, token, validate)?;
    if result.sub != REFRESH_TOKEN_SUBJECT {
        return Err(JwtError::Invalid);
    }
    let manager_id = result.aud.parse::<i64>().map_err(|_| JwtError::Parse)?;
    if manager_id != MANAGER_ID {
        return Err(JwtError::Invalid);
    }
    let credential_epoch =
        Uuid::parse_str(&result.credential_epoch).map_err(|_| JwtError::Parse)?;
    Ok(RefreshJwtResult {
        manager_id,
        login_instance_id: result.iid,
        token: token.to_string(),
        jwt_id: result.jti,
        issued_at: result.iat as i64,
        expires_at: result.exp as i64,
        credential_epoch,
    })
}

impl ManagerAccessClaims {
    fn new(
        manager_id: i64,
        login_instance_id: i64,
        access_jti: &str,
        credential_epoch: &Uuid,
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
            credential_epoch: credential_epoch.to_string(),
        }
    }
}

pub fn issue_access_token(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    credential_epoch: &Uuid,
    issued_at: i64,
) -> String {
    issue_access_token_with_expiration(
        manager_id,
        login_instance_id,
        access_jti,
        credential_epoch,
        issued_at,
        issued_at + ACCESS_TOKEN_ISSUE_SEC,
    )
}

fn issue_access_token_with_expiration(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerAccessClaims::new(
        manager_id,
        login_instance_id,
        access_jti,
        credential_epoch,
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
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    issue_access_token_with_expiration(
        manager_id,
        login_instance_id,
        access_jti,
        credential_epoch,
        issued_at,
        expires_at,
    )
}

pub fn decode_access_token(token: &str) -> Result<ManagerAuthContext, JwtError> {
    let validate = JwtValidation {
        validate_aud: false,
        issuer: ISSUER,
        required_spec: &[
            "aud",
            "jti",
            "sub",
            "iat",
            "exp",
            "iss",
            "iid",
            "credential_epoch",
        ],
    };
    let result = decode_jwt::<ManagerAccessClaims>(&KEYS.decoding, token, validate)?;
    if result.sub != MANAGER_SUBJECT {
        return Err(JwtError::Invalid);
    }
    let manager_id = result.aud.parse::<i64>().map_err(|_| JwtError::Parse)?;
    if manager_id != MANAGER_ID {
        return Err(JwtError::Invalid);
    }
    let credential_epoch =
        Uuid::parse_str(&result.credential_epoch).map_err(|_| JwtError::Parse)?;
    Ok(ManagerAuthContext {
        manager_id,
        manager_subject: result.sub,
        login_instance_id: result.iid,
        token: token.to_string(),
        access_jti: result.jti,
        issued_at: result.iat as i64,
        expires_at: result.exp as i64,
        credential_epoch,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BearerTokenError {
    Missing,
    Invalid,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessGuardError {
    MissingAuthorization,
    InvalidToken,
    CredentialMismatch,
    CredentialUnavailable,
}

impl IntoResponse for AccessGuardError {
    fn into_response(self) -> Response {
        let (status, error_code, error_message) = match self {
            AccessGuardError::MissingAuthorization => (
                StatusCode::UNAUTHORIZED,
                1431,
                "header Authorization is needed",
            ),
            AccessGuardError::InvalidToken => {
                (StatusCode::UNAUTHORIZED, 1432, "token invalid or expired")
            }
            AccessGuardError::CredentialMismatch => (
                StatusCode::UNAUTHORIZED,
                1433,
                "manager credential no longer valid",
            ),
            AccessGuardError::CredentialUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1434,
                "manager credential unavailable",
            ),
        };
        let body = Json(json!({
            "code": error_code,
            "msg": error_message,
        }));
        (status, body).into_response()
    }
}

pub fn extract_bearer_token_from_headers(headers: &HeaderMap) -> Result<&str, BearerTokenError> {
    let auth_header = headers
        .get(header::AUTHORIZATION)
        .ok_or(BearerTokenError::Missing)?
        .to_str()
        .map_err(|_| BearerTokenError::Invalid)?;

    let mut parts = auth_header.split_whitespace();
    let scheme = parts.next().ok_or(BearerTokenError::Invalid)?;
    let token = parts.next().ok_or(BearerTokenError::Invalid)?;

    if !scheme.eq_ignore_ascii_case("Bearer") || token.is_empty() || parts.next().is_some() {
        return Err(BearerTokenError::Invalid);
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
) -> Result<Response<Body>, AccessGuardError> {
    let token = extract_bearer_token_from_headers(req.headers())
        .map_err(access_guard_from_bearer_error)?
        .to_string();
    let token_data = decode_refresh_token(&token).map_err(|_| AccessGuardError::InvalidToken)?;
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

pub async fn authorization_access_token_middleware(
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AccessGuardError> {
    let token = match extract_bearer_token_from_headers(req.headers()) {
        Ok(token) => token.to_string(),
        Err(err) => {
            log_access_rejected("header");
            return Err(access_guard_from_bearer_error(err));
        }
    };
    let token_data = match decode_access_token(&token) {
        Ok(data) => data,
        Err(_) => {
            log_access_rejected("token");
            return Err(AccessGuardError::InvalidToken);
        }
    };
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

pub async fn authorization_access_middleware(
    State(app_state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AccessGuardError> {
    let token = match extract_bearer_token_from_headers(req.headers()) {
        Ok(token) => token.to_string(),
        Err(err) => {
            log_access_rejected("header");
            return Err(access_guard_from_bearer_error(err));
        }
    };
    let token_data = match decode_access_token(&token) {
        Ok(data) => data,
        Err(_) => {
            log_access_rejected("token");
            return Err(AccessGuardError::InvalidToken);
        }
    };
    app_state
        .admin
        .auth
        .validate_access_context(&token_data)
        .map_err(|error| match error {
            AccessCredentialError::EpochMismatchOrUninitialized => {
                log_access_rejected("credential_epoch");
                AccessGuardError::CredentialMismatch
            }
            AccessCredentialError::Unavailable => {
                log_access_rejected("credential_unavailable");
                AccessGuardError::CredentialUnavailable
            }
        })?;
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

fn access_guard_from_bearer_error(error: BearerTokenError) -> AccessGuardError {
    match error {
        BearerTokenError::Missing => AccessGuardError::MissingAuthorization,
        BearerTokenError::Invalid => AccessGuardError::InvalidToken,
    }
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, header};
    use cyder_tools::auth::issue_jwt;
    use serde::Serialize;
    use uuid::Uuid;

    use super::{
        ACCESS_TOKEN_ISSUE_SEC, BearerTokenError, ISSUER, KEYS, MANAGER_ID, MANAGER_SUBJECT,
        REFRESH_TOKEN_ISSUE_SEC, decode_access_token, decode_refresh_token,
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
        let credential_epoch = Uuid::new_v4();
        let refresh_token = issue_refresh_token(
            MANAGER_ID,
            42,
            "refresh-jti",
            &credential_epoch,
            now,
            now + REFRESH_TOKEN_ISSUE_SEC,
        );
        let access_token = issue_access_token(MANAGER_ID, 42, "access-jti", &credential_epoch, now);

        let refresh = decode_refresh_token(&refresh_token).expect("refresh should decode");
        assert_eq!(refresh.manager_id, MANAGER_ID);
        assert_eq!(refresh.login_instance_id, 42);
        assert_eq!(refresh.jwt_id, "refresh-jti");
        assert_eq!(refresh.issued_at, now);
        assert_eq!(refresh.expires_at, now + REFRESH_TOKEN_ISSUE_SEC);
        assert_eq!(refresh.credential_epoch, credential_epoch);

        let access = decode_access_token(&access_token).expect("access should decode");
        assert_eq!(access.manager_id, MANAGER_ID);
        assert_eq!(access.manager_subject, MANAGER_SUBJECT);
        assert_eq!(access.login_instance_id, 42);
        assert_eq!(access.access_jti, "access-jti");
        assert_eq!(access.issued_at, now);
        assert_eq!(access.expires_at, now + ACCESS_TOKEN_ISSUE_SEC);
        assert_eq!(access.credential_epoch, credential_epoch);
    }

    #[test]
    fn pre_epoch_access_and_refresh_tokens_are_rejected() {
        #[derive(Debug, Serialize)]
        struct PreEpochClaims {
            aud: String,
            exp: u64,
            iat: u64,
            iss: String,
            sub: String,
            iid: i64,
            jti: String,
        }

        let now = get_current_timestamp();
        let access_token = issue_jwt(
            &KEYS.encoding,
            &PreEpochClaims {
                aud: MANAGER_ID.to_string(),
                exp: (now + ACCESS_TOKEN_ISSUE_SEC) as u64,
                iat: now as u64,
                iss: ISSUER.to_string(),
                sub: MANAGER_SUBJECT.to_string(),
                iid: 42,
                jti: "pre-epoch-access".to_string(),
            },
        );
        let refresh_token = issue_jwt(
            &KEYS.encoding,
            &PreEpochClaims {
                aud: MANAGER_ID.to_string(),
                exp: (now + REFRESH_TOKEN_ISSUE_SEC) as u64,
                iat: now as u64,
                iss: ISSUER.to_string(),
                sub: super::REFRESH_TOKEN_SUBJECT.to_string(),
                iid: 42,
                jti: "pre-epoch-refresh".to_string(),
            },
        );

        assert!(decode_access_token(&access_token).is_err());
        assert!(decode_refresh_token(&refresh_token).is_err());
    }

    #[test]
    fn authorization_header_requires_single_bearer_token() {
        assert_eq!(
            extract_bearer_token_from_headers(&HeaderMap::new()).unwrap_err(),
            BearerTokenError::Missing
        );
        assert_eq!(
            extract_bearer_token_from_headers(&headers("Token abc")).unwrap_err(),
            BearerTokenError::Invalid
        );
        assert_eq!(
            extract_bearer_token_from_headers(&headers("Bearer abc extra")).unwrap_err(),
            BearerTokenError::Invalid
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
}
