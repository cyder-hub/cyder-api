use std::fmt;

use diesel::prelude::*;

use super::manager_auth_instance::{ManagerAuthInstance, NewManagerAuthInstance};
use super::manager_totp_recovery_code::NewManagerTotpRecoveryCode;
use super::runtime::{
    DatabaseRuntime, DatabaseWorkload, RuntimeConnection, db_execute as async_db_execute,
};

pub const MANAGER_ID: i64 = 0;
pub const MANAGER_SUBJECT: &str = "admin";

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerCredential {
    pub manager_id: i64,
    pub manager_subject: String,
    pub password_verifier: String,
    pub credential_epoch: String,
    pub totp_secret_ciphertext: Option<Vec<u8>>,
    pub totp_secret_nonce: Option<Vec<u8>>,
    pub totp_secret_format_version: Option<i32>,
    pub totp_secret_key_fingerprint: Option<String>,
    pub totp_last_accepted_step: Option<i64>,
    pub totp_enabled_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerTotpSecret {
    pub secret_ciphertext: Vec<u8>,
    pub secret_nonce: Vec<u8>,
    pub secret_format_version: i32,
    pub secret_key_fingerprint: String,
    pub last_accepted_step: i64,
    pub enabled_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExpectedManagerTotpState {
    Disabled,
    Enabled,
}

#[derive(Clone)]
pub struct NewManagerCredential {
    pub password_verifier: String,
    pub credential_epoch: String,
    pub now: i64,
}

#[derive(Clone)]
pub struct RotatedManagerCredential {
    pub password_verifier: String,
    pub credential_epoch: String,
    pub now: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagerCredentialRepositoryError {
    AlreadyInitialized,
    EpochConflict,
    StateConflict,
    TotpStepConflict,
    RecoveryCodeConflict,
    ContractViolation(String),
    Storage(String),
}

impl fmt::Display for ManagerCredentialRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => {
                formatter.write_str("manager credential already initialized")
            }
            Self::EpochConflict => formatter.write_str("manager credential epoch conflict"),
            Self::StateConflict => formatter.write_str("manager TOTP state conflict"),
            Self::TotpStepConflict => formatter.write_str("manager TOTP step conflict"),
            Self::RecoveryCodeConflict => {
                formatter.write_str("manager TOTP recovery code conflict")
            }
            Self::ContractViolation(context) => {
                write!(
                    formatter,
                    "manager credential mutation contract violation: {context}"
                )
            }
            Self::Storage(context) => {
                write!(formatter, "manager credential storage error: {context}")
            }
        }
    }
}

impl std::error::Error for ManagerCredentialRepositoryError {}

impl From<diesel::result::Error> for ManagerCredentialRepositoryError {
    fn from(error: diesel::result::Error) -> Self {
        storage_error("transaction failed", error)
    }
}

impl From<crate::database::error::PersistenceError> for ManagerCredentialRepositoryError {
    fn from(error: crate::database::error::PersistenceError) -> Self {
        manager_credential_storage_error("database runtime failed", error)
    }
}

pub struct ManagerCredentialSessionMutation {
    pub credential: ManagerCredential,
    pub session: ManagerAuthInstance,
    pub revoked_sessions: usize,
}

pub struct ManagerRecoveryStartMutation {
    pub revoked_sessions: usize,
}

mod _postgres_model {
    use super::*;
    use crate::database::_postgres_schema::manager_credential;

    #[derive(Queryable, Selectable, Identifiable)]
    #[diesel(table_name = manager_credential)]
    #[diesel(primary_key(manager_id))]
    pub(super) struct ManagerCredentialDb {
        manager_id: i64,
        manager_subject: String,
        password_verifier: String,
        credential_epoch: String,
        totp_secret_ciphertext: Option<Vec<u8>>,
        totp_secret_nonce: Option<Vec<u8>>,
        totp_secret_format_version: Option<i32>,
        totp_secret_key_fingerprint: Option<String>,
        totp_last_accepted_step: Option<i64>,
        totp_enabled_at: Option<i64>,
        created_at: i64,
        updated_at: i64,
    }

    #[derive(Insertable)]
    #[diesel(table_name = manager_credential)]
    pub(super) struct NewManagerCredentialDb {
        manager_id: i64,
        manager_subject: String,
        password_verifier: String,
        credential_epoch: String,
        created_at: i64,
        updated_at: i64,
    }

    impl ManagerCredentialDb {
        pub(super) fn from_db(self) -> ManagerCredential {
            ManagerCredential {
                manager_id: self.manager_id,
                manager_subject: self.manager_subject,
                password_verifier: self.password_verifier,
                credential_epoch: self.credential_epoch,
                totp_secret_ciphertext: self.totp_secret_ciphertext,
                totp_secret_nonce: self.totp_secret_nonce,
                totp_secret_format_version: self.totp_secret_format_version,
                totp_secret_key_fingerprint: self.totp_secret_key_fingerprint,
                totp_last_accepted_step: self.totp_last_accepted_step,
                totp_enabled_at: self.totp_enabled_at,
                created_at: self.created_at,
                updated_at: self.updated_at,
            }
        }
    }

    impl NewManagerCredentialDb {
        pub(super) fn from_new(value: &NewManagerCredential) -> Self {
            Self {
                manager_id: MANAGER_ID,
                manager_subject: MANAGER_SUBJECT.to_string(),
                password_verifier: value.password_verifier.clone(),
                credential_epoch: value.credential_epoch.clone(),
                created_at: value.now,
                updated_at: value.now,
            }
        }
    }
}

mod _sqlite_model {
    use super::*;
    use crate::database::_sqlite_schema::manager_credential;

    #[derive(Queryable, Selectable, Identifiable)]
    #[diesel(table_name = manager_credential)]
    #[diesel(primary_key(manager_id))]
    pub(super) struct ManagerCredentialDb {
        manager_id: i64,
        manager_subject: String,
        password_verifier: String,
        credential_epoch: String,
        totp_secret_ciphertext: Option<Vec<u8>>,
        totp_secret_nonce: Option<Vec<u8>>,
        totp_secret_format_version: Option<i32>,
        totp_secret_key_fingerprint: Option<String>,
        totp_last_accepted_step: Option<i64>,
        totp_enabled_at: Option<i64>,
        created_at: i64,
        updated_at: i64,
    }

    #[derive(Insertable)]
    #[diesel(table_name = manager_credential)]
    pub(super) struct NewManagerCredentialDb {
        manager_id: i64,
        manager_subject: String,
        password_verifier: String,
        credential_epoch: String,
        created_at: i64,
        updated_at: i64,
    }

    impl ManagerCredentialDb {
        pub(super) fn from_db(self) -> ManagerCredential {
            ManagerCredential {
                manager_id: self.manager_id,
                manager_subject: self.manager_subject,
                password_verifier: self.password_verifier,
                credential_epoch: self.credential_epoch,
                totp_secret_ciphertext: self.totp_secret_ciphertext,
                totp_secret_nonce: self.totp_secret_nonce,
                totp_secret_format_version: self.totp_secret_format_version,
                totp_secret_key_fingerprint: self.totp_secret_key_fingerprint,
                totp_last_accepted_step: self.totp_last_accepted_step,
                totp_enabled_at: self.totp_enabled_at,
                created_at: self.created_at,
                updated_at: self.updated_at,
            }
        }
    }

    impl NewManagerCredentialDb {
        pub(super) fn from_new(value: &NewManagerCredential) -> Self {
            Self {
                manager_id: MANAGER_ID,
                manager_subject: MANAGER_SUBJECT.to_string(),
                password_verifier: value.password_verifier.clone(),
                credential_epoch: value.credential_epoch.clone(),
                created_at: value.now,
                updated_at: value.now,
            }
        }
    }
}

pub(crate) fn manager_credential_storage_error(
    context: &str,
    error: impl fmt::Display,
) -> ManagerCredentialRepositoryError {
    ManagerCredentialRepositoryError::Storage(format!("{context}: {error}"))
}

fn storage_error(context: &str, error: impl fmt::Display) -> ManagerCredentialRepositoryError {
    manager_credential_storage_error(context, error)
}

fn map_insert_error(error: diesel::result::Error) -> ManagerCredentialRepositoryError {
    match error {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => ManagerCredentialRepositoryError::AlreadyInitialized,
        other => storage_error("insert failed", other),
    }
}

