use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::{Arc, Mutex as StdMutex, RwLock};

use cyder_tools::log::{debug, info, warn};
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::controller::BaseError;
use crate::database::manager_auth_instance::{ManagerAuthInstance, NewManagerAuthInstance};
use crate::database::manager_credential::{
    ExpectedManagerTotpState, MANAGER_ID, MANAGER_SUBJECT, ManagerCredential,
    ManagerCredentialRepositoryError, ManagerTotpSecret, NewManagerCredential,
    RotatedManagerCredential,
};
use crate::database::manager_totp_recovery_code::{
    ManagerTotpRecoveryCode, NewManagerTotpRecoveryCode,
};
use crate::service::secret_encryption::{SecretDomain, SecretEncryptionService, SensitiveSecret};
use crate::utils::ID_GENERATOR;
use crate::utils::auth::{
    ManagerAuthContext, REFRESH_TOKEN_ISSUE_SEC, decode_access_token, decode_refresh_token,
    generate_token_jti, get_current_timestamp, issue_access_token, issue_mediator_token,
    issue_refresh_token, manager_jwt_key_id,
};

pub(crate) mod password;
pub(crate) mod totp;

use password::{
    CredentialUnavailableReason, ManagerCredentialSnapshot, ManagerTotpState, PasswordEngine,
    PasswordOperationError, PasswordPolicyError, ReadyManagerCredential, normalize_password,
};
use totp::{
    MANAGER_TOTP_PERIOD_SEC, ManagerTotpPrimitiveError, generate_manager_totp_provisioning,
    generate_manager_totp_recovery_codes, match_manager_totp_step,
    normalize_manager_totp_recovery_code, validate_manager_totp_code,
};

const LOGIN_FAILURE_LIMIT: u32 = 5;
const LOGIN_FAILURE_WINDOW_SEC: i64 = 60;
const LOGIN_FAILURE_LOCK_SEC: i64 = 60;
const LOGIN_SOURCE_CAPACITY: usize = 4_096;
const GLOBAL_LOGIN_VERIFICATION_LIMIT: usize = 30;
const GLOBAL_LOGIN_VERIFICATION_WINDOW_SEC: i64 = 60;
pub const REFRESH_FAMILY_IDLE_SEC: i64 = 7 * 24 * 3600;
const ACCESS_CACHE_REFRESH_THRESHOLD_SEC: i64 = 60;
const LOGIN_TOTP_CHALLENGE_TTL_SEC: i64 = 5 * 60;
const SETUP_TOTP_CHALLENGE_TTL_SEC: i64 = 10 * 60;
const LOGIN_TOTP_CHALLENGE_CAPACITY: usize = 256;
const SETUP_TOTP_CHALLENGE_CAPACITY: usize = 64;
const RECOVERY_TOTP_CHALLENGE_CAPACITY: usize = 4;
const TOTP_CHALLENGE_ATTEMPT_LIMIT: u8 = 5;
const TOTP_FAILURE_LIMIT: usize = 5;
const TOTP_FAILURE_WINDOW_SEC: i64 = 60;
const TOTP_FAILURE_LOCK_SEC: i64 = 60;
const TOTP_SOURCE_CAPACITY: usize = 4_096;
const GLOBAL_TOTP_VERIFICATION_LIMIT: usize = 30;
const GLOBAL_TOTP_VERIFICATION_WINDOW_SEC: i64 = 60;
pub(crate) const SECRET_GOVERNANCE_REAUTH_TTL_SEC: i64 = 5 * 60;
const DUMMY_RECOVERY_CODE: &str = "00000000000000000000";
const DUMMY_RECOVERY_CODE_VERIFIER: &str = "$argon2id$v=19$m=65536,t=3,p=4$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

type NowFn = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone)]
pub struct AuthTokenPair {
    pub refresh_token: String,
    pub access_token: String,
    pub mediator_token: String,
    pub mediator_expires_at: i64,
    pub reauth: Option<ManagerSecretGovernanceReauth>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStatus {
    Uninitialized,
    Ready,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapStatusError {
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapError {
    AlreadyInitialized,
    PasswordPolicy(PasswordPolicyError),
    Busy,
    Unavailable,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginError {
    Uninitialized,
    InvalidPassword,
    SourceRateLimited { retry_after: u64 },
    GlobalRateLimited { retry_after: u64 },
    Busy,
    Unavailable,
    Storage,
}

pub enum LoginPasswordResult {
    Authenticated(AuthTokenPair),
    TotpRequired(ManagerLoginTotpChallenge),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginPasswordError {
    Uninitialized,
    InvalidPassword,
    SourceRateLimited { retry_after: u64 },
    GlobalRateLimited { retry_after: u64 },
    Busy,
    ManagerTotpUnavailable,
    Unavailable,
    Storage,
}

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerLoginTotpChallenge {
    pub challenge: Uuid,
    pub expires_in: u64,
}

impl std::fmt::Debug for ManagerLoginTotpChallenge {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ManagerLoginTotpChallenge(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerTotpPublicState {
    Disabled,
    Enabled { enabled_at: i64 },
    Unavailable { enabled_at: Option<i64> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerTotpVerificationError {
    Required,
    Invalid,
    StepReplayed { retry_after: u64 },
    StepStale { retry_after: u64 },
    SourceRateLimited { retry_after: u64 },
    GlobalRateLimited { retry_after: u64 },
    ChallengeInvalidOrExpired,
    ChallengeAttemptsExhausted,
    Unavailable,
    StateConflict,
    CurrentPasswordInvalid,
    RecoveryCredentialsInvalid,
    Busy,
    Storage,
    RequestInvalid,
    ReauthRequired,
    ReauthMethodChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerTotpSensitiveVerification {
    Disabled,
    Verified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerReauthEvidence {
    Password,
    Totp,
    PasswordTotp,
    RecoveryTotp,
}

impl ManagerReauthEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::Totp => "totp",
            Self::PasswordTotp => "password_totp",
            Self::RecoveryTotp => "recovery_totp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManagerSecretGovernanceReauth {
    pub evidence: ManagerReauthEvidence,
    pub verified_at: i64,
    pub verified_until: i64,
}

impl ManagerSecretGovernanceReauth {
    fn new(evidence: ManagerReauthEvidence, now: i64) -> Self {
        Self {
            evidence,
            verified_at: now,
            verified_until: now + SECRET_GOVERNANCE_REAUTH_TTL_SEC,
        }
    }
}

pub enum ManagerReauthCredential<'a> {
    Password(&'a str),
    Totp(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerTotpSetupAction {
    Enroll,
    Replace,
}

pub struct ManagerTotpSetup {
    pub challenge: Uuid,
    pub manual_secret: SensitiveSecret,
    pub otpauth_uri: SensitiveSecret,
    pub expires_in: u64,
}

impl std::fmt::Debug for ManagerTotpSetup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ManagerTotpSetup(<redacted>)")
    }
}

pub struct ManagerTotpRecoveryCodes(Vec<SensitiveSecret>);

impl ManagerTotpRecoveryCodes {
    pub fn expose(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(SensitiveSecret::expose)
    }
}

impl std::fmt::Debug for ManagerTotpRecoveryCodes {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ManagerTotpRecoveryCodes(<redacted>)")
    }
}

pub struct ManagerTotpLifecycleResult {
    pub tokens: AuthTokenPair,
    pub state: ManagerTotpPublicState,
    pub recovery_codes: Option<ManagerTotpRecoveryCodes>,
    pub revoked_sessions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotatePasswordError {
    InvalidCurrentPassword,
    PasswordPolicy(PasswordPolicyError),
    SamePassword,
    Totp(ManagerTotpVerificationError),
    Busy,
    EpochConflict,
    Unavailable,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshError {
    Invalid,
    EpochMismatch,
    Replay,
    Unavailable,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessTokenError {
    Invalid,
    Replay,
    Unavailable,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoutError {
    InvalidCredential,
    Unavailable,
    Storage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessCredentialError {
    EpochMismatchOrUninitialized,
    Unavailable,
    SessionInvalid,
    SessionUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SessionValidationError {
    Invalid,
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedAccessToken {
    token: String,
    access_jti: String,
    expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActiveManagerSession {
    manager_id: i64,
    manager_subject: String,
    session_version: i64,
    expires_at: i64,
    credential_epoch: Uuid,
    current_refresh_jti: String,
    refresh_generation: i64,
    current_refresh_token: String,
    access: Option<CachedAccessToken>,
    secret_governance_reauth: Option<ManagerSecretGovernanceReauth>,
}

impl ActiveManagerSession {
    fn from_persisted(instance: &ManagerAuthInstance) -> Option<Self> {
        let credential_epoch = Uuid::parse_str(&instance.credential_epoch).ok()?;
        let current_refresh_token = issue_refresh_token(
            MANAGER_ID,
            instance.id,
            &instance.current_refresh_jti,
            instance.refresh_generation,
            &credential_epoch,
            instance.last_rotated_at,
            instance.absolute_expires_at,
        );
        Some(Self {
            manager_id: instance.manager_id,
            manager_subject: instance.manager_subject.clone(),
            session_version: instance.session_version,
            expires_at: instance.idle_expires_at.min(instance.absolute_expires_at),
            credential_epoch,
            current_refresh_jti: instance.current_refresh_jti.clone(),
            refresh_generation: instance.refresh_generation,
            current_refresh_token,
            access: None,
            secret_governance_reauth: None,
        })
    }

    fn from_token_pair(instance: &ManagerAuthInstance, pair: &AuthTokenPair) -> Option<Self> {
        let mut session = Self::from_persisted(instance)?;
        let access = decode_access_token(&pair.access_token).ok()?;
        session.current_refresh_token = pair.refresh_token.clone();
        session.access = Some(CachedAccessToken {
            token: pair.access_token.clone(),
            access_jti: access.access_jti,
            expires_at: access.expires_at,
        });
        session.secret_governance_reauth = pair.reauth;
        Some(session)
    }
}

#[derive(Debug)]
enum SessionRegistryState {
    Ready(HashMap<i64, ActiveManagerSession>),
    Unavailable,
}

#[derive(Debug, Default)]
struct SourceLoginFailureState {
    failures: VecDeque<i64>,
    locked_until: Option<i64>,
}

#[derive(Debug, Default)]
struct LoginProtectionState {
    sources: HashMap<IpAddr, SourceLoginFailureState>,
    global_verifications: VecDeque<i64>,
}

#[derive(Debug, Default)]
struct TotpProtectionState {
    sources: HashMap<IpAddr, SourceLoginFailureState>,
    global_verifications: VecDeque<i64>,
}

struct LoginTotpChallenge {
    credential_epoch: Uuid,
    source: IpAddr,
    expires_at: i64,
    attempts: u8,
}

struct SetupTotpChallenge {
    action: ManagerTotpSetupAction,
    login_instance_id: i64,
    credential_epoch: Uuid,
    source: IpAddr,
    expires_at: i64,
    attempts: u8,
    manual_secret: SensitiveSecret,
}

struct RecoveryTotpChallenge {
    credential_epoch: Uuid,
    source: IpAddr,
    expires_at: i64,
    attempts: u8,
    manual_secret: SensitiveSecret,
}

#[derive(Default)]
struct TotpChallengeState {
    login: HashMap<Uuid, LoginTotpChallenge>,
    setup: HashMap<Uuid, SetupTotpChallenge>,
    recovery: HashMap<Uuid, RecoveryTotpChallenge>,
}

pub struct ManagerAuthService {
    login_protection: StdMutex<LoginProtectionState>,
    totp_protection: StdMutex<TotpProtectionState>,
    totp_challenges: StdMutex<TotpChallengeState>,
    credential_snapshot: RwLock<ManagerCredentialSnapshot>,
    session_registry: RwLock<SessionRegistryState>,
    access_flights: StdMutex<HashMap<i64, Arc<AsyncMutex<()>>>>,
    credential_lifecycle: AsyncMutex<()>,
    password_engine: PasswordEngine,
    secret_encryption: Arc<SecretEncryptionService>,
    now: NowFn,
}

impl ManagerAuthService {
    pub(crate) fn new(secret_encryption: Arc<SecretEncryptionService>) -> Self {
        Self::new_with_clock(Arc::new(get_current_timestamp), secret_encryption)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(now: NowFn) -> Self {
        Self::new_with_clock(
            now,
            Arc::new(SecretEncryptionService::from_config(
                &crate::config::SecretEncryptionConfig::default(),
            )),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_test_with_secret_encryption(
        now: NowFn,
        secret_encryption: Arc<SecretEncryptionService>,
    ) -> Self {
        Self::new_with_clock(now, secret_encryption)
    }

    fn new_with_clock(now: NowFn, secret_encryption: Arc<SecretEncryptionService>) -> Self {
        let current_time = now();
        let credential_snapshot = ManagerCredentialSnapshot::load(&secret_encryption);
        if let ManagerCredentialSnapshot::Unavailable(reason) = &credential_snapshot {
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.credential_snapshot_unavailable",
                    &[("reason", Some(snapshot_reason_code(*reason).to_string()))],
                )
            );
        }
        if let ManagerCredentialSnapshot::Ready(ready) = &credential_snapshot
            && let ManagerTotpState::Unavailable {
                reason, enabled_at, ..
            } = ready.totp()
        {
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.totp_snapshot_unavailable",
                    &[
                        (
                            "reason",
                            Some(manager_totp_unavailable_reason_code(*reason).to_string()),
                        ),
                        ("enabled_at", enabled_at.map(|value| value.to_string())),
                    ],
                )
            );
        }

        if ManagerAuthInstance::revoke_signing_key_mismatches(manager_jwt_key_id(), current_time)
            .is_err()
        {
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.signing_key_session_revocation_failed",
                    &[("reason", Some("storage".to_string()))],
                )
            );
        }
        if ManagerAuthInstance::cleanup_expired_instances(current_time).is_err() {
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.session_cleanup_failed",
                    &[("reason", Some("storage".to_string()))],
                )
            );
        }
        let session_registry = match ManagerAuthInstance::list_active_instances(current_time) {
            Ok(instances) => Self::registry_from_instances(instances),
            Err(_) => {
                warn!(
                    "{}",
                    crate::logging::event_message_with_fields(
                        "manager.auth.session_registry_unavailable",
                        &[("reason", Some("storage".to_string()))],
                    )
                );
                SessionRegistryState::Unavailable
            }
        };

        Self {
            login_protection: StdMutex::new(LoginProtectionState::default()),
            totp_protection: StdMutex::new(TotpProtectionState::default()),
            totp_challenges: StdMutex::new(TotpChallengeState::default()),
            credential_snapshot: RwLock::new(credential_snapshot),
            session_registry: RwLock::new(session_registry),
            access_flights: StdMutex::new(HashMap::new()),
            credential_lifecycle: AsyncMutex::new(()),
            password_engine: PasswordEngine::new(),
            secret_encryption,
            now,
        }
    }

    pub fn bootstrap_status(&self) -> Result<BootstrapStatus, BootstrapStatusError> {
        match self.snapshot() {
            ManagerCredentialSnapshot::Uninitialized => Ok(BootstrapStatus::Uninitialized),
            ManagerCredentialSnapshot::Ready(_) => Ok(BootstrapStatus::Ready),
            ManagerCredentialSnapshot::Unavailable(_) => Err(BootstrapStatusError::Unavailable),
        }
    }

    pub async fn bootstrap(&self, password: &str) -> Result<AuthTokenPair, BootstrapError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| BootstrapError::Busy)?;
        match self.snapshot() {
            ManagerCredentialSnapshot::Uninitialized => {}
            ManagerCredentialSnapshot::Ready(_) => {
                self.log_bootstrap_rejected("already_initialized");
                return Err(BootstrapError::AlreadyInitialized);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                self.log_bootstrap_rejected("snapshot_unavailable");
                return Err(BootstrapError::Unavailable);
            }
        }
        self.ensure_session_registry_ready()
            .map_err(|_| BootstrapError::Unavailable)?;

        let password = normalize_password(password).map_err(BootstrapError::PasswordPolicy)?;
        let verifier = self
            .password_engine
            .hash(password)
            .await
            .map_err(map_bootstrap_password_error)?;
        let now = self.now();
        let epoch = Uuid::new_v4();
        let refresh_jti = generate_token_jti();
        let absolute_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let idle_expires_at = (now + REFRESH_FAMILY_IDLE_SEC).min(absolute_expires_at);
        let mutation = ManagerCredential::bootstrap_with_session(
            NewManagerCredential {
                password_verifier: verifier.to_string(),
                credential_epoch: epoch.to_string(),
                now,
            },
            new_session(
                &refresh_jti,
                &epoch,
                now,
                idle_expires_at,
                absolute_expires_at,
            ),
            "credential_bootstrap",
        )
        .map_err(map_bootstrap_repository_error)?;
        let ready = self.install_ready_snapshot(mutation.credential)?;
        let mut pair = issue_token_pair(
            &mutation.session,
            &refresh_jti,
            ready.credential_epoch(),
            now,
            absolute_expires_at,
        );
        pair.reauth = Some(ManagerSecretGovernanceReauth::new(
            ManagerReauthEvidence::Password,
            now,
        ));
        self.replace_sessions_with(&mutation.session, &pair);

        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.bootstrap_succeeded",
                &[
                    ("login_instance_id", Some(mutation.session.id.to_string())),
                    (
                        "revoked_sessions",
                        Some(mutation.revoked_sessions.to_string()),
                    ),
                ],
            )
        );
        Ok(pair)
    }

    pub async fn login(
        &self,
        source: IpAddr,
        submitted_password: &str,
    ) -> Result<AuthTokenPair, LoginError> {
        match self.login_password(source, submitted_password).await {
            Ok(LoginPasswordResult::Authenticated(tokens)) => Ok(tokens),
            Ok(LoginPasswordResult::TotpRequired(_)) => Err(LoginError::Unavailable),
            Err(error) => Err(map_login_password_error(error)),
        }
    }

    pub async fn login_password(
        &self,
        source: IpAddr,
        submitted_password: &str,
    ) -> Result<LoginPasswordResult, LoginPasswordError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| LoginPasswordError::Busy)?;
        let now = self.now();
        self.ensure_session_registry_ready()
            .map_err(|_| LoginPasswordError::Unavailable)?;
        if let Some(retry_after) = self.source_retry_after(source, now) {
            self.log_login_rejected("source_rate_limited");
            return Err(LoginPasswordError::SourceRateLimited { retry_after });
        }

        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(LoginPasswordError::Uninitialized);
            }
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(LoginPasswordError::Unavailable);
            }
        };
        let password = match normalize_password(submitted_password) {
            Ok(password) => password,
            Err(_) => {
                self.log_login_rejected("invalid_password");
                return Err(LoginPasswordError::InvalidPassword);
            }
        };
        self.reserve_global_verification(now)
            .map_err(map_login_error_to_password_error)?;
        match self
            .password_engine
            .verify(password, ready.password_verifier())
            .await
        {
            Ok(()) => {}
            Err(PasswordOperationError::IncorrectPassword) => {
                self.record_login_failure(source, now);
                self.log_login_rejected("invalid_password");
                return Err(LoginPasswordError::InvalidPassword);
            }
            Err(PasswordOperationError::Busy) => {
                self.release_global_verification(now);
                return Err(LoginPasswordError::Busy);
            }
            Err(PasswordOperationError::InvalidVerifier | PasswordOperationError::Runtime) => {
                self.mark_unavailable(CredentialUnavailableReason::InvalidVerifier);
                return Err(LoginPasswordError::Unavailable);
            }
        }
        self.clear_login_failures(source);

        match ready.totp() {
            ManagerTotpState::Disabled => self
                .create_login_session(&ready, now, ManagerReauthEvidence::Password)
                .map(LoginPasswordResult::Authenticated)
                .map_err(|_| LoginPasswordError::Storage),
            ManagerTotpState::Enabled(_) => {
                let challenge =
                    self.create_login_totp_challenge(ready.credential_epoch(), source, now)?;
                Ok(LoginPasswordResult::TotpRequired(challenge))
            }
            ManagerTotpState::Unavailable { .. } => {
                self.log_login_rejected("manager_totp_unavailable");
                Err(LoginPasswordError::ManagerTotpUnavailable)
            }
        }
    }

    pub async fn login_totp(
        &self,
        source: IpAddr,
        challenge: Uuid,
        code: &str,
    ) -> Result<AuthTokenPair, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        self.ensure_session_registry_ready()
            .map_err(|_| ManagerTotpVerificationError::Unavailable)?;
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };
        let now = self.now();
        self.validate_login_totp_challenge(challenge, ready.credential_epoch(), source, now)?;

        if let Err(error) = self.verify_active_totp(&ready, source, code, now) {
            if counts_challenge_attempt(error) {
                self.record_login_challenge_failure(challenge)?;
            }
            return Err(error);
        }
        self.remove_login_challenge(challenge);
        self.create_login_session(&ready, now, ManagerReauthEvidence::Totp)
    }

    pub fn totp_status(&self) -> Result<ManagerTotpPublicState, ManagerTotpVerificationError> {
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };
        Ok(public_totp_state(ready.totp()))
    }

