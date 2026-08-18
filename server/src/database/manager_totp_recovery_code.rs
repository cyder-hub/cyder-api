use diesel::prelude::*;

use super::manager_credential::{
    MANAGER_ID, ManagerCredentialRepositoryError, manager_credential_storage_error,
};
use super::runtime::{DatabaseRuntime, DatabaseWorkload, db_execute as async_db_execute};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerTotpRecoveryCode {
    pub code_id: String,
    pub manager_id: i64,
    pub code_verifier: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewManagerTotpRecoveryCode {
    pub code_id: String,
    pub code_verifier: String,
    pub created_at: i64,
}

macro_rules! recovery_code_model {
    ($schema:ident) => {
        paste::paste! {
            use crate::database::$schema::manager_totp_recovery_code;

            #[derive(Queryable, Selectable, Identifiable)]
            #[diesel(table_name = manager_totp_recovery_code)]
            #[diesel(primary_key(code_id))]
            pub(crate) struct ManagerTotpRecoveryCodeDb {
                pub(crate) code_id: String,
                pub(crate) manager_id: i64,
                pub(crate) code_verifier: String,
                pub(crate) created_at: i64,
            }

            #[derive(Insertable)]
            #[diesel(table_name = manager_totp_recovery_code)]
            pub(crate) struct NewManagerTotpRecoveryCodeDb {
                pub(crate) code_id: String,
                pub(crate) manager_id: i64,
                pub(crate) code_verifier: String,
                pub(crate) created_at: i64,
            }

            impl ManagerTotpRecoveryCodeDb {
                pub(crate) fn from_db(self) -> ManagerTotpRecoveryCode {
                    ManagerTotpRecoveryCode {
                        code_id: self.code_id,
                        manager_id: self.manager_id,
                        code_verifier: self.code_verifier,
                        created_at: self.created_at,
                    }
                }
            }

            impl NewManagerTotpRecoveryCodeDb {
                pub(crate) fn from_new(value: &NewManagerTotpRecoveryCode) -> Self {
                    Self {
                        code_id: value.code_id.clone(),
                        manager_id: MANAGER_ID,
                        code_verifier: value.code_verifier.clone(),
                        created_at: value.created_at,
                    }
                }
            }
        }
    };
}

pub(crate) mod _postgres_model {
    use super::*;
    recovery_code_model!(_postgres_schema);
}

pub(crate) mod _sqlite_model {
    use super::*;
    recovery_code_model!(_sqlite_schema);
}

impl ManagerTotpRecoveryCode {
    pub async fn load_by_code_id(
        database: &DatabaseRuntime,
        code_id_value: &str,
    ) -> Result<Option<Self>, ManagerCredentialRepositoryError> {
        let code_id_value = code_id_value.to_string();
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = manager_totp_recovery_code::table
                            .filter(manager_totp_recovery_code::dsl::code_id.eq(&code_id_value))
                            .filter(manager_totp_recovery_code::dsl::manager_id.eq(MANAGER_ID))
                            .select(ManagerTotpRecoveryCodeDb::as_select());
                        diesel_async::RunQueryDsl::first::<ManagerTotpRecoveryCodeDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .optional()
                        .map(|row| row.map(ManagerTotpRecoveryCodeDb::from_db))
                        .map_err(|error| {
                            manager_credential_storage_error("recovery code load failed", error)
                        })
                    })
                })
            })
            .await
    }

    pub async fn list(
        database: &DatabaseRuntime,
    ) -> Result<Vec<Self>, ManagerCredentialRepositoryError> {
        database
            .run_domain_error(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = manager_totp_recovery_code::table
                            .filter(manager_totp_recovery_code::dsl::manager_id.eq(MANAGER_ID))
                            .order(manager_totp_recovery_code::dsl::code_id.asc())
                            .select(ManagerTotpRecoveryCodeDb::as_select());
                        diesel_async::RunQueryDsl::load::<ManagerTotpRecoveryCodeDb>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(|rows| {
                            rows.into_iter()
                                .map(ManagerTotpRecoveryCodeDb::from_db)
                                .collect()
                        })
                        .map_err(|error| {
                            manager_credential_storage_error("recovery code list failed", error)
                        })
                    })
                })
            })
            .await
    }
}
