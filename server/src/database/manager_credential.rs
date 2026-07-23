use std::fmt;

use diesel::{Connection, PgConnection, SqliteConnection, prelude::*};

use crate::db_execute;

use super::manager_auth_instance::{ManagerAuthInstance, NewManagerAuthInstance};
use super::{DbConnection, get_connection};

pub const MANAGER_ID: i64 = 0;
pub const MANAGER_SUBJECT: &str = "admin";

#[derive(Clone, PartialEq, Eq)]
pub struct ManagerCredential {
    pub manager_id: i64,
    pub manager_subject: String,
    pub password_verifier: String,
    pub credential_epoch: String,
    pub created_at: i64,
    pub updated_at: i64,
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
    Storage(String),
}

impl fmt::Display for ManagerCredentialRepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInitialized => {
                formatter.write_str("manager credential already initialized")
            }
            Self::EpochConflict => formatter.write_str("manager credential epoch conflict"),
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

pub struct ManagerCredentialSessionMutation {
    pub credential: ManagerCredential,
    pub session: ManagerAuthInstance,
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

fn storage_error(context: &str, error: impl fmt::Display) -> ManagerCredentialRepositoryError {
    ManagerCredentialRepositoryError::Storage(format!("{context}: {error}"))
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

fn connection() -> Result<DbConnection, ManagerCredentialRepositoryError> {
    get_connection().map_err(|error| storage_error("connection failed", format!("{error:?}")))
}

impl ManagerCredential {
    pub fn load() -> Result<Option<Self>, ManagerCredentialRepositoryError> {
        let conn = &mut connection()?;
        db_execute!(conn, {
            let row = manager_credential::table
                .filter(manager_credential::dsl::manager_id.eq(MANAGER_ID))
                .select(ManagerCredentialDb::as_select())
                .first::<ManagerCredentialDb>(conn)
                .optional()
                .map_err(|error| storage_error("load failed", error))?;

            Ok(row.map(ManagerCredentialDb::from_db))
        })
    }

    pub fn insert_once(
        new_credential: NewManagerCredential,
    ) -> Result<Self, ManagerCredentialRepositoryError> {
        let conn = &mut connection()?;
        db_execute!(conn, {
            let inserted = diesel::insert_into(manager_credential::table)
                .values(NewManagerCredentialDb::from_new(&new_credential))
                .returning(ManagerCredentialDb::as_returning())
                .get_result::<ManagerCredentialDb>(conn)
                .map_err(map_insert_error)?;

            Ok(inserted.from_db())
        })
    }

    pub fn rotate_if_epoch(
        expected_epoch: &str,
        rotated: RotatedManagerCredential,
    ) -> Result<Self, ManagerCredentialRepositoryError> {
        let conn = &mut connection()?;
        db_execute!(conn, {
            let updated = diesel::update(
                manager_credential::table.filter(
                    manager_credential::dsl::manager_id
                        .eq(MANAGER_ID)
                        .and(manager_credential::dsl::credential_epoch.eq(expected_epoch)),
                ),
            )
            .set((
                manager_credential::dsl::password_verifier.eq(rotated.password_verifier),
                manager_credential::dsl::credential_epoch.eq(rotated.credential_epoch),
                manager_credential::dsl::updated_at.eq(rotated.now),
            ))
            .returning(ManagerCredentialDb::as_returning())
            .get_result::<ManagerCredentialDb>(conn)
            .optional()
            .map_err(|error| storage_error("rotation failed", error))?;

            updated
                .map(ManagerCredentialDb::from_db)
                .ok_or(ManagerCredentialRepositoryError::EpochConflict)
        })
    }

    pub fn bootstrap_with_session(
        new_credential: NewManagerCredential,
        new_session: NewManagerAuthInstance,
        revoke_reason: &str,
    ) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
        let mut conn = connection()?;
        match &mut conn {
            DbConnection::Postgres(conn) => conn.transaction(|conn| {
                bootstrap_postgres(conn, &new_credential, &new_session, revoke_reason)
            }),
            DbConnection::Sqlite(conn) => conn.transaction(|conn| {
                bootstrap_sqlite(conn, &new_credential, &new_session, revoke_reason)
            }),
        }
    }

    pub fn rotate_with_session(
        expected_epoch: &str,
        rotated: RotatedManagerCredential,
        new_session: NewManagerAuthInstance,
        revoke_reason: &str,
    ) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
        let mut conn = connection()?;
        match &mut conn {
            DbConnection::Postgres(conn) => conn.transaction(|conn| {
                rotate_postgres(conn, expected_epoch, &rotated, &new_session, revoke_reason)
            }),
            DbConnection::Sqlite(conn) => conn.transaction(|conn| {
                rotate_sqlite(conn, expected_epoch, &rotated, &new_session, revoke_reason)
            }),
        }
    }
}