    pub fn verify_sensitive_totp(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        code: Option<&str>,
    ) -> Result<ManagerTotpSensitiveVerification, ManagerTotpVerificationError> {
        self.validate_access_context(auth_context)
            .map_err(map_access_to_totp_error)?;
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };
        match ready.totp() {
            ManagerTotpState::Disabled => Ok(ManagerTotpSensitiveVerification::Disabled),
            ManagerTotpState::Enabled(_) => {
                let code = code.ok_or(ManagerTotpVerificationError::Required)?;
                self.verify_active_totp(&ready, source, code, self.now())?;
                Ok(ManagerTotpSensitiveVerification::Verified)
            }
            ManagerTotpState::Unavailable { .. } => Err(ManagerTotpVerificationError::Unavailable),
        }
    }

    pub async fn reauthenticate_secret_governance(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        credential: ManagerReauthCredential<'_>,
    ) -> Result<ManagerSecretGovernanceReauth, ManagerTotpVerificationError> {
        self.validate_access_context(auth_context)
            .map_err(map_access_to_totp_error)?;
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() == auth_context.credential_epoch =>
            {
                ready
            }
            ManagerCredentialSnapshot::Ready(_) | ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };

        let evidence = match (ready.totp(), credential) {
            (ManagerTotpState::Disabled, ManagerReauthCredential::Password(password)) => {
                self.verify_current_password(&ready, source, password)
                    .await?;
                ManagerReauthEvidence::Password
            }
            (ManagerTotpState::Enabled(_), ManagerReauthCredential::Totp(code)) => {
                self.verify_active_totp(&ready, source, code, self.now())?;
                ManagerReauthEvidence::Totp
            }
            (ManagerTotpState::Unavailable { .. }, _) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
            (ManagerTotpState::Disabled, ManagerReauthCredential::Totp(_))
            | (ManagerTotpState::Enabled(_), ManagerReauthCredential::Password(_)) => {
                return Err(ManagerTotpVerificationError::ReauthMethodChanged);
            }
        };

        let grant = ManagerSecretGovernanceReauth::new(evidence, self.now());
        self.install_secret_governance_reauth(auth_context, grant)?;
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.secret_governance_reauthenticated",
                &[
                    (
                        "login_instance_id",
                        Some(auth_context.login_instance_id.to_string()),
                    ),
                    ("method", Some(evidence.as_str().to_string())),
                    ("verified_until", Some(grant.verified_until.to_string())),
                ],
            )
        );
        Ok(grant)
    }

    pub fn authorize_secret_governance(
        &self,
        auth_context: &ManagerAuthContext,
    ) -> Result<ManagerSecretGovernanceReauth, ManagerTotpVerificationError> {
        self.validate_access_context(auth_context)
            .map_err(map_access_to_totp_error)?;
        match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() == auth_context.credential_epoch =>
            {
                if matches!(ready.totp(), ManagerTotpState::Unavailable { .. }) {
                    return Err(ManagerTotpVerificationError::Unavailable);
                }
            }
            ManagerCredentialSnapshot::Ready(_) | ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        }
        self.secret_governance_reauth_for_context(auth_context, self.now())
    }

    pub fn secret_governance_reauth_for_session(
        &self,
        login_instance_id: i64,
        credential_epoch: Uuid,
    ) -> Result<Option<ManagerSecretGovernanceReauth>, AccessTokenError> {
        let now = self.now();
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let SessionRegistryState::Ready(sessions) = &mut *registry else {
            return Err(AccessTokenError::Unavailable);
        };
        let Some(session) = sessions.get_mut(&login_instance_id) else {
            return Err(AccessTokenError::Invalid);
        };
        if session.expires_at <= now {
            sessions.remove(&login_instance_id);
            return Err(AccessTokenError::Invalid);
        }
        if session.credential_epoch != credential_epoch {
            return Err(AccessTokenError::Invalid);
        }
        let reauth = session
            .secret_governance_reauth
            .filter(|reauth| reauth.verified_until > now);
        session.secret_governance_reauth = reauth;
        Ok(reauth)
    }

    pub async fn start_totp_enrollment(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        current_password: &str,
    ) -> Result<ManagerTotpSetup, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = self.ready_for_lifecycle(auth_context)?;
        if !matches!(ready.totp(), ManagerTotpState::Disabled) {
            return Err(ManagerTotpVerificationError::StateConflict);
        }
        self.verify_current_password(&ready, source, current_password)
            .await?;
        self.create_setup_totp_challenge(
            ManagerTotpSetupAction::Enroll,
            auth_context.login_instance_id,
            ready.credential_epoch(),
            source,
            self.now(),
        )
    }

    pub async fn confirm_totp_enrollment(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        challenge: Uuid,
        code: &str,
    ) -> Result<ManagerTotpLifecycleResult, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = self.ready_for_lifecycle(auth_context)?;
        if !matches!(ready.totp(), ManagerTotpState::Disabled) {
            return Err(ManagerTotpVerificationError::StateConflict);
        }
        let now = self.now();
        let (manual_secret, matched_step) = self.verify_setup_totp_challenge(
            challenge,
            ManagerTotpSetupAction::Enroll,
            auth_context.login_instance_id,
            ready.credential_epoch(),
            source,
            code,
            None,
            now,
        )?;
        self.complete_totp_install(
            &ready,
            ExpectedManagerTotpState::Disabled,
            manual_secret,
            matched_step,
            now,
            ManagerReauthEvidence::PasswordTotp,
            "totp_enrolled",
            "manager.auth.totp_enrolled",
        )
        .await
    }

    pub async fn start_totp_replacement(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        current_password: &str,
        current_totp_code: &str,
    ) -> Result<ManagerTotpSetup, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = self.ready_for_lifecycle(auth_context)?;
        if !matches!(ready.totp(), ManagerTotpState::Enabled(_)) {
            return match ready.totp() {
                ManagerTotpState::Unavailable { .. } => {
                    Err(ManagerTotpVerificationError::Unavailable)
                }
                _ => Err(ManagerTotpVerificationError::StateConflict),
            };
        }
        self.verify_current_password(&ready, source, current_password)
            .await?;
        self.verify_active_totp(&ready, source, current_totp_code, self.now())?;
        self.create_setup_totp_challenge(
            ManagerTotpSetupAction::Replace,
            auth_context.login_instance_id,
            ready.credential_epoch(),
            source,
            self.now(),
        )
    }

    pub async fn confirm_totp_replacement(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        challenge: Uuid,
        code: &str,
    ) -> Result<ManagerTotpLifecycleResult, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = self.ready_for_lifecycle(auth_context)?;
        let last_accepted_step = match ready.totp() {
            ManagerTotpState::Enabled(totp) => totp.last_accepted_step(),
            ManagerTotpState::Unavailable { .. } => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
            ManagerTotpState::Disabled => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
        };
        let now = self.now();
        let (manual_secret, matched_step) = self.verify_setup_totp_challenge(
            challenge,
            ManagerTotpSetupAction::Replace,
            auth_context.login_instance_id,
            ready.credential_epoch(),
            source,
            code,
            Some(last_accepted_step),
            now,
        )?;
        self.complete_totp_install(
            &ready,
            ExpectedManagerTotpState::Enabled,
            manual_secret,
            matched_step,
            now,
            ManagerReauthEvidence::PasswordTotp,
            "totp_replaced",
            "manager.auth.totp_replaced",
        )
        .await
    }

    pub async fn disable_totp(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        current_password: &str,
        current_totp_code: &str,
    ) -> Result<ManagerTotpLifecycleResult, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = self.ready_for_lifecycle(auth_context)?;
        match ready.totp() {
            ManagerTotpState::Enabled(_) => {}
            ManagerTotpState::Unavailable { .. } => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
            ManagerTotpState::Disabled => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
        }
        self.verify_current_password(&ready, source, current_password)
            .await?;
        self.verify_active_totp(&ready, source, current_totp_code, self.now())?;

        let now = self.now();
        let new_epoch = Uuid::new_v4();
        let refresh_jti = generate_token_jti();
        let absolute_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let idle_expires_at = (now + REFRESH_FAMILY_IDLE_SEC).min(absolute_expires_at);
        let mutation = ManagerCredential::disable_totp_with_session(
            &ready.credential_epoch().to_string(),
            &new_epoch.to_string(),
            new_session(
                &refresh_jti,
                &new_epoch,
                now,
                idle_expires_at,
                absolute_expires_at,
            ),
            now,
            "totp_disabled",
        )
        .map_err(map_totp_repository_error)?;
        let installed = self
            .install_ready_snapshot(mutation.credential)
            .map_err(|_| ManagerTotpVerificationError::Unavailable)?;
        if !matches!(installed.totp(), ManagerTotpState::Disabled) {
            return Err(ManagerTotpVerificationError::Unavailable);
        }
        let mut tokens = issue_token_pair(
            &mutation.session,
            &refresh_jti,
            installed.credential_epoch(),
            now,
            absolute_expires_at,
        );
        tokens.reauth = Some(ManagerSecretGovernanceReauth::new(
            ManagerReauthEvidence::PasswordTotp,
            now,
        ));
        self.replace_sessions_with(&mutation.session, &tokens);
        self.clear_all_totp_challenges();
        self.log_totp_lifecycle(
            "manager.auth.totp_disabled",
            mutation.session.id,
            mutation.revoked_sessions,
        );
        Ok(ManagerTotpLifecycleResult {
            tokens,
            state: ManagerTotpPublicState::Disabled,
            recovery_codes: None,
            revoked_sessions: mutation.revoked_sessions,
        })
    }

    pub async fn start_totp_recovery(
        &self,
        source: IpAddr,
        submitted_password: &str,
        submitted_recovery_code: &str,
    ) -> Result<ManagerTotpSetup, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::RecoveryCredentialsInvalid);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };
        if matches!(ready.totp(), ManagerTotpState::Disabled) {
            return Err(ManagerTotpVerificationError::RecoveryCredentialsInvalid);
        }
        self.verify_recovery_password(&ready, source, submitted_password)
            .await?;

        let normalized = normalize_manager_totp_recovery_code(submitted_recovery_code).ok();
        let recovery_row = match normalized.as_ref() {
            Some((code_id, _)) => ManagerTotpRecoveryCode::load_by_code_id(code_id)
                .map_err(|_| ManagerTotpVerificationError::Storage)?,
            None => None,
        };
        let verifier = recovery_row
            .as_ref()
            .map(|row| row.code_verifier.as_str())
            .unwrap_or(DUMMY_RECOVERY_CODE_VERIFIER);
        let candidate = normalized
            .as_ref()
            .map(|(_, normalized)| Zeroizing::new(normalized.to_string()))
            .unwrap_or_else(|| Zeroizing::new(DUMMY_RECOVERY_CODE.to_string()));
        let now = self.now();
        self.precheck_totp_source(source, now)?;
        self.reserve_totp_global_verification(now)?;
        let verification = self
            .password_engine
            .verify(candidate, Zeroizing::new(verifier.to_string()))
            .await;
        match verification {
            Ok(()) if recovery_row.is_some() => {}
            Ok(()) | Err(PasswordOperationError::IncorrectPassword) => {
                self.record_totp_failure(source, now);
                self.log_totp_rejected("recovery_credentials_invalid", None);
                return Err(ManagerTotpVerificationError::RecoveryCredentialsInvalid);
            }
            Err(PasswordOperationError::Busy) => {
                self.release_totp_global_verification(now);
                return Err(ManagerTotpVerificationError::Busy);
            }
            Err(PasswordOperationError::InvalidVerifier) => {
                self.record_totp_failure(source, now);
                self.log_totp_rejected("recovery_credentials_invalid", None);
                return Err(ManagerTotpVerificationError::RecoveryCredentialsInvalid);
            }
            Err(PasswordOperationError::Runtime) => {
                return Err(ManagerTotpVerificationError::Storage);
            }
        }

        let provisioning = generate_manager_totp_provisioning()
            .map_err(|_| ManagerTotpVerificationError::Storage)?;
        let recovery_row =
            recovery_row.expect("successful recovery verification requires a repository row");
        let mutation = ManagerCredential::consume_recovery_code_and_revoke_sessions(
            &ready.credential_epoch().to_string(),
            &recovery_row.code_id,
            &recovery_row.code_verifier,
            now,
            "totp_recovery_started",
        )
        .map_err(|error| match error {
            ManagerCredentialRepositoryError::RecoveryCodeConflict => {
                ManagerTotpVerificationError::RecoveryCredentialsInvalid
            }
            other => map_totp_repository_error(other),
        })?;
        let setup = self.create_recovery_totp_challenge(
            ready.credential_epoch(),
            source,
            now,
            &provisioning,
        )?;
        self.clear_sessions();
        self.clear_login_failures(source);
        self.clear_totp_failures(source);
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.totp_recovery_started",
                &[(
                    "revoked_sessions",
                    Some(mutation.revoked_sessions.to_string()),
                )],
            )
        );
        Ok(setup)
    }

    pub async fn confirm_totp_recovery(
        &self,
        source: IpAddr,
        challenge: Uuid,
        code: &str,
    ) -> Result<ManagerTotpLifecycleResult, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };
        let last_accepted_step = match ready.totp() {
            ManagerTotpState::Enabled(totp) => totp.last_accepted_step(),
            ManagerTotpState::Unavailable { .. } => ManagerCredential::load()
                .map_err(|_| ManagerTotpVerificationError::Storage)?
                .and_then(|credential| credential.totp_last_accepted_step)
                .ok_or(ManagerTotpVerificationError::StateConflict)?,
            ManagerTotpState::Disabled => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
        };
        let now = self.now();
        let (manual_secret, matched_step) = self.verify_recovery_totp_challenge(
            challenge,
            ready.credential_epoch(),
            source,
            code,
            last_accepted_step,
            now,
        )?;
        self.complete_totp_install(
            &ready,
            ExpectedManagerTotpState::Enabled,
            manual_secret,
            matched_step,
            now,
            ManagerReauthEvidence::RecoveryTotp,
            "totp_recovery_completed",
            "manager.auth.totp_recovery_completed",
        )
        .await
    }

    pub async fn rotate_password(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        totp_code: Option<&str>,
        current_password: &str,
        new_password: &str,
    ) -> Result<AuthTokenPair, RotatePasswordError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| RotatePasswordError::Busy)?;
        self.ensure_session_registry_ready()
            .map_err(|_| RotatePasswordError::Unavailable)?;
        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(RotatePasswordError::EpochConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(RotatePasswordError::Unavailable);
            }
        };
        if auth_context.credential_epoch != ready.credential_epoch() {
            return Err(RotatePasswordError::EpochConflict);
        }

        let current_password = normalize_password(current_password)
            .map_err(|_| RotatePasswordError::InvalidCurrentPassword)?;
        let new_password =
            normalize_password(new_password).map_err(RotatePasswordError::PasswordPolicy)?;
        if current_password.as_str() == new_password.as_str() {
            return Err(RotatePasswordError::SamePassword);
        }

        match self
            .password_engine
            .verify(current_password, ready.password_verifier())
            .await
        {
            Ok(()) => {}
            Err(PasswordOperationError::IncorrectPassword) => {
                return Err(RotatePasswordError::InvalidCurrentPassword);
            }
            Err(PasswordOperationError::Busy) => return Err(RotatePasswordError::Busy),
            Err(_) => {
                self.mark_unavailable(CredentialUnavailableReason::InvalidVerifier);
                return Err(RotatePasswordError::Unavailable);
            }
        }
        self.verify_sensitive_totp(auth_context, source, totp_code)
            .map_err(RotatePasswordError::Totp)?;
        let verifier = self
            .password_engine
            .hash(new_password)
            .await
            .map_err(map_rotate_password_error)?;
        let now = self.now();
        let new_epoch = Uuid::new_v4();
        let refresh_jti = generate_token_jti();
        let absolute_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let idle_expires_at = (now + REFRESH_FAMILY_IDLE_SEC).min(absolute_expires_at);
        let mutation = ManagerCredential::rotate_with_session(
            &ready.credential_epoch().to_string(),
            RotatedManagerCredential {
                password_verifier: verifier.to_string(),
                credential_epoch: new_epoch.to_string(),
                now,
            },
            new_session(
                &refresh_jti,
                &new_epoch,
                now,
                idle_expires_at,
                absolute_expires_at,
            ),
            "credential_rotated",
        )
        .map_err(map_rotate_repository_error)?;
        let installed = self
            .install_ready_snapshot(mutation.credential)
            .map_err(|_| RotatePasswordError::Unavailable)?;
        let evidence = match ready.totp() {
            ManagerTotpState::Disabled => ManagerReauthEvidence::Password,
            ManagerTotpState::Enabled(_) => ManagerReauthEvidence::PasswordTotp,
            ManagerTotpState::Unavailable { .. } => {
                return Err(RotatePasswordError::Unavailable);
            }
        };
        let mut pair = issue_token_pair(
            &mutation.session,
            &refresh_jti,
            installed.credential_epoch(),
            now,
            absolute_expires_at,
        );
        pair.reauth = Some(ManagerSecretGovernanceReauth::new(evidence, now));
        self.replace_sessions_with(&mutation.session, &pair);

        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.password_rotated",
                &[
                    ("login_instance_id", Some(mutation.session.id.to_string())),
                    (
                        "revoked_sessions",
                        Some(mutation.revoked_sessions.to_string()),
                    ),
                ],
            )
        );
        Ok(pair)
    }

    pub async fn access_for_session(
        &self,
        login_instance_id: i64,
        credential_epoch: Uuid,
    ) -> Result<String, AccessTokenError> {
        self.ensure_session_registry_ready()
            .map_err(|_| AccessTokenError::Unavailable)?;
        let now = self.now();
        let before_flight = self.session_snapshot(login_instance_id, credential_epoch, now)?;
        if let Some(access) = &before_flight.access
            && access.expires_at - now > ACCESS_CACHE_REFRESH_THRESHOLD_SEC
        {
            return Ok(access.token.clone());
        }

        let flight = self.access_flight(login_instance_id);
        let _flight_guard = flight.lock().await;
        let now = self.now();
        let after_flight = self.session_snapshot(login_instance_id, credential_epoch, now)?;
        if let Some(access) = &after_flight.access
            && access.expires_at - now > ACCESS_CACHE_REFRESH_THRESHOLD_SEC
        {
            return Ok(access.token.clone());
        }
        let fallback = after_flight
            .access
            .clone()
            .filter(|access| access.expires_at > now);

        match self
            .rotate_refresh_token(&after_flight.current_refresh_token)
            .await
        {
            Ok(pair) => Ok(pair.access_token),
            Err(RefreshError::Storage | RefreshError::Unavailable) => fallback
                .map(|access| access.token)
                .ok_or(AccessTokenError::Storage),
            Err(RefreshError::Replay) => Err(AccessTokenError::Replay),
            Err(RefreshError::Invalid | RefreshError::EpochMismatch) => {
                Err(AccessTokenError::Invalid)
            }
        }
    }

    #[cfg(test)]
    pub async fn refresh(&self, refresh_token: &str) -> Result<AuthTokenPair, RefreshError> {
        let refresh = decode_refresh_token(refresh_token).map_err(|_| RefreshError::Invalid)?;
        let flight = self.access_flight(refresh.login_instance_id);
        let _flight_guard = flight.lock().await;
        self.rotate_refresh_token(refresh_token).await
    }

    async fn rotate_refresh_token(
        &self,
        refresh_token: &str,
    ) -> Result<AuthTokenPair, RefreshError> {
        self.ensure_session_registry_ready()
            .map_err(|_| RefreshError::Unavailable)?;
        let refresh = decode_refresh_token(refresh_token).map_err(|_| {
            self.log_refresh_rejected("invalid_token", None);
            RefreshError::Invalid
        })?;
        self.validate_refresh_epoch(refresh.credential_epoch)?;
        let current_session = self
            .session_snapshot(
                refresh.login_instance_id,
                refresh.credential_epoch,
                self.now(),
            )
            .map_err(|error| match error {
                AccessTokenError::Unavailable => RefreshError::Unavailable,
                AccessTokenError::Storage => RefreshError::Storage,
                AccessTokenError::Invalid | AccessTokenError::Replay => RefreshError::Invalid,
            })?;
        if current_session.current_refresh_token != refresh_token
            || current_session.current_refresh_jti != refresh.jwt_id
            || current_session.refresh_generation != refresh.refresh_generation
        {
            return self.revoke_refresh_replay(
                refresh.login_instance_id,
                "stale_token",
                self.now(),
            );
        }

        let instance = ManagerAuthInstance::get_instance(refresh.login_instance_id)
            .map_err(|_| RefreshError::Storage)?
            .ok_or_else(|| {
                self.log_refresh_rejected("instance_missing", Some(refresh.login_instance_id));
                RefreshError::Invalid
            })?;
        let now = self.now();
        if instance.manager_id != refresh.manager_id || instance.manager_subject != MANAGER_SUBJECT
        {
            self.log_refresh_rejected("instance_mismatch", Some(instance.id));
            return Err(RefreshError::Invalid);
        }
        if instance.revoked_at.is_some()
            || instance.idle_expires_at <= now
            || instance.absolute_expires_at <= now
        {
            self.remove_session(instance.id);
            self.log_refresh_rejected("instance_inactive", Some(instance.id));
            return Err(RefreshError::Invalid);
        }
        if instance.current_refresh_jti != refresh.jwt_id
            || instance.refresh_generation != refresh.refresh_generation
            || instance.credential_epoch != refresh.credential_epoch.to_string()
            || instance.signing_key_id != manager_jwt_key_id()
        {
            return self.revoke_refresh_replay(instance.id, "stale_token", now);
        }

        let new_refresh_jti = generate_token_jti();
        let new_idle_expires_at = (now + REFRESH_FAMILY_IDLE_SEC).min(instance.absolute_expires_at);
        let rotated = ManagerAuthInstance::rotate_refresh_jti(
            instance.id,
            &refresh.jwt_id,
            refresh.refresh_generation,
            &refresh.credential_epoch.to_string(),
            new_refresh_jti.clone(),
            now,
            new_idle_expires_at,
        )
        .map_err(|_| RefreshError::Storage)?;
        let rotated = match rotated {
            Some(rotated) => rotated,
            None => {
                let current = ManagerAuthInstance::get_instance(instance.id)
                    .map_err(|_| RefreshError::Storage)?;
                if current.is_some_and(|current| {
                    current.manager_id == refresh.manager_id
                        && current.manager_subject == MANAGER_SUBJECT
                        && current.revoked_at.is_none()
                        && current.idle_expires_at > now
                        && current.absolute_expires_at > now
                        && (current.current_refresh_jti != refresh.jwt_id
                            || current.refresh_generation != refresh.refresh_generation
                            || current.credential_epoch != refresh.credential_epoch.to_string())
                }) {
                    return self.revoke_refresh_replay(instance.id, "rotation_conflict", now);
                }
                self.log_refresh_rejected("rotation_conflict", Some(instance.id));
                return Err(RefreshError::Invalid);
            }
        };
        self.validate_refresh_epoch(refresh.credential_epoch)?;
        debug!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.refresh_rotated",
                &[("login_instance_id", Some(rotated.id.to_string()))],
            )
        );

        let mut pair = issue_token_pair(
            &rotated,
            &new_refresh_jti,
            refresh.credential_epoch,
            now,
            rotated.absolute_expires_at,
        );
        pair.reauth = current_session
            .secret_governance_reauth
            .filter(|reauth| reauth.verified_until > now);
        self.insert_session(&rotated, &pair);
        Ok(pair)
    }

    pub async fn logout_session(
        &self,
        login_instance_id: i64,
        credential_epoch: Uuid,
    ) -> Result<(), LogoutError> {
        let now = self.now();
        let revoked = ManagerAuthInstance::revoke_instance_for_epoch(
            login_instance_id,
            &credential_epoch.to_string(),
            now,
            "logout",
        )
        .map_err(|_| LogoutError::Storage)?;
        self.remove_session(login_instance_id);
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.logout",
                &[
                    ("login_instance_id", Some(login_instance_id.to_string())),
                    ("revoked", Some(revoked.is_some().to_string())),
                ],
            )
        );
        Ok(())
    }

    pub async fn logout_all(
        &self,
        auth_context: &ManagerAuthContext,
        source: IpAddr,
        current_password: &str,
        totp_code: Option<&str>,
    ) -> Result<usize, ManagerTotpVerificationError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| ManagerTotpVerificationError::Busy)?;
        let ready = self.ready_for_lifecycle(auth_context)?;
        self.verify_current_password(&ready, source, current_password)
            .await?;
        match ready.totp() {
            ManagerTotpState::Disabled => {}
            ManagerTotpState::Enabled(_) => {
                let code = totp_code.ok_or(ManagerTotpVerificationError::Required)?;
                self.verify_active_totp(&ready, source, code, self.now())?;
            }
            ManagerTotpState::Unavailable { .. } => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        }
        let now = self.now();
        let revoked = ManagerAuthInstance::revoke_all_active(now, "logout_all")
            .map_err(|_| ManagerTotpVerificationError::Storage)?;
        self.clear_sessions();
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.logout_all",
                &[("revoked_sessions", Some(revoked.to_string()))],
            )
        );
        Ok(revoked)
    }

    pub fn validate_access_context(
        &self,
        auth_context: &ManagerAuthContext,
    ) -> Result<(), AccessCredentialError> {
        match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() == auth_context.credential_epoch =>
            {
                self.validate_session(
                    auth_context.login_instance_id,
                    auth_context.manager_id,
                    &auth_context.manager_subject,
                    auth_context.session_version,
                    &auth_context.access_jti,
                )
                .map_err(|error| match error {
                    SessionValidationError::Invalid => AccessCredentialError::SessionInvalid,
                    SessionValidationError::Unavailable => {
                        AccessCredentialError::SessionUnavailable
                    }
                })
            }
            ManagerCredentialSnapshot::Ready(_) | ManagerCredentialSnapshot::Uninitialized => {
                Err(AccessCredentialError::EpochMismatchOrUninitialized)
            }
            ManagerCredentialSnapshot::Unavailable(_) => Err(AccessCredentialError::Unavailable),
        }
    }

    pub fn cleanup_expired_instances(&self) -> Result<usize, BaseError> {
        let now = self.now();
        let removed = ManagerAuthInstance::cleanup_expired_instances(now)?;
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            sessions.retain(|_, session| session.expires_at > now);
        }
        Ok(removed)
    }

    fn ready_for_lifecycle(
        &self,
        auth_context: &ManagerAuthContext,
    ) -> Result<ReadyManagerCredential, ManagerTotpVerificationError> {
        self.ensure_session_registry_ready()
            .map_err(|_| ManagerTotpVerificationError::Unavailable)?;
        self.validate_access_context(auth_context)
            .map_err(map_access_to_totp_error)?;
        match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() == auth_context.credential_epoch =>
            {
                Ok(ready)
            }
            ManagerCredentialSnapshot::Ready(_) | ManagerCredentialSnapshot::Uninitialized => {
                Err(ManagerTotpVerificationError::StateConflict)
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                Err(ManagerTotpVerificationError::Unavailable)
            }
        }
    }

    fn create_login_session(
        &self,
        ready: &ReadyManagerCredential,
        now: i64,
        evidence: ManagerReauthEvidence,
    ) -> Result<AuthTokenPair, ManagerTotpVerificationError> {
        let refresh_jti = generate_token_jti();
        let absolute_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let idle_expires_at = (now + REFRESH_FAMILY_IDLE_SEC).min(absolute_expires_at);
        let instance = ManagerAuthInstance::create_instance(
            refresh_jti.clone(),
            manager_jwt_key_id().to_string(),
            ready.credential_epoch().to_string(),
            now,
            idle_expires_at,
            absolute_expires_at,
        )
        .map_err(|_| ManagerTotpVerificationError::Storage)?;
        let mut pair = issue_token_pair(
            &instance,
            &refresh_jti,
            ready.credential_epoch(),
            now,
            absolute_expires_at,
        );
        pair.reauth = Some(ManagerSecretGovernanceReauth::new(evidence, now));
        self.insert_session(&instance, &pair);
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.login_succeeded",
                &[("login_instance_id", Some(instance.id.to_string()))],
            )
        );
        Ok(pair)
    }

    fn create_login_totp_challenge(
        &self,
        credential_epoch: Uuid,
        source: IpAddr,
        now: i64,
    ) -> Result<ManagerLoginTotpChallenge, LoginPasswordError> {
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_challenges(&mut challenges, now);
        challenges.login.retain(|_, challenge| {
            challenge.credential_epoch != credential_epoch || challenge.source != source
        });
        if challenges.login.len() >= LOGIN_TOTP_CHALLENGE_CAPACITY {
            return Err(LoginPasswordError::Busy);
        }
        let challenge_id = unique_challenge_id(&challenges.login);
        challenges.login.insert(
            challenge_id,
            LoginTotpChallenge {
                credential_epoch,
                source,
                expires_at: now + LOGIN_TOTP_CHALLENGE_TTL_SEC,
                attempts: 0,
            },
        );
        Ok(ManagerLoginTotpChallenge {
            challenge: challenge_id,
            expires_in: LOGIN_TOTP_CHALLENGE_TTL_SEC as u64,
        })
    }

    fn validate_login_totp_challenge(
        &self,
        challenge_id: Uuid,
        credential_epoch: Uuid,
        source: IpAddr,
        now: i64,
    ) -> Result<(), ManagerTotpVerificationError> {
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_challenges(&mut challenges, now);
        let challenge = challenges
            .login
            .get(&challenge_id)
            .ok_or(ManagerTotpVerificationError::ChallengeInvalidOrExpired)?;
        if challenge.credential_epoch != credential_epoch || challenge.source != source {
            return Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired);
        }
        Ok(())
    }

    fn record_login_challenge_failure(
        &self,
        challenge_id: Uuid,
    ) -> Result<(), ManagerTotpVerificationError> {
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let exhausted = {
            let challenge = challenges
                .login
                .get_mut(&challenge_id)
                .ok_or(ManagerTotpVerificationError::ChallengeInvalidOrExpired)?;
            challenge.attempts = challenge.attempts.saturating_add(1);
            challenge.attempts >= TOTP_CHALLENGE_ATTEMPT_LIMIT
        };
        if exhausted {
            challenges.login.remove(&challenge_id);
            return Err(ManagerTotpVerificationError::ChallengeAttemptsExhausted);
        }
        Ok(())
    }

    fn remove_login_challenge(&self, challenge_id: Uuid) {
        self.totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .login
            .remove(&challenge_id);
    }

    fn create_setup_totp_challenge(
        &self,
        action: ManagerTotpSetupAction,
        login_instance_id: i64,
        credential_epoch: Uuid,
        source: IpAddr,
        now: i64,
    ) -> Result<ManagerTotpSetup, ManagerTotpVerificationError> {
        let provisioning = generate_manager_totp_provisioning()
            .map_err(|_| ManagerTotpVerificationError::Storage)?;
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_challenges(&mut challenges, now);
        challenges
            .setup
            .retain(|_, challenge| challenge.login_instance_id != login_instance_id);
        if challenges.setup.len() >= SETUP_TOTP_CHALLENGE_CAPACITY {
            return Err(ManagerTotpVerificationError::Busy);
        }
        let challenge_id = unique_challenge_id(&challenges.setup);
        challenges.setup.insert(
            challenge_id,
            SetupTotpChallenge {
                action,
                login_instance_id,
                credential_epoch,
                source,
                expires_at: now + SETUP_TOTP_CHALLENGE_TTL_SEC,
                attempts: 0,
                manual_secret: SensitiveSecret::new(
                    provisioning.manual_secret().to_unprotected_string(),
                ),
            },
        );
        Ok(ManagerTotpSetup {
            challenge: challenge_id,
            manual_secret: SensitiveSecret::new(
                provisioning.manual_secret().to_unprotected_string(),
            ),
            otpauth_uri: SensitiveSecret::new(provisioning.otpauth_uri().to_unprotected_string()),
            expires_in: SETUP_TOTP_CHALLENGE_TTL_SEC as u64,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_setup_totp_challenge(
        &self,
        challenge_id: Uuid,
        action: ManagerTotpSetupAction,
        login_instance_id: i64,
        credential_epoch: Uuid,
        source: IpAddr,
        code: &str,
        minimum_step: Option<i64>,
        now: i64,
    ) -> Result<(SensitiveSecret, i64), ManagerTotpVerificationError> {
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_challenges(&mut challenges, now);
        let challenge = challenges
            .setup
            .get_mut(&challenge_id)
            .ok_or(ManagerTotpVerificationError::ChallengeInvalidOrExpired)?;
        if challenge.action != action
            || challenge.login_instance_id != login_instance_id
            || challenge.credential_epoch != credential_epoch
            || challenge.source != source
        {
            return Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired);
        }

        let result =
            self.verify_new_totp(&challenge.manual_secret, source, code, minimum_step, now);
        match result {
            Ok(matched_step) => {
                let manual_secret =
                    SensitiveSecret::new(challenge.manual_secret.to_unprotected_string());
                challenges.setup.remove(&challenge_id);
                Ok((manual_secret, matched_step))
            }
            Err(error) if counts_challenge_attempt(error) => {
                challenge.attempts = challenge.attempts.saturating_add(1);
                let exhausted = challenge.attempts >= TOTP_CHALLENGE_ATTEMPT_LIMIT;
                if exhausted {
                    challenges.setup.remove(&challenge_id);
                    Err(ManagerTotpVerificationError::ChallengeAttemptsExhausted)
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn create_recovery_totp_challenge(
        &self,
        credential_epoch: Uuid,
        source: IpAddr,
        now: i64,
        provisioning: &totp::ManagerTotpProvisioning,
    ) -> Result<ManagerTotpSetup, ManagerTotpVerificationError> {
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_challenges(&mut challenges, now);
        challenges.recovery.clear();
        if challenges.recovery.len() >= RECOVERY_TOTP_CHALLENGE_CAPACITY {
            return Err(ManagerTotpVerificationError::Busy);
        }
        let challenge_id = unique_challenge_id(&challenges.recovery);
        challenges.recovery.insert(
            challenge_id,
            RecoveryTotpChallenge {
                credential_epoch,
                source,
                expires_at: now + SETUP_TOTP_CHALLENGE_TTL_SEC,
                attempts: 0,
                manual_secret: SensitiveSecret::new(
                    provisioning.manual_secret().to_unprotected_string(),
                ),
            },
        );
        Ok(ManagerTotpSetup {
            challenge: challenge_id,
            manual_secret: SensitiveSecret::new(
                provisioning.manual_secret().to_unprotected_string(),
            ),
            otpauth_uri: SensitiveSecret::new(provisioning.otpauth_uri().to_unprotected_string()),
            expires_in: SETUP_TOTP_CHALLENGE_TTL_SEC as u64,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_recovery_totp_challenge(
        &self,
        challenge_id: Uuid,
        credential_epoch: Uuid,
        source: IpAddr,
        code: &str,
        minimum_step: i64,
        now: i64,
    ) -> Result<(SensitiveSecret, i64), ManagerTotpVerificationError> {
        let mut challenges = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_challenges(&mut challenges, now);
        let challenge = challenges
            .recovery
            .get_mut(&challenge_id)
            .ok_or(ManagerTotpVerificationError::ChallengeInvalidOrExpired)?;
        if challenge.credential_epoch != credential_epoch || challenge.source != source {
            return Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired);
        }

        let result = self.verify_new_totp(
            &challenge.manual_secret,
            source,
            code,
            Some(minimum_step),
            now,
        );
        match result {
            Ok(matched_step) => {
                let manual_secret =
                    SensitiveSecret::new(challenge.manual_secret.to_unprotected_string());
                challenges.recovery.remove(&challenge_id);
                Ok((manual_secret, matched_step))
            }
            Err(error) if counts_challenge_attempt(error) => {
                challenge.attempts = challenge.attempts.saturating_add(1);
                let exhausted = challenge.attempts >= TOTP_CHALLENGE_ATTEMPT_LIMIT;
                if exhausted {
                    challenges.recovery.remove(&challenge_id);
                    Err(ManagerTotpVerificationError::ChallengeAttemptsExhausted)
                } else {
                    Err(error)
                }
            }
            Err(error) => Err(error),
        }
    }

    fn verify_active_totp(
        &self,
        ready: &ReadyManagerCredential,
        source: IpAddr,
        code: &str,
        now: i64,
    ) -> Result<i64, ManagerTotpVerificationError> {
        let active = match ready.totp() {
            ManagerTotpState::Enabled(active) => active,
            ManagerTotpState::Disabled => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerTotpState::Unavailable { .. } => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        };
        self.precheck_totp_source(source, now)?;
        if validate_manager_totp_code(code).is_err() {
            self.record_totp_failure(source, now);
            self.log_totp_rejected("invalid_format", None);
            return Err(ManagerTotpVerificationError::Invalid);
        }
        self.reserve_totp_global_verification(now)?;
        let secret = self
            .secret_encryption
            .decrypt_current(
                SecretDomain::ManagerTotp(MANAGER_ID),
                active.encrypted_secret(),
            )
            .map_err(|_| ManagerTotpVerificationError::Unavailable)?;
        let matched_step = match match_manager_totp_step(&secret, code, now) {
            Ok(Some(matched_step)) => matched_step,
            Ok(None) | Err(ManagerTotpPrimitiveError::InvalidCodeFormat) => {
                self.record_totp_failure(source, now);
                self.log_totp_rejected("invalid", None);
                return Err(ManagerTotpVerificationError::Invalid);
            }
            Err(_) => return Err(ManagerTotpVerificationError::Unavailable),
        };

        match ManagerCredential::advance_totp_step_if_newer(
            &ready.credential_epoch().to_string(),
            matched_step,
            now,
        ) {
            Ok(credential) => {
                self.install_totp_step_snapshot_if_current(
                    credential,
                    ready.credential_epoch(),
                    matched_step,
                )?;
                self.clear_totp_failures(source);
                Ok(matched_step)
            }
            Err(ManagerCredentialRepositoryError::TotpStepConflict) => {
                let error =
                    self.classify_totp_step_conflict(ready.credential_epoch(), matched_step, now);
                self.record_totp_failure(source, now);
                self.log_totp_rejected(
                    match error {
                        ManagerTotpVerificationError::StepReplayed { .. } => "replay",
                        ManagerTotpVerificationError::StepStale { .. } => "stale",
                        _ => "state_conflict",
                    },
                    None,
                );
                Err(error)
            }
            Err(error) => Err(map_totp_repository_error(error)),
        }
    }

    fn verify_new_totp(
        &self,
        secret: &SensitiveSecret,
        source: IpAddr,
        code: &str,
        minimum_step: Option<i64>,
        now: i64,
    ) -> Result<i64, ManagerTotpVerificationError> {
        self.precheck_totp_source(source, now)?;
        if validate_manager_totp_code(code).is_err() {
            self.record_totp_failure(source, now);
            self.log_totp_rejected("invalid_format", None);
            return Err(ManagerTotpVerificationError::Invalid);
        }
        self.reserve_totp_global_verification(now)?;
        let matched_step = match match_manager_totp_step(secret, code, now) {
            Ok(Some(matched_step)) => matched_step,
            Ok(None) | Err(ManagerTotpPrimitiveError::InvalidCodeFormat) => {
                self.record_totp_failure(source, now);
                self.log_totp_rejected("invalid", None);
                return Err(ManagerTotpVerificationError::Invalid);
            }
            Err(_) => return Err(ManagerTotpVerificationError::Unavailable),
        };
        if let Some(minimum_step) = minimum_step
            && matched_step <= minimum_step
        {
            self.record_totp_failure(source, now);
            let retry_after = totp_retry_after(now);
            let error = if matched_step == minimum_step {
                ManagerTotpVerificationError::StepReplayed { retry_after }
            } else {
                ManagerTotpVerificationError::StepStale { retry_after }
            };
            self.log_totp_rejected(
                if matched_step == minimum_step {
                    "replay"
                } else {
                    "stale"
                },
                None,
            );
            return Err(error);
        }
        self.clear_totp_failures(source);
        Ok(matched_step)
    }

    fn classify_totp_step_conflict(
        &self,
        expected_epoch: Uuid,
        matched_step: i64,
        now: i64,
    ) -> ManagerTotpVerificationError {
        let current = ManagerCredential::load().ok().flatten();
        let Some(current) = current else {
            return ManagerTotpVerificationError::Storage;
        };
        if current.credential_epoch != expected_epoch.to_string() {
            return ManagerTotpVerificationError::StateConflict;
        }
        let Some(last_accepted_step) = current.totp_last_accepted_step else {
            return ManagerTotpVerificationError::StateConflict;
        };
        let retry_after = totp_retry_after(now);
        if matched_step == last_accepted_step {
            ManagerTotpVerificationError::StepReplayed { retry_after }
        } else if matched_step < last_accepted_step {
            ManagerTotpVerificationError::StepStale { retry_after }
        } else {
            ManagerTotpVerificationError::StateConflict
        }
    }

    async fn verify_current_password(
        &self,
        ready: &ReadyManagerCredential,
        source: IpAddr,
        submitted_password: &str,
    ) -> Result<(), ManagerTotpVerificationError> {
        let now = self.now();
        if let Some(retry_after) = self.source_retry_after(source, now) {
            return Err(ManagerTotpVerificationError::SourceRateLimited { retry_after });
        }
        let password = normalize_password(submitted_password)
            .map_err(|_| ManagerTotpVerificationError::CurrentPasswordInvalid)?;
        self.reserve_global_verification(now)
            .map_err(map_login_error_to_totp_error)?;
        match self
            .password_engine
            .verify(password, ready.password_verifier())
            .await
        {
            Ok(()) => {
                self.clear_login_failures(source);
                Ok(())
            }
            Err(PasswordOperationError::IncorrectPassword) => {
                self.record_login_failure(source, now);
                Err(ManagerTotpVerificationError::CurrentPasswordInvalid)
            }
            Err(PasswordOperationError::Busy) => {
                self.release_global_verification(now);
                Err(ManagerTotpVerificationError::Busy)
            }
            Err(PasswordOperationError::InvalidVerifier | PasswordOperationError::Runtime) => {
                self.mark_unavailable(CredentialUnavailableReason::InvalidVerifier);
                Err(ManagerTotpVerificationError::Unavailable)
            }
        }
    }

    async fn verify_recovery_password(
        &self,
        ready: &ReadyManagerCredential,
        source: IpAddr,
        submitted_password: &str,
    ) -> Result<(), ManagerTotpVerificationError> {
        self.verify_current_password(ready, source, submitted_password)
            .await
            .map_err(|error| match error {
                ManagerTotpVerificationError::CurrentPasswordInvalid => {
                    ManagerTotpVerificationError::RecoveryCredentialsInvalid
                }
                other => other,
            })
    }

    async fn hash_recovery_codes(
        &self,
        now: i64,
    ) -> Result<
        (Vec<NewManagerTotpRecoveryCode>, ManagerTotpRecoveryCodes),
        ManagerTotpVerificationError,
    > {
        let generated = generate_manager_totp_recovery_codes();
        let mut verifier_rows = Vec::with_capacity(generated.len());
        let mut display_codes = Vec::with_capacity(generated.len());
        for code in generated {
            let verifier = self
                .password_engine
                .hash(code.normalized_for_hash())
                .await
                .map_err(|error| match error {
                    PasswordOperationError::Busy => ManagerTotpVerificationError::Busy,
                    _ => ManagerTotpVerificationError::Storage,
                })?;
            verifier_rows.push(NewManagerTotpRecoveryCode {
                code_id: code.code_id().to_string(),
                code_verifier: verifier.to_string(),
                created_at: now,
            });
            display_codes.push(SensitiveSecret::new(code.display().to_unprotected_string()));
        }
        Ok((verifier_rows, ManagerTotpRecoveryCodes(display_codes)))
    }

    #[allow(clippy::too_many_arguments)]
    async fn complete_totp_install(
        &self,
        ready: &ReadyManagerCredential,
        expected_state: ExpectedManagerTotpState,
        manual_secret: SensitiveSecret,
        matched_step: i64,
        now: i64,
        evidence: ManagerReauthEvidence,
        revoke_reason: &str,
        event: &'static str,
    ) -> Result<ManagerTotpLifecycleResult, ManagerTotpVerificationError> {
        let encrypted = self
            .secret_encryption
            .encrypt_current(SecretDomain::ManagerTotp(MANAGER_ID), &manual_secret)
            .map_err(|_| ManagerTotpVerificationError::Unavailable)?;
        let (recovery_verifiers, recovery_codes) = self.hash_recovery_codes(now).await?;
        let new_epoch = Uuid::new_v4();
        let refresh_jti = generate_token_jti();
        let absolute_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let idle_expires_at = (now + REFRESH_FAMILY_IDLE_SEC).min(absolute_expires_at);
        let mutation = ManagerCredential::install_totp_with_session(
            &ready.credential_epoch().to_string(),
            expected_state,
            &new_epoch.to_string(),
            ManagerTotpSecret {
                secret_ciphertext: encrypted.ciphertext().to_vec(),
                secret_nonce: encrypted.nonce().to_vec(),
                secret_format_version: encrypted.format_version(),
                secret_key_fingerprint: encrypted.key_fingerprint().as_str().to_string(),
                last_accepted_step: matched_step,
                enabled_at: now,
            },
            recovery_verifiers,
            new_session(
                &refresh_jti,
                &new_epoch,
                now,
                idle_expires_at,
                absolute_expires_at,
            ),
            now,
            revoke_reason,
        )
        .map_err(|error| match error {
            ManagerCredentialRepositoryError::StateConflict
                if expected_state == ExpectedManagerTotpState::Enabled =>
            {
                self.classify_totp_step_conflict(ready.credential_epoch(), matched_step, now)
            }
            other => map_totp_repository_error(other),
        })?;
        let installed = self
            .install_ready_snapshot(mutation.credential)
            .map_err(|_| ManagerTotpVerificationError::Unavailable)?;
        if !matches!(installed.totp(), ManagerTotpState::Enabled(_)) {
            return Err(ManagerTotpVerificationError::Unavailable);
        }
        let mut tokens = issue_token_pair(
            &mutation.session,
            &refresh_jti,
            installed.credential_epoch(),
            now,
            absolute_expires_at,
        );
        tokens.reauth = Some(ManagerSecretGovernanceReauth::new(evidence, now));
        self.replace_sessions_with(&mutation.session, &tokens);
        self.clear_all_totp_challenges();
        self.log_totp_lifecycle(event, mutation.session.id, mutation.revoked_sessions);
        Ok(ManagerTotpLifecycleResult {
            tokens,
            state: ManagerTotpPublicState::Enabled { enabled_at: now },
            recovery_codes: Some(recovery_codes),
            revoked_sessions: mutation.revoked_sessions,
        })
    }

    fn precheck_totp_source(
        &self,
        source: IpAddr,
        now: i64,
    ) -> Result<(), ManagerTotpVerificationError> {
        let mut state = self
            .totp_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_protection(&mut state, now);
        if let Some(retry_after) = state
            .sources
            .get(&source)
            .and_then(|source_state| source_state.locked_until)
            .filter(|locked_until| *locked_until > now)
            .map(|locked_until| (locked_until - now).max(1) as u64)
        {
            return Err(ManagerTotpVerificationError::SourceRateLimited { retry_after });
        }
        Ok(())
    }

    fn reserve_totp_global_verification(
        &self,
        now: i64,
    ) -> Result<(), ManagerTotpVerificationError> {
        let mut state = self
            .totp_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_protection(&mut state, now);
        if state.global_verifications.len() >= GLOBAL_TOTP_VERIFICATION_LIMIT {
            let retry_after = state
                .global_verifications
                .front()
                .map(|first| (first + GLOBAL_TOTP_VERIFICATION_WINDOW_SEC - now).max(1) as u64)
                .unwrap_or(1);
            return Err(ManagerTotpVerificationError::GlobalRateLimited { retry_after });
        }
        state.global_verifications.push_back(now);
        Ok(())
    }

    fn release_totp_global_verification(&self, reserved_at: i64) {
        let mut state = self
            .totp_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.global_verifications.back() == Some(&reserved_at) {
            state.global_verifications.pop_back();
        }
    }

    fn record_totp_failure(&self, source: IpAddr, now: i64) {
        let mut state = self
            .totp_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_totp_protection(&mut state, now);
        if !state.sources.contains_key(&source) && state.sources.len() >= TOTP_SOURCE_CAPACITY {
            return;
        }
        let source_state = state.sources.entry(source).or_default();
        source_state.failures.push_back(now);
        if source_state.failures.len() >= TOTP_FAILURE_LIMIT {
            source_state.locked_until = Some(now + TOTP_FAILURE_LOCK_SEC);
        }
    }

    fn clear_totp_failures(&self, source: IpAddr) {
        self.totp_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .sources
            .remove(&source);
    }

    fn prune_totp_protection(state: &mut TotpProtectionState, now: i64) {
        let failure_cutoff = now - TOTP_FAILURE_WINDOW_SEC;
        for source_state in state.sources.values_mut() {
            while source_state
                .failures
                .front()
                .is_some_and(|failed_at| *failed_at <= failure_cutoff)
            {
                source_state.failures.pop_front();
            }
            if source_state
                .locked_until
                .is_some_and(|locked_until| locked_until <= now)
            {
                source_state.locked_until = None;
            }
        }
        state.sources.retain(|_, source_state| {
            !source_state.failures.is_empty() || source_state.locked_until.is_some()
        });
        let global_cutoff = now - GLOBAL_TOTP_VERIFICATION_WINDOW_SEC;
        while state
            .global_verifications
            .front()
            .is_some_and(|attempted_at| *attempted_at <= global_cutoff)
        {
            state.global_verifications.pop_front();
        }
    }

    fn prune_totp_challenges(state: &mut TotpChallengeState, now: i64) {
        state
            .login
            .retain(|_, challenge| challenge.expires_at > now);
        state
            .setup
            .retain(|_, challenge| challenge.expires_at > now);
        state
            .recovery
            .retain(|_, challenge| challenge.expires_at > now);
    }

    fn clear_all_totp_challenges(&self) {
        let mut state = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.login.clear();
        state.setup.clear();
        state.recovery.clear();
    }

    fn log_totp_rejected(&self, reason: &str, login_instance_id: Option<i64>) {
        let mut fields = vec![("reason", Some(reason.to_string()))];
        if let Some(login_instance_id) = login_instance_id {
            fields.push(("login_instance_id", Some(login_instance_id.to_string())));
        }
        warn!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.totp_verification_rejected",
                &fields,
            )
        );
    }

    fn log_totp_lifecycle(&self, event: &'static str, login_instance_id: i64, revoked: usize) {
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                event,
                &[
                    ("login_instance_id", Some(login_instance_id.to_string())),
                    ("revoked_sessions", Some(revoked.to_string())),
                ],
            )
        );
    }

    fn registry_from_instances(instances: Vec<ManagerAuthInstance>) -> SessionRegistryState {
        let mut sessions = HashMap::with_capacity(instances.len());
        for instance in instances {
            if instance.manager_id != MANAGER_ID
                || instance.manager_subject != MANAGER_SUBJECT
                || instance.session_version < 1
                || instance.refresh_generation < 1
                || instance.signing_key_id != manager_jwt_key_id()
                || Uuid::parse_str(&instance.credential_epoch).is_err()
            {
                warn!(
                    "{}",
                    crate::logging::event_message_with_fields(
                        "manager.auth.session_registry_unavailable",
                        &[("reason", Some("invalid_session_identity".to_string()))],
                    )
                );
                return SessionRegistryState::Unavailable;
            }
            let Some(session) = ActiveManagerSession::from_persisted(&instance) else {
                return SessionRegistryState::Unavailable;
            };
            sessions.insert(instance.id, session);
        }
        SessionRegistryState::Ready(sessions)
    }

    fn ensure_session_registry_ready(&self) -> Result<(), SessionValidationError> {
        let registry = self
            .session_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*registry {
            SessionRegistryState::Ready(_) => Ok(()),
            SessionRegistryState::Unavailable => Err(SessionValidationError::Unavailable),
        }
    }

    fn validate_session(
        &self,
        login_instance_id: i64,
        manager_id: i64,
        manager_subject: &str,
        session_version: i64,
        access_jti: &str,
    ) -> Result<(), SessionValidationError> {
        let now = self.now();
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let SessionRegistryState::Ready(sessions) = &mut *registry else {
            return Err(SessionValidationError::Unavailable);
        };
        let Some(session) = sessions.get(&login_instance_id) else {
            return Err(SessionValidationError::Invalid);
        };
        if session.expires_at <= now {
            sessions.remove(&login_instance_id);
            return Err(SessionValidationError::Invalid);
        }
        if session.manager_id != manager_id
            || session.manager_subject != manager_subject
            || session.session_version != session_version
            || session
                .access
                .as_ref()
                .is_none_or(|access| access.access_jti != access_jti)
        {
            return Err(SessionValidationError::Invalid);
        }
        Ok(())
    }

    fn insert_session(&self, instance: &ManagerAuthInstance, pair: &AuthTokenPair) {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            if let Some(session) = ActiveManagerSession::from_token_pair(instance, pair) {
                sessions.insert(instance.id, session);
            } else {
                *registry = SessionRegistryState::Unavailable;
            }
        }
    }

    fn replace_sessions_with(&self, instance: &ManagerAuthInstance, pair: &AuthTokenPair) {
        let mut sessions = HashMap::with_capacity(1);
        let Some(session) = ActiveManagerSession::from_token_pair(instance, pair) else {
            *self
                .session_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                SessionRegistryState::Unavailable;
            return;
        };
        sessions.insert(instance.id, session);
        *self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            SessionRegistryState::Ready(sessions);
        self.access_flights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|login_instance_id, _| *login_instance_id == instance.id);
    }

    fn remove_session(&self, login_instance_id: i64) {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            sessions.remove(&login_instance_id);
        }
        self.access_flights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&login_instance_id);
    }

    fn clear_sessions(&self) {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            sessions.clear();
        }
        self.access_flights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn access_flight(&self, login_instance_id: i64) -> Arc<AsyncMutex<()>> {
        self.access_flights
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(login_instance_id)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    fn session_snapshot(
        &self,
        login_instance_id: i64,
        credential_epoch: Uuid,
        now: i64,
    ) -> Result<ActiveManagerSession, AccessTokenError> {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let SessionRegistryState::Ready(sessions) = &mut *registry else {
            return Err(AccessTokenError::Unavailable);
        };
        let Some(session) = sessions.get(&login_instance_id) else {
            return Err(AccessTokenError::Invalid);
        };
        if session.expires_at <= now {
            sessions.remove(&login_instance_id);
            return Err(AccessTokenError::Invalid);
        }
        if session.credential_epoch != credential_epoch {
            return Err(AccessTokenError::Invalid);
        }
        Ok(session.clone())
    }

    fn install_secret_governance_reauth(
        &self,
        auth_context: &ManagerAuthContext,
        grant: ManagerSecretGovernanceReauth,
    ) -> Result<(), ManagerTotpVerificationError> {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let SessionRegistryState::Ready(sessions) = &mut *registry else {
            return Err(ManagerTotpVerificationError::Unavailable);
        };
        let Some(session) = sessions.get_mut(&auth_context.login_instance_id) else {
            return Err(ManagerTotpVerificationError::StateConflict);
        };
        if session.expires_at <= grant.verified_at
            || session.manager_id != auth_context.manager_id
            || session.manager_subject != auth_context.manager_subject
            || session.session_version != auth_context.session_version
            || session.credential_epoch != auth_context.credential_epoch
        {
            return Err(ManagerTotpVerificationError::StateConflict);
        }
        session.secret_governance_reauth = Some(grant);
        Ok(())
    }

    fn secret_governance_reauth_for_context(
        &self,
        auth_context: &ManagerAuthContext,
        now: i64,
    ) -> Result<ManagerSecretGovernanceReauth, ManagerTotpVerificationError> {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let SessionRegistryState::Ready(sessions) = &mut *registry else {
            return Err(ManagerTotpVerificationError::Unavailable);
        };
        let Some(session) = sessions.get_mut(&auth_context.login_instance_id) else {
            return Err(ManagerTotpVerificationError::StateConflict);
        };
        if session.expires_at <= now
            || session.manager_id != auth_context.manager_id
            || session.manager_subject != auth_context.manager_subject
            || session.session_version != auth_context.session_version
            || session.credential_epoch != auth_context.credential_epoch
        {
            return Err(ManagerTotpVerificationError::StateConflict);
        }
        let Some(grant) = session
            .secret_governance_reauth
            .filter(|reauth| reauth.verified_until > now)
        else {
            session.secret_governance_reauth = None;
            return Err(ManagerTotpVerificationError::ReauthRequired);
        };
        session.secret_governance_reauth = Some(grant);
        Ok(grant)
    }

    fn revoke_refresh_replay(
        &self,
        login_instance_id: i64,
        reason: &str,
        detected_at: i64,
    ) -> Result<AuthTokenPair, RefreshError> {
        match ManagerAuthInstance::revoke_instance(login_instance_id, detected_at, "refresh_replay")
        {
            Ok(_) => {
                self.remove_session(login_instance_id);
                self.log_refresh_replay(login_instance_id, reason, detected_at);
                Err(RefreshError::Replay)
            }
            Err(_) => {
                self.remove_session(login_instance_id);
                self.log_refresh_rejected("replay_revocation_storage", Some(login_instance_id));
                Err(RefreshError::Storage)
            }
        }
    }

    #[cfg(test)]
    fn session_count(&self) -> Result<usize, SessionValidationError> {
        let registry = self
            .session_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*registry {
            SessionRegistryState::Ready(sessions) => Ok(sessions.len()),
            SessionRegistryState::Unavailable => Err(SessionValidationError::Unavailable),
        }
    }

    #[cfg(test)]
    fn login_protection_counts(&self) -> (usize, usize) {
        let state = self
            .login_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.sources.len(), state.global_verifications.len())
    }

    #[cfg(test)]
    fn totp_protection_counts(&self) -> (usize, usize) {
        let state = self
            .totp_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.sources.len(), state.global_verifications.len())
    }

    #[cfg(test)]
    fn totp_challenge_counts(&self) -> (usize, usize, usize) {
        let state = self
            .totp_challenges
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.login.len(), state.setup.len(), state.recovery.len())
    }

    #[cfg(test)]
    pub(crate) fn secret_encryption(&self) -> &Arc<SecretEncryptionService> {
        &self.secret_encryption
    }

    fn validate_refresh_epoch(&self, epoch: Uuid) -> Result<(), RefreshError> {
        match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready) if ready.credential_epoch() == epoch => Ok(()),
            ManagerCredentialSnapshot::Ready(_) | ManagerCredentialSnapshot::Uninitialized => {
                Err(RefreshError::EpochMismatch)
            }
            ManagerCredentialSnapshot::Unavailable(_) => Err(RefreshError::Unavailable),
        }
    }

    fn install_ready_snapshot(
        &self,
        credential: ManagerCredential,
    ) -> Result<ReadyManagerCredential, BootstrapError> {
        let snapshot =
            ManagerCredentialSnapshot::from_credential(credential, &self.secret_encryption);
        let ManagerCredentialSnapshot::Ready(ready) = &snapshot else {
            self.replace_snapshot(snapshot);
            return Err(BootstrapError::Unavailable);
        };
        let ready = ready.clone();
        self.replace_snapshot(snapshot);
        Ok(ready)
    }

    fn install_totp_step_snapshot_if_current(
        &self,
        credential: ManagerCredential,
        expected_epoch: Uuid,
        matched_step: i64,
    ) -> Result<(), ManagerTotpVerificationError> {
        if Uuid::parse_str(&credential.credential_epoch).ok() != Some(expected_epoch)
            || credential.totp_last_accepted_step != Some(matched_step)
        {
            return Err(ManagerTotpVerificationError::StateConflict);
        }

        let snapshot =
            ManagerCredentialSnapshot::from_credential(credential, &self.secret_encryption);
        let mut current = self
            .credential_snapshot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match &*current {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() != expected_epoch =>
            {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Ready(ready) => match ready.totp() {
                ManagerTotpState::Enabled(totp) if totp.last_accepted_step() >= matched_step => {
                    return Ok(());
                }
                ManagerTotpState::Enabled(_) => {}
                ManagerTotpState::Disabled => {
                    return Err(ManagerTotpVerificationError::StateConflict);
                }
                ManagerTotpState::Unavailable { .. } => {
                    return Err(ManagerTotpVerificationError::Unavailable);
                }
            },
            ManagerCredentialSnapshot::Uninitialized => {
                return Err(ManagerTotpVerificationError::StateConflict);
            }
            ManagerCredentialSnapshot::Unavailable(_) => {
                return Err(ManagerTotpVerificationError::Unavailable);
            }
        }

        match &snapshot {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() == expected_epoch
                    && matches!(
                        ready.totp(),
                        ManagerTotpState::Enabled(totp)
                            if totp.last_accepted_step() == matched_step
                    ) =>
            {
                *current = snapshot;
                Ok(())
            }
            _ => {
                *current = snapshot;
                Err(ManagerTotpVerificationError::Unavailable)
            }
        }
    }

    fn mark_unavailable(&self, reason: CredentialUnavailableReason) {
        self.replace_snapshot(ManagerCredentialSnapshot::Unavailable(reason));
        warn!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.credential_snapshot_unavailable",
                &[("reason", Some(snapshot_reason_code(reason).to_string()))],
            )
        );
    }

    fn snapshot(&self) -> ManagerCredentialSnapshot {
        self.credential_snapshot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    fn replace_snapshot(&self, snapshot: ManagerCredentialSnapshot) {
        *self
            .credential_snapshot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot;
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    fn source_retry_after(&self, source: IpAddr, now: i64) -> Option<u64> {
        let mut state = self
            .login_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_login_protection(&mut state, now);
        state
            .sources
            .get(&source)
            .and_then(|source_state| source_state.locked_until)
            .filter(|locked_until| *locked_until > now)
            .map(|locked_until| (locked_until - now).max(1) as u64)
    }

    fn record_login_failure(&self, source: IpAddr, now: i64) {
        let mut state = self
            .login_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_login_protection(&mut state, now);
        if !state.sources.contains_key(&source) && state.sources.len() >= LOGIN_SOURCE_CAPACITY {
            return;
        }
        let source_state = state.sources.entry(source).or_default();
        source_state.failures.push_back(now);
        if source_state.failures.len() >= LOGIN_FAILURE_LIMIT as usize {
            source_state.locked_until = Some(now + LOGIN_FAILURE_LOCK_SEC);
        }
    }

    fn clear_login_failures(&self, source: IpAddr) {
        let mut state = self
            .login_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.sources.remove(&source);
    }

    fn reserve_global_verification(&self, now: i64) -> Result<(), LoginError> {
        let mut state = self
            .login_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::prune_login_protection(&mut state, now);
        if state.global_verifications.len() >= GLOBAL_LOGIN_VERIFICATION_LIMIT {
            let retry_after = state
                .global_verifications
                .front()
                .map(|first| (first + GLOBAL_LOGIN_VERIFICATION_WINDOW_SEC - now).max(1) as u64)
                .unwrap_or(1);
            self.log_login_rejected("global_rate_limited");
            return Err(LoginError::GlobalRateLimited { retry_after });
        }
        state.global_verifications.push_back(now);
        Ok(())
    }

    fn release_global_verification(&self, reserved_at: i64) {
        let mut state = self
            .login_protection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.global_verifications.back() == Some(&reserved_at) {
            state.global_verifications.pop_back();
        }
    }

    fn prune_login_protection(state: &mut LoginProtectionState, now: i64) {
        let failure_cutoff = now - LOGIN_FAILURE_WINDOW_SEC;
        for source_state in state.sources.values_mut() {
            while source_state
                .failures
                .front()
                .is_some_and(|failed_at| *failed_at <= failure_cutoff)
            {
                source_state.failures.pop_front();
            }
            if source_state
                .locked_until
                .is_some_and(|locked_until| locked_until <= now)
            {
                source_state.locked_until = None;
            }
        }
        state.sources.retain(|_, source_state| {
            !source_state.failures.is_empty() || source_state.locked_until.is_some()
        });

        let global_cutoff = now - GLOBAL_LOGIN_VERIFICATION_WINDOW_SEC;
        while state
            .global_verifications
            .front()
            .is_some_and(|attempted_at| *attempted_at <= global_cutoff)
        {
            state.global_verifications.pop_front();
        }
    }

    fn log_bootstrap_rejected(&self, reason: &str) {
        warn!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.bootstrap_rejected",
                &[("reason", Some(reason.to_string()))],
            )
        );
    }

    fn log_login_rejected(&self, reason: &str) {
        warn!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.login_failed",
                &[("reason", Some(reason.to_string()))],
            )
        );
    }

    fn log_refresh_rejected(&self, reason: &str, login_instance_id: Option<i64>) {
        let mut fields = vec![("reason", Some(reason.to_string()))];
        if let Some(login_instance_id) = login_instance_id {
            fields.push(("login_instance_id", Some(login_instance_id.to_string())));
        }
        warn!(
            "{}",
            crate::logging::event_message_with_fields("manager.auth.refresh_rejected", &fields)
        );
    }

    fn log_refresh_replay(&self, login_instance_id: i64, reason: &str, detected_at: i64) {
        warn!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.refresh_replay_detected",
                &[
                    ("login_instance_id", Some(login_instance_id.to_string())),
                    ("reason", Some(reason.to_string())),
                    ("detected_at", Some(detected_at.to_string())),
                ],
            )
        );
    }
}