macro_rules! revoke_active_async {
    ($conn:expr, $now:expr, $reason:expr) => {{
        let query = diesel::update(
            manager_auth_instance::table.filter(
                manager_auth_instance::dsl::manager_id
                    .eq(MANAGER_ID)
                    .and(manager_auth_instance::dsl::revoked_at.is_null()),
            ),
        )
        .set((
            manager_auth_instance::dsl::revoked_at.eq(Some($now)),
            manager_auth_instance::dsl::revoked_reason.eq(Some($reason.to_string())),
        ));
        let result: Result<usize, diesel::result::Error> =
            diesel_async::RunQueryDsl::execute(query, &mut *$conn).await;
        result.map_err(|error| storage_error("session revocation failed", error))
    }};
}

macro_rules! insert_session_async {
    ($conn:expr, $new_session:expr) => {{
        let query = diesel::insert_into(manager_auth_instance::table)
            .values(NewManagerAuthInstanceDb::to_db($new_session))
            .returning(ManagerAuthInstanceDb::as_returning());
        diesel_async::RunQueryDsl::get_result::<ManagerAuthInstanceDb>(query, &mut *$conn)
            .await
            .map(ManagerAuthInstanceDb::from_db)
            .map_err(|error| storage_error("session insert failed", error))
    }};
}

macro_rules! require_lifecycle_update_async {
    ($conn:expr, $expected_epoch:expr, $updated:expr) => {{
        if let Some(updated) = $updated {
            updated
        } else {
            let query = manager_credential::table
                .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID))
                .select(manager_credential::dsl::credential_epoch);
            let epoch = diesel_async::RunQueryDsl::first::<String>(query, &mut *$conn)
                .await
                .optional()
                .map_err(|error| storage_error("TOTP lifecycle conflict lookup failed", error))?;
            return match epoch {
                Some(epoch) if epoch.as_str() != $expected_epoch.as_str() => {
                    Err(ManagerCredentialRepositoryError::EpochConflict)
                }
                _ => Err(ManagerCredentialRepositoryError::StateConflict),
            };
        }
    }};
}

macro_rules! install_totp_async_body {
    ($conn:expr, $expected_epoch:expr, $expected_state:expr, $new_epoch:expr, $totp:expr, $recovery_codes:expr, $new_session:expr, $now:expr, $revoke_reason:expr) => {{
        let target = manager_credential::table.filter(
            manager_credential::dsl::manager_id
                .eq(MANAGER_ID)
                .and(manager_credential::dsl::credential_epoch.eq($expected_epoch)),
        );
        let updated = match $expected_state {
            ExpectedManagerTotpState::Disabled => {
                let query = diesel::update(
                    target.filter(manager_credential::dsl::totp_secret_ciphertext.is_null()),
                )
                .set((
                    manager_credential::dsl::credential_epoch.eq($new_epoch),
                    manager_credential::dsl::totp_secret_ciphertext
                        .eq(Some(&$totp.secret_ciphertext)),
                    manager_credential::dsl::totp_secret_nonce.eq(Some(&$totp.secret_nonce)),
                    manager_credential::dsl::totp_secret_format_version
                        .eq(Some($totp.secret_format_version)),
                    manager_credential::dsl::totp_secret_key_fingerprint
                        .eq(Some(&$totp.secret_key_fingerprint)),
                    manager_credential::dsl::totp_last_accepted_step
                        .eq(Some($totp.last_accepted_step)),
                    manager_credential::dsl::totp_enabled_at.eq(Some($totp.enabled_at)),
                    manager_credential::dsl::updated_at.eq($now),
                ))
                .returning(ManagerCredentialDb::as_returning());
                diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(query, &mut *$conn)
                    .await
                    .optional()
            }
            ExpectedManagerTotpState::Enabled => {
                let query = diesel::update(
                    target.filter(
                        manager_credential::dsl::totp_secret_ciphertext
                            .is_not_null()
                            .and(
                                manager_credential::dsl::totp_last_accepted_step
                                    .lt($totp.last_accepted_step),
                            ),
                    ),
                )
                .set((
                    manager_credential::dsl::credential_epoch.eq($new_epoch),
                    manager_credential::dsl::totp_secret_ciphertext
                        .eq(Some(&$totp.secret_ciphertext)),
                    manager_credential::dsl::totp_secret_nonce.eq(Some(&$totp.secret_nonce)),
                    manager_credential::dsl::totp_secret_format_version
                        .eq(Some($totp.secret_format_version)),
                    manager_credential::dsl::totp_secret_key_fingerprint
                        .eq(Some(&$totp.secret_key_fingerprint)),
                    manager_credential::dsl::totp_last_accepted_step
                        .eq(Some($totp.last_accepted_step)),
                    manager_credential::dsl::totp_enabled_at.eq(Some($totp.enabled_at)),
                    manager_credential::dsl::updated_at.eq($now),
                ))
                .returning(ManagerCredentialDb::as_returning());
                diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(query, &mut *$conn)
                    .await
                    .optional()
            }
        }
        .map_err(|error| storage_error("TOTP credential install failed", error))?;
        let credential = require_lifecycle_update_async!($conn, $expected_epoch, updated).from_db();
        let query = diesel::delete(
            manager_totp_recovery_code::table
                .filter(manager_totp_recovery_code::dsl::manager_id.eq(MANAGER_ID)),
        );
        diesel_async::RunQueryDsl::execute(query, &mut *$conn)
            .await
            .map_err(|error| storage_error("old recovery code deletion failed", error))?;
        let recovery_rows = $recovery_codes
            .iter()
            .map(NewManagerTotpRecoveryCodeDb::from_new)
            .collect::<Vec<_>>();
        for recovery_row in &recovery_rows {
            let query = diesel::insert_into(manager_totp_recovery_code::table).values(recovery_row);
            diesel_async::RunQueryDsl::execute(query, &mut *$conn)
                .await
                .map_err(|error| storage_error("recovery code replacement failed", error))?;
        }
        let revoked_sessions = revoke_active_async!($conn, $now, $revoke_reason)?;
        let session = insert_session_async!($conn, $new_session)?;
        Ok(ManagerCredentialSessionMutation {
            credential,
            session,
            revoked_sessions,
        })
    }};
}

macro_rules! disable_totp_async_body {
    ($conn:expr, $expected_epoch:expr, $new_epoch:expr, $new_session:expr, $now:expr, $revoke_reason:expr) => {{
        let query = diesel::update(
            manager_credential::table.filter(
                manager_credential::dsl::manager_id
                    .eq(MANAGER_ID)
                    .and(manager_credential::dsl::credential_epoch.eq($expected_epoch))
                    .and(manager_credential::dsl::totp_secret_ciphertext.is_not_null()),
            ),
        )
        .set((
            manager_credential::dsl::credential_epoch.eq($new_epoch),
            manager_credential::dsl::totp_secret_ciphertext.eq(Option::<Vec<u8>>::None),
            manager_credential::dsl::totp_secret_nonce.eq(Option::<Vec<u8>>::None),
            manager_credential::dsl::totp_secret_format_version.eq(Option::<i32>::None),
            manager_credential::dsl::totp_secret_key_fingerprint.eq(Option::<String>::None),
            manager_credential::dsl::totp_last_accepted_step.eq(Option::<i64>::None),
            manager_credential::dsl::totp_enabled_at.eq(Option::<i64>::None),
            manager_credential::dsl::updated_at.eq($now),
        ))
        .returning(ManagerCredentialDb::as_returning());
        let updated =
            diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(query, &mut *$conn)
                .await
                .optional()
                .map_err(|error| storage_error("TOTP disable failed", error))?;
        let credential = require_lifecycle_update_async!($conn, $expected_epoch, updated).from_db();
        let query = diesel::delete(
            manager_totp_recovery_code::table
                .filter(manager_totp_recovery_code::dsl::manager_id.eq(MANAGER_ID)),
        );
        diesel_async::RunQueryDsl::execute(query, &mut *$conn)
            .await
            .map_err(|error| storage_error("recovery code deletion failed", error))?;
        let revoked_sessions = revoke_active_async!($conn, $now, $revoke_reason)?;
        let session = insert_session_async!($conn, $new_session)?;
        Ok(ManagerCredentialSessionMutation {
            credential,
            session,
            revoked_sessions,
        })
    }};
}