fn bootstrap_postgres(
    conn: &mut PgConnection,
    new_credential: &NewManagerCredential,
    new_session: &NewManagerAuthInstance,
    revoke_reason: &str,
) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
    use crate::database::_postgres_schema::{manager_auth_instance, manager_credential};
    use crate::database::manager_auth_instance::_postgres_model::{
        ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
    };

    let credential = diesel::insert_into(manager_credential::table)
        .values(_postgres_model::NewManagerCredentialDb::from_new(
            new_credential,
        ))
        .returning(_postgres_model::ManagerCredentialDb::as_returning())
        .get_result::<_postgres_model::ManagerCredentialDb>(conn)
        .map_err(map_insert_error)?
        .from_db();
    let revoked_sessions = revoke_active_postgres(conn, new_credential.now, revoke_reason)?;
    let session = diesel::insert_into(manager_auth_instance::table)
        .values(NewManagerAuthInstanceDb::to_db(new_session))
        .returning(ManagerAuthInstanceDb::as_returning())
        .get_result::<ManagerAuthInstanceDb>(conn)
        .map_err(|error| storage_error("session insert failed", error))?
        .from_db();

    Ok(ManagerCredentialSessionMutation {
        credential,
        session,
        revoked_sessions,
    })
}

fn bootstrap_sqlite(
    conn: &mut SqliteConnection,
    new_credential: &NewManagerCredential,
    new_session: &NewManagerAuthInstance,
    revoke_reason: &str,
) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
    use crate::database::_sqlite_schema::{manager_auth_instance, manager_credential};
    use crate::database::manager_auth_instance::_sqlite_model::{
        ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
    };

    let credential = diesel::insert_into(manager_credential::table)
        .values(_sqlite_model::NewManagerCredentialDb::from_new(
            new_credential,
        ))
        .returning(_sqlite_model::ManagerCredentialDb::as_returning())
        .get_result::<_sqlite_model::ManagerCredentialDb>(conn)
        .map_err(map_insert_error)?
        .from_db();
    let revoked_sessions = revoke_active_sqlite(conn, new_credential.now, revoke_reason)?;
    let session = diesel::insert_into(manager_auth_instance::table)
        .values(NewManagerAuthInstanceDb::to_db(new_session))
        .returning(ManagerAuthInstanceDb::as_returning())
        .get_result::<ManagerAuthInstanceDb>(conn)
        .map_err(|error| storage_error("session insert failed", error))?
        .from_db();

    Ok(ManagerCredentialSessionMutation {
        credential,
        session,
        revoked_sessions,
    })
}

fn rotate_postgres(
    conn: &mut PgConnection,
    expected_epoch: &str,
    rotated: &RotatedManagerCredential,
    new_session: &NewManagerAuthInstance,
    revoke_reason: &str,
) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
    use crate::database::_postgres_schema::{manager_auth_instance, manager_credential};
    use crate::database::manager_auth_instance::_postgres_model::{
        ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
    };

    let credential = diesel::update(
        manager_credential::table.filter(
            manager_credential::dsl::manager_id
                .eq(MANAGER_ID)
                .and(manager_credential::dsl::credential_epoch.eq(expected_epoch)),
        ),
    )
    .set((
        manager_credential::dsl::password_verifier.eq(&rotated.password_verifier),
        manager_credential::dsl::credential_epoch.eq(&rotated.credential_epoch),
        manager_credential::dsl::updated_at.eq(rotated.now),
    ))
    .returning(_postgres_model::ManagerCredentialDb::as_returning())
    .get_result::<_postgres_model::ManagerCredentialDb>(conn)
    .optional()
    .map_err(|error| storage_error("rotation failed", error))?
    .ok_or(ManagerCredentialRepositoryError::EpochConflict)?
    .from_db();
    let revoked_sessions = revoke_active_postgres(conn, rotated.now, revoke_reason)?;
    let session = diesel::insert_into(manager_auth_instance::table)
        .values(NewManagerAuthInstanceDb::to_db(new_session))
        .returning(ManagerAuthInstanceDb::as_returning())
        .get_result::<ManagerAuthInstanceDb>(conn)
        .map_err(|error| storage_error("session insert failed", error))?
        .from_db();

    Ok(ManagerCredentialSessionMutation {
        credential,
        session,
        revoked_sessions,
    })
}

