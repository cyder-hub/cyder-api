use std::{
    sync::{Arc, LazyLock},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::Json;
use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use cyder_tools::auth::JwtError;
use hmac::{Hmac, Mac};
use jsonwebtoken::{
    Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, decode_header, encode,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::CONFIG;
use crate::database::manager_auth_instance::{MANAGER_ID, MANAGER_SUBJECT};
use crate::service::admin::auth::AccessCredentialError;
use crate::service::app_state::AppState;

type HmacSha256 = Hmac<Sha256>;

const ISSUER: &str = "cyder-api";
const ACCESS_AUDIENCE: &str = "cyder-manager-api";
const REFRESH_AUDIENCE: &str = "cyder-manager-refresh";
const MEDIATOR_AUDIENCE: &str = "cyder-manager-mediator";
const ACCESS_TOKEN_USE: &str = "access";
const REFRESH_TOKEN_USE: &str = "refresh";
const MEDIATOR_TOKEN_USE: &str = "mediator_session";
const ACCESS_KEY_LABEL: &[u8] = b"cyder-manager-access-v1";
const REFRESH_KEY_LABEL: &[u8] = b"cyder-manager-refresh-v1";
const MEDIATOR_KEY_LABEL: &[u8] = b"cyder-manager-mediator-v1";
pub const REFRESH_TOKEN_ISSUE_SEC: i64 = 30 * 24 * 3600;
pub const ACCESS_TOKEN_ISSUE_SEC: i64 = 10 * 60;

struct TokenKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl TokenKeys {
    fn from_derived_secret(secret: &[u8]) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret),
            decoding: DecodingKey::from_secret(secret),
        }
    }
}

struct ManagerJwtKeys {
    key_id: String,
    access: TokenKeys,
    refresh: TokenKeys,
    mediator: TokenKeys,
}

impl ManagerJwtKeys {
    fn from_root_secret(secret: &[u8]) -> Self {
        Self {
            key_id: root_key_id(secret),
            access: TokenKeys::from_derived_secret(&derive_domain_key(secret, ACCESS_KEY_LABEL)),
            refresh: TokenKeys::from_derived_secret(&derive_domain_key(secret, REFRESH_KEY_LABEL)),
            mediator: TokenKeys::from_derived_secret(&derive_domain_key(
                secret,
                MEDIATOR_KEY_LABEL,
            )),
        }
    }
}

static KEYS: LazyLock<ManagerJwtKeys> =
    LazyLock::new(|| ManagerJwtKeys::from_root_secret(CONFIG.jwt_secret.as_bytes()));

fn derive_domain_key(root_secret: &[u8], label: &[u8]) -> [u8; 32] {
    let mut mac =
        HmacSha256::new_from_slice(root_secret).expect("HMAC accepts manager JWT root keys");
    mac.update(label);
    mac.finalize().into_bytes().into()
}

