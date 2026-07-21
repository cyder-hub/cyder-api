use diesel::prelude::*;

use crate::{
    controller::BaseError,
    database::{
        DbResult,
        api_key::{ApiKey, NewApiKey, UpdateApiKeyData, hash_api_key, key_last4, key_prefix},
        api_key_acl_rule::{ApiKeyAclRule, NewApiKeyAclRule},
    },
    schema::enum_def::Action,
    service::secret_encryption::EncryptedSecret,
};

use super::{PortableRepositoryConnection, map_write_error};

#[derive(Clone)]
pub(crate) struct RawApiKeyImportInput {
    pub id: i64,
    pub raw_api_key: String,
    pub encrypted_secret: Option<EncryptedSecret>,
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action,
    pub is_enabled: bool,
    pub expires_at: Option<i64>,
    pub rate_limit_rpm: Option<i32>,
    pub max_concurrent_requests: Option<i32>,
    pub quota_daily_requests: Option<i64>,
    pub quota_daily_tokens: Option<i64>,
    pub quota_monthly_tokens: Option<i64>,
    pub budget_daily_nanos: Option<i64>,
    pub budget_daily_currency: Option<String>,
    pub budget_monthly_nanos: Option<i64>,
    pub budget_monthly_currency: Option<String>,
    pub now: i64,
}

impl std::fmt::Debug for RawApiKeyImportInput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RawApiKeyImportInput(<redacted>)")
    }
}

impl RawApiKeyImportInput {
    #[cfg(test)]
    fn test(raw_api_key: &str, name: &str, now: i64) -> Self {
        Self {
            id: crate::utils::ID_GENERATOR.generate_id(),
            raw_api_key: raw_api_key.to_string(),
            encrypted_secret: None,
            name: name.to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            now,
        }
    }
}

pub(crate) fn find_active_api_key_by_raw_key(
    conn: &mut PortableRepositoryConnection<'_>,
    raw_api_key: &str,
) -> DbResult<Option<ApiKey>> {
    let api_key_hash = hash_api_key(raw_api_key);
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::ApiKeyDb;

            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(api_key_hash)
                        .and(api_key::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(*conn)
                .optional()
                .map(|row| row.map(ApiKeyDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup api key by hash", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::ApiKeyDb;

            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(api_key_hash)
                        .and(api_key::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(*conn)
                .optional()
                .map(|row| row.map(ApiKeyDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup api key by hash", err))
        }
    }
}

pub(crate) fn insert_raw_api_key(
    conn: &mut PortableRepositoryConnection<'_>,
    input: &RawApiKeyImportInput,
) -> DbResult<ApiKey> {
    validate_raw_api_key_input(input)?;
    let new_key = NewApiKey {
        id: input.id,
        api_key_hash: hash_api_key(&input.raw_api_key),
        key_prefix: key_prefix(&input.raw_api_key),
        key_last4: key_last4(&input.raw_api_key),
        name: input.name.trim().to_string(),
        description: input.description.clone(),
        default_action: input.default_action.clone(),
        is_enabled: input.is_enabled,
        expires_at: input.expires_at,
        rate_limit_rpm: input.rate_limit_rpm,
        max_concurrent_requests: input.max_concurrent_requests,
        quota_daily_requests: input.quota_daily_requests,
        quota_daily_tokens: input.quota_daily_tokens,
        quota_monthly_tokens: input.quota_monthly_tokens,
        budget_daily_nanos: input.budget_daily_nanos,
        budget_daily_currency: input.budget_daily_currency.clone(),
        budget_monthly_nanos: input.budget_monthly_nanos,
        budget_monthly_currency: input.budget_monthly_currency.clone(),
        deleted_at: None,
        created_at: input.now,
        updated_at: input.now,
    };

    let inserted = match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::{ApiKeyDb, NewApiKeyDb};

            diesel::insert_into(api_key::table)
                .values(NewApiKeyDb::to_db(&new_key))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to import raw api key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::{ApiKeyDb, NewApiKeyDb};

            diesel::insert_into(api_key::table)
                .values(NewApiKeyDb::to_db(&new_key))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to import raw api key", err))
        }
    }?;
    if let Some(encrypted) = input.encrypted_secret.as_ref() {
        update_api_key_secret(conn, inserted.id, encrypted)?;
    }
    Ok(inserted)
}

pub(crate) fn update_api_key_secret(
    conn: &mut PortableRepositoryConnection<'_>,
    api_key_id: i64,
    encrypted: &EncryptedSecret,
) -> DbResult<()> {
    let ciphertext = Some(encrypted.ciphertext().to_vec());
    let nonce = Some(encrypted.nonce().to_vec());
    let format_version = Some(encrypted.format_version());
    let fingerprint = Some(encrypted.key_fingerprint().as_str().to_string());
    let updated = match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            diesel::update(
                api_key::table.filter(
                    api_key::dsl::id
                        .eq(api_key_id)
                        .and(api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                api_key::dsl::secret_ciphertext.eq(ciphertext),
                api_key::dsl::secret_nonce.eq(nonce),
                api_key::dsl::secret_format_version.eq(format_version),
                api_key::dsl::secret_key_fingerprint.eq(fingerprint),
            ))
            .execute(*conn)
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            diesel::update(
                api_key::table.filter(
                    api_key::dsl::id
                        .eq(api_key_id)
                        .and(api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                api_key::dsl::secret_ciphertext.eq(ciphertext),
                api_key::dsl::secret_nonce.eq(nonce),
                api_key::dsl::secret_format_version.eq(format_version),
                api_key::dsl::secret_key_fingerprint.eq(fingerprint),
            ))
            .execute(*conn)
        }
    }
    .map_err(|err| map_write_error("Failed to store imported api key secret", err))?;
    if updated != 1 {
        return Err(BaseError::NotFound(Some(format!(
            "Api key {api_key_id} not found"
        ))));
    }
    Ok(())
}

pub(crate) fn update_api_key_metadata(
    conn: &mut PortableRepositoryConnection<'_>,
    api_key_id: i64,
    data: &UpdateApiKeyData,
    updated_at: i64,
) -> DbResult<ApiKey> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::{ApiKeyDb, UpdateApiKeyDataDb};

            diesel::update(api_key::table.find(api_key_id))
                .set((
                    UpdateApiKeyDataDb::to_db(data),
                    api_key::dsl::updated_at.eq(updated_at),
                ))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported api key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::{ApiKeyDb, UpdateApiKeyDataDb};

            diesel::update(api_key::table.find(api_key_id))
                .set((
                    UpdateApiKeyDataDb::to_db(data),
                    api_key::dsl::updated_at.eq(updated_at),
                ))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported api key", err))
        }
    }
}