fn rotate_sqlite(
    conn: &mut SqliteConnection,
    expected_epoch: &str,
    rotated: &RotatedManagerCredential,
    new_session: &NewManagerAuthInstance,
    revoke_reason: &str,
) -> Result<ManagerCredentialSessionMutation, ManagerCredentialRepositoryError> {
    use crate::database::_sqlite_schema::{manager_auth_instance, manager_credential};
    use crate::database::manager_auth_instance::_sqlite_model::{
        ManagerAuthInstanceDb, NewManagerAuthInstanceDb,
    };

    let credential = diesel::update(
        manager_credential::table.filter(
            manager_credential::dsl::manager_id
                .eq(MANAGER_ID)
                .and(manager_credential::dsl::credential_epoch.eq(expected_epoch)),
        ),
    )
    .set((
        manager_credential::dsl::password_verifier.eq(&rotated.password_verifier),
        manager_credential::dsl::credential_epoch.eq(&rotated.credential_epoch),
        manager_credential::dsl::updated_at.eq(rotated.now),
    ))
    .returning(_sqlite_model::ManagerCredentialDb::as_returning())
    .get_result::<_sqlite_model::ManagerCredentialDb>(conn)
    .optional()
    .map_err(|error| storage_error("rotation failed", error))?
    .ok_or(ManagerCredentialRepositoryError::EpochConflict)?
    .from_db();
    let revoked_sessions = revoke_active_sqlite(conn, rotated.now, revoke_reason)?;
    let session = diesel::insert_into(manager_auth_instance::table)
        .values(NewManagerAuthInstanceDb::to_db(new_session))
        .returning(ManagerAuthInstanceDb::as_returning())
        .get_result::<ManagerAuthInstanceDb>(conn)
        .map_err(|error| storage_error("session insert failed", error))?
        .from_db();

    Ok(ManagerCredentialSessionMutation {
        credential,
        session,
        revoked_sessions,
    })
}

fn revoke_active_postgres(
    conn: &mut PgConnection,
    now: i64,
    reason: &str,
) -> Result<usize, ManagerCredentialRepositoryError> {
    use crate::database::_postgres_schema::manager_auth_instance;

    diesel::update(
        manager_auth_instance::table.filter(
            manager_auth_instance::dsl::manager_id
                .eq(MANAGER_ID)
                .and(manager_auth_instance::dsl::revoked_at.is_null()),
        ),
    )
    .set((
        manager_auth_instance::dsl::revoked_at.eq(Some(now)),
        manager_auth_instance::dsl::revoked_reason.eq(Some(reason.to_string())),
    ))
    .execute(conn)
    .map_err(|error| storage_error("session revocation failed", error))
}

