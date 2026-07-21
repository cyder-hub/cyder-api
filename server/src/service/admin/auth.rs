use std::collections::HashMap;
use std::collections::VecDeque;
use std::net::IpAddr;
use std::sync::{Arc, Mutex as StdMutex, RwLock};

use cyder_tools::log::{debug, info, warn};
use serde::Serialize;
use tokio::sync::Mutex as AsyncMutex;
use uuid::Uuid;

use crate::controller::BaseError;
use crate::database::manager_auth_instance::{ManagerAuthInstance, NewManagerAuthInstance};
use crate::database::manager_credential::{
    MANAGER_ID, MANAGER_SUBJECT, ManagerCredential, ManagerCredentialRepositoryError,
    NewManagerCredential, RotatedManagerCredential,
};
use crate::utils::ID_GENERATOR;
use crate::utils::auth::{
    ManagerAuthContext, REFRESH_TOKEN_ISSUE_SEC, decode_refresh_token, generate_token_jti,
    get_current_timestamp, issue_access_token, issue_refresh_token,
};

pub(crate) mod password;

use password::{
    CredentialUnavailableReason, ManagerCredentialSnapshot, PasswordEngine, PasswordOperationError,
    PasswordPolicyError, ReadyManagerCredential, normalize_password,
};

const LOGIN_FAILURE_LIMIT: u32 = 5;
const LOGIN_FAILURE_WINDOW_SEC: i64 = 60;
const LOGIN_FAILURE_LOCK_SEC: i64 = 60;
const GLOBAL_LOGIN_VERIFICATION_LIMIT: usize = 30;
const GLOBAL_LOGIN_VERIFICATION_WINDOW_SEC: i64 = 60;