fn unique_challenge_id<T>(challenges: &HashMap<Uuid, T>) -> Uuid {
    loop {
        let challenge = Uuid::new_v4();
        if !challenges.contains_key(&challenge) {
            return challenge;
        }
    }
}

fn public_totp_state(state: &ManagerTotpState) -> ManagerTotpPublicState {
    match state {
        ManagerTotpState::Disabled => ManagerTotpPublicState::Disabled,
        ManagerTotpState::Enabled(totp) => ManagerTotpPublicState::Enabled {
            enabled_at: totp.enabled_at(),
        },
        ManagerTotpState::Unavailable { enabled_at, .. } => ManagerTotpPublicState::Unavailable {
            enabled_at: *enabled_at,
        },
    }
}

fn counts_challenge_attempt(error: ManagerTotpVerificationError) -> bool {
    matches!(
        error,
        ManagerTotpVerificationError::Invalid
            | ManagerTotpVerificationError::StepReplayed { .. }
            | ManagerTotpVerificationError::StepStale { .. }
            | ManagerTotpVerificationError::RecoveryCredentialsInvalid
    )
}

fn totp_retry_after(now: i64) -> u64 {
    (MANAGER_TOTP_PERIOD_SEC - now.rem_euclid(MANAGER_TOTP_PERIOD_SEC)).max(1) as u64
}

