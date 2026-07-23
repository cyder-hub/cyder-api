use diesel::prelude::*;

use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use super::{DbResult, get_connection};

pub const MANAGER_ID: i64 = 0;
pub const MANAGER_SUBJECT: &str = "admin";
pub const INITIAL_SESSION_VERSION: i64 = 1;
pub const INITIAL_REFRESH_GENERATION: i64 = 1;

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Clone, PartialEq, Eq)]
    #[diesel(table_name = manager_auth_instance)]
    pub struct ManagerAuthInstance {
        pub id: i64,
        pub manager_id: i64,
        pub manager_subject: String,
        pub current_refresh_jti: String,
        pub refresh_generation: i64,
        pub session_version: i64,
        pub signing_key_id: String,
        pub credential_epoch: String,
        pub created_at: i64,
        pub last_rotated_at: i64,
        pub idle_expires_at: i64,
        pub absolute_expires_at: i64,
        pub revoked_at: Option<i64>,
        pub revoked_reason: Option<String>,
    }

    #[derive(Insertable, Clone)]
    #[diesel(table_name = manager_auth_instance)]
    pub struct NewManagerAuthInstance {
        pub id: i64,
        pub manager_id: i64,
        pub manager_subject: String,
        pub current_refresh_jti: String,
        pub refresh_generation: i64,
        pub session_version: i64,
        pub signing_key_id: String,
        pub credential_epoch: String,
        pub created_at: i64,
        pub last_rotated_at: i64,
        pub idle_expires_at: i64,
        pub absolute_expires_at: i64,
        pub revoked_at: Option<i64>,
        pub revoked_reason: Option<String>,
    }
}

fn map_write_error(context: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}