macro_rules! consume_recovery_async_body {
    ($conn:expr, $expected_epoch:expr, $code_id:expr, $expected_verifier:expr, $now:expr, $revoke_reason:expr) => {{
        let active_credential = manager_credential::table.filter(
            manager_credential::dsl::manager_id
                .eq(MANAGER_ID)
                .and(manager_credential::dsl::credential_epoch.eq($expected_epoch))
                .and(manager_credential::dsl::totp_secret_ciphertext.is_not_null()),
        );
        let query = diesel::delete(
            manager_totp_recovery_code::table.filter(
                manager_totp_recovery_code::dsl::code_id
                    .eq($code_id)
                    .and(manager_totp_recovery_code::dsl::manager_id.eq(MANAGER_ID))
                    .and(manager_totp_recovery_code::dsl::code_verifier.eq($expected_verifier))
                    .and(diesel::dsl::exists(active_credential)),
            ),
        );
        let deleted = diesel_async::RunQueryDsl::execute(query, &mut *$conn)
            .await
            .map_err(|error| storage_error("recovery code consumption failed", error))?;
        if deleted != 1 {
            let query = manager_credential::table
                .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID))
                .select((
                    manager_credential::dsl::credential_epoch,
                    manager_credential::dsl::totp_secret_ciphertext.is_not_null(),
                ));
            let state = diesel_async::RunQueryDsl::first::<(String, bool)>(query, &mut *$conn)
                .await
                .optional()
                .map_err(|error| storage_error("recovery conflict lookup failed", error))?;
            return match state {
                Some((epoch, _)) if epoch.as_str() != $expected_epoch.as_str() => {
                    Err(ManagerCredentialRepositoryError::EpochConflict)
                }
                Some((_, false)) | None => Err(ManagerCredentialRepositoryError::StateConflict),
                Some((_, true)) => Err(ManagerCredentialRepositoryError::RecoveryCodeConflict),
            };
        }
        let revoked_sessions = revoke_active_async!($conn, $now, $revoke_reason)?;
        Ok(ManagerRecoveryStartMutation { revoked_sessions })
    }};
}

impl ManagerCredential {
    pub fn totp_secret(&self) -> Option<ManagerTotpSecret> {
        match (
            self.totp_secret_ciphertext.as_ref(),
            self.totp_secret_nonce.as_ref(),
            self.totp_secret_format_version,
            self.totp_secret_key_fingerprint.as_ref(),
            self.totp_last_accepted_step,
            self.totp_enabled_at,
        ) {
            (
                Some(secret_ciphertext),
                Some(secret_nonce),
                Some(secret_format_version),
                Some(secret_key_fingerprint),
                Some(last_accepted_step),
                Some(enabled_at),
            ) => Some(ManagerTotpSecret {
                secret_ciphertext: secret_ciphertext.clone(),
                secret_nonce: secret_nonce.clone(),
                secret_format_version,
                secret_key_fingerprint: secret_key_fingerprint.clone(),
                last_accepted_step,
                enabled_at,
            }),
            _ => None,
        }
    }