fn map_access_to_totp_error(error: AccessCredentialError) -> ManagerTotpVerificationError {
    match error {
        AccessCredentialError::EpochMismatchOrUninitialized
        | AccessCredentialError::SessionInvalid => ManagerTotpVerificationError::StateConflict,
        AccessCredentialError::Unavailable | AccessCredentialError::SessionUnavailable => {
            ManagerTotpVerificationError::Unavailable
        }
    }
}

fn map_login_error_to_password_error(error: LoginError) -> LoginPasswordError {
    match error {
        LoginError::Uninitialized => LoginPasswordError::Uninitialized,
        LoginError::InvalidPassword => LoginPasswordError::InvalidPassword,
        LoginError::SourceRateLimited { retry_after } => {
            LoginPasswordError::SourceRateLimited { retry_after }
        }
        LoginError::GlobalRateLimited { retry_after } => {
            LoginPasswordError::GlobalRateLimited { retry_after }
        }
        LoginError::Busy => LoginPasswordError::Busy,
        LoginError::Unavailable => LoginPasswordError::Unavailable,
        LoginError::Storage => LoginPasswordError::Storage,
    }
}

fn map_login_password_error(error: LoginPasswordError) -> LoginError {
    match error {
        LoginPasswordError::Uninitialized => LoginError::Uninitialized,
        LoginPasswordError::InvalidPassword => LoginError::InvalidPassword,
        LoginPasswordError::SourceRateLimited { retry_after } => {
            LoginError::SourceRateLimited { retry_after }
        }
        LoginPasswordError::GlobalRateLimited { retry_after } => {
            LoginError::GlobalRateLimited { retry_after }
        }
        LoginPasswordError::Busy => LoginError::Busy,
        LoginPasswordError::ManagerTotpUnavailable | LoginPasswordError::Unavailable => {
            LoginError::Unavailable
        }
        LoginPasswordError::Storage => LoginError::Storage,
    }
}

