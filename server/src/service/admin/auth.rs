use std::sync::{Arc, Mutex};

use cyder_tools::log::{debug, info, warn};
use serde::Serialize;

use crate::config::CONFIG;
use crate::controller::BaseError;
use crate::database::manager_auth_instance::{MANAGER_ID, MANAGER_SUBJECT, ManagerAuthInstance};
use crate::utils::auth::{
    ManagerAuthContext, REFRESH_TOKEN_ISSUE_SEC, constant_time_eq, decode_refresh_token,
    generate_token_jti, get_current_timestamp, issue_access_token, issue_refresh_token,
};

const LOGIN_FAILURE_LIMIT: u32 = 5;
const LOGIN_FAILURE_LOCK_SEC: i64 = 60;

type NowFn = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
pub struct AuthTokenPair {
    pub refresh_token: String,
    pub access_token: String,
}

#[derive(Debug, Default)]
struct LoginFailureState {
    consecutive_failures: u32,
    locked_until: Option<i64>,
}

pub struct ManagerAuthService {
    login_failures: Mutex<LoginFailureState>,
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
        Self {
            login_failures: Mutex::new(LoginFailureState::default()),
            now,
        }
    }

    pub async fn login(&self, submitted_key: &str) -> Result<AuthTokenPair, BaseError> {
        let now = self.now();
        if self.is_login_locked(now) {
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.login_failed",
                    &[("reason", Some("rate_limited".to_string()))],
                )
            );
            return Err(BaseError::Unauthorized(Some(
                "Too many failed login attempts".to_string(),
            )));
        }

        if !constant_time_eq(submitted_key, &CONFIG.secret_key) {
            self.record_login_failure(now);
            warn!(
                "{}",
                crate::logging::event_message_with_fields(
                    "manager.auth.login_failed",
                    &[("reason", Some("invalid_key".to_string()))],
                )
            );
            return Err(BaseError::Unauthorized(Some("Invalid key".to_string())));
        }

        let refresh_jti = generate_token_jti();
        let refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let instance =
            ManagerAuthInstance::create_instance(refresh_jti.clone(), now, refresh_expires_at)?;
        let token_pair = issue_token_pair(instance.id, &refresh_jti, now, refresh_expires_at);

        self.clear_login_failures();
        info!(
            "{}",
            crate::logging::event_message_with_fields(
                "manager.auth.login_succeeded",
                &[("login_instance_id", Some(instance.id.to_string()))],
            )
        );

        Ok(token_pair)
    }

    pub async fn refresh(&self, refresh_token: &str) -> Result<AuthTokenPair, BaseError> {
        let refresh = decode_refresh_token(refresh_token).map_err(|_| {
            self.log_refresh_rejected("invalid_token", None);
            BaseError::Unauthorized(Some("Invalid refresh token".to_string()))
        })?;

        let instance =
            ManagerAuthInstance::get_instance(refresh.login_instance_id)?.ok_or_else(|| {
                self.log_refresh_rejected("instance_missing", Some(refresh.login_instance_id));
                BaseError::Unauthorized(Some("Invalid refresh token".to_string()))
            })?;

        let now = self.now();
        if instance.manager_id != refresh.manager_id || instance.manager_subject != MANAGER_SUBJECT
        {
            self.log_refresh_rejected("instance_mismatch", Some(instance.id));
            return Err(BaseError::Unauthorized(Some(
                "Invalid refresh token".to_string(),
            )));
        }

        if instance.revoked_at.is_some() {
            self.log_refresh_rejected("instance_revoked", Some(instance.id));
            return Err(BaseError::Unauthorized(Some(
                "Invalid refresh token".to_string(),
            )));
        }

        if instance.expires_at <= now {
            self.log_refresh_rejected("instance_expired", Some(instance.id));
            return Err(BaseError::Unauthorized(Some(
                "Invalid refresh token".to_string(),
            )));
        }

        if instance.current_refresh_jti != refresh.jwt_id {
            self.log_refresh_rejected("stale_refresh_jti", Some(instance.id));
            return Err(BaseError::Unauthorized(Some(
                "Invalid refresh token".to_string(),
            )));
        }

        let new_refresh_jti = generate_token_jti();
        let new_refresh_expires_at = now + REFRESH_TOKEN_ISSUE_SEC;
        let rotated = ManagerAuthInstance::rotate_refresh_jti(
            instance.id,
            &refresh.jwt_id,
            new_refresh_jti.clone(),
            now,
            new_refresh_expires_at,
        )?
        .ok_or_else(|| {
            self.log_refresh_rejected("rotation_conflict", Some(instance.id));
            BaseError::Unauthorized(Some("Invalid refresh token".to_string()))
        })?;

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
            now,
            new_refresh_expires_at,
        ))
    }

    pub async fn logout(&self, auth_context: &ManagerAuthContext) -> Result<(), BaseError> {
        let now = self.now();
        let revoked =
            ManagerAuthInstance::revoke_instance(auth_context.login_instance_id, now, "logout")?;
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

    pub fn cleanup_expired_instances(&self) -> Result<usize, BaseError> {
        ManagerAuthInstance::cleanup_expired_instances(self.now())
    }

    fn now(&self) -> i64 {
        (self.now)()
    }

    fn is_login_locked(&self, now: i64) -> bool {
        let state = self
            .login_failures
            .lock()
            .expect("login failure state should not be poisoned");
        state
            .locked_until
            .is_some_and(|locked_until| now < locked_until)
    }

    fn record_login_failure(&self, now: i64) {
        let mut state = self
            .login_failures
            .lock()
            .expect("login failure state should not be poisoned");
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);
        if state.consecutive_failures >= LOGIN_FAILURE_LIMIT {
            state.locked_until = Some(now + LOGIN_FAILURE_LOCK_SEC);
        }
    }

    fn clear_login_failures(&self) {
        let mut state = self
            .login_failures
            .lock()
            .expect("login failure state should not be poisoned");
        *state = LoginFailureState::default();
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

fn issue_token_pair(
    login_instance_id: i64,
    refresh_jti: &str,
    now: i64,
    refresh_expires_at: i64,
) -> AuthTokenPair {
    let access_jti = generate_token_jti();
    AuthTokenPair {
        refresh_token: issue_refresh_token(
            MANAGER_ID,
            login_instance_id,
            refresh_jti,
            now,
            refresh_expires_at,
        ),
        access_token: issue_access_token(MANAGER_ID, login_instance_id, &access_jti, now),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use crate::config::CONFIG;
    use crate::controller::BaseError;
    use crate::database::TestDbContext;
    use crate::utils::auth::{decode_access_token, decode_refresh_token};

    use super::ManagerAuthService;

    fn unauthorized_message(error: BaseError) -> String {
        match error {
            BaseError::Unauthorized(Some(message)) => message,
            other => panic!("expected unauthorized error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn manager_auth_service_rotates_refresh_and_logs_out_only_current_instance() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-service-rotation.sqlite");

        test_db_context
            .run_async(async {
                let service = ManagerAuthService::new();

                let first = service
                    .login(&CONFIG.secret_key)
                    .await
                    .expect("first login should succeed");
                let second = service
                    .login(&CONFIG.secret_key)
                    .await
                    .expect("second login should succeed");

                let first_refresh = decode_refresh_token(&first.refresh_token)
                    .expect("first refresh should decode");
                let second_refresh = decode_refresh_token(&second.refresh_token)
                    .expect("second refresh should decode");
                assert_ne!(
                    first_refresh.login_instance_id,
                    second_refresh.login_instance_id
                );

                let first_rotated = service
                    .refresh(&first.refresh_token)
                    .await
                    .expect("first refresh should rotate");
                assert!(
                    service.refresh(&first.refresh_token).await.is_err(),
                    "old refresh token should be rejected after rotation"
                );

                let second_rotated = service
                    .refresh(&second.refresh_token)
                    .await
                    .expect("second instance should refresh independently");

                let first_access = decode_access_token(&first_rotated.access_token)
                    .expect("first access should decode");
                service
                    .logout(&first_access)
                    .await
                    .expect("logout should revoke first instance refresh chain");

                assert!(
                    service.refresh(&first_rotated.refresh_token).await.is_err(),
                    "logged out instance refresh should be rejected"
                );
                service
                    .refresh(&second_rotated.refresh_token)
                    .await
                    .expect("second instance should survive first logout");

                decode_access_token(&first_rotated.access_token)
                    .expect("offline access token should remain valid until exp");
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_service_rate_limits_login_failures_and_success_clears_counter() {
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

                    for _ in 0..4 {
                        assert_eq!(
                            unauthorized_message(
                                service
                                    .login("wrong-secret")
                                    .await
                                    .expect_err("should fail")
                            ),
                            "Invalid key"
                        );
                    }
                    service
                        .login(&CONFIG.secret_key)
                        .await
                        .expect("success should be allowed before failure limit");

                    for _ in 0..4 {
                        assert_eq!(
                            unauthorized_message(
                                service
                                    .login("wrong-secret")
                                    .await
                                    .expect_err("should fail")
                            ),
                            "Invalid key"
                        );
                    }
                    service
                        .login(&CONFIG.secret_key)
                        .await
                        .expect("success should clear previous failure counter");

                    for _ in 0..5 {
                        assert_eq!(
                            unauthorized_message(
                                service
                                    .login("wrong-secret")
                                    .await
                                    .expect_err("should fail")
                            ),
                            "Invalid key"
                        );
                    }
                    assert_eq!(
                        unauthorized_message(
                            service
                                .login(&CONFIG.secret_key)
                                .await
                                .expect_err("locked login should fail")
                        ),
                        "Too many failed login attempts"
                    );

                    now.fetch_add(61, Ordering::SeqCst);
                    service
                        .login(&CONFIG.secret_key)
                        .await
                        .expect("login should recover after lock window");
                }
            })
            .await;
    }
}
