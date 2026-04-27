use diesel::prelude::*;

use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use super::{DbResult, get_connection};

pub const MANAGER_ID: i64 = 0;
pub const MANAGER_SUBJECT: &str = "admin";

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, PartialEq, Eq)]
    #[diesel(table_name = manager_auth_instance)]
    pub struct ManagerAuthInstance {
        pub id: i64,
        pub manager_id: i64,
        pub manager_subject: String,
        pub current_refresh_jti: String,
        pub created_at: i64,
        pub last_rotated_at: i64,
        pub expires_at: i64,
        pub revoked_at: Option<i64>,
        pub revoked_reason: Option<String>,
    }

    #[derive(Insertable, Debug, Clone)]
    #[diesel(table_name = manager_auth_instance)]
    pub struct NewManagerAuthInstance {
        pub id: i64,
        pub manager_id: i64,
        pub manager_subject: String,
        pub current_refresh_jti: String,
        pub created_at: i64,
        pub last_rotated_at: i64,
        pub expires_at: i64,
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
        now: i64,
        expires_at_value: i64,
    ) -> DbResult<ManagerAuthInstance> {
        let new_instance = NewManagerAuthInstance {
            id: ID_GENERATOR.generate_id(),
            manager_id: MANAGER_ID,
            manager_subject: MANAGER_SUBJECT.to_string(),
            current_refresh_jti: current_refresh_jti_value,
            created_at: now,
            last_rotated_at: now,
            expires_at: expires_at_value,
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

    pub fn rotate_refresh_jti(
        id_value: i64,
        expected_current_refresh_jti: &str,
        new_refresh_jti: String,
        now: i64,
        expires_at_value: i64,
    ) -> DbResult<Option<ManagerAuthInstance>> {
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
                        .and(manager_auth_instance::dsl::revoked_at.is_null()),
                ),
            )
            .set((
                manager_auth_instance::dsl::current_refresh_jti.eq(new_refresh_jti),
                manager_auth_instance::dsl::last_rotated_at.eq(now),
                manager_auth_instance::dsl::expires_at.eq(expires_at_value),
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
                        .and(manager_auth_instance::dsl::revoked_at.is_null()),
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

    pub fn cleanup_expired_instances(now: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::delete(
                manager_auth_instance::table.filter(manager_auth_instance::dsl::expires_at.le(now)),
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
}

#[cfg(test)]
mod tests {
    use super::ManagerAuthInstance;
    use crate::database::TestDbContext;

    #[test]
    fn manager_auth_instance_repository_covers_rotation_revoke_and_cleanup() {
        let test_db_context = TestDbContext::new_sqlite("manager-auth-instance-repository.sqlite");

        test_db_context.run_sync(|| {
            let first = ManagerAuthInstance::create_instance("jti-first".to_string(), 1_000, 2_000)
                .expect("first instance should create");
            let second =
                ManagerAuthInstance::create_instance("jti-second".to_string(), 1_010, 3_000)
                    .expect("second instance should create");

            assert_ne!(first.id, second.id);
            assert_eq!(first.current_refresh_jti, "jti-first");
            assert_eq!(second.current_refresh_jti, "jti-second");

            let rotated = ManagerAuthInstance::rotate_refresh_jti(
                first.id,
                "jti-first",
                "jti-first-rotated".to_string(),
                1_020,
                2_020,
            )
            .expect("rotation should query")
            .expect("matching current jti should rotate");
            assert_eq!(rotated.current_refresh_jti, "jti-first-rotated");
            assert_eq!(rotated.last_rotated_at, 1_020);
            assert_eq!(rotated.expires_at, 2_020);

            let stale_rotation = ManagerAuthInstance::rotate_refresh_jti(
                first.id,
                "jti-first",
                "stale-rotation".to_string(),
                1_030,
                2_030,
            )
            .expect("stale rotation should query");
            assert!(stale_rotation.is_none());

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