fn revoke_active_sqlite(
    conn: &mut SqliteConnection,
    now: i64,
    reason: &str,
) -> Result<usize, ManagerCredentialRepositoryError> {
    use crate::database::_sqlite_schema::manager_auth_instance;

    diesel::update(
        manager_auth_instance::table.filter(
            manager_auth_instance::dsl::manager_id
                .eq(MANAGER_ID)
                .and(manager_auth_instance::dsl::revoked_at.is_null()),
        ),
    )
    .set((
        manager_auth_instance::dsl::revoked_at.eq(Some(now)),
        manager_auth_instance::dsl::revoked_reason.eq(Some(reason.to_string())),
    ))
    .execute(conn)
    .map_err(|error| storage_error("session revocation failed", error))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::TestDbContext;
    use crate::utils::ID_GENERATOR;

    fn new_credential(epoch: &str, now: i64) -> NewManagerCredential {
        NewManagerCredential {
            password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$fixture".to_string(),
            credential_epoch: epoch.to_string(),
            now,
        }
    }

    fn new_session(jti: &str, now: i64) -> NewManagerAuthInstance {
        NewManagerAuthInstance {
            id: ID_GENERATOR.generate_id(),
            manager_id: MANAGER_ID,
            manager_subject: MANAGER_SUBJECT.to_string(),
            current_refresh_jti: jti.to_string(),
            refresh_generation: super::super::manager_auth_instance::INITIAL_REFRESH_GENERATION,
            session_version: super::super::manager_auth_instance::INITIAL_SESSION_VERSION,
            signing_key_id: "a".repeat(64),
            credential_epoch: "018fa7d8-6a00-7c9a-8f7e-444444444444".to_string(),
            created_at: now,
            last_rotated_at: now,
            idle_expires_at: now + 3_600,
            absolute_expires_at: now + 7_200,
            revoked_at: None,
            revoked_reason: None,
        }
    }

    #[test]
    fn manager_credential_repository_covers_singleton_insert_load_and_epoch_cas() {
        let test_db_context = TestDbContext::new_sqlite("manager-credential-repository.sqlite");

        test_db_context.run_sync(|| {
            assert!(
                ManagerCredential::load()
                    .expect("load should succeed")
                    .is_none()
            );

            let inserted = ManagerCredential::insert_once(new_credential(
                "018fa7d8-6a00-7c9a-8f7e-111111111111",
                1_000,
            ))
            .expect("first insert should succeed");
            assert_eq!(inserted.manager_id, MANAGER_ID);
            assert_eq!(inserted.manager_subject, MANAGER_SUBJECT);
            assert_ne!(inserted.password_verifier, "fixture-password");

            let duplicate = ManagerCredential::insert_once(new_credential(
                "018fa7d8-6a00-7c9a-8f7e-222222222222",
                1_001,
            ));
            assert!(matches!(
                duplicate,
                Err(ManagerCredentialRepositoryError::AlreadyInitialized)
            ));

            let stale = ManagerCredential::rotate_if_epoch(
                "018fa7d8-6a00-7c9a-8f7e-000000000000",
                RotatedManagerCredential {
                    password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$rotated".to_string(),
                    credential_epoch: "018fa7d8-6a00-7c9a-8f7e-333333333333".to_string(),
                    now: 1_002,
                },
            );
            assert!(matches!(
                stale,
                Err(ManagerCredentialRepositoryError::EpochConflict)
            ));

            let rotated = ManagerCredential::rotate_if_epoch(
                &inserted.credential_epoch,
                RotatedManagerCredential {
                    password_verifier: "$argon2id$v=19$m=65536,t=3,p=4$fixture$rotated".to_string(),
                    credential_epoch: "018fa7d8-6a00-7c9a-8f7e-333333333333".to_string(),
                    now: 1_003,
                },
            )
            .expect("matching epoch should rotate");
            assert_eq!(rotated.created_at, 1_000);
            assert_eq!(rotated.updated_at, 1_003);
        });
    }

    #[test]
    fn manager_credential_transaction_rolls_back_credential_and_revocation_on_session_failure() {
        let test_db_context = TestDbContext::new_sqlite("manager-credential-transaction.sqlite");

        test_db_context.run_sync(|| {
            let existing = ManagerAuthInstance::create_instance(
                "duplicate-session-jti".to_string(),
                "a".repeat(64),
                "018fa7d8-6a00-7c9a-8f7e-444444444444".to_string(),
                1_000,
                4_600,
                8_200,
            )
            .expect("existing session should create");

            let result = ManagerCredential::bootstrap_with_session(
                new_credential("018fa7d8-6a00-7c9a-8f7e-444444444444", 1_100),
                new_session("duplicate-session-jti", 1_100),
                "credential_bootstrap",
            );
            assert!(matches!(
                result,
                Err(ManagerCredentialRepositoryError::Storage(_))
            ));
            assert!(
                ManagerCredential::load()
                    .expect("load should succeed")
                    .is_none()
            );

            let existing = ManagerAuthInstance::get_instance(existing.id)
                .expect("session lookup should succeed")
                .expect("existing session should remain");
            assert_eq!(existing.revoked_at, None);
            assert_eq!(existing.revoked_reason, None);
        });
    }
}