    pub async fn load(
        database: &DatabaseRuntime,
    ) -> Result<Option<Self>, ManagerCredentialRepositoryError> {
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = manager_credential::table
                            .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID))
                            .select(ManagerCredentialDb::as_select());
                        diesel_async::RunQueryDsl::first::<ManagerCredentialDb>(query, &mut **conn)
                            .await
                            .optional()
                            .map(|row| row.map(ManagerCredentialDb::from_db))
                            .map_err(|error| storage_error("load failed", error))
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn set_totp_secret_for_test(
        database: &DatabaseRuntime,
        totp: ManagerTotpSecret,
    ) -> Result<(), ManagerCredentialRepositoryError> {
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
                            manager_credential::table
                                .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID)),
                        )
                        .set((
                            manager_credential::dsl::totp_secret_ciphertext
                                .eq(Some(totp.secret_ciphertext)),
                            manager_credential::dsl::totp_secret_nonce.eq(Some(totp.secret_nonce)),
                            manager_credential::dsl::totp_secret_format_version
                                .eq(Some(totp.secret_format_version)),
                            manager_credential::dsl::totp_secret_key_fingerprint
                                .eq(Some(totp.secret_key_fingerprint)),
                            manager_credential::dsl::totp_last_accepted_step
                                .eq(Some(totp.last_accepted_step)),
                            manager_credential::dsl::totp_enabled_at.eq(Some(totp.enabled_at)),
                        ));
                        let updated = diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map_err(|error| {
                                storage_error("test TOTP fixture update failed", error)
                            })?;
                        if updated == 1 {
                            Ok(())
                        } else {
                            Err(ManagerCredentialRepositoryError::StateConflict)
                        }
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn corrupt_totp_ciphertext_for_test(
        database: &DatabaseRuntime,
    ) -> Result<(), ManagerCredentialRepositoryError> {
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
                            manager_credential::table
                                .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID)),
                        )
                        .set(manager_credential::dsl::totp_secret_ciphertext.eq(Some(vec![1_u8])));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map(|_| ())
                            .map_err(|error| storage_error("test TOTP corruption failed", error))
                    })
                })
            })
            .await
    }

    pub async fn insert_once(
        database: &DatabaseRuntime,
        new_credential: NewManagerCredential,
    ) -> Result<Self, ManagerCredentialRepositoryError> {
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::insert_into(manager_credential::table)
                            .values(NewManagerCredentialDb::from_new(&new_credential))
                            .returning(ManagerCredentialDb::as_returning());
                        diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(ManagerCredentialDb::from_db)
                        .map_err(map_insert_error)
                    })
                })
            })
            .await
    }

    pub async fn rotate_if_epoch(
        database: &DatabaseRuntime,
        expected_epoch: &str,
        rotated: RotatedManagerCredential,
    ) -> Result<Self, ManagerCredentialRepositoryError> {
        let expected_epoch = expected_epoch.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
                            manager_credential::table.filter(
                                manager_credential::dsl::manager_id.eq(MANAGER_ID).and(
                                    manager_credential::dsl::credential_epoch.eq(&expected_epoch),
                                ),
                            ),
                        )
                        .set((
                            manager_credential::dsl::password_verifier
                                .eq(rotated.password_verifier),
                            manager_credential::dsl::credential_epoch.eq(rotated.credential_epoch),
                            manager_credential::dsl::updated_at.eq(rotated.now),
                        ))
                        .returning(ManagerCredentialDb::as_returning());
                        diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .optional()
                        .map_err(|error| storage_error("rotation failed", error))?
                        .map(ManagerCredentialDb::from_db)
                        .ok_or(ManagerCredentialRepositoryError::EpochConflict)
                    })
                })
            })
            .await
    }

    pub async fn advance_totp_step_if_newer(
        database: &DatabaseRuntime,
        expected_epoch: &str,
        matched_step: i64,
        now: i64,
    ) -> Result<Self, ManagerCredentialRepositoryError> {
        let expected_epoch = expected_epoch.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
                            manager_credential::table.filter(
                                manager_credential::dsl::manager_id
                                    .eq(MANAGER_ID)
                                    .and(
                                        manager_credential::dsl::credential_epoch
                                            .eq(&expected_epoch),
                                    )
                                    .and(
                                        manager_credential::dsl::totp_secret_ciphertext
                                            .is_not_null(),
                                    )
                                    .and(manager_credential::dsl::totp_secret_nonce.is_not_null())
                                    .and(
                                        manager_credential::dsl::totp_secret_format_version
                                            .is_not_null(),
                                    )
                                    .and(
                                        manager_credential::dsl::totp_secret_key_fingerprint
                                            .is_not_null(),
                                    )
                                    .and(
                                        manager_credential::dsl::totp_last_accepted_step
                                            .lt(matched_step),
                                    )
                                    .and(manager_credential::dsl::totp_enabled_at.is_not_null()),
                            ),
                        )
                        .set((
                            manager_credential::dsl::totp_last_accepted_step.eq(Some(matched_step)),
                            manager_credential::dsl::updated_at.eq(now),
                        ))
                        .returning(ManagerCredentialDb::as_returning());
                        let updated = diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .optional()
                        .map_err(|error| storage_error("TOTP step advance failed", error))?;
                        if let Some(updated) = updated {
                            return Ok(updated.from_db());
                        }
                        let query = manager_credential::table
                            .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID))
                            .select((
                                manager_credential::dsl::credential_epoch,
                                manager_credential::dsl::totp_last_accepted_step,
                            ));
                        let current = diesel_async::RunQueryDsl::first::<(String, Option<i64>)>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .optional()
                        .map_err(|error| {
                            storage_error("TOTP step conflict lookup failed", error)
                        })?;
                        match current {
                            Some((epoch, _)) if epoch != expected_epoch => {
                                Err(ManagerCredentialRepositoryError::EpochConflict)
                            }
                            Some((_, Some(_))) => {
                                Err(ManagerCredentialRepositoryError::TotpStepConflict)
                            }
                            _ => Err(ManagerCredentialRepositoryError::StateConflict),
                        }
                    })
                })
            })
            .await
    }

    pub async fn bootstrap_with_session(
        database: &DatabaseRuntime,
        new_credential: NewManagerCredential,
        new_session: NewManagerAuthInstance,
        revoke_reason: &str,
    ) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
        let revoke_reason = revoke_reason.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    use diesel_async::AsyncConnection;
                    match connection {
                        RuntimeConnection::Postgres(conn) => {
                            use crate::database::_postgres_schema::{
                                manager_auth_instance, manager_credential,
                            };
                            use crate::database::manager_auth_instance::_postgres_model::{
                                ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
                            };
                            use _postgres_model::{ManagerCredentialDb, NewManagerCredentialDb};
                            conn.transaction(async move |conn| {
                                let query = diesel::insert_into(manager_credential::table)
                                    .values(NewManagerCredentialDb::from_new(&new_credential))
                                    .returning(ManagerCredentialDb::as_returning());
                                let credential =
                                    diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                                        query, &mut *conn,
                                    )
                                    .await
                                    .map(ManagerCredentialDb::from_db)
                                    .map_err(map_insert_error)?;
                                let revoked_sessions =
                                    revoke_active_async!(conn, new_credential.now, &revoke_reason)?;
                                let session = insert_session_async!(conn, &new_session)?;
                                Ok(ManagerCredentialSessionMutation {
                                    credential,
                                    session,
                                    revoked_sessions,
                                })
                            })
                            .await
                        }
                        RuntimeConnection::Sqlite(conn) => {
                            use crate::database::_sqlite_schema::{
                                manager_auth_instance, manager_credential,
                            };
                            use crate::database::manager_auth_instance::_sqlite_model::{
                                ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
                            };
                            use _sqlite_model::{ManagerCredentialDb, NewManagerCredentialDb};
                            conn.transaction(async move |conn| {
                                let query = diesel::insert_into(manager_credential::table)
                                    .values(NewManagerCredentialDb::from_new(&new_credential))
                                    .returning(ManagerCredentialDb::as_returning());
                                let credential =
                                    diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                                        query, &mut *conn,
                                    )
                                    .await
                                    .map(ManagerCredentialDb::from_db)
                                    .map_err(map_insert_error)?;
                                let revoked_sessions =
                                    revoke_active_async!(conn, new_credential.now, &revoke_reason)?;
                                let session = insert_session_async!(conn, &new_session)?;
                                Ok(ManagerCredentialSessionMutation {
                                    credential,
                                    session,
                                    revoked_sessions,
                                })
                            })
                            .await
                        }
                    }
                })
            })
            .await
    }

    pub async fn rotate_with_session(
        database: &DatabaseRuntime,
        expected_epoch: &str,
        rotated: RotatedManagerCredential,
        new_session: NewManagerAuthInstance,
        revoke_reason: &str,
    ) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
        let expected_epoch = expected_epoch.to_string();
        let revoke_reason = revoke_reason.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    use diesel_async::AsyncConnection;
                    match connection {
                        RuntimeConnection::Postgres(conn) => {
                            use crate::database::_postgres_schema::{
                                manager_auth_instance, manager_credential,
                            };
                            use crate::database::manager_auth_instance::_postgres_model::{
                                ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
                            };
                            use _postgres_model::ManagerCredentialDb;
                            conn.transaction(async move |conn| {
                                let query = diesel::update(
                                    manager_credential::table.filter(
                                        manager_credential::dsl::manager_id.eq(MANAGER_ID).and(
                                            manager_credential::dsl::credential_epoch
                                                .eq(&expected_epoch),
                                        ),
                                    ),
                                )
                                .set((
                                    manager_credential::dsl::password_verifier
                                        .eq(&rotated.password_verifier),
                                    manager_credential::dsl::credential_epoch
                                        .eq(&rotated.credential_epoch),
                                    manager_credential::dsl::updated_at.eq(rotated.now),
                                ))
                                .returning(ManagerCredentialDb::as_returning());
                                let credential =
                                    diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                                        query, &mut *conn,
                                    )
                                    .await
                                    .optional()
                                    .map_err(|error| storage_error("rotation failed", error))?
                                    .ok_or(ManagerCredentialRepositoryError::EpochConflict)?
                                    .from_db();
                                let revoked_sessions =
                                    revoke_active_async!(conn, rotated.now, &revoke_reason)?;
                                let session = insert_session_async!(conn, &new_session)?;
                                Ok(ManagerCredentialSessionMutation {
                                    credential,
                                    session,
                                    revoked_sessions,
                                })
                            })
                            .await
                        }
                        RuntimeConnection::Sqlite(conn) => {
                            use crate::database::_sqlite_schema::{
                                manager_auth_instance, manager_credential,
                            };
                            use crate::database::manager_auth_instance::_sqlite_model::{
                                ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
                            };
                            use _sqlite_model::ManagerCredentialDb;
                            conn.transaction(async move |conn| {
                                let query = diesel::update(
                                    manager_credential::table.filter(
                                        manager_credential::dsl::manager_id.eq(MANAGER_ID).and(
                                            manager_credential::dsl::credential_epoch
                                                .eq(&expected_epoch),
                                        ),
                                    ),
                                )
                                .set((
                                    manager_credential::dsl::password_verifier
                                        .eq(&rotated.password_verifier),
                                    manager_credential::dsl::credential_epoch
                                        .eq(&rotated.credential_epoch),
                                    manager_credential::dsl::updated_at.eq(rotated.now),
                                ))
                                .returning(ManagerCredentialDb::as_returning());
                                let credential =
                                    diesel_async::RunQueryDsl::get_result::<ManagerCredentialDb>(
                                        query, &mut *conn,
                                    )
                                    .await
                                    .optional()
                                    .map_err(|error| storage_error("rotation failed", error))?
                                    .ok_or(ManagerCredentialRepositoryError::EpochConflict)?
                                    .from_db();
                                let revoked_sessions =
                                    revoke_active_async!(conn, rotated.now, &revoke_reason)?;
                                let session = insert_session_async!(conn, &new_session)?;
                                Ok(ManagerCredentialSessionMutation {
                                    credential,
                                    session,
                                    revoked_sessions,
                                })
                            })
                            .await
                        }
                    }
                })
            })
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn install_totp_with_session(
        database: &DatabaseRuntime,
        expected_epoch: &str,
        expected_state: ExpectedManagerTotpState,
        new_epoch: &str,
        totp: ManagerTotpSecret,
        recovery_codes: Vec<NewManagerTotpRecoveryCode>,
        new_session: NewManagerAuthInstance,
        now: i64,
        revoke_reason: &str,
    ) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
        validate_totp_lifecycle_input(new_epoch, &recovery_codes, &new_session)?;
        let expected_epoch = expected_epoch.to_string();
        let new_epoch = new_epoch.to_string();
        let revoke_reason = revoke_reason.to_string();
        database.run_domain_error(DatabaseWorkload::Foreground, move |connection| Box::pin(async move {
            use diesel_async::AsyncConnection;
            match connection {
                RuntimeConnection::Postgres(conn) => {
                    use crate::database::_postgres_schema::{manager_auth_instance, manager_credential, manager_totp_recovery_code};
                    use crate::database::manager_auth_instance::_postgres_model::{ManagerAuthInstanceDb, NewManagerAuthInstanceDb};
                    use crate::database::manager_totp_recovery_code::_postgres_model::NewManagerTotpRecoveryCodeDb;
                    use _postgres_model::ManagerCredentialDb;
                    conn.transaction(async move |conn| {
                        install_totp_async_body!(conn, &expected_epoch, expected_state, &new_epoch, &totp, &recovery_codes, &new_session, now, &revoke_reason)
                    }).await
                }
                RuntimeConnection::Sqlite(conn) => {
                    use crate::database::_sqlite_schema::{manager_auth_instance, manager_credential, manager_totp_recovery_code};
                    use crate::database::manager_auth_instance::_sqlite_model::{ManagerAuthInstanceDb, NewManagerAuthInstanceDb};
                    use crate::database::manager_totp_recovery_code::_sqlite_model::NewManagerTotpRecoveryCodeDb;
                    use _sqlite_model::ManagerCredentialDb;
                    conn.transaction(async move |conn| {
                        install_totp_async_body!(conn, &expected_epoch, expected_state, &new_epoch, &totp, &recovery_codes, &new_session, now, &revoke_reason)
                    }).await
                }
            }
        })).await
    }

    pub async fn disable_totp_with_session(
        database: &DatabaseRuntime,
        expected_epoch: &str,
        new_epoch: &str,
        new_session: NewManagerAuthInstance,
        now: i64,
        revoke_reason: &str,
    ) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
        validate_new_session(new_epoch, &new_session)?;
        let expected_epoch = expected_epoch.to_string();
        let new_epoch = new_epoch.to_string();
        let revoke_reason = revoke_reason.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    use diesel_async::AsyncConnection;
                    match connection {
                        RuntimeConnection::Postgres(conn) => {
                            use crate::database::_postgres_schema::{
                                manager_auth_instance, manager_credential,
                                manager_totp_recovery_code,
                            };
                            use crate::database::manager_auth_instance::_postgres_model::{
                                ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
                            };
                            use _postgres_model::ManagerCredentialDb;
                            conn.transaction(async move |conn| {
                                disable_totp_async_body!(
                                    conn,
                                    &expected_epoch,
                                    &new_epoch,
                                    &new_session,
                                    now,
                                    &revoke_reason
                                )
                            })
                            .await
                        }
                        RuntimeConnection::Sqlite(conn) => {
                            use crate::database::_sqlite_schema::{
                                manager_auth_instance, manager_credential,
                                manager_totp_recovery_code,
                            };
                            use crate::database::manager_auth_instance::_sqlite_model::{
                                ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
                            };
                            use _sqlite_model::ManagerCredentialDb;
                            conn.transaction(async move |conn| {
                                disable_totp_async_body!(
                                    conn,
                                    &expected_epoch,
                                    &new_epoch,
                                    &new_session,
                                    now,
                                    &revoke_reason
                                )
                            })
                            .await
                        }
                    }
                })
            })
            .await
    }

    pub async fn consume_recovery_code_and_revoke_sessions(
        database: &DatabaseRuntime,
        expected_epoch: &str,
        code_id: &str,
        expected_verifier: &str,
        now: i64,
        revoke_reason: &str,
    ) -> Result<ManagerRecoveryStartMutation, ManagerCredentialRepositoryError> {
        let expected_epoch = expected_epoch.to_string();
        let code_id = code_id.to_string();
        let expected_verifier = expected_verifier.to_string();
        let revoke_reason = revoke_reason.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    use diesel_async::AsyncConnection;
                    match connection {
                        RuntimeConnection::Postgres(conn) => {
                            use crate::database::_postgres_schema::{
                                manager_auth_instance, manager_credential,
                                manager_totp_recovery_code,
                            };
                            conn.transaction(async move |conn| {
                                consume_recovery_async_body!(
                                    conn,
                                    &expected_epoch,
                                    &code_id,
                                    &expected_verifier,
                                    now,
                                    &revoke_reason
                                )
                            })
                            .await
                        }
                        RuntimeConnection::Sqlite(conn) => {
                            use crate::database::_sqlite_schema::{
                                manager_auth_instance, manager_credential,
                                manager_totp_recovery_code,
                            };
                            conn.transaction(async move |conn| {
                                consume_recovery_async_body!(
                                    conn,
                                    &expected_epoch,
                                    &code_id,
                                    &expected_verifier,
                                    now,
                                    &revoke_reason
                                )
                            })
                            .await
                        }
                    }
                })
            })
            .await
    }
}

