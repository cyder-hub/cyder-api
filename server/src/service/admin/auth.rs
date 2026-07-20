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
const LOGIN_FAILURE_LOCK_SEC: i64 = 60;

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
    RateLimited,
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
}

#[derive(Debug, Default)]
struct LoginFailureState {
    consecutive_failures: u32,
    locked_until: Option<i64>,
}

pub struct ManagerAuthService {
    login_failures: StdMutex<LoginFailureState>,
    credential_snapshot: RwLock<ManagerCredentialSnapshot>,
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

        Self {
            login_failures: StdMutex::new(LoginFailureState::default()),
            credential_snapshot: RwLock::new(credential_snapshot),
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
        self.clear_login_failures();

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
            mutation.session.id,
            &refresh_jti,
            ready.credential_epoch(),
            now,
            refresh_expires_at,
        ))
    }

    pub async fn login(&self, submitted_password: &str) -> Result<AuthTokenPair, LoginError> {
        let _lifecycle = self
            .credential_lifecycle
            .try_lock()
            .map_err(|_| LoginError::Busy)?;
        let now = self.now();
        if self.is_login_locked(now) {
            self.log_login_rejected("rate_limited");
            return Err(LoginError::RateLimited);
        }

        let ready = match self.snapshot() {
            ManagerCredentialSnapshot::Uninitialized => return Err(LoginError::Uninitialized),
            ManagerCredentialSnapshot::Ready(ready) => ready,
            ManagerCredentialSnapshot::Unavailable(_) => return Err(LoginError::Unavailable),
        };
        let password = match normalize_password(submitted_password) {
            Ok(password) => password,
            Err(_) => {
                self.record_login_failure(now);
                self.log_login_rejected("invalid_password");
                return Err(LoginError::InvalidPassword);
            }
        };
        match self
            .password_engine
            .verify(password, ready.password_verifier())
            .await
        {
            Ok(()) => {}
            Err(PasswordOperationError::IncorrectPassword) => {
                self.record_login_failure(now);
                self.log_login_rejected("invalid_password");
                return Err(LoginError::InvalidPassword);
            }
            Err(PasswordOperationError::Busy) => return Err(LoginError::Busy),
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
        self.clear_login_failures();
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.login_succeeded",
                &[("login_instance_id", Some(instance.id.to_string()))],
            )
        );

        Ok(issue_token_pair(
            instance.id,
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
        self.clear_login_failures();

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
            mutation.session.id,
            &refresh_jti,
            installed.credential_epoch(),
            now,
            refresh_expires_at,
        ))
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<AuthTokenPair, RefreshError> {
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
        if instance.current_refresh_jti != refresh.jwt_id {
            self.log_refresh_rejected("stale_refresh_jti", Some(instance.id));
            return Err(RefreshError::Invalid);
        }

        let new_refresh_jti = generate_token_jti();
        let new_refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let rotated = ManagerAuthInstance::rotate_refresh_jti(
            instance.id,
            &refresh.jwt_id,
            new_refresh_jti.clone(),
            now,
            new_refresh_expires_at,
        )
        .map_err(|_| RefreshError::Storage)?
        .ok_or_else(|| {
            self.log_refresh_rejected("rotation_conflict", Some(instance.id));
            RefreshError::Invalid
        })?;
        self.validate_refresh_epoch(refresh.credential_epoch)?;
        debug!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.refresh_rotated",
                &[("login_instance_id", Some(rotated.id.to_string()))],
            )
        );

        Ok(issue_token_pair(
            rotated.id,
            &new_refresh_jti,
            refresh.credential_epoch,
            now,
            new_refresh_expires_at,
        ))
    }

    pub async fn logout(&self, auth_context: &ManagerAuthContext) -> Result<(), LogoutError> {
        self.validate_access_context(auth_context)
            .map_err(|error| match error {
                AccessCredentialError::EpochMismatchOrUninitialized => {
                    LogoutError::InvalidCredential
                }
                AccessCredentialError::Unavailable => LogoutError::Unavailable,
            })?;
        let now = self.now();
        let revoked =
            ManagerAuthInstance::revoke_instance(auth_context.login_instance_id, now, "logout")
                .map_err(|_| LogoutError::Storage)?;
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

    pub fn validate_access_context(
        &self,
        auth_context: &ManagerAuthContext,
    ) -> Result<(), AccessCredentialError> {
        match self.snapshot() {
            ManagerCredentialSnapshot::Ready(ready)
                if ready.credential_epoch() == auth_context.credential_epoch =>
            {
                Ok(())
            }
            ManagerCredentialSnapshot::Ready(_) | ManagerCredentialSnapshot::Uninitialized => {
                Err(AccessCredentialError::EpochMismatchOrUninitialized)
            }
            ManagerCredentialSnapshot::Unavailable(_) => Err(AccessCredentialError::Unavailable),
        }
    }

    pub fn cleanup_expired_instances(&self) -> Result<usize, BaseError> {
        ManagerAuthInstance::cleanup_expired_instances(self.now())
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

    fn is_login_locked(&self, now: i64) -> bool {
        let state = self
            .login_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state
            .locked_until
            .is_some_and(|locked_until| now < locked_until)
    }

    fn record_login_failure(&self, now: i64) {
        let mut state = self
            .login_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures >= LOGIN_FAILURE_LIMIT {
            state.locked_until = Some(now + LOGIN_FAILURE_LOCK_SEC);
        }
    }

    fn clear_login_failures(&self) {
        let mut state = self
            .login_failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = LoginFailureState::default();
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
}

fn new_session(refresh_jti: &str, now: i64, expires_at: i64) -> NewManagerAuthInstance {
    NewManagerAuthInstance {
        id: ID_GENERATOR.generate_id(),
        manager_id: MANAGER_ID,
        manager_subject: MANAGER_SUBJECT.to_string(),
        current_refresh_jti: refresh_jti.to_string(),
        created_at: now,
        last_rotated_at: now,
        expires_at,
        revoked_at: None,
        revoked_reason: None,
    }
}

fn issue_token_pair(
    login_instance_id: i64,
    refresh_jti: &str,
    credential_epoch: Uuid,
    now: i64,
    refresh_expires_at: i64,
) -> AuthTokenPair {
    let access_jti = generate_token_jti();
    AuthTokenPair {
        refresh_token: issue_refresh_token(
            MANAGER_ID,
            login_instance_id,
            refresh_jti,
            &credential_epoch,
            now,
            refresh_expires_at,
        ),
        access_token: issue_access_token(
            MANAGER_ID,
            login_instance_id,
            &access_jti,
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
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use tokio::sync::Barrier;

    use crate::database::TestDbContext;
    use crate::database::manager_credential::{ManagerCredential, NewManagerCredential};
    use crate::database::{DbConnection, get_connection};
    use crate::service::app_state::create_test_app_state;
    use crate::utils::auth::{decode_access_token, decode_refresh_token};
    use diesel::RunQueryDsl;

    use super::{
        AccessCredentialError, BootstrapError, BootstrapStatus, LoginError, LogoutError,
        ManagerAuthService, RefreshError, RotatePasswordError,
    };

    const INITIAL_PASSWORD: &str = "correct horse battery staple";
    const ROTATED_PASSWORD: &str = "correct horse battery staple rotated";

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
                    .login(INITIAL_PASSWORD)
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
                    service.login(INITIAL_PASSWORD).await,
                    Err(LoginError::InvalidPassword)
                ));
                service
                    .login(ROTATED_PASSWORD)
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
                            service.login("wrong horse battery staple").await,
                            Err(LoginError::InvalidPassword)
                        ));
                    }
                    assert!(matches!(
                        service.login(INITIAL_PASSWORD).await,
                        Err(LoginError::RateLimited)
                    ));
                    now.fetch_add(61, Ordering::SeqCst);
                    service
                        .login(INITIAL_PASSWORD)
                        .await
                        .expect("login should recover after lock window");
                }
            })
            .await;
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