type NowFn = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone, Serialize)]
pub struct AuthTokenPair {
    pub refresh_token: String,
    pub access_token: String,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotatePasswordError {
    InvalidCurrentPassword,
    PasswordPolicy(PasswordPolicyError),
    SamePassword,
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
struct ActiveManagerSession {
    manager_id: i64,
    manager_subject: String,
    session_version: i64,
    expires_at: i64,
}

impl From<&ManagerAuthInstance> for ActiveManagerSession {
    fn from(instance: &ManagerAuthInstance) -> Self {
        Self {
            manager_id: instance.manager_id,
            manager_subject: instance.manager_subject.clone(),
            session_version: instance.session_version,
            expires_at: instance.expires_at,
        }
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

pub struct ManagerAuthService {
    login_protection: StdMutex<LoginProtectionState>,
    credential_snapshot: RwLock<ManagerCredentialSnapshot>,
    session_registry: RwLock<SessionRegistryState>,
    credential_lifecycle: AsyncMutex<()>,
    password_engine: PasswordEngine,
    now: NowFn,
}

impl ManagerAuthService {
    pub(crate) fn new() -> Self {
        Self::new_with_clock(Arc::new(get_current_timestamp))
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(now: NowFn) -> Self {
        Self::new_with_clock(now)
    }

    fn new_with_clock(now: NowFn) -> Self {
        let current_time = now();
        let credential_snapshot = ManagerCredentialSnapshot::load();
        if let ManagerCredentialSnapshot::Unavailable(reason) = &credential_snapshot {
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.credential_snapshot_unavailable",
                    &[("reason", Some(snapshot_reason_code(*reason).to_string()))],
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
            credential_snapshot: RwLock::new(credential_snapshot),
            session_registry: RwLock::new(session_registry),
            credential_lifecycle: AsyncMutex::new(()),
            password_engine: PasswordEngine::new(),
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
        let refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let mutation = ManagerCredential::bootstrap_with_session(
            NewManagerCredential {
                password_verifier: verifier.to_string(),
                credential_epoch: epoch.to_string(),
                now,
            },
            new_session(&refresh_jti, now, refresh_expires_at),
            "credential_bootstrap",
        )
        .map_err(map_bootstrap_repository_error)?;
        let ready = self.install_ready_snapshot(mutation.credential)?;
        self.replace_sessions_with(&mutation.session);

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
        Ok(issue_token_pair(
            &mutation.session,
            &refresh_jti,
            ready.credential_epoch(),
            now,
            refresh_expires_at,
        ))
    }

    pub async fn login(
        &self,
        source: IpAddr,
        submitted_password: &str,
    ) -> Result<AuthTokenPair, LoginError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| LoginError::Busy)?;
        let now = self.now();
        self.ensure_session_registry_ready()
            .map_err(|_| LoginError::Unavailable)?;
        if let Some(retry_after) = self.source_retry_after(source, now) {
            self.log_login_rejected("source_rate_limited");
            return Err(LoginError::SourceRateLimited { retry_after });
        }

        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Uninitialized => return Err(LoginError::Uninitialized),
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Unavailable(_) => return Err(LoginError::Unavailable),
        };
        let password = match normalize_password(submitted_password) {
            Ok(password) => password,
            Err(_) => {
                self.log_login_rejected("invalid_password");
                return Err(LoginError::InvalidPassword);
            }
        };
        self.reserve_global_verification(now)?;
        match self
            .password_engine
            .verify(password, ready.password_verifier())
            .await
        {
            Ok(()) => {}
            Err(PasswordOperationError::IncorrectPassword) => {
                self.record_login_failure(source, now);
                self.log_login_rejected("invalid_password");
                return Err(LoginError::InvalidPassword);
            }
            Err(PasswordOperationError::Busy) => {
                self.release_global_verification(now);
                return Err(LoginError::Busy);
            }
            Err(PasswordOperationError::InvalidVerifier | PasswordOperationError::Runtime) => {
                self.mark_unavailable(CredentialUnavailableReason::InvalidVerifier);
                return Err(LoginError::Unavailable);
            }
        }

        let refresh_jti = generate_token_jti();
        let refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let instance =
            ManagerAuthInstance::create_instance(refresh_jti.clone(), now, refresh_expires_at)
                .map_err(|_| LoginError::Storage)?;
        self.insert_session(&instance);
        self.clear_login_failures(source);
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.login_succeeded",
                &[("login_instance_id", Some(instance.id.to_string()))],
            )
        );

        Ok(issue_token_pair(
            &instance,
            &refresh_jti,
            ready.credential_epoch(),
            now,
            refresh_expires_at,
        ))
    }

    pub async fn rotate_password(
        &self,
        auth_context: &ManagerAuthContext,
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
        let verifier = self
            .password_engine
            .hash(new_password)
            .await
            .map_err(map_rotate_password_error)?;
        let now = self.now();
        let new_epoch = Uuid::new_v4();
        let refresh_jti = generate_token_jti();
        let refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let mutation = ManagerCredential::rotate_with_session(
            &ready.credential_epoch().to_string(),
            RotatedManagerCredential {
                password_verifier: verifier.to_string(),
                credential_epoch: new_epoch.to_string(),
                now,
            },
            new_session(&refresh_jti, now, refresh_expires_at),
            "credential_rotated",
        )
        .map_err(map_rotate_repository_error)?;
        let installed = self
            .install_ready_snapshot(mutation.credential)
            .map_err(|_| RotatePasswordError::Unavailable)?;
        self.replace_sessions_with(&mutation.session);

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
        Ok(issue_token_pair(
            &mutation.session,
            &refresh_jti,
            installed.credential_epoch(),
            now,
            refresh_expires_at,
        ))
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<AuthTokenPair, RefreshError> {
        self.ensure_session_registry_ready()
            .map_err(|_| RefreshError::Unavailable)?;
        let refresh = decode_refresh_token(refresh_token).map_err(|_| {
            self.log_refresh_rejected("invalid_token", None);
            RefreshError::Invalid
        })?;
        self.validate_refresh_epoch(refresh.credential_epoch)?;

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
        if instance.revoked_at.is_some() || instance.expires_at <= now {
            self.log_refresh_rejected("instance_inactive", Some(instance.id));
            return Err(RefreshError::Invalid);
        }
        if instance.current_refresh_jti != refresh.jwt_id
            || instance.session_version != refresh.session_version
        {
            self.log_refresh_replay(instance.id, "stale_token", now);
            return Err(RefreshError::Replay);
        }

        let new_refresh_jti = generate_token_jti();
        let new_refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let rotated = ManagerAuthInstance::rotate_refresh_jti(
            instance.id,
            &refresh.jwt_id,
            refresh.session_version,
            new_refresh_jti.clone(),
            now,
            new_refresh_expires_at,
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
                        && current.expires_at > now
                        && (current.current_refresh_jti != refresh.jwt_id
                            || current.session_version != refresh.session_version)
                }) {
                    self.log_refresh_replay(instance.id, "rotation_conflict", now);
                    return Err(RefreshError::Replay);
                }
                self.log_refresh_rejected("rotation_conflict", Some(instance.id));
                return Err(RefreshError::Invalid);
            }
        };
        self.insert_session(&rotated);
        self.validate_refresh_epoch(refresh.credential_epoch)?;
        debug!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.refresh_rotated",
                &[("login_instance_id", Some(rotated.id.to_string()))],
            )
        );

        Ok(issue_token_pair(
            &rotated,
            &new_refresh_jti,
            refresh.credential_epoch,
            now,
            new_refresh_expires_at,
        ))
    }

    pub async fn logout(&self, auth_context: &ManagerAuthContext) -> Result<(), LogoutError> {
        self.ensure_session_registry_ready()
            .map_err(|_| LogoutError::Unavailable)?;
        self.validate_access_context(auth_context)
            .map_err(|error| match error {
                AccessCredentialError::EpochMismatchOrUninitialized => {
                    LogoutError::InvalidCredential
                }
                AccessCredentialError::Unavailable => LogoutError::Unavailable,
                AccessCredentialError::SessionInvalid => LogoutError::InvalidCredential,
                AccessCredentialError::SessionUnavailable => LogoutError::Unavailable,
            })?;
        let now = self.now();
        let revoked =
            ManagerAuthInstance::revoke_instance(auth_context.login_instance_id, now, "logout")
                .map_err(|_| LogoutError::Storage)?;
        self.remove_session(auth_context.login_instance_id);
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.logout",
                &[
                    (
                        "login_instance_id",
                        Some(auth_context.login_instance_id.to_string()),
                    ),
                    ("revoked", Some(revoked.is_some().to_string())),
                ],
            )
        );
        Ok(())
    }

    pub async fn logout_all(
        &self,
        auth_context: &ManagerAuthContext,
    ) -> Result<usize, LogoutError> {
        self.ensure_session_registry_ready()
            .map_err(|_| LogoutError::Unavailable)?;
        self.validate_access_context(auth_context)
            .map_err(|error| match error {
                AccessCredentialError::EpochMismatchOrUninitialized
                | AccessCredentialError::SessionInvalid => LogoutError::InvalidCredential,
                AccessCredentialError::Unavailable | AccessCredentialError::SessionUnavailable => {
                    LogoutError::Unavailable
                }
            })?;
        let now = self.now();
        let revoked = ManagerAuthInstance::revoke_all_active(now, "logout_all")
            .map_err(|_| LogoutError::Storage)?;
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

    fn registry_from_instances(instances: Vec<ManagerAuthInstance>) -> SessionRegistryState {
        let mut sessions = HashMap::with_capacity(instances.len());
        for instance in instances {
            if instance.manager_id != MANAGER_ID
                || instance.manager_subject != MANAGER_SUBJECT
                || instance.session_version < 1
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
            sessions.insert(instance.id, ActiveManagerSession::from(&instance));
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
        {
            return Err(SessionValidationError::Invalid);
        }
        Ok(())
    }

    fn insert_session(&self, instance: &ManagerAuthInstance) {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            sessions.insert(instance.id, ActiveManagerSession::from(instance));
        }
    }

    fn replace_sessions_with(&self, instance: &ManagerAuthInstance) {
        let mut sessions = HashMap::with_capacity(1);
        sessions.insert(instance.id, ActiveManagerSession::from(instance));
        *self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            SessionRegistryState::Ready(sessions);
    }

    fn remove_session(&self, login_instance_id: i64) {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            sessions.remove(&login_instance_id);
        }
    }

    fn clear_sessions(&self) {
        let mut registry = self
            .session_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let SessionRegistryState::Ready(sessions) = &mut *registry {
            sessions.clear();
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
        let snapshot = ManagerCredentialSnapshot::from_credential(credential);
        let ManagerCredentialSnapshot::Ready(ready) = &snapshot else {
            self.replace_snapshot(snapshot);
            return Err(BootstrapError::Unavailable);
        };
        let ready = ready.clone();
        self.replace_snapshot(snapshot);
        Ok(ready)
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

fn new_session(refresh_jti: &str, now: i64, expires_at: i64) -> NewManagerAuthInstance {
    NewManagerAuthInstance {
        id: ID_GENERATOR.generate_id(),
        manager_id: MANAGER_ID,
        manager_subject: MANAGER_SUBJECT.to_string(),
        current_refresh_jti: refresh_jti.to_string(),
        session_version: crate::database::manager_auth_instance::INITIAL_SESSION_VERSION,
        created_at: now,
        last_rotated_at: now,
        expires_at,
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
            session.session_version,
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
        | ManagerCredentialRepositoryError::Storage(_) => BootstrapError::Storage,
    }
}

fn map_rotate_repository_error(error: ManagerCredentialRepositoryError) -> RotatePasswordError {
    match error {
        ManagerCredentialRepositoryError::EpochConflict => RotatePasswordError::EpochConflict,
        ManagerCredentialRepositoryError::AlreadyInitialized
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
    use crate::database::{DbConnection, get_connection};
    use crate::service::app_state::create_test_app_state;
    use crate::utils::auth::{decode_access_token, decode_refresh_token};
    use diesel::RunQueryDsl;

    use super::{
        AccessCredentialError, BootstrapError, BootstrapStatus, GLOBAL_LOGIN_VERIFICATION_LIMIT,
        LOGIN_FAILURE_LIMIT, LoginError, LogoutError, ManagerAuthService, RefreshError,
        RotatePasswordError,
    };

    const INITIAL_PASSWORD: &str = "correct horse battery staple";
    const ROTATED_PASSWORD: &str = "correct horse battery staple rotated";

    fn test_source(last_octet: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, last_octet))
    }

    #[tokio::test]
    async fn manager_auth_session_registry_tracks_mutations_and_restart_state() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-session-registry.sqlite");

        test_db_context
            .run_async(async {
                let service = ManagerAuthService::new();
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
                    ),
                    Ok(()),
                    "manual database changes require restart before affecting memory"
                );
                let restarted = ManagerAuthService::new();
                assert_eq!(restarted.session_count(), Ok(1));
                assert_eq!(
                    restarted.validate_session(
                        bootstrap_access.login_instance_id,
                        bootstrap_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                    ),
                    Err(super::SessionValidationError::Invalid)
                );
                assert_eq!(
                    restarted.validate_session(
                        login_access.login_instance_id,
                        login_access.manager_id,
                        SESSION_MANAGER_SUBJECT,
                        INITIAL_SESSION_VERSION,
                    ),
                    Ok(())
                );

                let rotated = service
                    .rotate_password(&bootstrap_access, INITIAL_PASSWORD, ROTATED_PASSWORD)
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
                let service = ManagerAuthService::new();
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
            let expired = ManagerAuthInstance::create_instance("expired".to_string(), 1, 10)
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
                let service = ManagerAuthService::new();
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
                    .logout(&access)
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
                let service = Arc::new(ManagerAuthService::new());
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
                assert_eq!(winner_access.session_version, 2);
                assert_eq!(service.validate_access_context(&winner_access), Ok(()));
                assert_eq!(
                    service.validate_access_context(&initial_access),
                    Err(AccessCredentialError::SessionInvalid)
                );
                assert!(matches!(
                    service.refresh(&initial.refresh_token).await,
                    Err(RefreshError::Replay)
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_logout_all_revokes_every_active_session_and_clears_registry() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-logout-all.sqlite");

        test_db_context
            .run_async(async {
                let service = ManagerAuthService::new();
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

                assert_eq!(service.logout_all(&second_access).await, Ok(2));
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
                let service = ManagerAuthService::new();
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
                    .rotate_password(&initial_access, INITIAL_PASSWORD, ROTATED_PASSWORD)
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
                        .rotate_password(&rotated_access, ROTATED_PASSWORD, ROTATED_PASSWORD)
                        .await,
                    Err(RotatePasswordError::SamePassword)
                ));
                assert!(matches!(
                    service
                        .rotate_password(
                            &rotated_access,
                            "wrong horse battery staple",
                            "another sufficiently long manager password",
                        )
                        .await,
                    Err(RotatePasswordError::InvalidCurrentPassword)
                ));
                assert!(matches!(
                    service
                        .rotate_password(&rotated_access, ROTATED_PASSWORD, "too short")
                        .await,
                    Err(RotatePasswordError::PasswordPolicy(_))
                ));
                service
                    .logout(&rotated_access)
                    .await
                    .expect("logout should revoke the current session");
                assert!(matches!(
                    service.refresh(&rotated.refresh_token).await,
                    Err(RefreshError::Invalid)
                ));
                assert!(matches!(
                    service.logout(&initial_access).await,
                    Err(LogoutError::InvalidCredential)
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn concurrent_bootstrap_has_exactly_one_winner_without_timing_assumptions() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-bootstrap-race.sqlite");

        test_db_context
            .run_async(async {
                let service = Arc::new(ManagerAuthService::new());
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
                let service = Arc::new(ManagerAuthService::new());
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
                            .rotate_password(&access, INITIAL_PASSWORD, ROTATED_PASSWORD)
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
                let service = ManagerAuthService::new();
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
                let service = ManagerAuthService::new();
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
                let restarted = ManagerAuthService::new();
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
                ManagerAuthService::new()
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