fn map_login_error_to_totp_error(error: LoginError) -> ManagerTotpVerificationError {
    match error {
        LoginError::SourceRateLimited { retry_after } => {
            ManagerTotpVerificationError::SourceRateLimited { retry_after }
        }
        LoginError::GlobalRateLimited { retry_after } => {
            ManagerTotpVerificationError::GlobalRateLimited { retry_after }
        }
        LoginError::Busy => ManagerTotpVerificationError::Busy,
        LoginError::Storage => ManagerTotpVerificationError::Storage,
        LoginError::Uninitialized | LoginError::InvalidPassword | LoginError::Unavailable => {
            ManagerTotpVerificationError::Unavailable
        }
    }
}

fn map_totp_repository_error(
    error: ManagerCredentialRepositoryError,
) -> ManagerTotpVerificationError {
    match error {
        ManagerCredentialRepositoryError::EpochConflict
        | ManagerCredentialRepositoryError::StateConflict
        | ManagerCredentialRepositoryError::TotpStepConflict
        | ManagerCredentialRepositoryError::AlreadyInitialized => {
            ManagerTotpVerificationError::StateConflict
        }
        ManagerCredentialRepositoryError::RecoveryCodeConflict => {
            ManagerTotpVerificationError::RecoveryCredentialsInvalid
        }
        ManagerCredentialRepositoryError::ContractViolation(_)
        | ManagerCredentialRepositoryError::Storage(_) => ManagerTotpVerificationError::Storage,
    }
}

fn new_session(
    refresh_jti: &str,
    credential_epoch: &Uuid,
    now: i64,
    idle_expires_at: i64,
    absolute_expires_at: i64,
) -> NewManagerAuthInstance {
    NewManagerAuthInstance {
        id: ID_GENERATOR.generate_id(),
        manager_id: MANAGER_ID,
        manager_subject: MANAGER_SUBJECT.to_string(),
        current_refresh_jti: refresh_jti.to_string(),
        refresh_generation: crate::database::manager_auth_instance::INITIAL_REFRESH_GENERATION,
        session_version: crate::database::manager_auth_instance::INITIAL_SESSION_VERSION,
        signing_key_id: manager_jwt_key_id().to_string(),
        credential_epoch: credential_epoch.to_string(),
        created_at: now,
        last_rotated_at: now,
        idle_expires_at,
        absolute_expires_at,
        revoked_at: None,
        revoked_reason: None,
    }
}

fn issue_token_pair(
    session: &ManagerAuthInstance,
    refresh_jti: &str,
    credential_epoch: Uuid,
    now: i64,
    refresh_expires_at: i64,
) -> AuthTokenPair {
    let access_jti = generate_token_jti();
    AuthTokenPair {
        refresh_token: issue_refresh_token(
            MANAGER_ID,
            session.id,
            refresh_jti,
            session.refresh_generation,
            &credential_epoch,
            now,
            refresh_expires_at,
        ),
        access_token: issue_access_token(
            MANAGER_ID,
            session.id,
            &access_jti,
            session.session_version,
            &credential_epoch,
            now,
        ),
        mediator_token: issue_mediator_token(
            session.id,
            &credential_epoch,
            now,
            session.absolute_expires_at,
        ),
        mediator_expires_at: session.absolute_expires_at,
        reauth: None,
    }
}

fn map_bootstrap_password_error(error: PasswordOperationError) -> BootstrapError {
    match error {
        PasswordOperationError::Busy => BootstrapError::Busy,
        PasswordOperationError::IncorrectPassword => BootstrapError::Storage,
        PasswordOperationError::InvalidVerifier | PasswordOperationError::Runtime => {
            BootstrapError::Storage
        }
    }
}

fn map_rotate_password_error(error: PasswordOperationError) -> RotatePasswordError {
    match error {
        PasswordOperationError::Busy => RotatePasswordError::Busy,
        PasswordOperationError::IncorrectPassword => RotatePasswordError::InvalidCurrentPassword,
        PasswordOperationError::InvalidVerifier | PasswordOperationError::Runtime => {
            RotatePasswordError::Storage
        }
    }
}

fn map_bootstrap_repository_error(error: ManagerCredentialRepositoryError) -> BootstrapError {
    match error {
        ManagerCredentialRepositoryError::AlreadyInitialized => BootstrapError::AlreadyInitialized,
        ManagerCredentialRepositoryError::EpochConflict
        | ManagerCredentialRepositoryError::StateConflict
        | ManagerCredentialRepositoryError::TotpStepConflict
        | ManagerCredentialRepositoryError::RecoveryCodeConflict
        | ManagerCredentialRepositoryError::ContractViolation(_)
        | ManagerCredentialRepositoryError::Storage(_) => BootstrapError::Storage,
    }
}

fn map_rotate_repository_error(error: ManagerCredentialRepositoryError) -> RotatePasswordError {
    match error {
        ManagerCredentialRepositoryError::EpochConflict => RotatePasswordError::EpochConflict,
        ManagerCredentialRepositoryError::AlreadyInitialized
        | ManagerCredentialRepositoryError::StateConflict
        | ManagerCredentialRepositoryError::TotpStepConflict
        | ManagerCredentialRepositoryError::RecoveryCodeConflict
        | ManagerCredentialRepositoryError::ContractViolation(_)
        | ManagerCredentialRepositoryError::Storage(_) => RotatePasswordError::Storage,
    }
}

fn snapshot_reason_code(reason: CredentialUnavailableReason) -> &'static str {
    match reason {
        CredentialUnavailableReason::Storage => "storage",
        CredentialUnavailableReason::InvalidIdentity => "invalid_identity",
        CredentialUnavailableReason::InvalidVerifier => "invalid_verifier",
        CredentialUnavailableReason::InvalidEpoch => "invalid_epoch",
    }
}