const RECOVERY_CODE_COUNT: usize = 10;

fn validate_new_session(
    new_epoch: &str,
    new_session: &NewManagerAuthInstance,
) -> Result<(), ManagerCredentialRepositoryError> {
    if new_session.manager_id != MANAGER_ID
        || new_session.manager_subject != MANAGER_SUBJECT
        || new_session.credential_epoch != new_epoch
        || new_session.revoked_at.is_some()
        || new_session.revoked_reason.is_some()
    {
        return Err(ManagerCredentialRepositoryError::ContractViolation(
            "new session must be active and bound to the replacement manager credential epoch"
                .to_string(),
        ));
    }
    Ok(())
}

fn validate_totp_lifecycle_input(
    new_epoch: &str,
    recovery_codes: &[NewManagerTotpRecoveryCode],
    new_session: &NewManagerAuthInstance,
) -> Result<(), ManagerCredentialRepositoryError> {
    validate_new_session(new_epoch, new_session)?;
    if recovery_codes.len() != RECOVERY_CODE_COUNT {
        return Err(ManagerCredentialRepositoryError::ContractViolation(
            "TOTP lifecycle must install exactly ten recovery code verifiers".to_string(),
        ));
    }
    let mut code_ids = recovery_codes
        .iter()
        .map(|code| code.code_id.as_str())
        .collect::<Vec<_>>();
    code_ids.sort_unstable();
    if code_ids.windows(2).any(|window| window[0] == window[1]) {
        return Err(ManagerCredentialRepositoryError::ContractViolation(
            "TOTP recovery code identifiers must be unique".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDatabase;
    use crate::database::manager_totp_recovery_code::{
        ManagerTotpRecoveryCode, NewManagerTotpRecoveryCode,
    };
    use crate::utils::ID_GENERATOR;
    use diesel::{
        Connection, PgConnection, QueryableByName, RunQueryDsl, connection::SimpleConnection,
        sql_types::Text,
    };
    use std::{env, sync::Arc};
    use tokio::sync::Barrier;

    const POSTGRES_SMOKE_URL_ENV: &str = "CYDER_R1_POSTGRES_MIGRATION_SMOKE_URL";
    const POSTGRES_SMOKE_DATABASE: &str = "cyder_r1_migration_smoke";

    #[derive(QueryableByName)]
    struct DatabaseNameRow {
        #[diesel(sql_type = Text)]
        name: String,
    }

    fn new_credential(epoch: &str, now: i64) -> NewManagerCredential {
        NewManagerCredential {
            password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$fixture".to_string(),
            credential_epoch: epoch.to_string(),
            now,
        }
    }

    fn new_session(jti: &str, epoch: &str, now: i64) -> NewManagerAuthInstance {
        NewManagerAuthInstance {
            id: ID_GENERATOR.generate_id(),
            manager_id: MANAGER_ID,
            manager_subject: MANAGER_SUBJECT.to_string(),
            current_refresh_jti: jti.to_string(),
            refresh_generation: super::super::manager_auth_instance::INITIAL_REFRESH_GENERATION,
            session_version: super::super::manager_auth_instance::INITIAL_SESSION_VERSION,
            signing_key_id: "a".repeat(64),
            credential_epoch: epoch.to_string(),
            created_at: now,
            last_rotated_at: now,
            idle_expires_at: now + 3_600,
            absolute_expires_at: now + 7_200,
            revoked_at: None,
            revoked_reason: None,
        }
    }

    fn totp_secret(step: i64, enabled_at: i64) -> ManagerTotpSecret {
        ManagerTotpSecret {
            secret_ciphertext: vec![1, 2, 3, 4],
            secret_nonce: vec![5; 24],
            secret_format_version: 1,
            secret_key_fingerprint: "b".repeat(64),
            last_accepted_step: step,
            enabled_at,
        }
    }

    fn recovery_codes(prefix: char, now: i64) -> Vec<NewManagerTotpRecoveryCode> {
        (0..RECOVERY_CODE_COUNT)
            .map(|index| NewManagerTotpRecoveryCode {
                code_id: format!("{prefix}{index:03}"),
                code_verifier: format!("verifier-{prefix}-{index}"),
                created_at: now,
            })
            .collect()
    }

    async fn assert_lifecycle_state_unchanged(
        database: &DatabaseRuntime,
        expected_credential: &ManagerCredential,
        expected_recovery_codes: &[ManagerTotpRecoveryCode],
        expected_session: &ManagerAuthInstance,
        now: i64,
    ) {
        let credential = ManagerCredential::load(database)
            .await
            .expect("credential should load")
            .expect("credential should exist");
        assert!(
            credential == *expected_credential,
            "credential changes must roll back"
        );
        assert_eq!(
            ManagerTotpRecoveryCode::list(database)
                .await
                .expect("recovery codes should load"),
            expected_recovery_codes
        );
        let session = ManagerAuthInstance::get_instance(database, expected_session.id)
            .await
            .expect("session should load")
            .expect("session should remain");
        assert_eq!(
            serde_json::to_value(&session).expect("session should serialize"),
            serde_json::to_value(expected_session).expect("expected session should serialize"),
            "session mutation must roll back"
        );
        let active = ManagerAuthInstance::list_active_instances(database, now)
            .await
            .expect("active sessions should load");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, expected_session.id);
    }

    #[tokio::test]
    async fn manager_credential_repository_covers_singleton_insert_load_and_epoch_cas() {
        let database =
            TestDatabase::new_sqlite_default("manager-credential-repository.sqlite").await;
        let runtime = database.runtime();
        assert!(
            ManagerCredential::load(&runtime)
                .await
                .expect("load should succeed")
                .is_none()
        );

        let inserted = ManagerCredential::insert_once(
            &runtime,
            new_credential("018fa7d8-6a00-7c9a-8f7e-111111111111", 1_000),
        )
        .await
        .expect("first insert should succeed");
        assert_eq!(inserted.manager_id, MANAGER_ID);
        assert_eq!(inserted.manager_subject, MANAGER_SUBJECT);
        assert_ne!(inserted.password_verifier, "fixture-password");

        let duplicate = ManagerCredential::insert_once(
            &runtime,
            new_credential("018fa7d8-6a00-7c9a-8f7e-222222222222", 1_001),
        )
        .await;
        assert!(matches!(
            duplicate,
            Err(ManagerCredentialRepositoryError::AlreadyInitialized)
        ));

        let stale = ManagerCredential::rotate_if_epoch(
            &runtime,
            "018fa7d8-6a00-7c9a-8f7e-000000000000",
            RotatedManagerCredential {
                password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$rotated".to_string(),
                credential_epoch: "018fa7d8-6a00-7c9a-8f7e-333333333333".to_string(),
                now: 1_002,
            },
        )
        .await;
        assert!(matches!(
            stale,
            Err(ManagerCredentialRepositoryError::EpochConflict)
        ));

        let rotated = ManagerCredential::rotate_if_epoch(
            &runtime,
            &inserted.credential_epoch,
            RotatedManagerCredential {
                password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$rotated".to_string(),
                credential_epoch: "018fa7d8-6a00-7c9a-8f7e-333333333333".to_string(),
                now: 1_003,
            },
        )
        .await
        .expect("matching epoch should rotate");
        assert_eq!(rotated.created_at, 1_000);
        assert_eq!(rotated.updated_at, 1_003);
    }

    #[tokio::test]
    async fn manager_credential_transaction_rolls_back_credential_and_revocation_on_session_failure()
     {
        let database =
            TestDatabase::new_sqlite_default("manager-credential-transaction.sqlite").await;
        let runtime = database.runtime();
        let existing = ManagerAuthInstance::create_instance(
            &runtime,
            "duplicate-session-jti".to_string(),
            "a".repeat(64),
            "018fa7d8-6a00-7c9a-8f7e-444444444444".to_string(),
            1_000,
            4_600,
            8_200,
        )
        .await
        .expect("existing session should create");

        let result = ManagerCredential::bootstrap_with_session(
            &runtime,
            new_credential("018fa7d8-6a00-7c9a-8f7e-444444444444", 1_100),
            new_session(
                "duplicate-session-jti",
                "018fa7d8-6a00-7c9a-8f7e-444444444444",
                1_100,
            ),
            "credential_bootstrap",
        )
        .await;
        assert!(matches!(
            result,
            Err(ManagerCredentialRepositoryError::Storage(_))
        ));
        assert!(
            ManagerCredential::load(&runtime)
                .await
                .expect("load should succeed")
                .is_none()
        );

        let existing = ManagerAuthInstance::get_instance(&runtime, existing.id)
            .await
            .expect("session lookup should succeed")
            .expect("existing session should remain");
        assert_eq!(existing.revoked_at, None);
        assert_eq!(existing.revoked_reason, None);
    }

    #[tokio::test]
    async fn manager_totp_step_cas_allows_only_one_concurrent_acceptance() {
        let database = TestDatabase::new_sqlite_default("manager-totp-step-cas.sqlite").await;
        let runtime = database.runtime();
        let initial_epoch = "018fa7d8-6a00-7c9a-8f7e-111111111111";
        let enabled_epoch = "018fa7d8-6a00-7c9a-8f7e-222222222222";
        ManagerCredential::insert_once(&runtime, new_credential(initial_epoch, 1_000))
            .await
            .expect("credential should insert");
        ManagerCredential::install_totp_with_session(
            &runtime,
            initial_epoch,
            ExpectedManagerTotpState::Disabled,
            enabled_epoch,
            totp_secret(100, 1_010),
            recovery_codes('A', 1_010),
            new_session("enabled-session", enabled_epoch, 1_010),
            1_010,
            "totp_enrolled",
        )
        .await
        .expect("TOTP enrollment should succeed");

        let barrier = Arc::new(Barrier::new(3));
        let mut workers = Vec::new();
        for now in [1_020, 1_021] {
            let runtime = Arc::clone(&runtime);
            let barrier = Arc::clone(&barrier);
            workers.push(tokio::spawn(async move {
                barrier.wait().await;
                ManagerCredential::advance_totp_step_if_newer(&runtime, enabled_epoch, 101, now)
                    .await
            }));
        }
        barrier.wait().await;
        let mut results = Vec::new();
        for worker in workers {
            results.push(worker.await.expect("CAS worker should not panic"));
        }
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        assert_eq!(
            results
                .iter()
                .filter(|result| {
                    matches!(
                        result,
                        Err(ManagerCredentialRepositoryError::TotpStepConflict)
                    )
                })
                .count(),
            1
        );

        let credential = ManagerCredential::load(&runtime)
            .await
            .expect("credential should load")
            .expect("credential should exist");
        assert_eq!(credential.totp_last_accepted_step, Some(101));
        for rejected_step in [100, 101] {
            assert!(matches!(
                ManagerCredential::advance_totp_step_if_newer(
                    &runtime,
                    enabled_epoch,
                    rejected_step,
                    1_030
                )
                .await,
                Err(ManagerCredentialRepositoryError::TotpStepConflict)
            ));
        }
    }

    #[tokio::test]
    async fn manager_totp_lifecycle_transactions_replace_codes_epochs_and_sessions_atomically() {
        let database = TestDatabase::new_sqlite_default("manager-totp-lifecycle.sqlite").await;
        let runtime = database.runtime();
        let initial_epoch = "018fa7d8-6a00-7c9a-8f7e-100000000000";
        let enabled_epoch = "018fa7d8-6a00-7c9a-8f7e-200000000000";
        let replaced_epoch = "018fa7d8-6a00-7c9a-8f7e-300000000000";
        let recovered_epoch = "018fa7d8-6a00-7c9a-8f7e-400000000000";
        let disabled_epoch = "018fa7d8-6a00-7c9a-8f7e-500000000000";

        ManagerCredential::insert_once(&runtime, new_credential(initial_epoch, 1_000))
            .await
            .expect("credential should insert");
        let old_session = ManagerAuthInstance::create_instance(
            &runtime,
            "pre-enrollment-session".to_string(),
            "a".repeat(64),
            initial_epoch.to_string(),
            1_001,
            4_601,
            8_201,
        )
        .await
        .expect("pre-enrollment session should create");

        let enrolled = ManagerCredential::install_totp_with_session(
            &runtime,
            initial_epoch,
            ExpectedManagerTotpState::Disabled,
            enabled_epoch,
            totp_secret(10, 1_010),
            recovery_codes('A', 1_010),
            new_session("enrolled-session", enabled_epoch, 1_010),
            1_010,
            "totp_enrolled",
        )
        .await
        .expect("enrollment transaction should succeed");
        assert_eq!(enrolled.revoked_sessions, 1);
        assert_eq!(
            ManagerTotpRecoveryCode::list(&runtime).await.unwrap().len(),
            10
        );
        assert_eq!(
            ManagerAuthInstance::get_instance(&runtime, old_session.id)
                .await
                .unwrap()
                .unwrap()
                .revoked_reason
                .as_deref(),
            Some("totp_enrolled")
        );

        let replaced = ManagerCredential::install_totp_with_session(
            &runtime,
            enabled_epoch,
            ExpectedManagerTotpState::Enabled,
            replaced_epoch,
            totp_secret(20, 1_020),
            recovery_codes('B', 1_020),
            new_session("replaced-session", replaced_epoch, 1_020),
            1_020,
            "totp_replaced",
        )
        .await
        .expect("replacement transaction should succeed");
        assert_eq!(replaced.revoked_sessions, 1);
        let replacement_codes = ManagerTotpRecoveryCode::list(&runtime).await.unwrap();
        assert_eq!(replacement_codes.len(), 10);
        assert!(
            replacement_codes
                .iter()
                .all(|code| code.code_id.starts_with('B'))
        );

        let consumed = ManagerCredential::consume_recovery_code_and_revoke_sessions(
            &runtime,
            replaced_epoch,
            "B000",
            "verifier-B-0",
            1_030,
            "totp_recovery_started",
        )
        .await
        .expect("recovery start should consume exactly one verifier");
        assert_eq!(consumed.revoked_sessions, 1);
        assert_eq!(
            ManagerTotpRecoveryCode::list(&runtime).await.unwrap().len(),
            9
        );
        assert!(matches!(
            ManagerCredential::consume_recovery_code_and_revoke_sessions(
                &runtime,
                replaced_epoch,
                "B000",
                "verifier-B-0",
                1_031,
                "totp_recovery_started",
            )
            .await,
            Err(ManagerCredentialRepositoryError::RecoveryCodeConflict)
        ));

        let recovered = ManagerCredential::install_totp_with_session(
            &runtime,
            replaced_epoch,
            ExpectedManagerTotpState::Enabled,
            recovered_epoch,
            totp_secret(30, 1_040),
            recovery_codes('C', 1_040),
            new_session("recovered-session", recovered_epoch, 1_040),
            1_040,
            "totp_recovery_completed",
        )
        .await
        .expect("recovery confirmation should succeed");
        assert_eq!(recovered.revoked_sessions, 0);
        assert_eq!(
            ManagerTotpRecoveryCode::list(&runtime).await.unwrap().len(),
            10
        );

        let disabled = ManagerCredential::disable_totp_with_session(
            &runtime,
            recovered_epoch,
            disabled_epoch,
            new_session("disabled-session", disabled_epoch, 1_050),
            1_050,
            "totp_disabled",
        )
        .await
        .expect("disable transaction should succeed");
        assert_eq!(disabled.revoked_sessions, 1);
        assert!(disabled.credential.totp_secret().is_none());
        assert!(
            ManagerTotpRecoveryCode::list(&runtime)
                .await
                .unwrap()
                .is_empty()
        );
        let active = ManagerAuthInstance::list_active_instances(&runtime, 1_050)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].credential_epoch, disabled_epoch);
    }

    #[tokio::test]
    async fn manager_totp_lifecycle_rolls_back_all_state_when_session_insert_fails() {
        let database = TestDatabase::new_sqlite_default("manager-totp-rollback.sqlite").await;
        let runtime = database.runtime();
        let initial_epoch = "018fa7d8-6a00-7c9a-8f7e-600000000000";
        let enabled_epoch = "018fa7d8-6a00-7c9a-8f7e-700000000000";
        let failed_epoch = "018fa7d8-6a00-7c9a-8f7e-800000000000";
        ManagerCredential::insert_once(&runtime, new_credential(initial_epoch, 1_000))
            .await
            .expect("credential should insert");
        ManagerCredential::install_totp_with_session(
            &runtime,
            initial_epoch,
            ExpectedManagerTotpState::Disabled,
            enabled_epoch,
            totp_secret(40, 1_010),
            recovery_codes('D', 1_010),
            new_session("duplicate-jti", enabled_epoch, 1_010),
            1_010,
            "totp_enrolled",
        )
        .await
        .expect("initial enrollment should succeed");

        let failed = ManagerCredential::install_totp_with_session(
            &runtime,
            enabled_epoch,
            ExpectedManagerTotpState::Enabled,
            failed_epoch,
            totp_secret(50, 1_020),
            recovery_codes('E', 1_020),
            new_session("duplicate-jti", failed_epoch, 1_020),
            1_020,
            "totp_replaced",
        )
        .await;
        assert!(matches!(
            failed,
            Err(ManagerCredentialRepositoryError::Storage(_))
        ));

        let credential = ManagerCredential::load(&runtime).await.unwrap().unwrap();
        assert_eq!(credential.credential_epoch, enabled_epoch);
        assert_eq!(credential.totp_last_accepted_step, Some(40));
        let codes = ManagerTotpRecoveryCode::list(&runtime).await.unwrap();
        assert_eq!(codes.len(), 10);
        assert!(codes.iter().all(|code| code.code_id.starts_with('D')));
        let active = ManagerAuthInstance::list_active_instances(&runtime, 1_020)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].current_refresh_jti, "duplicate-jti");
        assert_eq!(active[0].revoked_at, None);
    }

    #[tokio::test]
    async fn manager_totp_lifecycle_rolls_back_at_every_sqlite_write_boundary() {
        let database =
            TestDatabase::new_sqlite_default("manager-totp-write-boundary-rollback.sqlite").await;
        let runtime = database.runtime();
        let initial_epoch = "018fa7d8-6a00-7c9a-8f7e-900000000000";
        let enabled_epoch = "018fa7d8-6a00-7c9a-8f7e-910000000000";
        ManagerCredential::insert_once(&runtime, new_credential(initial_epoch, 1_000))
            .await
            .expect("credential should insert");
        let installed = ManagerCredential::install_totp_with_session(
            &runtime,
            initial_epoch,
            ExpectedManagerTotpState::Disabled,
            enabled_epoch,
            totp_secret(60, 1_010),
            recovery_codes('F', 1_010),
            new_session("write-boundary-baseline", enabled_epoch, 1_010),
            1_010,
            "totp_enrolled",
        )
        .await
        .expect("baseline enrollment should succeed");
        let baseline_credential = installed.credential;
        let baseline_session = installed.session;
        let baseline_codes = ManagerTotpRecoveryCode::list(&runtime)
            .await
            .expect("baseline recovery codes should load");

        let install_failures = [
            (
                "fail_totp_credential_update",
                "CREATE TRIGGER fail_totp_credential_update
                     BEFORE UPDATE ON manager_credential
                     BEGIN SELECT RAISE(ABORT, 'blocked credential update'); END;",
            ),
            (
                "fail_old_recovery_delete",
                "CREATE TRIGGER fail_old_recovery_delete
                     BEFORE DELETE ON manager_totp_recovery_code
                     BEGIN SELECT RAISE(ABORT, 'blocked recovery delete'); END;",
            ),
            (
                "fail_new_recovery_insert",
                "CREATE TRIGGER fail_new_recovery_insert
                     BEFORE INSERT ON manager_totp_recovery_code
                     BEGIN SELECT RAISE(ABORT, 'blocked recovery insert'); END;",
            ),
            (
                "fail_session_revoke",
                "CREATE TRIGGER fail_session_revoke
                     BEFORE UPDATE OF revoked_at ON manager_auth_instance
                     BEGIN SELECT RAISE(ABORT, 'blocked session revoke'); END;",
            ),
            (
                "fail_new_session_insert",
                "CREATE TRIGGER fail_new_session_insert
                     BEFORE INSERT ON manager_auth_instance
                     BEGIN SELECT RAISE(ABORT, 'blocked session insert'); END;",
            ),
        ];
        for (index, (trigger, sql)) in install_failures.iter().enumerate() {
            database
                .execute_sqlite_batch(*sql)
                .await
                .expect("failure trigger should create");
            let new_epoch = format!("018fa7d8-6a00-7c9a-8f7e-92{index:010}");
            let result = ManagerCredential::install_totp_with_session(
                &runtime,
                enabled_epoch,
                ExpectedManagerTotpState::Enabled,
                &new_epoch,
                totp_secret(70 + index as i64, 1_020 + index as i64),
                recovery_codes('G', 1_020 + index as i64),
                new_session(
                    &format!("failed-install-{index}"),
                    &new_epoch,
                    1_020 + index as i64,
                ),
                1_020 + index as i64,
                "totp_replaced",
            )
            .await;
            assert!(
                matches!(result, Err(ManagerCredentialRepositoryError::Storage(_))),
                "{trigger} must abort the complete install transaction"
            );
            database
                .execute_sqlite_batch(format!("DROP TRIGGER {trigger};"))
                .await
                .expect("failure trigger should drop");
            assert_lifecycle_state_unchanged(
                &runtime,
                &baseline_credential,
                &baseline_codes,
                &baseline_session,
                1_100,
            )
            .await;
        }

        let disable_failures = [
            (
                "fail_disable_credential",
                "CREATE TRIGGER fail_disable_credential
                     BEFORE UPDATE ON manager_credential
                     BEGIN SELECT RAISE(ABORT, 'blocked disable credential'); END;",
            ),
            (
                "fail_disable_recovery_delete",
                "CREATE TRIGGER fail_disable_recovery_delete
                     BEFORE DELETE ON manager_totp_recovery_code
                     BEGIN SELECT RAISE(ABORT, 'blocked disable recovery delete'); END;",
            ),
            (
                "fail_disable_session_revoke",
                "CREATE TRIGGER fail_disable_session_revoke
                     BEFORE UPDATE OF revoked_at ON manager_auth_instance
                     BEGIN SELECT RAISE(ABORT, 'blocked disable revoke'); END;",
            ),
            (
                "fail_disable_session_insert",
                "CREATE TRIGGER fail_disable_session_insert
                     BEFORE INSERT ON manager_auth_instance
                     BEGIN SELECT RAISE(ABORT, 'blocked disable session insert'); END;",
            ),
        ];
        for (index, (trigger, sql)) in disable_failures.iter().enumerate() {
            database
                .execute_sqlite_batch(*sql)
                .await
                .expect("failure trigger should create");
            let new_epoch = format!("018fa7d8-6a00-7c9a-8f7e-93{index:010}");
            let result = ManagerCredential::disable_totp_with_session(
                &runtime,
                enabled_epoch,
                &new_epoch,
                new_session(
                    &format!("failed-disable-{index}"),
                    &new_epoch,
                    1_100 + index as i64,
                ),
                1_100 + index as i64,
                "totp_disabled",
            )
            .await;
            assert!(
                matches!(result, Err(ManagerCredentialRepositoryError::Storage(_))),
                "{trigger} must abort the complete disable transaction"
            );
            database
                .execute_sqlite_batch(format!("DROP TRIGGER {trigger};"))
                .await
                .expect("failure trigger should drop");
            assert_lifecycle_state_unchanged(
                &runtime,
                &baseline_credential,
                &baseline_codes,
                &baseline_session,
                1_200,
            )
            .await;
        }

        database
            .execute_sqlite_batch(
                "CREATE TRIGGER fail_recovery_session_revoke
                 BEFORE UPDATE OF revoked_at ON manager_auth_instance
                 BEGIN SELECT RAISE(ABORT, 'blocked recovery revoke'); END;",
            )
            .await
            .expect("failure trigger should create");
        let recovery_result = ManagerCredential::consume_recovery_code_and_revoke_sessions(
            &runtime,
            enabled_epoch,
            "F000",
            "verifier-F-0",
            1_200,
            "totp_recovery_started",
        )
        .await;
        assert!(matches!(
            recovery_result,
            Err(ManagerCredentialRepositoryError::Storage(_))
        ));
        database
            .execute_sqlite_batch("DROP TRIGGER fail_recovery_session_revoke;")
            .await
            .expect("failure trigger should drop");
        assert_lifecycle_state_unchanged(
            &runtime,
            &baseline_credential,
            &baseline_codes,
            &baseline_session,
            1_300,
        )
        .await;
    }

    #[tokio::test]
    #[ignore = "requires a dedicated PostgreSQL 17 database"]
    async fn postgres_manager_totp_repository_matches_sqlite_contract() {
        let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
            panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
        });
        let mut setup = PgConnection::establish(&database_url)
            .expect("dedicated postgres smoke database should be reachable");
        let database_name = diesel::sql_query("SELECT current_database()::text AS name")
            .get_result::<DatabaseNameRow>(&mut setup)
            .expect("postgres database name should query")
            .name;
        assert_eq!(database_name, POSTGRES_SMOKE_DATABASE);
        setup
            .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .expect("postgres smoke schema should reset");
        drop(setup);

        let database = TestDatabase::new_postgres(&database_url).await;
        let runtime = database.runtime();
        let initial_epoch = "018fa7d8-6a00-7c9a-8f7e-a00000000000";
        let enabled_epoch = "018fa7d8-6a00-7c9a-8f7e-a10000000000";
        let replaced_epoch = "018fa7d8-6a00-7c9a-8f7e-a20000000000";
        let disabled_epoch = "018fa7d8-6a00-7c9a-8f7e-a30000000000";
        ManagerCredential::insert_once(&runtime, new_credential(initial_epoch, 2_000))
            .await
            .expect("postgres credential should insert");
        ManagerAuthInstance::create_instance(
            &runtime,
            "postgres-pre-enrollment".to_string(),
            "a".repeat(64),
            initial_epoch.to_string(),
            2_001,
            5_601,
            9_201,
        )
        .await
        .expect("postgres pre-enrollment session should create");

        let enrolled = ManagerCredential::install_totp_with_session(
            &runtime,
            initial_epoch,
            ExpectedManagerTotpState::Disabled,
            enabled_epoch,
            totp_secret(100, 2_010),
            recovery_codes('H', 2_010),
            new_session("postgres-enrolled", enabled_epoch, 2_010),
            2_010,
            "totp_enrolled",
        )
        .await
        .expect("postgres enrollment should succeed");
        assert_eq!(enrolled.revoked_sessions, 1);
        assert_eq!(
            ManagerTotpRecoveryCode::list(&runtime).await.unwrap().len(),
            10
        );
        ManagerCredential::advance_totp_step_if_newer(&runtime, enabled_epoch, 101, 2_020)
            .await
            .expect("postgres step CAS should advance");
        assert!(matches!(
            ManagerCredential::advance_totp_step_if_newer(&runtime, enabled_epoch, 101, 2_021,)
                .await,
            Err(ManagerCredentialRepositoryError::TotpStepConflict)
        ));

        let recovery = ManagerCredential::consume_recovery_code_and_revoke_sessions(
            &runtime,
            enabled_epoch,
            "H000",
            "verifier-H-0",
            2_030,
            "totp_recovery_started",
        )
        .await
        .expect("postgres recovery start should consume and revoke");
        assert_eq!(recovery.revoked_sessions, 1);
        assert_eq!(
            ManagerTotpRecoveryCode::list(&runtime).await.unwrap().len(),
            9
        );

        let replaced = ManagerCredential::install_totp_with_session(
            &runtime,
            enabled_epoch,
            ExpectedManagerTotpState::Enabled,
            replaced_epoch,
            totp_secret(102, 2_040),
            recovery_codes('I', 2_040),
            new_session("postgres-replaced", replaced_epoch, 2_040),
            2_040,
            "totp_recovery_completed",
        )
        .await
        .expect("postgres replacement should succeed");
        assert_eq!(replaced.revoked_sessions, 0);
        assert_eq!(
            ManagerTotpRecoveryCode::list(&runtime).await.unwrap().len(),
            10
        );

        let disabled = ManagerCredential::disable_totp_with_session(
            &runtime,
            replaced_epoch,
            disabled_epoch,
            new_session("postgres-disabled", disabled_epoch, 2_050),
            2_050,
            "totp_disabled",
        )
        .await
        .expect("postgres disable should succeed");
        assert_eq!(disabled.revoked_sessions, 1);
        assert!(disabled.credential.totp_secret().is_none());
        assert!(
            ManagerTotpRecoveryCode::list(&runtime)
                .await
                .unwrap()
                .is_empty()
        );
        let active = ManagerAuthInstance::list_active_instances(&runtime, 2_050)
            .await
            .unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].credential_epoch, disabled_epoch);
        drop(database);

        let mut cleanup = PgConnection::establish(&database_url)
            .expect("dedicated postgres smoke database should remain reachable");
        cleanup
            .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .expect("postgres smoke schema should clean up");
    }
}