fn root_key_id(root_secret: &[u8]) -> String {
    let digest = Sha256::digest(root_secret);
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        write!(&mut encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

pub fn manager_jwt_key_id() -> &'static str {
    &KEYS.key_id
}

fn issue_manager_jwt<T: Serialize>(keys: &TokenKeys, claims: &T) -> String {
    let mut header = Header::new(Algorithm::HS256);
    header.kid = Some(KEYS.key_id.clone());
    encode(&header, claims, &keys.encoding).expect("failed to issue a manager jwt")
}

fn decode_manager_jwt<T: for<'de> Deserialize<'de>>(
    keys: &TokenKeys,
    token: &str,
    audience: &str,
) -> Result<T, JwtError> {
    let header = decode_header(token).map_err(|_| JwtError::Decode)?;
    if header.alg != Algorithm::HS256 || header.kid.as_deref() != Some(KEYS.key_id.as_str()) {
        return Err(JwtError::Invalid);
    }

    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = 0;
    validation.set_audience(&[audience]);
    validation.set_issuer(&[ISSUER]);
    validation.sub = Some(MANAGER_SUBJECT.to_string());
    validation.set_required_spec_claims(&["aud", "exp", "iat", "iss", "sub"]);
    decode::<T>(token, &keys.decoding, &validation)
        .map(|data| data.claims)
        .map_err(|_| JwtError::Decode)
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ManagerRefreshClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    token_use: String,
    sid: i64,
    jti: String,
    refresh_generation: i64,
    credential_epoch: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ManagerAccessClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    token_use: String,
    sid: i64,
    jti: String,
    session_version: i64,
    credential_epoch: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ManagerMediatorClaims {
    aud: String,
    exp: u64,
    iat: u64,
    iss: String,
    sub: String,
    token_use: String,
    sid: i64,
    credential_epoch: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct RefreshJwtResult {
    pub manager_id: i64,
    pub login_instance_id: i64,
    pub jwt_id: String,
    pub refresh_generation: i64,
    pub issued_at: i64,
    pub expires_at: i64,
    pub credential_epoch: Uuid,
    pub token: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerMediatorContext {
    pub manager_id: i64,
    pub manager_subject: String,
    pub login_instance_id: i64,
    pub issued_at: i64,
    pub expires_at: i64,
    pub credential_epoch: Uuid,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerAuthContext {
    pub manager_id: i64,
    pub manager_subject: String,
    pub login_instance_id: i64,
    pub access_jti: String,
    pub session_version: i64,
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
        login_instance_id: i64,
        refresh_jti: &str,
        refresh_generation: i64,
        credential_epoch: &Uuid,
        issued_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            aud: REFRESH_AUDIENCE.to_string(),
            exp: expires_at as u64,
            iat: issued_at as u64,
            iss: ISSUER.to_string(),
            sub: MANAGER_SUBJECT.to_string(),
            token_use: REFRESH_TOKEN_USE.to_string(),
            sid: login_instance_id,
            jti: refresh_jti.to_string(),
            refresh_generation,
            credential_epoch: credential_epoch.to_string(),
        }
    }
}

pub fn issue_refresh_token(
    _manager_id: i64,
    login_instance_id: i64,
    refresh_jti: &str,
    refresh_generation: i64,
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerRefreshClaims::new(
        login_instance_id,
        refresh_jti,
        refresh_generation,
        credential_epoch,
        issued_at,
        expires_at,
    );
    issue_manager_jwt(&KEYS.refresh, &claims)
}

pub fn decode_refresh_token(token: &str) -> Result<RefreshJwtResult, JwtError> {
    let result =
        decode_manager_jwt::<ManagerRefreshClaims>(&KEYS.refresh, token, REFRESH_AUDIENCE)?;
    if result.token_use != REFRESH_TOKEN_USE || result.refresh_generation < 1 {
        return Err(JwtError::Invalid);
    }
    let credential_epoch =
        Uuid::parse_str(&result.credential_epoch).map_err(|_| JwtError::Parse)?;
    Ok(RefreshJwtResult {
        manager_id: MANAGER_ID,
        login_instance_id: result.sid,
        token: token.to_string(),
        jwt_id: result.jti,
        refresh_generation: result.refresh_generation,
        issued_at: result.iat as i64,
        expires_at: result.exp as i64,
        credential_epoch,
    })
}

impl ManagerAccessClaims {
    fn new(
        login_instance_id: i64,
        access_jti: &str,
        session_version: i64,
        credential_epoch: &Uuid,
        issued_at: i64,
        expires_at: i64,
    ) -> Self {
        Self {
            aud: ACCESS_AUDIENCE.to_string(),
            exp: expires_at as u64,
            iat: issued_at as u64,
            iss: ISSUER.to_string(),
            sub: MANAGER_SUBJECT.to_string(),
            token_use: ACCESS_TOKEN_USE.to_string(),
            sid: login_instance_id,
            jti: access_jti.to_string(),
            session_version,
            credential_epoch: credential_epoch.to_string(),
        }
    }
}

pub fn issue_access_token(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    session_version: i64,
    credential_epoch: &Uuid,
    issued_at: i64,
) -> String {
    issue_access_token_with_expiration(
        manager_id,
        login_instance_id,
        access_jti,
        session_version,
        credential_epoch,
        issued_at,
        issued_at + ACCESS_TOKEN_ISSUE_SEC,
    )
}

fn issue_access_token_with_expiration(
    _manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    session_version: i64,
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerAccessClaims::new(
        login_instance_id,
        access_jti,
        session_version,
        credential_epoch,
        issued_at,
        expires_at,
    );
    issue_manager_jwt(&KEYS.access, &claims)
}

#[cfg(test)]
pub(crate) fn issue_access_token_with_expiration_for_test(
    manager_id: i64,
    login_instance_id: i64,
    access_jti: &str,
    session_version: i64,
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    issue_access_token_with_expiration(
        manager_id,
        login_instance_id,
        access_jti,
        session_version,
        credential_epoch,
        issued_at,
        expires_at,
    )
}

pub fn decode_access_token(token: &str) -> Result<ManagerAuthContext, JwtError> {
    let result = decode_manager_jwt::<ManagerAccessClaims>(&KEYS.access, token, ACCESS_AUDIENCE)?;
    if result.token_use != ACCESS_TOKEN_USE || result.session_version < 1 {
        return Err(JwtError::Invalid);
    }
    let credential_epoch =
        Uuid::parse_str(&result.credential_epoch).map_err(|_| JwtError::Parse)?;
    Ok(ManagerAuthContext {
        manager_id: MANAGER_ID,
        manager_subject: result.sub,
        login_instance_id: result.sid,
        token: token.to_string(),
        access_jti: result.jti,
        session_version: result.session_version,
        issued_at: result.iat as i64,
        expires_at: result.exp as i64,
        credential_epoch,
    })
}

pub fn issue_mediator_token(
    login_instance_id: i64,
    credential_epoch: &Uuid,
    issued_at: i64,
    expires_at: i64,
) -> String {
    let claims = ManagerMediatorClaims {
        aud: MEDIATOR_AUDIENCE.to_string(),
        exp: expires_at as u64,
        iat: issued_at as u64,
        iss: ISSUER.to_string(),
        sub: MANAGER_SUBJECT.to_string(),
        token_use: MEDIATOR_TOKEN_USE.to_string(),
        sid: login_instance_id,
        credential_epoch: credential_epoch.to_string(),
    };
    issue_manager_jwt(&KEYS.mediator, &claims)
}

pub fn decode_mediator_token(token: &str) -> Result<ManagerMediatorContext, JwtError> {
    let result =
        decode_manager_jwt::<ManagerMediatorClaims>(&KEYS.mediator, token, MEDIATOR_AUDIENCE)?;
    if result.token_use != MEDIATOR_TOKEN_USE {
        return Err(JwtError::Invalid);
    }
    let credential_epoch =
        Uuid::parse_str(&result.credential_epoch).map_err(|_| JwtError::Parse)?;
    Ok(ManagerMediatorContext {
        manager_id: MANAGER_ID,
        manager_subject: result.sub,
        login_instance_id: result.sid,
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
    SessionInvalid,
    SessionUnavailable,
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
            AccessGuardError::SessionInvalid => (
                StatusCode::UNAUTHORIZED,
                1435,
                "manager session no longer valid",
            ),
            AccessGuardError::SessionUnavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                1436,
                "manager session state unavailable",
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

pub async fn authorization_access_middleware(
    State(app_state): State<Arc<AppState>>,
    mut req: Request,
    next: Next,
) -> Result<Response<Body>, AccessGuardError> {
    let token_data = validate_access_headers(&app_state, req.headers())?;
    req.extensions_mut().insert(token_data);
    Ok(next.run(req).await)
}

pub fn validate_access_headers(
    app_state: &AppState,
    headers: &HeaderMap,
) -> Result<ManagerAuthContext, AccessGuardError> {
    let token = match extract_bearer_token_from_headers(headers) {
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
            AccessCredentialError::SessionInvalid => {
                log_access_rejected("session_invalid");
                AccessGuardError::SessionInvalid
            }
            AccessCredentialError::SessionUnavailable => {
                log_access_rejected("session_unavailable");
                AccessGuardError::SessionUnavailable
            }
        })?;
    Ok(token_data)
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
    use jsonwebtoken::{Algorithm, Header, encode};
    use serde::Serialize;
    use uuid::Uuid;

    use super::{
        ACCESS_AUDIENCE, ACCESS_TOKEN_ISSUE_SEC, ACCESS_TOKEN_USE, BearerTokenError, ISSUER, KEYS,
        MANAGER_ID, MANAGER_SUBJECT, ManagerAccessClaims, ManagerJwtKeys, REFRESH_TOKEN_ISSUE_SEC,
        decode_access_token, decode_mediator_token, decode_refresh_token, derive_domain_key,
        extract_bearer_token_from_headers, get_current_timestamp, issue_access_token,
        issue_manager_jwt, issue_mediator_token, issue_refresh_token, root_key_id,
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
            7,
            &credential_epoch,
            now,
            now + REFRESH_TOKEN_ISSUE_SEC,
        );
        let access_token =
            issue_access_token(MANAGER_ID, 42, "access-jti", 7, &credential_epoch, now);

        let refresh = decode_refresh_token(&refresh_token).expect("refresh should decode");
        assert_eq!(refresh.manager_id, MANAGER_ID);
        assert_eq!(refresh.login_instance_id, 42);
        assert_eq!(refresh.jwt_id, "refresh-jti");
        assert_eq!(refresh.refresh_generation, 7);
        assert_eq!(refresh.issued_at, now);
        assert_eq!(refresh.expires_at, now + REFRESH_TOKEN_ISSUE_SEC);
        assert_eq!(refresh.credential_epoch, credential_epoch);

        let access = decode_access_token(&access_token).expect("access should decode");
        assert_eq!(access.manager_id, MANAGER_ID);
        assert_eq!(access.manager_subject, MANAGER_SUBJECT);
        assert_eq!(access.login_instance_id, 42);
        assert_eq!(access.access_jti, "access-jti");
        assert_eq!(access.session_version, 7);
        assert_eq!(access.issued_at, now);
        assert_eq!(access.expires_at, now + ACCESS_TOKEN_ISSUE_SEC);
        assert_eq!(access.credential_epoch, credential_epoch);

        let mediator =
            issue_mediator_token(42, &credential_epoch, now, now + REFRESH_TOKEN_ISSUE_SEC);
        let mediator = decode_mediator_token(&mediator).expect("mediator should decode");
        assert_eq!(mediator.manager_id, MANAGER_ID);
        assert_eq!(mediator.manager_subject, MANAGER_SUBJECT);
        assert_eq!(mediator.login_instance_id, 42);
        assert_eq!(mediator.credential_epoch, credential_epoch);
    }

    #[test]
    fn manager_jwt_domains_are_deterministic_and_not_interchangeable() {
        let root = b"0123456789abcdef0123456789abcdef";
        assert_eq!(root_key_id(root), root_key_id(root));
        assert_eq!(root_key_id(root).len(), 64);
        assert_ne!(
            derive_domain_key(root, super::ACCESS_KEY_LABEL),
            derive_domain_key(root, super::REFRESH_KEY_LABEL)
        );
        assert_ne!(
            derive_domain_key(root, super::REFRESH_KEY_LABEL),
            derive_domain_key(root, super::MEDIATOR_KEY_LABEL)
        );

        let now = get_current_timestamp();
        let epoch = Uuid::new_v4();
        let access = issue_access_token(MANAGER_ID, 42, "access", 1, &epoch, now);
        let refresh = issue_refresh_token(
            MANAGER_ID,
            42,
            "refresh",
            1,
            &epoch,
            now,
            now + REFRESH_TOKEN_ISSUE_SEC,
        );
        let mediator = issue_mediator_token(42, &epoch, now, now + REFRESH_TOKEN_ISSUE_SEC);
        assert!(decode_refresh_token(&access).is_err());
        assert!(decode_mediator_token(&access).is_err());
        assert!(decode_access_token(&refresh).is_err());
        assert!(decode_mediator_token(&refresh).is_err());
        assert!(decode_access_token(&mediator).is_err());
        assert!(decode_refresh_token(&mediator).is_err());
    }

    #[test]
    fn manager_jwt_rejects_missing_epoch_wrong_kid_and_wrong_algorithm() {
        #[derive(Debug, Serialize)]
        struct PreEpochClaims {
            aud: String,
            exp: u64,
            iat: u64,
            iss: String,
            sub: String,
            token_use: String,
            sid: i64,
            jti: String,
            session_version: i64,
        }

        let now = get_current_timestamp();
        let access_token = issue_manager_jwt(
            &KEYS.access,
            &PreEpochClaims {
                aud: ACCESS_AUDIENCE.to_string(),
                exp: (now + ACCESS_TOKEN_ISSUE_SEC) as u64,
                iat: now as u64,
                iss: ISSUER.to_string(),
                sub: MANAGER_SUBJECT.to_string(),
                token_use: ACCESS_TOKEN_USE.to_string(),
                sid: 42,
                jti: "pre-epoch-access".to_string(),
                session_version: 1,
            },
        );
        assert!(decode_access_token(&access_token).is_err());

        let claims = ManagerAccessClaims::new(
            42,
            "access",
            1,
            &Uuid::new_v4(),
            now,
            now + ACCESS_TOKEN_ISSUE_SEC,
        );
        let mut wrong_kid = Header::new(Algorithm::HS256);
        wrong_kid.kid = Some("wrong".to_string());
        let wrong_kid = encode(&wrong_kid, &claims, &KEYS.access.encoding)
            .expect("wrong-kid token should encode");
        assert!(decode_access_token(&wrong_kid).is_err());

        let mut wrong_algorithm = Header::new(Algorithm::HS512);
        wrong_algorithm.kid = Some(KEYS.key_id.clone());
        let wrong_algorithm = encode(
            &wrong_algorithm,
            &claims,
            &jsonwebtoken::EncodingKey::from_secret(&derive_domain_key(
                b"0123456789abcdef0123456789abcdef",
                super::ACCESS_KEY_LABEL,
            )),
        )
        .expect("wrong-algorithm token should encode");
        assert!(decode_access_token(&wrong_algorithm).is_err());

        let alternate_keys = ManagerJwtKeys::from_root_secret(b"0123456789abcdef0123456789abcdef");
        let wrong_root = {
            let mut header = Header::new(Algorithm::HS256);
            header.kid = Some(alternate_keys.key_id.clone());
            encode(&header, &claims, &alternate_keys.access.encoding)
                .expect("wrong-root token should encode")
        };
        assert!(decode_access_token(&wrong_root).is_err());
    }

    #[test]
    fn non_positive_access_version_and_refresh_generation_are_rejected() {
        let now = get_current_timestamp();
        let credential_epoch = Uuid::new_v4();
        let zero_version_access = issue_access_token(
            MANAGER_ID,
            42,
            "zero-version-access",
            0,
            &credential_epoch,
            now,
        );
        let zero_generation_refresh = issue_refresh_token(
            MANAGER_ID,
            42,
            "zero-generation-refresh",
            0,
            &credential_epoch,
            now,
            now + REFRESH_TOKEN_ISSUE_SEC,
        );

        assert!(decode_access_token(&zero_version_access).is_err());
        assert!(decode_refresh_token(&zero_generation_refresh).is_err());
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