fn manager_totp_unavailable_reason_code(
    reason: password::ManagerTotpUnavailableReason,
) -> &'static str {
    match reason {
        password::ManagerTotpUnavailableReason::IncompleteStoredSecret => "incomplete_fields",
        password::ManagerTotpUnavailableReason::InvalidStoredSecret => "invalid_format",
        password::ManagerTotpUnavailableReason::DecryptFailed => "decrypt_failed",
        password::ManagerTotpUnavailableReason::InvalidSecret => "invalid_secret",
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use tokio::sync::Barrier;

    use crate::database::TestDbContext;
    use crate::database::manager_auth_instance::{
        INITIAL_SESSION_VERSION, MANAGER_SUBJECT as SESSION_MANAGER_SUBJECT, ManagerAuthInstance,
    };
    use crate::database::manager_credential::{ManagerCredential, NewManagerCredential};
    use crate::database::manager_totp_recovery_code::ManagerTotpRecoveryCode;
    use crate::database::{DbConnection, get_connection};
    use crate::service::admin::auth::totp::{
        generate_manager_totp_code, generate_manager_totp_provisioning,
    };
    use crate::service::app_state::create_test_app_state;
    use crate::service::secret_encryption::{SecretEncryptionService, SensitiveSecret};
    use crate::utils::auth::{ACCESS_TOKEN_ISSUE_SEC, decode_access_token, decode_refresh_token};
    use diesel::RunQueryDsl;
    use zeroize::Zeroizing;

    use super::{
        AccessCredentialError, AccessTokenError, BootstrapError, BootstrapStatus,
        DUMMY_RECOVERY_CODE, DUMMY_RECOVERY_CODE_VERIFIER, GLOBAL_LOGIN_VERIFICATION_LIMIT,
        GLOBAL_TOTP_VERIFICATION_LIMIT, LOGIN_FAILURE_LIMIT, LOGIN_SOURCE_CAPACITY,
        LOGIN_TOTP_CHALLENGE_CAPACITY, LOGIN_TOTP_CHALLENGE_TTL_SEC, LoginError,
        LoginPasswordError, LoginPasswordResult, ManagerAuthContext, ManagerAuthService,
        ManagerLoginTotpChallenge, ManagerReauthCredential, ManagerReauthEvidence,
        ManagerTotpPublicState, ManagerTotpSensitiveVerification, ManagerTotpSetupAction,
        ManagerTotpVerificationError, PasswordEngine, PasswordOperationError,
        RECOVERY_TOTP_CHALLENGE_CAPACITY, RefreshError, RotatePasswordError,
        SECRET_GOVERNANCE_REAUTH_TTL_SEC, SETUP_TOTP_CHALLENGE_CAPACITY,
        SETUP_TOTP_CHALLENGE_TTL_SEC, TOTP_CHALLENGE_ATTEMPT_LIMIT, TOTP_FAILURE_LIMIT,
        TOTP_SOURCE_CAPACITY,
    };

    const INITIAL_PASSWORD: &str = "correct horse battery staple";
    const ROTATED_PASSWORD: &str = "correct horse battery staple rotated";
    const TEST_SECRET_ENCRYPTION_KEY: &str =
        "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";

    struct EnrolledManager {
        initial_access: ManagerAuthContext,
        access: ManagerAuthContext,
        secret: String,
        recovery_codes: Vec<String>,
    }

    fn test_source(last_octet: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, last_octet))
    }

    fn test_auth_service() -> ManagerAuthService {
        ManagerAuthService::new(Arc::new(
            crate::service::secret_encryption::SecretEncryptionService::from_config(
                &crate::config::SecretEncryptionConfig::default(),
            ),
        ))
    }

    fn test_secret_encryption() -> Arc<SecretEncryptionService> {
        let config = serde_yaml::from_str(&format!(
            "downstream_mode: one_time\nencryption_key: '{TEST_SECRET_ENCRYPTION_KEY}'\n"
        ))
        .expect("test secret encryption config should parse");
        Arc::new(SecretEncryptionService::from_config(&config))
    }

    fn test_totp_auth_service(
        now: &Arc<AtomicI64>,
        secret_encryption: Arc<SecretEncryptionService>,
    ) -> ManagerAuthService {
        let service_now = Arc::clone(now);
        ManagerAuthService::new_for_test_with_secret_encryption(
            Arc::new(move || service_now.load(Ordering::SeqCst)),
            secret_encryption,
        )
    }

    fn code_for(secret: &str, now: i64) -> String {
        let secret = SensitiveSecret::new(secret.to_string());
        generate_manager_totp_code(&secret, now)
            .expect("test TOTP code should generate")
            .to_string()
    }

    async fn enroll_manager(
        service: &ManagerAuthService,
        source: IpAddr,
        now: i64,
    ) -> EnrolledManager {
        let bootstrapped = service
            .bootstrap(INITIAL_PASSWORD)
            .await
            .expect("bootstrap should succeed");
        let initial_access = decode_access_token(&bootstrapped.access_token)
            .expect("bootstrap access should decode");
        let setup = service
            .start_totp_enrollment(&initial_access, source, INITIAL_PASSWORD)
            .await
            .expect("enrollment start should succeed");
        let secret = setup.manual_secret.expose().to_string();
        let result = service
            .confirm_totp_enrollment(
                &initial_access,
                source,
                setup.challenge,
                &code_for(&secret, now),
            )
            .await
            .expect("enrollment confirmation should succeed");
        assert_eq!(
            result.state,
            ManagerTotpPublicState::Enabled { enabled_at: now }
        );
        let recovery_codes = result
            .recovery_codes
            .as_ref()
            .expect("enrollment must return recovery codes")
            .expose()
            .map(str::to_string)
            .collect();
        let access = decode_access_token(&result.tokens.access_token)
            .expect("enrolled access should decode");
        EnrolledManager {
            initial_access,
            access,
            secret,
            recovery_codes,
        }
    }

    #[tokio::test]
    async fn manager_totp_disabled_and_enabled_login_session_contracts_are_distinct() {
        let disabled_db = TestDbContext::new_sqlite("manager-totp-disabled-login.sqlite");
        disabled_db
            .run_async(async {
                let service = test_auth_service();
                service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let result = service
                    .login_password(test_source(1), INITIAL_PASSWORD)
                    .await
                    .expect("disabled password login should authenticate");
                assert!(matches!(result, LoginPasswordResult::Authenticated(_)));
                assert_eq!(service.session_count(), Ok(2));
                assert_eq!(service.totp_status(), Ok(ManagerTotpPublicState::Disabled));
            })
            .await;

        let enabled_db = TestDbContext::new_sqlite("manager-totp-enabled-login.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        enabled_db
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service = test_totp_auth_service(&now, test_secret_encryption());
                    let enrolled =
                        enroll_manager(&service, test_source(2), now.load(Ordering::SeqCst)).await;
                    assert_eq!(
                        service.validate_access_context(&enrolled.initial_access),
                        Err(AccessCredentialError::EpochMismatchOrUninitialized)
                    );
                    assert_eq!(service.session_count(), Ok(1));

                    let first = match service
                        .login_password(test_source(3), INITIAL_PASSWORD)
                        .await
                        .expect("enabled password stage should succeed")
                    {
                        LoginPasswordResult::TotpRequired(challenge) => challenge,
                        LoginPasswordResult::Authenticated(_) => {
                            panic!("enabled password stage must not authenticate")
                        }
                    };
                    assert_eq!(first.expires_in, LOGIN_TOTP_CHALLENGE_TTL_SEC as u64);
                    assert_eq!(service.session_count(), Ok(1));

                    let replacement = match service
                        .login_password(test_source(3), INITIAL_PASSWORD)
                        .await
                        .expect("same-context password stage should replace its challenge")
                    {
                        LoginPasswordResult::TotpRequired(challenge) => challenge,
                        LoginPasswordResult::Authenticated(_) => {
                            panic!("enabled password stage must not authenticate")
                        }
                    };
                    assert_ne!(first.challenge, replacement.challenge);
                    let protection_before_invalid_challenge = service.totp_protection_counts();
                    assert!(matches!(
                        service
                            .login_totp(test_source(3), first.challenge, "000000",)
                            .await,
                        Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
                    ));
                    assert_eq!(
                        service.totp_protection_counts(),
                        protection_before_invalid_challenge
                    );

                    now.fetch_add(30, Ordering::SeqCst);
                    let current = now.load(Ordering::SeqCst);
                    assert!(matches!(
                        service
                            .login_totp(
                                test_source(4),
                                replacement.challenge,
                                &code_for(&enrolled.secret, current),
                            )
                            .await,
                        Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
                    ));
                    assert_eq!(
                        service.totp_protection_counts(),
                        protection_before_invalid_challenge
                    );
                    assert_eq!(
                        service
                            .login_totp(
                                test_source(3),
                                replacement.challenge,
                                &code_for(&enrolled.secret, current),
                            )
                            .await
                            .map(|_| ()),
                        Ok(())
                    );
                    assert_eq!(service.session_count(), Ok(2));
                    assert!(matches!(
                        service
                            .login_totp(
                                test_source(3),
                                replacement.challenge,
                                &code_for(&enrolled.secret, current),
                            )
                            .await,
                        Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
                    ));
                }
            })
            .await;
    }

    #[test]
    fn manager_totp_challenge_containers_enforce_binding_ttl_attempts_replacement_and_capacity() {
        let test_db_context = TestDbContext::new_sqlite("manager-totp-challenge-containers.sqlite");
        test_db_context.run_sync(|| {
            let service = test_auth_service();
            let now = 1_800_000_000;
            let epoch = uuid::Uuid::from_u128(1);
            let source = test_source(10);

            let first = service
                .create_login_totp_challenge(epoch, source, now)
                .expect("first login challenge should fit");
            assert_eq!(first.challenge.get_version_num(), 4);
            assert_eq!(
                service.validate_login_totp_challenge(
                    first.challenge,
                    uuid::Uuid::from_u128(2),
                    source,
                    now,
                ),
                Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
            );
            assert_eq!(
                service
                    .validate_login_totp_challenge(first.challenge, epoch, test_source(11), now,),
                Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
            );

            let replacement = service
                .create_login_totp_challenge(epoch, source, now)
                .expect("same login context should replace its challenge");
            assert_ne!(first.challenge, replacement.challenge);
            assert_eq!(service.totp_challenge_counts(), (1, 0, 0));
            assert_eq!(
                service.validate_login_totp_challenge(first.challenge, epoch, source, now),
                Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
            );
            for _ in 1..TOTP_CHALLENGE_ATTEMPT_LIMIT {
                assert_eq!(
                    service.record_login_challenge_failure(replacement.challenge),
                    Ok(())
                );
            }
            assert_eq!(
                service.record_login_challenge_failure(replacement.challenge),
                Err(ManagerTotpVerificationError::ChallengeAttemptsExhausted)
            );
            assert_eq!(service.totp_challenge_counts(), (0, 0, 0));

            let expiring = service
                .create_login_totp_challenge(epoch, source, now)
                .expect("expiring challenge should fit");
            assert_eq!(
                service.validate_login_totp_challenge(
                    expiring.challenge,
                    epoch,
                    source,
                    now + LOGIN_TOTP_CHALLENGE_TTL_SEC,
                ),
                Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
            );
            assert_eq!(service.totp_challenge_counts(), (0, 0, 0));

            {
                let mut challenges = service
                    .totp_challenges
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for index in 0..LOGIN_TOTP_CHALLENGE_CAPACITY {
                    challenges.login.insert(
                        uuid::Uuid::new_v4(),
                        super::LoginTotpChallenge {
                            credential_epoch: uuid::Uuid::from_u128(index as u128 + 10),
                            source,
                            expires_at: now + 1_000,
                            attempts: 0,
                        },
                    );
                }
            }
            assert!(matches!(
                service.create_login_totp_challenge(
                    uuid::Uuid::from_u128(10_000),
                    test_source(12),
                    now,
                ),
                Err(LoginPasswordError::Busy)
            ));

            service.clear_all_totp_challenges();
            let setup_first = service
                .create_setup_totp_challenge(ManagerTotpSetupAction::Enroll, 41, epoch, source, now)
                .expect("first setup challenge should fit");
            let setup_replacement = service
                .create_setup_totp_challenge(
                    ManagerTotpSetupAction::Replace,
                    41,
                    epoch,
                    source,
                    now,
                )
                .expect("same session should replace its setup challenge");
            assert_ne!(setup_first.challenge, setup_replacement.challenge);
            assert_eq!(setup_replacement.challenge.get_version_num(), 4);
            assert_eq!(service.totp_challenge_counts(), (0, 1, 0));
            {
                let challenges = service
                    .totp_challenges
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let stored = challenges
                    .setup
                    .get(&setup_replacement.challenge)
                    .expect("replacement setup challenge should remain");
                assert_eq!(stored.action, ManagerTotpSetupAction::Replace);
                assert_eq!(stored.login_instance_id, 41);
                assert_eq!(stored.credential_epoch, epoch);
                assert_eq!(stored.source, source);
                assert_eq!(stored.expires_at, now + SETUP_TOTP_CHALLENGE_TTL_SEC);
            }
            let protection_before_setup_mismatch = service.totp_protection_counts();
            assert!(matches!(
                service.verify_setup_totp_challenge(
                    setup_replacement.challenge,
                    ManagerTotpSetupAction::Enroll,
                    41,
                    epoch,
                    source,
                    "000000",
                    None,
                    now,
                ),
                Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
            ));
            assert_eq!(
                service.totp_protection_counts(),
                protection_before_setup_mismatch
            );
            assert_eq!(service.totp_challenge_counts(), (0, 1, 0));
            for _ in 1..TOTP_CHALLENGE_ATTEMPT_LIMIT {
                assert!(matches!(
                    service.verify_setup_totp_challenge(
                        setup_replacement.challenge,
                        ManagerTotpSetupAction::Replace,
                        41,
                        epoch,
                        source,
                        "abcdef",
                        None,
                        now,
                    ),
                    Err(ManagerTotpVerificationError::Invalid)
                ));
            }
            assert!(matches!(
                service.verify_setup_totp_challenge(
                    setup_replacement.challenge,
                    ManagerTotpSetupAction::Replace,
                    41,
                    epoch,
                    source,
                    "abcdef",
                    None,
                    now,
                ),
                Err(ManagerTotpVerificationError::ChallengeAttemptsExhausted)
            ));
            assert_eq!(service.totp_challenge_counts(), (0, 0, 0));
            assert_eq!(
                service.totp_protection_counts(),
                (1, protection_before_setup_mismatch.1)
            );
            service.clear_totp_failures(source);

            service.clear_all_totp_challenges();
            {
                let mut challenges = service
                    .totp_challenges
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for index in 0..SETUP_TOTP_CHALLENGE_CAPACITY {
                    challenges.setup.insert(
                        uuid::Uuid::new_v4(),
                        super::SetupTotpChallenge {
                            action: ManagerTotpSetupAction::Enroll,
                            login_instance_id: index as i64,
                            credential_epoch: epoch,
                            source,
                            expires_at: now + 1_000,
                            attempts: 0,
                            manual_secret: SensitiveSecret::new("A".repeat(32)),
                        },
                    );
                }
            }
            assert!(matches!(
                service.create_setup_totp_challenge(
                    ManagerTotpSetupAction::Enroll,
                    10_000,
                    epoch,
                    source,
                    now,
                ),
                Err(ManagerTotpVerificationError::Busy)
            ));

            service.clear_all_totp_challenges();
            {
                let mut challenges = service
                    .totp_challenges
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                for _ in 0..RECOVERY_TOTP_CHALLENGE_CAPACITY {
                    challenges.recovery.insert(
                        uuid::Uuid::new_v4(),
                        super::RecoveryTotpChallenge {
                            credential_epoch: epoch,
                            source,
                            expires_at: now + 1_000,
                            attempts: 0,
                            manual_secret: SensitiveSecret::new("A".repeat(32)),
                        },
                    );
                }
            }
            let provisioning =
                generate_manager_totp_provisioning().expect("provisioning should generate");
            let recovery = service
                .create_recovery_totp_challenge(epoch, source, now, &provisioning)
                .expect("new recovery start should replace prior challenges");
            assert_eq!(recovery.challenge.get_version_num(), 4);
            assert_eq!(service.totp_challenge_counts(), (0, 0, 1));
            let protection_before_recovery_mismatch = service.totp_protection_counts();
            assert!(matches!(
                service.verify_recovery_totp_challenge(
                    recovery.challenge,
                    epoch,
                    test_source(13),
                    "000000",
                    0,
                    now,
                ),
                Err(ManagerTotpVerificationError::ChallengeInvalidOrExpired)
            ));
            assert_eq!(
                service.totp_protection_counts(),
                protection_before_recovery_mismatch
            );
            {
                let mut challenges = service
                    .totp_challenges
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                ManagerAuthService::prune_totp_challenges(
                    &mut challenges,
                    now + SETUP_TOTP_CHALLENGE_TTL_SEC,
                );
            }
            assert_eq!(service.totp_challenge_counts(), (0, 0, 0));
        });
    }

    #[tokio::test]
    async fn manager_totp_protection_is_independent_bounded_and_dummy_verifier_is_valid() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-totp-independent-protection.sqlite");
        test_db_context
            .run_async(async {
                let service = test_auth_service();
                let now = 1_800_000_000;
                let locked_source = test_source(20);
                for _ in 0..TOTP_FAILURE_LIMIT {
                    service.record_totp_failure(locked_source, now);
                }
                assert_eq!(
                    service.precheck_totp_source(locked_source, now),
                    Err(ManagerTotpVerificationError::SourceRateLimited { retry_after: 60 })
                );
                assert_eq!(service.login_protection_counts(), (0, 0));

                service.clear_totp_failures(locked_source);
                for index in 0..=TOTP_SOURCE_CAPACITY {
                    service.record_totp_failure(
                        IpAddr::V4(Ipv4Addr::from(0x0a00_0000_u32 + index as u32)),
                        now,
                    );
                }
                assert_eq!(service.totp_protection_counts().0, TOTP_SOURCE_CAPACITY);

                for _ in 0..GLOBAL_TOTP_VERIFICATION_LIMIT {
                    assert_eq!(service.reserve_totp_global_verification(now), Ok(()));
                }
                assert_eq!(
                    service.reserve_totp_global_verification(now),
                    Err(ManagerTotpVerificationError::GlobalRateLimited { retry_after: 60 })
                );
                assert_eq!(service.login_protection_counts(), (0, 0));
                assert_eq!(service.reserve_totp_global_verification(now + 60), Ok(()));

                assert_eq!(
                    PasswordEngine::new()
                        .verify(
                            Zeroizing::new(DUMMY_RECOVERY_CODE.to_string()),
                            Zeroizing::new(DUMMY_RECOVERY_CODE_VERIFIER.to_string()),
                        )
                        .await,
                    Err(PasswordOperationError::IncorrectPassword),
                    "the fixed dummy PHC must be structurally valid and perform Argon2 verification"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn manager_totp_enroll_replace_and_disable_rotate_epoch_sessions_and_recovery_codes() {
        let test_db_context = TestDbContext::new_sqlite("manager-totp-service-lifecycle.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service = test_totp_auth_service(&now, test_secret_encryption());
                    let enrolled =
                        enroll_manager(&service, test_source(30), now.load(Ordering::SeqCst)).await;
                    assert_eq!(enrolled.recovery_codes.len(), 10);
                    assert_eq!(
                        ManagerTotpRecoveryCode::list()
                            .expect("recovery rows should load")
                            .len(),
                        10
                    );
                    assert_eq!(service.session_count(), Ok(1));

                    now.fetch_add(30, Ordering::SeqCst);
                    let replacement_started_at = now.load(Ordering::SeqCst);
                    let old_recovery_rows =
                        ManagerTotpRecoveryCode::list().expect("old recovery rows should load");
                    let replacement = service
                        .start_totp_replacement(
                            &enrolled.access,
                            test_source(30),
                            INITIAL_PASSWORD,
                            &code_for(&enrolled.secret, replacement_started_at),
                        )
                        .await
                        .expect("replacement start should accept the old authenticator");
                    assert_eq!(replacement.expires_in, SETUP_TOTP_CHALLENGE_TTL_SEC as u64);
                    let replacement_secret = replacement.manual_secret.expose().to_string();
                    assert_eq!(
                        ManagerTotpRecoveryCode::list().expect("old recovery rows should remain"),
                        old_recovery_rows,
                        "replacement start must preserve old recovery codes"
                    );
                    assert_eq!(
                        service.session_count(),
                        Ok(1),
                        "replacement start must not rotate sessions"
                    );

                    assert!(matches!(
                        service
                            .confirm_totp_replacement(
                                &enrolled.access,
                                test_source(30),
                                replacement.challenge,
                                &code_for(&replacement_secret, replacement_started_at),
                            )
                            .await,
                        Err(ManagerTotpVerificationError::StepReplayed { .. })
                    ));
                    assert_eq!(service.totp_challenge_counts(), (0, 1, 0));
                    {
                        let challenges = service
                            .totp_challenges
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        assert_eq!(
                            challenges
                                .setup
                                .get(&replacement.challenge)
                                .expect("replayed setup challenge should remain")
                                .attempts,
                            1
                        );
                    }

                    now.fetch_add(30, Ordering::SeqCst);
                    let replaced_at = now.load(Ordering::SeqCst);
                    let replaced = service
                        .confirm_totp_replacement(
                            &enrolled.access,
                            test_source(30),
                            replacement.challenge,
                            &code_for(&replacement_secret, replaced_at),
                        )
                        .await
                        .expect("replacement confirmation should accept the next step");
                    let replaced_access = decode_access_token(&replaced.tokens.access_token)
                        .expect("replacement access should decode");
                    assert_eq!(
                        service.validate_access_context(&enrolled.access),
                        Err(AccessCredentialError::EpochMismatchOrUninitialized)
                    );
                    assert_eq!(service.validate_access_context(&replaced_access), Ok(()));
                    assert_eq!(service.session_count(), Ok(1));
                    assert_eq!(
                        replaced
                            .recovery_codes
                            .as_ref()
                            .expect("replacement should return new recovery codes")
                            .expose()
                            .count(),
                        10
                    );
                    let new_recovery_rows =
                        ManagerTotpRecoveryCode::list().expect("new recovery rows should load");
                    assert_eq!(new_recovery_rows.len(), 10);
                    assert_ne!(
                        new_recovery_rows, old_recovery_rows,
                        "replacement must replace the complete recovery-code set"
                    );
                    for row in &new_recovery_rows {
                        assert!(
                            !replaced
                                .recovery_codes
                                .as_ref()
                                .expect("replacement codes should remain in memory")
                                .expose()
                                .any(|code| row.code_verifier.contains(code)),
                            "persisted rows must contain only Argon2 verifiers"
                        );
                    }

                    now.fetch_add(30, Ordering::SeqCst);
                    let disabled = service
                        .disable_totp(
                            &replaced_access,
                            test_source(30),
                            INITIAL_PASSWORD,
                            &code_for(&replacement_secret, now.load(Ordering::SeqCst)),
                        )
                        .await
                        .expect("disable should accept password and current TOTP");
                    let disabled_access = decode_access_token(&disabled.tokens.access_token)
                        .expect("disabled access should decode");
                    assert_eq!(disabled.state, ManagerTotpPublicState::Disabled);
                    assert!(disabled.recovery_codes.is_none());
                    assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 0);
                    assert_eq!(service.session_count(), Ok(1));
                    assert_eq!(
                        service.validate_access_context(&replaced_access),
                        Err(AccessCredentialError::EpochMismatchOrUninitialized)
                    );
                    assert_eq!(service.validate_access_context(&disabled_access), Ok(()));
                    assert_eq!(service.totp_status(), Ok(ManagerTotpPublicState::Disabled));
                    assert!(matches!(
                        service
                            .login_password(test_source(31), INITIAL_PASSWORD)
                            .await,
                        Ok(LoginPasswordResult::Authenticated(_))
                    ));
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_totp_unavailable_keeps_sessions_operational_and_recovery_rebinds() {
        let test_db_context = TestDbContext::new_sqlite("manager-totp-unavailable-recovery.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let secret_encryption = test_secret_encryption();
                    let service =
                        test_totp_auth_service(&now, Arc::clone(&secret_encryption));
                    let enrolled = enroll_manager(
                        &service,
                        test_source(40),
                        now.load(Ordering::SeqCst),
                    )
                    .await;
                    let stored_before = ManagerCredential::load()
                        .expect("credential should load")
                        .expect("credential should exist");
                    let stored_step = stored_before
                        .totp_last_accepted_step
                        .expect("enrolled credential should have a step");

                    let pending = match service
                        .login_password(test_source(41), INITIAL_PASSWORD)
                        .await
                        .expect("enabled password stage should create a challenge")
                    {
                        LoginPasswordResult::TotpRequired(challenge) => challenge,
                        LoginPasswordResult::Authenticated(_) => {
                            panic!("enabled password stage must not create a session")
                        }
                    };
                    let protection_before_invalid_code =
                        service.totp_protection_counts();
                    assert!(matches!(
                        service
                            .login_totp(test_source(41), pending.challenge, "abcdef")
                            .await,
                        Err(ManagerTotpVerificationError::Invalid)
                    ));
                    assert_eq!(service.totp_challenge_counts(), (1, 0, 0));
                    assert_eq!(
                        service.totp_protection_counts(),
                        (1, protection_before_invalid_code.1),
                        "format errors count toward the source and challenge, not global crypto"
                    );

                    let mut conn = get_connection().expect("connection should load");
                    match &mut conn {
                        DbConnection::Postgres(conn) => {
                            diesel::sql_query(
                                "UPDATE manager_credential SET totp_secret_ciphertext = $1 WHERE manager_id = 0",
                            )
                            .bind::<diesel::sql_types::Binary, _>(vec![1_u8])
                            .execute(conn)
                            .expect("PostgreSQL ciphertext corruption should succeed");
                        }
                        DbConnection::Sqlite(conn) => {
                            diesel::sql_query(
                                "UPDATE manager_credential SET totp_secret_ciphertext = ? WHERE manager_id = 0",
                            )
                            .bind::<diesel::sql_types::Binary, _>(vec![1_u8])
                            .execute(conn)
                            .expect("SQLite ciphertext corruption should succeed");
                        }
                    }
                    drop(conn);

                    let restarted =
                        test_totp_auth_service(&now, Arc::clone(&secret_encryption));
                    assert_eq!(
                        restarted.totp_challenge_counts(),
                        (0, 0, 0),
                        "restart must clear in-memory challenges"
                    );
                    assert_eq!(
                        restarted.totp_protection_counts(),
                        (0, 0),
                        "restart must clear in-memory TOTP protection"
                    );
                    assert_eq!(
                        ManagerCredential::load()
                            .unwrap()
                            .unwrap()
                            .totp_last_accepted_step,
                        Some(stored_step),
                        "restart must preserve the persisted replay watermark"
                    );
                    assert_eq!(restarted.session_count(), Ok(1));

                    let rebuilt_access = restarted
                        .access_for_session(
                            enrolled.access.login_instance_id,
                            enrolled.access.credential_epoch,
                        )
                        .await
                        .expect("existing session should recover an access token");
                    let rebuilt_access = decode_access_token(&rebuilt_access)
                        .expect("recovered access should decode");
                    assert_eq!(restarted.validate_access_context(&rebuilt_access), Ok(()));
                    assert_eq!(
                        restarted.totp_status(),
                        Ok(ManagerTotpPublicState::Unavailable {
                            enabled_at: Some(1_800_000_000)
                        })
                    );
                    assert!(matches!(
                        restarted
                            .login_password(test_source(42), INITIAL_PASSWORD)
                            .await,
                        Err(LoginPasswordError::ManagerTotpUnavailable)
                    ));
                    assert_eq!(restarted.session_count(), Ok(1));
                    assert_eq!(
                        restarted.verify_sensitive_totp(
                            &rebuilt_access,
                            test_source(42),
                            Some("000000"),
                        ),
                        Err(ManagerTotpVerificationError::Unavailable)
                    );

                    let existing_ids = ManagerTotpRecoveryCode::list()
                        .expect("recovery rows should load")
                        .into_iter()
                        .map(|row| row.code_id)
                        .collect::<Vec<_>>();
                    let missing_id = (0..10_000)
                        .map(|value| format!("{value:04}"))
                        .find(|candidate| !existing_ids.contains(candidate))
                        .expect("ten rows cannot exhaust four decimal characters");
                    let missing_recovery_code = format!("{missing_id}{}", "0".repeat(16));
                    assert!(matches!(
                        restarted
                            .start_totp_recovery(
                                test_source(43),
                                INITIAL_PASSWORD,
                                &missing_recovery_code,
                            )
                            .await,
                        Err(ManagerTotpVerificationError::RecoveryCredentialsInvalid)
                    ));
                    assert_eq!(restarted.session_count(), Ok(1));
                    assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 10);

                    let recovery = restarted
                        .start_totp_recovery(
                            test_source(43),
                            INITIAL_PASSWORD,
                            &enrolled.recovery_codes[0],
                        )
                        .await
                        .expect("a recovery code must recover an unavailable TOTP secret");
                    assert_eq!(restarted.session_count(), Ok(0));
                    assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 9);
                    let normalized_consumed = enrolled.recovery_codes[0].replace('-', "");
                    assert!(
                        ManagerTotpRecoveryCode::load_by_code_id(&normalized_consumed[..4])
                            .unwrap()
                            .is_none(),
                        "recovery start must consume exactly the submitted row"
                    );
                    let during_recovery = ManagerCredential::load().unwrap().unwrap();
                    assert_eq!(during_recovery.credential_epoch, stored_before.credential_epoch);
                    assert_eq!(during_recovery.totp_last_accepted_step, Some(stored_step));
                    assert_eq!(during_recovery.totp_secret_ciphertext, Some(vec![1_u8]));

                    now.fetch_add(30, Ordering::SeqCst);
                    let recovery_secret = recovery.manual_secret.expose().to_string();
                    let recovered = restarted
                        .confirm_totp_recovery(
                            test_source(43),
                            recovery.challenge,
                            &code_for(&recovery_secret, now.load(Ordering::SeqCst)),
                        )
                        .await
                        .expect("recovery confirmation should install a new TOTP tuple");
                    let recovered_access = decode_access_token(&recovered.tokens.access_token)
                        .expect("recovery access should decode");
                    assert_eq!(restarted.session_count(), Ok(1));
                    assert_eq!(
                        restarted.validate_access_context(&recovered_access),
                        Ok(())
                    );
                    assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 10);
                    assert!(matches!(
                        restarted.totp_status(),
                        Ok(ManagerTotpPublicState::Enabled { .. })
                    ));
                    let stored_after = ManagerCredential::load().unwrap().unwrap();
                    assert_ne!(stored_after.credential_epoch, stored_before.credential_epoch);
                    assert_ne!(stored_after.totp_secret_ciphertext, Some(vec![1_u8]));
                    assert!(
                        stored_after.totp_last_accepted_step.unwrap() > stored_step,
                        "recovery confirmation must install a strictly newer step"
                    );

                    let final_restart =
                        test_totp_auth_service(&now, Arc::clone(&secret_encryption));
                    assert_eq!(final_restart.totp_challenge_counts(), (0, 0, 0));
                    assert_eq!(final_restart.totp_protection_counts(), (0, 0));
                    assert!(matches!(
                        final_restart.totp_status(),
                        Ok(ManagerTotpPublicState::Enabled { .. })
                    ));
                    assert_eq!(ManagerTotpRecoveryCode::list().unwrap().len(), 10);
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_totp_same_step_login_and_sensitive_verification_have_one_winner() {
        let test_db_context = TestDbContext::new_sqlite("manager-totp-cross-scenario-cas.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                let spawn_context = test_db_context.clone();
                async move {
                    let service = Arc::new(test_totp_auth_service(&now, test_secret_encryption()));
                    let enrolled =
                        enroll_manager(&service, test_source(50), now.load(Ordering::SeqCst)).await;
                    now.fetch_add(30, Ordering::SeqCst);
                    let current = now.load(Ordering::SeqCst);
                    let challenge = match service
                        .login_password(test_source(51), INITIAL_PASSWORD)
                        .await
                        .expect("password stage should create a challenge")
                    {
                        LoginPasswordResult::TotpRequired(challenge) => challenge,
                        LoginPasswordResult::Authenticated(_) => {
                            panic!("enabled password stage must not authenticate")
                        }
                    };
                    let code = code_for(&enrolled.secret, current);
                    let barrier = Arc::new(Barrier::new(3));
                    let login = {
                        let service = Arc::clone(&service);
                        let barrier = Arc::clone(&barrier);
                        let code = code.clone();
                        spawn_context.spawn(async move {
                            barrier.wait().await;
                            service
                                .login_totp(test_source(51), challenge.challenge, &code)
                                .await
                                .map(|_| ManagerTotpSensitiveVerification::Verified)
                        })
                    };
                    let sensitive = {
                        let service = Arc::clone(&service);
                        let barrier = Arc::clone(&barrier);
                        let access = enrolled.access.clone();
                        spawn_context.spawn(async move {
                            barrier.wait().await;
                            service.verify_sensitive_totp(&access, test_source(52), Some(&code))
                        })
                    };

                    barrier.wait().await;
                    let results = [
                        login.await.expect("login task should join"),
                        sensitive.await.expect("sensitive task should join"),
                    ];
                    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
                    assert_eq!(
                        results
                            .iter()
                            .filter(|result| {
                                matches!(
                                    result,
                                    Err(ManagerTotpVerificationError::StepReplayed { .. })
                                        | Err(ManagerTotpVerificationError::StepStale { .. })
                                )
                            })
                            .count(),
                        1
                    );
                    let stored = ManagerCredential::load().unwrap().unwrap();
                    assert!(
                        stored.totp_last_accepted_step.unwrap()
                            > 1_800_000_000 / super::MANAGER_TOTP_PERIOD_SEC
                    );
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_secret_governance_password_reauth_is_fixed_non_sliding_and_session_bound() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-secret-governance-password-reauth.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service = test_totp_auth_service(&now, test_secret_encryption());
                    let bootstrapped = service
                        .bootstrap(INITIAL_PASSWORD)
                        .await
                        .expect("bootstrap should establish the first session");
                    let access = decode_access_token(&bootstrapped.access_token)
                        .expect("bootstrap access should decode");
                    let initial = bootstrapped
                        .reauth
                        .expect("bootstrap should seed password reauthentication");
                    assert_eq!(initial.evidence, ManagerReauthEvidence::Password);
                    assert_eq!(
                        initial.verified_until,
                        now.load(Ordering::SeqCst) + SECRET_GOVERNANCE_REAUTH_TTL_SEC
                    );
                    assert_eq!(service.authorize_secret_governance(&access), Ok(initial));

                    now.fetch_add(SECRET_GOVERNANCE_REAUTH_TTL_SEC - 1, Ordering::SeqCst);
                    assert_eq!(
                        service.authorize_secret_governance(&access),
                        Ok(initial),
                        "authorization checks must not slide the expiry"
                    );
                    now.fetch_add(1, Ordering::SeqCst);
                    assert_eq!(
                        service.authorize_secret_governance(&access),
                        Err(ManagerTotpVerificationError::ReauthRequired)
                    );
                    assert_eq!(
                        service
                            .reauthenticate_secret_governance(
                                &access,
                                test_source(57),
                                ManagerReauthCredential::Totp("000000"),
                            )
                            .await,
                        Err(ManagerTotpVerificationError::ReauthMethodChanged)
                    );

                    let renewed = service
                        .reauthenticate_secret_governance(
                            &access,
                            test_source(57),
                            ManagerReauthCredential::Password(INITIAL_PASSWORD),
                        )
                        .await
                        .expect("disabled TOTP should require the current password");
                    assert_eq!(renewed.evidence, ManagerReauthEvidence::Password);
                    assert_eq!(service.authorize_secret_governance(&access), Ok(renewed));
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_secret_governance_totp_reauth_consumes_once_and_is_lost_on_restart() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-secret-governance-totp-reauth.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let secret_encryption = test_secret_encryption();
                    let service = test_totp_auth_service(&now, Arc::clone(&secret_encryption));
                    let enrolled =
                        enroll_manager(&service, test_source(58), now.load(Ordering::SeqCst)).await;
                    now.fetch_add(SECRET_GOVERNANCE_REAUTH_TTL_SEC, Ordering::SeqCst);
                    assert_eq!(
                        service.authorize_secret_governance(&enrolled.access),
                        Err(ManagerTotpVerificationError::ReauthRequired)
                    );
                    assert_eq!(
                        service
                            .reauthenticate_secret_governance(
                                &enrolled.access,
                                test_source(58),
                                ManagerReauthCredential::Password(INITIAL_PASSWORD),
                            )
                            .await,
                        Err(ManagerTotpVerificationError::ReauthMethodChanged)
                    );

                    let code = code_for(&enrolled.secret, now.load(Ordering::SeqCst));
                    let grant = service
                        .reauthenticate_secret_governance(
                            &enrolled.access,
                            test_source(58),
                            ManagerReauthCredential::Totp(&code),
                        )
                        .await
                        .expect("enabled TOTP should establish the governance window");
                    assert_eq!(grant.evidence, ManagerReauthEvidence::Totp);
                    assert_eq!(
                        service.authorize_secret_governance(&enrolled.access),
                        Ok(grant)
                    );
                    assert_eq!(
                        service.authorize_secret_governance(&enrolled.access),
                        Ok(grant),
                        "multiple commands in one TOTP step must share the window"
                    );
                    assert!(matches!(
                        service
                            .reauthenticate_secret_governance(
                                &enrolled.access,
                                test_source(58),
                                ManagerReauthCredential::Totp(&code),
                            )
                            .await,
                        Err(ManagerTotpVerificationError::StepReplayed { .. })
                    ));

                    let restarted = test_totp_auth_service(&now, Arc::clone(&secret_encryption));
                    let rebuilt_access = restarted
                        .access_for_session(
                            enrolled.access.login_instance_id,
                            enrolled.access.credential_epoch,
                        )
                        .await
                        .expect("the persisted session should survive restart");
                    let rebuilt_access =
                        decode_access_token(&rebuilt_access).expect("rebuilt access should decode");
                    assert_eq!(
                        restarted.authorize_secret_governance(&rebuilt_access),
                        Err(ManagerTotpVerificationError::ReauthRequired),
                        "the in-memory governance grant must not survive restart"
                    );
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_totp_stale_cas_result_cannot_replace_rotated_epoch_snapshot() {
        let test_db_context = TestDbContext::new_sqlite("manager-totp-stale-cas-install.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service = test_totp_auth_service(&now, test_secret_encryption());
                    let enrolled =
                        enroll_manager(&service, test_source(54), now.load(Ordering::SeqCst)).await;

                    now.fetch_add(30, Ordering::SeqCst);
                    let stale_step = now.load(Ordering::SeqCst) / super::MANAGER_TOTP_PERIOD_SEC;
                    let stale_credential = ManagerCredential::advance_totp_step_if_newer(
                        &enrolled.access.credential_epoch.to_string(),
                        stale_step,
                        now.load(Ordering::SeqCst),
                    )
                    .expect("the simulated sensitive CAS should advance the old epoch");

                    now.fetch_add(30, Ordering::SeqCst);
                    let rotated = service
                        .rotate_password(
                            &enrolled.access,
                            test_source(54),
                            Some(&code_for(&enrolled.secret, now.load(Ordering::SeqCst))),
                            INITIAL_PASSWORD,
                            ROTATED_PASSWORD,
                        )
                        .await
                        .expect("password rotation should install a newer epoch");
                    let rotated_access = decode_access_token(&rotated.access_token)
                        .expect("rotated access should decode");

                    assert_eq!(
                        service.install_totp_step_snapshot_if_current(
                            stale_credential,
                            enrolled.access.credential_epoch,
                            stale_step,
                        ),
                        Err(ManagerTotpVerificationError::StateConflict),
                        "a late sensitive verification result must not replace the rotated snapshot"
                    );
                    assert_eq!(
                        service.validate_access_context(&rotated_access),
                        Ok(()),
                        "the new session must remain valid after the stale install attempt"
                    );
                    assert!(matches!(
                        service
                            .login_password(test_source(55), INITIAL_PASSWORD)
                            .await,
                        Err(LoginPasswordError::InvalidPassword)
                    ));
                    assert!(matches!(
                        service
                            .login_password(test_source(56), ROTATED_PASSWORD)
                            .await,
                        Ok(LoginPasswordResult::TotpRequired(_))
                    ));
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_totp_future_step_requires_waiting_past_persisted_watermark() {
        let test_db_context = TestDbContext::new_sqlite("manager-totp-future-step.sqlite");
        let now = Arc::new(AtomicI64::new(1_800_000_000));
        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service = test_totp_auth_service(&now, test_secret_encryption());
                    let enrolled =
                        enroll_manager(&service, test_source(53), now.load(Ordering::SeqCst)).await;

                    now.fetch_add(30, Ordering::SeqCst);
                    let current = now.load(Ordering::SeqCst);
                    let future = current + super::MANAGER_TOTP_PERIOD_SEC;
                    assert_eq!(
                        service.verify_sensitive_totp(
                            &enrolled.access,
                            test_source(53),
                            Some(&code_for(&enrolled.secret, future)),
                        ),
                        Ok(ManagerTotpSensitiveVerification::Verified),
                        "the +1 drift window may accept the future step"
                    );
                    let future_step = future / super::MANAGER_TOTP_PERIOD_SEC;
                    assert_eq!(
                        ManagerCredential::load()
                            .unwrap()
                            .unwrap()
                            .totp_last_accepted_step,
                        Some(future_step)
                    );

                    assert_eq!(
                        service.verify_sensitive_totp(
                            &enrolled.access,
                            test_source(53),
                            Some(&code_for(&enrolled.secret, current)),
                        ),
                        Err(ManagerTotpVerificationError::StepStale { retry_after: 30 })
                    );
                    now.fetch_add(30, Ordering::SeqCst);
                    assert_eq!(
                        service.verify_sensitive_totp(
                            &enrolled.access,
                            test_source(53),
                            Some(&code_for(&enrolled.secret, now.load(Ordering::SeqCst),)),
                        ),
                        Err(ManagerTotpVerificationError::StepReplayed { retry_after: 30 }),
                        "reaching the accepted future step is still a replay"
                    );

                    now.fetch_add(30, Ordering::SeqCst);
                    assert_eq!(
                        service.verify_sensitive_totp(
                            &enrolled.access,
                            test_source(53),
                            Some(&code_for(&enrolled.secret, now.load(Ordering::SeqCst),)),
                        ),
                        Ok(ManagerTotpSensitiveVerification::Verified),
                        "verification resumes only after the persisted watermark"
                    );
                    assert_eq!(
                        ManagerCredential::load()
                            .unwrap()
                            .unwrap()
                            .totp_last_accepted_step,
                        Some(future_step + 1)
                    );
                }
            })
            .await;
    }

    #[test]
    fn manager_totp_debug_contract_redacts_challenges_secrets_uris_and_recovery_codes() {
        let challenge = uuid::Uuid::parse_str("018fa7d8-6a00-7c9a-8f7e-123456789abc")
            .expect("sentinel challenge should parse");
        let login = ManagerLoginTotpChallenge {
            challenge,
            expires_in: 300,
        };
        let setup = super::ManagerTotpSetup {
            challenge,
            manual_secret: SensitiveSecret::new("SENTINELTOTPSECRET23456789012345".to_string()),
            otpauth_uri: SensitiveSecret::new(
                "otpauth://totp/Cyder:sentinel?secret=SENTINEL".to_string(),
            ),
            expires_in: 600,
        };
        let recovery = super::ManagerTotpRecoveryCodes(vec![SensitiveSecret::new(
            "ABCD-SENTINELRECOVERY".to_string(),
        )]);
        let debug_output = format!("{login:?} {setup:?} {recovery:?}");

        for forbidden in [
            challenge.to_string(),
            "SENTINELTOTPSECRET23456789012345".to_string(),
            "otpauth://totp/Cyder:sentinel?secret=SENTINEL".to_string(),
            "ABCD-SENTINELRECOVERY".to_string(),
        ] {
            assert!(
                !debug_output.contains(&forbidden),
                "sensitive Debug output must not contain {forbidden}"
            );
        }
        assert_eq!(
            debug_output,
            "ManagerLoginTotpChallenge(<redacted>) \
             ManagerTotpSetup(<redacted>) ManagerTotpRecoveryCodes(<redacted>)"
        );
    }

    #[tokio::test]
    async fn manager_auth_session_registry_tracks_mutations_and_restart_state() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-session-registry.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                let bootstrapped = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should create the first cached session");
                let bootstrap_access = decode_access_token(&bootstrapped.access_token)
                    .expect("bootstrap access should decode");
                assert_eq!(service.session_count(), Ok(1));
                assert_eq!(
                    service.validate_session(
                        bootstrap_access.login_instance_id,
                        bootstrap_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                        &bootstrap_access.access_jti,
                    ),
                    Ok(())
                );

                let login = service
                    .login(test_source(1), INITIAL_PASSWORD)
                    .await
                    .expect("login should create a second cached session");
                let login_access =
                    decode_access_token(&login.access_token).expect("login access should decode");
                assert_eq!(service.session_count(), Ok(2));

                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DELETE FROM manager_auth_instance WHERE id = $1")
                            .bind::<diesel::sql_types::BigInt, _>(
                                bootstrap_access.login_instance_id,
                            )
                            .execute(conn)
                            .expect("manual session delete should succeed");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DELETE FROM manager_auth_instance WHERE id = ?")
                            .bind::<diesel::sql_types::BigInt, _>(
                                bootstrap_access.login_instance_id,
                            )
                            .execute(conn)
                            .expect("manual session delete should succeed");
                    }
                }

                assert_eq!(
                    service.validate_session(
                        bootstrap_access.login_instance_id,
                        bootstrap_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                        &bootstrap_access.access_jti,
                    ),
                    Ok(()),
                    "manual database changes require restart before affecting memory"
                );
                let restarted = test_auth_service();
                assert_eq!(restarted.session_count(), Ok(1));
                assert_eq!(
                    restarted.validate_session(
                        bootstrap_access.login_instance_id,
                        bootstrap_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                        &bootstrap_access.access_jti,
                    ),
                    Err(super::SessionValidationError::Invalid)
                );
                assert_eq!(
                    restarted.validate_session(
                        login_access.login_instance_id,
                        login_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                        &login_access.access_jti,
                    ),
                    Err(super::SessionValidationError::Invalid),
                    "restart intentionally clears the access cache"
                );
                let rebuilt_access = restarted
                    .access_for_session(
                        login_access.login_instance_id,
                        login_access.credential_epoch,
                    )
                    .await
                    .expect("first access after restart should rebuild the cache");
                let rebuilt_access =
                    decode_access_token(&rebuilt_access).expect("rebuilt access should decode");
                assert_eq!(restarted.validate_access_context(&rebuilt_access), Ok(()));

                let rotated = service
                    .rotate_password(
                        &bootstrap_access,
                        test_source(1),
                        None,
                        INITIAL_PASSWORD,
                        ROTATED_PASSWORD,
                    )
                    .await
                    .expect("password rotation should replace cached sessions");
                let rotated_access = decode_access_token(&rotated.access_token)
                    .expect("rotated access should decode");
                assert_eq!(service.session_count(), Ok(1));
                assert_eq!(
                    service.validate_session(
                        rotated_access.login_instance_id,
                        rotated_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                        &rotated_access.access_jti,
                    ),
                    Ok(())
                );
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_session_registry_failure_degrades_only_manager_auth() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-auth-session-registry-unavailable.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                let bootstrapped = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should create a valid credential and session");
                let access = decode_access_token(&bootstrapped.access_token)
                    .expect("bootstrap access should decode");
                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("session table should drop");
                    }
                }
                drop(conn);

                let app_state = create_test_app_state(test_db_context.clone()).await;
                assert_eq!(
                    app_state.admin.auth.session_count(),
                    Err(super::SessionValidationError::Unavailable)
                );
                assert!(matches!(
                    app_state
                        .admin
                        .auth
                        .login(test_source(1), INITIAL_PASSWORD)
                        .await,
                    Err(LoginError::Unavailable)
                ));
                assert_eq!(
                    app_state.admin.auth.validate_access_context(&access),
                    Err(AccessCredentialError::SessionUnavailable)
                );
                assert!(
                    app_state.max_body_size > 0,
                    "proxy app state must remain available"
                );
            })
            .await;
    }

    #[test]
    fn manager_auth_session_registry_startup_cleans_expired_rows() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-auth-session-registry-cleanup.sqlite");

        test_db_context.run_sync(|| {
            let expired = ManagerAuthInstance::create_instance(
                "expired".to_string(),
                super::manager_jwt_key_id().to_string(),
                uuid::Uuid::new_v4().to_string(),
                1,
                10,
                20,
            )
            .expect("expired fixture should create");
            let service = ManagerAuthService::new_for_test(Arc::new(|| 11));
            assert_eq!(service.session_count(), Ok(0));
            assert!(
                ManagerAuthInstance::get_instance(expired.id)
                    .expect("expired lookup should query")
                    .is_none(),
                "startup cleanup should physically delete expired sessions"
            );
        });
    }

    #[tokio::test]
    async fn manager_access_context_rejects_wrong_version_and_revoked_session() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-auth-access-session-validation.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                let bootstrapped = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let access = decode_access_token(&bootstrapped.access_token)
                    .expect("bootstrap access should decode");
                assert_eq!(service.validate_access_context(&access), Ok(()));

                let mut wrong_version = access.clone();
                wrong_version.session_version += 1;
                assert_eq!(
                    service.validate_access_context(&wrong_version),
                    Err(AccessCredentialError::SessionInvalid)
                );

                service
                    .logout_session(access.login_instance_id, access.credential_epoch)
                    .await
                    .expect("logout should revoke the current session");
                assert_eq!(
                    service.validate_access_context(&access),
                    Err(AccessCredentialError::SessionInvalid)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_refresh_rotation_has_one_winner_and_detects_replay() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-refresh-replay.sqlite");

        test_db_context
            .run_async(async {
                let service = Arc::new(test_auth_service());
                let initial = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let initial_access = decode_access_token(&initial.access_token)
                    .expect("initial access should decode");
                let barrier = Arc::new(Barrier::new(3));
                let first = {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    let refresh_token = initial.refresh_token.clone();
                    test_db_context.spawn(async move {
                        barrier.wait().await;
                        service.refresh(&refresh_token).await
                    })
                };
                let second = {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    let refresh_token = initial.refresh_token.clone();
                    test_db_context.spawn(async move {
                        barrier.wait().await;
                        service.refresh(&refresh_token).await
                    })
                };

                barrier.wait().await;
                let results = [
                    first.await.expect("first refresh task should join"),
                    second.await.expect("second refresh task should join"),
                ];
                assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
                assert_eq!(
                    results
                        .iter()
                        .filter(|result| matches!(result, Err(RefreshError::Replay)))
                        .count(),
                    1
                );
                let winner = results
                    .iter()
                    .find_map(|result| result.as_ref().ok())
                    .expect("one refresh should win");
                let winner_access =
                    decode_access_token(&winner.access_token).expect("winner access should decode");
                assert_eq!(winner_access.session_version, INITIAL_SESSION_VERSION);
                assert_eq!(
                    service.validate_access_context(&winner_access),
                    Err(AccessCredentialError::SessionInvalid),
                    "a valid stale internal refresh is an integrity fault and revokes the family"
                );
                assert_eq!(
                    service.validate_access_context(&initial_access),
                    Err(AccessCredentialError::SessionInvalid)
                );
                assert_eq!(service.session_count(), Ok(0));
                assert!(matches!(
                    service.refresh(&initial.refresh_token).await,
                    Err(RefreshError::Invalid)
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_access_singleflight_returns_one_rotated_access_to_all_waiters() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-access-singleflight.sqlite");
        let now = Arc::new(AtomicI64::new(crate::utils::auth::get_current_timestamp()));

        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                let spawn_context = test_db_context.clone();
                async move {
                    let service_now = Arc::clone(&now);
                    let service = Arc::new(ManagerAuthService::new_for_test(Arc::new(move || {
                        service_now.load(Ordering::SeqCst)
                    })));
                    let initial = service
                        .bootstrap(INITIAL_PASSWORD)
                        .await
                        .expect("bootstrap should succeed");
                    let initial_access = decode_access_token(&initial.access_token)
                        .expect("initial access should decode");
                    assert_eq!(
                        service
                            .access_for_session(
                                initial_access.login_instance_id,
                                initial_access.credential_epoch,
                            )
                            .await
                            .expect("fresh cache should return"),
                        initial.access_token,
                        "fresh access must be returned without rotation"
                    );

                    now.fetch_add(ACCESS_TOKEN_ISSUE_SEC - 30, Ordering::SeqCst);
                    let barrier = Arc::new(Barrier::new(3));
                    let mut handles = Vec::new();
                    let login_instance_id = initial_access.login_instance_id;
                    let credential_epoch = initial_access.credential_epoch;
                    for _ in 0..2 {
                        let service = Arc::clone(&service);
                        let barrier = Arc::clone(&barrier);
                        handles.push(spawn_context.spawn(async move {
                            barrier.wait().await;
                            service
                                .access_for_session(login_instance_id, credential_epoch)
                                .await
                        }));
                    }
                    barrier.wait().await;
                    let first = handles
                        .remove(0)
                        .await
                        .expect("first access task should join")
                        .expect("first access should succeed");
                    let second = handles
                        .remove(0)
                        .await
                        .expect("second access task should join")
                        .expect("second access should succeed");
                    assert_eq!(first, second, "all waiters must receive one cached access");
                    assert_ne!(first, initial.access_token);

                    let persisted =
                        ManagerAuthInstance::get_instance(initial_access.login_instance_id)
                            .expect("session lookup should succeed")
                            .expect("session should remain active");
                    assert_eq!(persisted.refresh_generation, 2);
                    assert_eq!(persisted.session_version, INITIAL_SESSION_VERSION);
                    let rotated_access =
                        decode_access_token(&first).expect("rotated access should decode");
                    assert_eq!(service.validate_access_context(&rotated_access), Ok(()));
                    assert_eq!(
                        service.validate_access_context(&initial_access),
                        Err(AccessCredentialError::SessionInvalid)
                    );
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_access_cache_degrades_only_until_cached_access_expires() {
        let test_db_context =
            TestDbContext::new_sqlite("manager-auth-access-cache-storage-failure.sqlite");
        let now = Arc::new(AtomicI64::new(crate::utils::auth::get_current_timestamp()));

        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service_now = Arc::clone(&now);
                    let service = ManagerAuthService::new_for_test(Arc::new(move || {
                        service_now.load(Ordering::SeqCst)
                    }));
                    let initial = service
                        .bootstrap(INITIAL_PASSWORD)
                        .await
                        .expect("bootstrap should succeed");
                    let access = decode_access_token(&initial.access_token)
                        .expect("initial access should decode");

                    let mut conn = get_connection().expect("connection should load");
                    match &mut conn {
                        DbConnection::Postgres(conn) => {
                            diesel::sql_query("DROP TABLE manager_auth_instance")
                                .execute(conn)
                                .expect("session table should drop");
                        }
                        DbConnection::Sqlite(conn) => {
                            diesel::sql_query("DROP TABLE manager_auth_instance")
                                .execute(conn)
                                .expect("session table should drop");
                        }
                    }
                    drop(conn);

                    assert_eq!(
                        service
                            .access_for_session(access.login_instance_id, access.credential_epoch)
                            .await
                            .expect("fresh cache must not need storage"),
                        initial.access_token
                    );
                    now.fetch_add(ACCESS_TOKEN_ISSUE_SEC - 30, Ordering::SeqCst);
                    assert_eq!(
                        service
                            .access_for_session(access.login_instance_id, access.credential_epoch)
                            .await
                            .expect("unexpired fallback should survive storage failure"),
                        initial.access_token
                    );
                    now.fetch_add(31, Ordering::SeqCst);
                    assert_eq!(
                        service
                            .access_for_session(access.login_instance_id, access.credential_epoch)
                            .await,
                        Err(AccessTokenError::Storage)
                    );
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_logout_all_revokes_every_active_session_and_clears_registry() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-logout-all.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                let first = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let second = service
                    .login(test_source(1), INITIAL_PASSWORD)
                    .await
                    .expect("login should create another session");
                let first_access =
                    decode_access_token(&first.access_token).expect("first access should decode");
                let second_access =
                    decode_access_token(&second.access_token).expect("second access should decode");

                assert_eq!(
                    service
                        .logout_all(&second_access, test_source(94), INITIAL_PASSWORD, None)
                        .await,
                    Ok(2)
                );
                assert_eq!(service.session_count(), Ok(0));
                assert_eq!(
                    service.validate_access_context(&first_access),
                    Err(AccessCredentialError::SessionInvalid)
                );
                assert_eq!(
                    service.validate_access_context(&second_access),
                    Err(AccessCredentialError::SessionInvalid)
                );

                for session_id in [
                    first_access.login_instance_id,
                    second_access.login_instance_id,
                ] {
                    let session = ManagerAuthInstance::get_instance(session_id)
                        .expect("revoked session lookup should query")
                        .expect("revoked session should remain until expiry");
                    assert!(session.revoked_at.is_some());
                    assert_eq!(session.revoked_reason.as_deref(), Some("logout_all"));
                }
                assert!(matches!(
                    service.refresh(&first.refresh_token).await,
                    Err(RefreshError::Invalid)
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_service_bootstrap_login_rotate_and_epoch_invalidation() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-service-lifecycle.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                assert_eq!(
                    service.bootstrap_status().expect("status should load"),
                    BootstrapStatus::Uninitialized
                );

                let bootstrapped = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                assert_eq!(
                    service.bootstrap_status().expect("status should load"),
                    BootstrapStatus::Ready
                );
                let initial_access = decode_access_token(&bootstrapped.access_token)
                    .expect("bootstrap access should decode");
                let initial_refresh = decode_refresh_token(&bootstrapped.refresh_token)
                    .expect("bootstrap refresh should decode");
                assert_eq!(
                    initial_access.credential_epoch,
                    initial_refresh.credential_epoch
                );

                let login_tokens = service
                    .login(test_source(1), INITIAL_PASSWORD)
                    .await
                    .expect("initialized password should login");
                let rotated = service
                    .rotate_password(
                        &initial_access,
                        test_source(1),
                        None,
                        INITIAL_PASSWORD,
                        ROTATED_PASSWORD,
                    )
                    .await
                    .expect("rotation should succeed");
                let rotated_access = decode_access_token(&rotated.access_token)
                    .expect("rotated access should decode");
                assert_ne!(
                    initial_access.credential_epoch,
                    rotated_access.credential_epoch
                );
                assert_eq!(
                    service.validate_access_context(&initial_access),
                    Err(AccessCredentialError::EpochMismatchOrUninitialized)
                );
                assert!(matches!(
                    service.refresh(&bootstrapped.refresh_token).await,
                    Err(RefreshError::EpochMismatch)
                ));
                assert!(matches!(
                    service.refresh(&login_tokens.refresh_token).await,
                    Err(RefreshError::EpochMismatch)
                ));
                assert!(matches!(
                    service.login(test_source(1), INITIAL_PASSWORD).await,
                    Err(LoginError::InvalidPassword)
                ));
                service
                    .login(test_source(1), ROTATED_PASSWORD)
                    .await
                    .expect("rotated password should login");
                assert!(matches!(
                    service
                        .rotate_password(
                            &rotated_access,
                            test_source(1),
                            None,
                            ROTATED_PASSWORD,
                            ROTATED_PASSWORD,
                        )
                        .await,
                    Err(RotatePasswordError::SamePassword)
                ));
                assert!(matches!(
                    service
                        .rotate_password(
                            &rotated_access,
                            test_source(1),
                            None,
                            "wrong horse battery staple",
                            "another sufficiently long manager password",
                        )
                        .await,
                    Err(RotatePasswordError::InvalidCurrentPassword)
                ));
                assert!(matches!(
                    service
                        .rotate_password(
                            &rotated_access,
                            test_source(1),
                            None,
                            ROTATED_PASSWORD,
                            "too short",
                        )
                        .await,
                    Err(RotatePasswordError::PasswordPolicy(_))
                ));
                service
                    .logout_session(
                        rotated_access.login_instance_id,
                        rotated_access.credential_epoch,
                    )
                    .await
                    .expect("logout should revoke the current session");
                assert!(matches!(
                    service.refresh(&rotated.refresh_token).await,
                    Err(RefreshError::Invalid)
                ));
                service
                    .logout_session(
                        initial_access.login_instance_id,
                        initial_access.credential_epoch,
                    )
                    .await
                    .expect("stale current-session logout should be idempotent");
            })
            .await;
    }

    #[tokio::test]
    async fn concurrent_bootstrap_has_exactly_one_winner_without_timing_assumptions() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-bootstrap-race.sqlite");

        test_db_context
            .run_async(async {
                let service = Arc::new(test_auth_service());
                let barrier = Arc::new(Barrier::new(3));
                let first = {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    test_db_context.spawn(async move {
                        barrier.wait().await;
                        service.bootstrap(INITIAL_PASSWORD).await
                    })
                };
                let second = {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    test_db_context.spawn(async move {
                        barrier.wait().await;
                        service
                            .bootstrap("a distinct and sufficiently long bootstrap password")
                            .await
                    })
                };

                barrier.wait().await;
                let results = [
                    first.await.expect("first bootstrap task should join"),
                    second.await.expect("second bootstrap task should join"),
                ];
                assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
                assert!(
                    results
                        .iter()
                        .filter_map(|result| result.as_ref().err())
                        .all(|error| matches!(
                            error,
                            BootstrapError::Busy | BootstrapError::AlreadyInitialized
                        ))
                );
                assert_eq!(
                    service.bootstrap_status().expect("status should load"),
                    BootstrapStatus::Ready
                );
            })
            .await;
    }

    #[tokio::test]
    async fn concurrent_rotation_has_one_new_epoch_and_rejects_the_loser() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-rotation-race.sqlite");

        test_db_context
            .run_async(async {
                let service = Arc::new(test_auth_service());
                let bootstrapped = service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");
                let access = decode_access_token(&bootstrapped.access_token)
                    .expect("bootstrap access should decode");
                let barrier = Arc::new(Barrier::new(3));
                let first = {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    let access = access.clone();
                    test_db_context.spawn(async move {
                        barrier.wait().await;
                        service
                            .rotate_password(
                                &access,
                                test_source(1),
                                None,
                                INITIAL_PASSWORD,
                                ROTATED_PASSWORD,
                            )
                            .await
                    })
                };
                let second = {
                    let service = Arc::clone(&service);
                    let barrier = Arc::clone(&barrier);
                    let access = access.clone();
                    test_db_context.spawn(async move {
                        barrier.wait().await;
                        service
                            .rotate_password(
                                &access,
                                test_source(2),
                                None,
                                INITIAL_PASSWORD,
                                "a second sufficiently long rotated manager password",
                            )
                            .await
                    })
                };

                barrier.wait().await;
                let results = [
                    first.await.expect("first rotation task should join"),
                    second.await.expect("second rotation task should join"),
                ];
                assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
                assert!(
                    results
                        .iter()
                        .filter_map(|result| result.as_ref().err())
                        .all(|error| matches!(
                            error,
                            RotatePasswordError::Busy | RotatePasswordError::EpochConflict
                        ))
                );
                assert_eq!(
                    service.validate_access_context(&access),
                    Err(AccessCredentialError::EpochMismatchOrUninitialized)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_service_rate_limits_only_invalid_password_attempts() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-service-rate-limit.sqlite");
        let now = Arc::new(AtomicI64::new(crate::utils::auth::get_current_timestamp()));

        test_db_context
            .run_async({
                let now = Arc::clone(&now);
                async move {
                    let service_now = Arc::clone(&now);
                    let service = ManagerAuthService::new_for_test(Arc::new(move || {
                        service_now.load(Ordering::SeqCst)
                    }));
                    service
                        .bootstrap(INITIAL_PASSWORD)
                        .await
                        .expect("bootstrap should succeed");

                    for _ in 0..5 {
                        assert!(matches!(
                            service
                                .login(test_source(1), "wrong horse battery staple")
                                .await,
                            Err(LoginError::InvalidPassword)
                        ));
                    }
                    assert_eq!(
                        service.login(test_source(1), INITIAL_PASSWORD).await.err(),
                        Some(LoginError::SourceRateLimited { retry_after: 60 })
                    );
                    service
                        .login(test_source(2), INITIAL_PASSWORD)
                        .await
                        .expect("a different source must remain independent");
                    now.fetch_add(61, Ordering::SeqCst);
                    service
                        .login(test_source(1), INITIAL_PASSWORD)
                        .await
                        .expect("login should recover after lock window");
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_login_success_clears_only_its_source_and_format_errors_do_not_count() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-source-clear.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");

                for _ in 0..5 {
                    assert!(matches!(
                        service.login(test_source(3), "short").await,
                        Err(LoginError::InvalidPassword)
                    ));
                }
                service
                    .login(test_source(3), INITIAL_PASSWORD)
                    .await
                    .expect("format errors must not lock a source");

                for _ in 0..2 {
                    assert!(matches!(
                        service
                            .login(test_source(4), "wrong horse battery staple")
                            .await,
                        Err(LoginError::InvalidPassword)
                    ));
                }
                assert_eq!(service.login_protection_counts().0, 1);
                service
                    .login(test_source(4), INITIAL_PASSWORD)
                    .await
                    .expect("successful login should clear its source failures");
                assert_eq!(service.login_protection_counts().0, 0);
            })
            .await;
    }

    #[test]
    fn manager_auth_global_login_protection_is_bounded_and_recovers_by_window() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-global-protection.sqlite");

        test_db_context.run_sync(|| {
            let now = Arc::new(AtomicI64::new(10_000));
            let service_now = Arc::clone(&now);
            let service = ManagerAuthService::new_for_test(Arc::new(move || {
                service_now.load(Ordering::SeqCst)
            }));

            for _ in 0..GLOBAL_LOGIN_VERIFICATION_LIMIT {
                service
                    .reserve_global_verification(now.load(Ordering::SeqCst))
                    .expect("the configured global verification budget should be available");
            }
            assert_eq!(service.login_protection_counts(), (0, 30));
            assert_eq!(
                service
                    .reserve_global_verification(now.load(Ordering::SeqCst))
                    .err(),
                Some(LoginError::GlobalRateLimited { retry_after: 60 })
            );

            let source = test_source(5);
            for _ in 0..LOGIN_FAILURE_LIMIT {
                service.record_login_failure(source, now.load(Ordering::SeqCst));
            }
            assert_eq!(service.source_retry_after(source, 10_000), Some(60));
            assert_eq!(
                service.login_protection_counts().1,
                30,
                "source lock checks do not consume the global budget"
            );
            for index in 0..=LOGIN_SOURCE_CAPACITY {
                service.record_login_failure(
                    IpAddr::V4(Ipv4Addr::from(0xac10_0000_u32 + index as u32)),
                    now.load(Ordering::SeqCst),
                );
            }
            assert_eq!(
                service.login_protection_counts().0,
                LOGIN_SOURCE_CAPACITY,
                "password source protection must stay bounded"
            );

            now.fetch_add(61, Ordering::SeqCst);
            assert_eq!(service.source_retry_after(source, 10_061), None);
            service
                .reserve_global_verification(10_061)
                .expect("global protection should recover after its window");
            assert_eq!(service.login_protection_counts(), (0, 1));
        });
    }

    #[tokio::test]
    async fn manager_auth_service_reloads_manual_delete_only_after_restart() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-service-recovery.sqlite");

        test_db_context
            .run_async(async {
                let service = test_auth_service();
                service
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("bootstrap should succeed");

                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DELETE FROM manager_credential")
                            .execute(conn)
                            .expect("manual recovery delete should succeed");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DELETE FROM manager_credential")
                            .execute(conn)
                            .expect("manual recovery delete should succeed");
                    }
                }

                assert_eq!(
                    service
                        .bootstrap_status()
                        .expect("memory status should remain"),
                    BootstrapStatus::Ready
                );
                let restarted = test_auth_service();
                assert_eq!(
                    restarted.bootstrap_status().expect("restart should reload"),
                    BootstrapStatus::Uninitialized
                );
            })
            .await;
    }

    #[tokio::test]
    async fn app_state_builds_when_manager_credential_is_uninitialized_or_corrupt() {
        let uninitialized_db = TestDbContext::new_sqlite("app-state-auth-uninitialized.sqlite");
        uninitialized_db
            .run_async(async {
                let app_state = create_test_app_state(uninitialized_db.clone()).await;
                assert_eq!(
                    app_state
                        .admin
                        .auth
                        .bootstrap_status()
                        .expect("missing credential should be bootstrap state"),
                    BootstrapStatus::Uninitialized
                );
            })
            .await;

        let corrupt_db = TestDbContext::new_sqlite("app-state-auth-corrupt.sqlite");
        corrupt_db
            .run_async(async {
                ManagerCredential::insert_once(NewManagerCredential {
                    password_verifier: "not-a-phc".to_string(),
                    credential_epoch: uuid::Uuid::new_v4().to_string(),
                    now: 1,
                })
                .expect("corrupt fixture should persist");
                let app_state = create_test_app_state(corrupt_db.clone()).await;
                assert_eq!(
                    app_state.admin.auth.bootstrap_status(),
                    Err(super::BootstrapStatusError::Unavailable)
                );
                assert!(
                    app_state.max_body_size > 0,
                    "proxy app state should still build"
                );
            })
            .await;

        let corrupt_epoch_db = TestDbContext::new_sqlite("app-state-auth-corrupt-epoch.sqlite");
        corrupt_epoch_db
            .run_async(async {
                test_auth_service()
                    .bootstrap(INITIAL_PASSWORD)
                    .await
                    .expect("valid credential fixture should bootstrap");
                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query(
                            "UPDATE manager_credential SET credential_epoch = 'not-a-uuid'",
                        )
                        .execute(conn)
                        .expect("corrupt epoch fixture should persist");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query(
                            "UPDATE manager_credential SET credential_epoch = 'not-a-uuid'",
                        )
                        .execute(conn)
                        .expect("corrupt epoch fixture should persist");
                    }
                }
                let app_state = create_test_app_state(corrupt_epoch_db.clone()).await;
                assert_eq!(
                    app_state.admin.auth.bootstrap_status(),
                    Err(super::BootstrapStatusError::Unavailable)
                );
                assert!(
                    app_state.max_body_size > 0,
                    "proxy app state should still build"
                );
            })
            .await;
    }
}