pub(crate) fn insert_api_key_acl_rule(
    conn: &mut PortableRepositoryConnection<'_>,
    row: &NewApiKeyAclRule,
) -> DbResult<ApiKeyAclRule> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key_acl_rule;
            use crate::database::api_key_acl_rule::_postgres_model::{
                ApiKeyAclRuleDb, NewApiKeyAclRuleDb,
            };

            diesel::insert_into(api_key_acl_rule::table)
                .values(NewApiKeyAclRuleDb::to_db(row))
                .returning(ApiKeyAclRuleDb::as_returning())
                .get_result::<ApiKeyAclRuleDb>(*conn)
                .map(ApiKeyAclRuleDb::from_db)
                .map_err(|err| map_write_error("Failed to import api key ACL rule", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key_acl_rule;
            use crate::database::api_key_acl_rule::_sqlite_model::{
                ApiKeyAclRuleDb, NewApiKeyAclRuleDb,
            };

            diesel::insert_into(api_key_acl_rule::table)
                .values(NewApiKeyAclRuleDb::to_db(row))
                .returning(ApiKeyAclRuleDb::as_returning())
                .get_result::<ApiKeyAclRuleDb>(*conn)
                .map(ApiKeyAclRuleDb::from_db)
                .map_err(|err| map_write_error("Failed to import api key ACL rule", err))
        }
    }
}

fn validate_raw_api_key_input(input: &RawApiKeyImportInput) -> DbResult<()> {
    if input.raw_api_key.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "raw api key must not be empty".to_string(),
        )));
    }
    if input.name.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "api key name must not be empty".to_string(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::BaseError,
        database::{
            TestDbContext,
            api_key::{ApiKey, hash_api_key},
            get_connection,
        },
        service::admin::portable_config::repository::with_transaction,
    };

    use super::{RawApiKeyImportInput, insert_raw_api_key};

    #[test]
    fn raw_api_key_import_writes_hash_and_display_fields_in_transaction() {
        let test_db_context = TestDbContext::new_sqlite("portable-raw-api-key-import.sqlite");

        test_db_context.run_sync(|| {
            let raw_key = "cyder-imported-secret-1234567890";
            let mut conn = get_connection().expect("connection");
            let inserted = with_transaction(&mut conn, |tx| {
                insert_raw_api_key(tx, &RawApiKeyImportInput::test(raw_key, "imported", 1000))
            })
            .expect("raw api key import should commit");

            let expected_hash = hash_api_key(raw_key);
            assert_eq!(inserted.api_key_hash, expected_hash);
            assert_eq!(inserted.key_prefix, "cyder-import");
            assert_eq!(inserted.key_last4, "7890");

            let loaded = ApiKey::get_by_hash(&expected_hash).expect("imported key should load");
            assert_eq!(loaded.id, inserted.id);
            assert_eq!(loaded.api_key_hash, expected_hash);
        });
    }

    #[test]
    fn raw_api_key_import_rolls_back_on_later_error() {
        let test_db_context = TestDbContext::new_sqlite("portable-raw-api-key-rollback.sqlite");

        test_db_context.run_sync(|| {
            let raw_key = "cyder-rollback-secret-1234567890";
            let mut conn = get_connection().expect("connection");
            let result = with_transaction(&mut conn, |tx| {
                let _inserted =
                    insert_raw_api_key(tx, &RawApiKeyImportInput::test(raw_key, "rollback", 1000))?;
                Err::<(), BaseError>(BaseError::ParamInvalid(Some(
                    "forced portable import failure".to_string(),
                )))
            });

            assert!(matches!(result, Err(BaseError::ParamInvalid(_))));
            assert!(matches!(
                ApiKey::get_by_hash(&hash_api_key(raw_key)),
                Err(BaseError::NotFound(_))
            ));
        });
    }
}