impl ManagerAuthInstance {
    pub fn create_instance(
        current_refresh_jti_value: String,
        signing_key_id_value: String,
        credential_epoch_value: String,
        now: i64,
        idle_expires_at_value: i64,
        absolute_expires_at_value: i64,
    ) -> DbResult<ManagerAuthInstance> {
        let new_instance = NewManagerAuthInstance {
            id: ID_GENERATOR.generate_id(),
            manager_id: MANAGER_ID,
            manager_subject: MANAGER_SUBJECT.to_string(),
            current_refresh_jti: current_refresh_jti_value,
            refresh_generation: INITIAL_REFRESH_GENERATION,
            session_version: INITIAL_SESSION_VERSION,
            signing_key_id: signing_key_id_value,
            credential_epoch: credential_epoch_value,
            created_at: now,
            last_rotated_at: now,
            idle_expires_at: idle_expires_at_value,
            absolute_expires_at: absolute_expires_at_value,
            revoked_at: None,
            revoked_reason: None,
        };

        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let inserted = diesel::insert_into(manager_auth_instance::table)
                .values(NewManagerAuthInstanceDb::to_db(&new_instance))
                .returning(ManagerAuthInstanceDb::as_returning())
                .get_result::<ManagerAuthInstanceDb>(conn)
                .map_err(|e| map_write_error("Failed to create manager auth instance", e))?;
            Ok(inserted.from_db())
        })
    }

    pub fn get_instance(id_value: i64) -> DbResult<Option<ManagerAuthInstance>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let instance = manager_auth_instance::table
                .filter(manager_auth_instance::dsl::id.eq(id_value))
                .select(ManagerAuthInstanceDb::as_select())
                .first::<ManagerAuthInstanceDb>(conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to get manager auth instance {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(instance.map(|row| row.from_db()))
        })
    }

    pub fn list_active_instances(now: i64) -> DbResult<Vec<ManagerAuthInstance>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let instances = manager_auth_instance::table
                .filter(manager_auth_instance::dsl::revoked_at.is_null())
                .filter(manager_auth_instance::dsl::idle_expires_at.gt(now))
                .filter(manager_auth_instance::dsl::absolute_expires_at.gt(now))
                .select(ManagerAuthInstanceDb::as_select())
                .load::<ManagerAuthInstanceDb>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list active manager auth instances: {error}"
                    )))
                })?;
            Ok(instances.into_iter().map(|row| row.from_db()).collect())
        })
    }

    pub fn rotate_refresh_jti(
        id_value: i64,
        expected_current_refresh_jti: &str,
        expected_refresh_generation: i64,
        expected_credential_epoch: &str,
        new_refresh_jti: String,
        now: i64,
        idle_expires_at_value: i64,
    ) -> DbResult<Option<ManagerAuthInstance>> {
        let new_refresh_generation =
            expected_refresh_generation.checked_add(1).ok_or_else(|| {
                BaseError::InternalServerError(Some(
                    "Manager auth refresh generation overflow".to_string(),
                ))
            })?;
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let updated = diesel::update(
                manager_auth_instance::table.filter(
                    manager_auth_instance::dsl::id
                        .eq(id_value)
                        .and(
                            manager_auth_instance::dsl::current_refresh_jti
                                .eq(expected_current_refresh_jti),
                        )
                        .and(
                            manager_auth_instance::dsl::refresh_generation
                                .eq(expected_refresh_generation),
                        )
                        .and(
                            manager_auth_instance::dsl::credential_epoch
                                .eq(expected_credential_epoch),
                        )
                        .and(manager_auth_instance::dsl::revoked_at.is_null())
                        .and(manager_auth_instance::dsl::idle_expires_at.gt(now))
                        .and(manager_auth_instance::dsl::absolute_expires_at.gt(now)),
                ),
            )
            .set((
                manager_auth_instance::dsl::current_refresh_jti.eq(new_refresh_jti),
                manager_auth_instance::dsl::refresh_generation.eq(new_refresh_generation),
                manager_auth_instance::dsl::last_rotated_at.eq(now),
                manager_auth_instance::dsl::idle_expires_at.eq(idle_expires_at_value),
            ))
            .returning(ManagerAuthInstanceDb::as_returning())
            .get_result::<ManagerAuthInstanceDb>(conn)
            .optional()
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to rotate manager auth instance {}: {}",
                    id_value, e
                )))
            })?;

            Ok(updated.map(|row| row.from_db()))
        })
    }

    pub fn revoke_instance(
        id_value: i64,
        now: i64,
        reason: &str,
    ) -> DbResult<Option<ManagerAuthInstance>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let revoked = diesel::update(
                manager_auth_instance::table.filter(
                    manager_auth_instance::dsl::id
                        .eq(id_value)
                        .and(manager_auth_instance::dsl::revoked_at.is_null())
                        .and(manager_auth_instance::dsl::idle_expires_at.gt(now))
                        .and(manager_auth_instance::dsl::absolute_expires_at.gt(now)),
                ),
            )
            .set((
                manager_auth_instance::dsl::revoked_at.eq(Some(now)),
                manager_auth_instance::dsl::revoked_reason.eq(Some(reason.to_string())),
            ))
            .returning(ManagerAuthInstanceDb::as_returning())
            .get_result::<ManagerAuthInstanceDb>(conn)
            .optional()
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to revoke manager auth instance {}: {}",
                    id_value, e
                )))
            })?;

            Ok(revoked.map(|row| row.from_db()))
        })
    }

    pub fn revoke_instance_for_epoch(
        id_value: i64,
        credential_epoch_value: &str,
        now: i64,
        reason: &str,
    ) -> DbResult<Option<ManagerAuthInstance>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let revoked = diesel::update(
                manager_auth_instance::table.filter(
                    manager_auth_instance::dsl::id
                        .eq(id_value)
                        .and(
                            manager_auth_instance::dsl::credential_epoch.eq(credential_epoch_value),
                        )
                        .and(manager_auth_instance::dsl::revoked_at.is_null())
                        .and(manager_auth_instance::dsl::idle_expires_at.gt(now))
                        .and(manager_auth_instance::dsl::absolute_expires_at.gt(now)),
                ),
            )
            .set((
                manager_auth_instance::dsl::revoked_at.eq(Some(now)),
                manager_auth_instance::dsl::revoked_reason.eq(Some(reason.to_string())),
            ))
            .returning(ManagerAuthInstanceDb::as_returning())
            .get_result::<ManagerAuthInstanceDb>(conn)
            .optional()
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to revoke manager auth instance {} for credential epoch: {}",
                    id_value, error
                )))
            })?;

            Ok(revoked.map(|row| row.from_db()))
        })
    }

    pub fn revoke_all_active(now: i64, reason: &str) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::update(
                manager_auth_instance::table
                    .filter(manager_auth_instance::dsl::manager_id.eq(MANAGER_ID))
                    .filter(manager_auth_instance::dsl::revoked_at.is_null())
                    .filter(manager_auth_instance::dsl::idle_expires_at.gt(now))
                    .filter(manager_auth_instance::dsl::absolute_expires_at.gt(now)),
            )
            .set((
                manager_auth_instance::dsl::revoked_at.eq(Some(now)),
                manager_auth_instance::dsl::revoked_reason.eq(Some(reason.to_string())),
            ))
            .execute(conn)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to revoke all manager auth instances: {error}"
                )))
            })
        })
    }

    pub fn cleanup_expired_instances(now: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::delete(
                manager_auth_instance::table.filter(
                    manager_auth_instance::dsl::idle_expires_at
                        .le(now)
                        .or(manager_auth_instance::dsl::absolute_expires_at.le(now)),
                ),
            )
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to cleanup expired manager auth instances: {}",
                    e
                )))
            })
        })
    }

    pub fn revoke_signing_key_mismatches(
        current_signing_key_id: &str,
        now: i64,
    ) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::update(
                manager_auth_instance::table
                    .filter(manager_auth_instance::dsl::manager_id.eq(MANAGER_ID))
                    .filter(manager_auth_instance::dsl::revoked_at.is_null())
                    .filter(manager_auth_instance::dsl::signing_key_id.ne(current_signing_key_id)),
            )
            .set((
                manager_auth_instance::dsl::revoked_at.eq(Some(now)),
                manager_auth_instance::dsl::revoked_reason
                    .eq(Some("signing_key_changed".to_string())),
            ))
            .execute(conn)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to revoke manager auth instances signed by a previous key: {error}"
                )))
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{INITIAL_REFRESH_GENERATION, INITIAL_SESSION_VERSION, ManagerAuthInstance};
    use crate::database::TestDbContext;

    #[test]
    fn manager_auth_instance_repository_covers_rotation_revoke_and_cleanup() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-instance-repository.sqlite");

        test_db_context.run_sync(|| {
            let key_id = "a".repeat(64);
            let other_key_id = "b".repeat(64);
            let epoch = "018fa7d8-6a00-7c9a-8f7e-111111111111";
            let first = ManagerAuthInstance::create_instance(
                "jti-first".to_string(),
                key_id.clone(),
                epoch.to_string(),
                1_000,
                2_000,
                4_000,
            )
            .expect("first instance should create");
            let second = ManagerAuthInstance::create_instance(
                "jti-second".to_string(),
                key_id.clone(),
                epoch.to_string(),
                1_010,
                3_000,
                5_000,
            )
            .expect("second instance should create");
            let previous_key = ManagerAuthInstance::create_instance(
                "jti-previous-key".to_string(),
                other_key_id,
                epoch.to_string(),
                1_015,
                4_000,
                5_000,
            )
            .expect("previous-key instance should create");

            assert_ne!(first.id, second.id);
            assert_eq!(first.current_refresh_jti, "jti-first");
            assert_eq!(first.refresh_generation, INITIAL_REFRESH_GENERATION);
            assert_eq!(first.session_version, INITIAL_SESSION_VERSION);
            assert_eq!(second.current_refresh_jti, "jti-second");

            let rotated = ManagerAuthInstance::rotate_refresh_jti(
                first.id,
                "jti-first",
                INITIAL_REFRESH_GENERATION,
                epoch,
                "jti-first-rotated".to_string(),
                1_020,
                2_020,
            )
            .expect("rotation should query")
            .expect("matching current jti should rotate");
            assert_eq!(rotated.current_refresh_jti, "jti-first-rotated");
            assert_eq!(rotated.refresh_generation, 2);
            assert_eq!(rotated.session_version, INITIAL_SESSION_VERSION);
            assert_eq!(rotated.last_rotated_at, 1_020);
            assert_eq!(rotated.idle_expires_at, 2_020);
            assert_eq!(rotated.absolute_expires_at, 4_000);

            let stale_rotation = ManagerAuthInstance::rotate_refresh_jti(
                first.id,
                "jti-first",
                INITIAL_REFRESH_GENERATION,
                epoch,
                "stale-rotation".to_string(),
                1_030,
                2_030,
            )
            .expect("stale rotation should query");
            assert!(stale_rotation.is_none());

            let stale_generation_rotation = ManagerAuthInstance::rotate_refresh_jti(
                first.id,
                "jti-first-rotated",
                INITIAL_REFRESH_GENERATION,
                epoch,
                "stale-generation".to_string(),
                1_030,
                2_030,
            )
            .expect("stale generation rotation should query");
            assert!(stale_generation_rotation.is_none());

            let active = ManagerAuthInstance::list_active_instances(1_035)
                .expect("active instances should load");
            assert_eq!(active.len(), 3);

            let key_revoked = ManagerAuthInstance::revoke_signing_key_mismatches(&key_id, 1_036)
                .expect("previous signing keys should revoke");
            assert_eq!(key_revoked, 1);
            assert_eq!(
                ManagerAuthInstance::get_instance(previous_key.id)
                    .expect("previous-key lookup should query")
                    .expect("previous-key instance should remain for evidence")
                    .revoked_reason
                    .as_deref(),
                Some("signing_key_changed")
            );

            let revoked = ManagerAuthInstance::revoke_instance(first.id, 1_040, "logout")
                .expect("revoke should query")
                .expect("active instance should revoke");
            assert_eq!(revoked.revoked_at, Some(1_040));
            assert_eq!(revoked.revoked_reason.as_deref(), Some("logout"));

            let second_after_revoke = ManagerAuthInstance::get_instance(second.id)
                .expect("second lookup should query")
                .expect("second instance should still exist");
            assert_eq!(second_after_revoke.current_refresh_jti, "jti-second");
            assert_eq!(second_after_revoke.revoked_at, None);

            let revoked_all = ManagerAuthInstance::revoke_all_active(1_050, "logout_all")
                .expect("all active instances should revoke");
            assert_eq!(revoked_all, 1);
            assert!(
                ManagerAuthInstance::list_active_instances(1_051)
                    .expect("active instances should load after revoke all")
                    .is_empty()
            );

            let cleanup_count = ManagerAuthInstance::cleanup_expired_instances(2_025)
                .expect("cleanup should succeed");
            assert_eq!(cleanup_count, 1);
            assert!(
                ManagerAuthInstance::get_instance(first.id)
                    .expect("first lookup should query")
                    .is_none()
            );
            assert!(
                ManagerAuthInstance::get_instance(second.id)
                    .expect("second lookup should query")
                    .is_some()
            );
        });
    }
}
