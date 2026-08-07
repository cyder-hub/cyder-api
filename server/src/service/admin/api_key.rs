use std::sync::Arc;

use cyder_tools::log::{info, warn};

use crate::controller::BaseError;
use crate::database::api_key::{
    ApiKey, ApiKeyDetail, ApiKeyDetailWithSecret, ApiKeyIssuance, ApiKeyReveal, ApiKeySummary,
    CreateApiKeyPayload, UpdateApiKeyMetadataPayload, build_reveal, generate_api_key_secret,
};
use crate::logging::event_message_with_fields;
use crate::service::secret_encryption::{
    EncryptedSecret, SecretDomain, SecretEncryptionService, SensitiveSecret,
};
use crate::utils::ID_GENERATOR;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

pub struct ApiKeyAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
    secret_encryption: Arc<SecretEncryptionService>,
}

impl ApiKeyAdminService {
    pub(crate) fn new(
        mutation_runner: Arc<AdminMutationRunner>,
        secret_encryption: Arc<SecretEncryptionService>,
    ) -> Self {
        Self {
            mutation_runner,
            secret_encryption,
        }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    #[cfg(test)]
    pub(crate) fn secret_encryption(&self) -> &Arc<SecretEncryptionService> {
        &self.secret_encryption
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKeySummary>, BaseError> {
        let mut summaries = ApiKey::list_summary()?;
        let metadata = ApiKey::list_reveal_metadata()?;
        for summary in &mut summaries {
            if let Some(secret) = metadata.iter().find(|secret| secret.id == summary.id) {
                summary.can_reveal = self.secret_encryption.can_reveal(
                    secret.secret_tuple_complete,
                    secret.secret_key_fingerprint.as_deref(),
                );
            }
        }
        Ok(summaries)
    }

    pub fn get_api_key_detail(&self, id: i64) -> Result<ApiKeyDetail, BaseError> {
        let mut detail = ApiKey::get_detail(id)?;
        self.apply_can_reveal(&mut detail)?;
        Ok(detail)
    }

    pub fn ensure_api_key_exists(&self, id: i64) -> Result<(), BaseError> {
        ApiKey::get_by_id(id).map(|_| ())
    }

    pub async fn create_api_key(
        &self,
        payload: CreateApiKeyPayload,
    ) -> Result<ApiKeyDetailWithSecret, BaseError> {
        let secret = SensitiveSecret::new(generate_api_key_secret());
        let id = ID_GENERATOR.generate_id();
        let encrypted = self.encrypt_for_current_mode(id, &secret)?;
        let issuance = ApiKeyIssuance::new(id, secret.expose(), encrypted);
        let mut detail = ApiKey::create_issued(&payload, &issuance)?;
        detail.can_reveal = issuance.has_encrypted_secret();
        let reveal = ApiKeyReveal {
            id: detail.id,
            name: detail.name.clone(),
            key_prefix: detail.key_prefix.clone(),
            key_last4: detail.key_last4.clone(),
            api_key: secret.to_unprotected_string(),
            updated_at: detail.updated_at,
            can_reveal: detail.can_reveal,
        };
        let created = ApiKeyDetailWithSecret { detail, reveal };
        self.run_post_commit_effects(vec![AdminMutationEffect::audit(api_key_audit_event(
            "create",
            created.detail.id,
            &created.detail.name,
            Some(created.detail.is_enabled),
        ))])
        .await;
        Ok(created)
    }

    pub async fn update_api_key(
        &self,
        id: i64,
        payload: UpdateApiKeyMetadataPayload,
    ) -> Result<ApiKeyDetail, BaseError> {
        let mut updated = ApiKey::update_metadata(id, &payload)?;
        self.apply_can_reveal(&mut updated)?;
        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyId { id }),
            AdminMutationEffect::audit(api_key_audit_event(
                "update",
                updated.id,
                &updated.name,
                Some(updated.is_enabled),
            )),
        ])
        .await;
        Ok(updated)
    }

    pub async fn rotate_api_key(&self, id: i64) -> Result<ApiKeyReveal, BaseError> {
        let existing = ApiKey::get_by_id(id)?;
        let old_hash = existing.api_key_hash.clone();
        let secret = SensitiveSecret::new(generate_api_key_secret());
        let encrypted = self.encrypt_for_current_mode(id, &secret)?;
        let issuance = ApiKeyIssuance::new(id, secret.expose(), encrypted);
        let rotated = ApiKey::rotate_issued(id, &issuance)?;
        let reveal = build_reveal(
            &rotated,
            secret.to_unprotected_string(),
            issuance.has_encrypted_secret(),
        );

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyHash {
                api_key_hash: old_hash,
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyId {
                id: rotated.id,
            }),
            AdminMutationEffect::audit(api_key_audit_event(
                "rotate",
                rotated.id,
                &rotated.name,
                Some(existing.is_enabled),
            )),
        ])
        .await;
        Ok(reveal)
    }

    pub fn reveal_api_key(&self, id: i64) -> Result<ApiKeyReveal, BaseError> {
        let metadata = ApiKey::get_reveal_metadata(id)?;
        if !self.secret_encryption.can_reveal(
            metadata.secret_tuple_complete,
            metadata.secret_key_fingerprint.as_deref(),
        ) {
            self.log_reveal_unavailable(id, "not_current_or_recoverable");
            return Err(BaseError::ApiKeySecretUnavailable);
        }

        let stored = ApiKey::get_stored_secret(id)?;
        let encrypted = EncryptedSecret::from_parts(
            stored.ciphertext,
            stored.nonce,
            stored.format_version,
            stored.key_fingerprint,
        )
        .map_err(|_| {
            self.log_reveal_unavailable(id, "invalid_stored_format");
            BaseError::ApiKeySecretUnavailable
        })?;
        let plaintext = self
            .secret_encryption
            .decrypt_current(SecretDomain::DownstreamApiKey(id), &encrypted)
            .map_err(|_| {
                self.log_reveal_unavailable(id, "authentication_failed");
                BaseError::ApiKeySecretUnavailable
            })?;
        info!(
            "{}",
            event_message_with_fields(
                "manager.api_key_revealed",
                &[("api_key_id", Some(id.to_string()))],
            )
        );
        Ok(ApiKeyReveal {
            id: stored.id,
            name: stored.name,
            key_prefix: stored.key_prefix,
            key_last4: stored.key_last4,
            api_key: plaintext.to_unprotected_string(),
            updated_at: stored.updated_at,
            can_reveal: true,
        })
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<(), BaseError> {
        let existing = ApiKey::get_by_id(id)?;
        let api_key_hash = existing.api_key_hash.clone();
        ApiKey::delete(id)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyHash {
                api_key_hash,
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyId { id }),
            AdminMutationEffect::audit(api_key_audit_event(
                "delete",
                existing.id,
                &existing.name,
                Some(existing.is_enabled),
            )),
        ])
        .await;
        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }

    fn apply_can_reveal(&self, detail: &mut ApiKeyDetail) -> Result<(), BaseError> {
        let metadata = ApiKey::get_reveal_metadata(detail.id)?;
        detail.can_reveal = self.secret_encryption.can_reveal(
            metadata.secret_tuple_complete,
            metadata.secret_key_fingerprint.as_deref(),
        );
        Ok(())
    }

    fn encrypt_for_current_mode(
        &self,
        id: i64,
        secret: &SensitiveSecret,
    ) -> Result<Option<EncryptedSecret>, BaseError> {
        if self.secret_encryption.downstream_mode() == crate::config::DownstreamSecretMode::OneTime
        {
            return Ok(None);
        }
        self.secret_encryption
            .encrypt_current(SecretDomain::DownstreamApiKey(id), secret)
            .map(Some)
            .map_err(|_| {
                BaseError::InternalServerError(Some("failed to protect api key secret".to_string()))
            })
    }

    fn log_reveal_unavailable(&self, id: i64, reason_code: &'static str) {
        warn!(
            "{}",
            event_message_with_fields(
                "manager.api_key_reveal_unavailable",
                &[
                    ("api_key_id", Some(id.to_string())),
                    ("reason_code", Some(reason_code.to_string())),
                ],
            )
        );
    }
}

fn api_key_audit_event(
    action: &'static str,
    api_key_id: i64,
    api_key_name: &str,
    is_enabled: Option<bool>,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.api_key_created",
        "update" => "manager.api_key_updated",
        "rotate" => "manager.api_key_rotated",
        "delete" => "manager.api_key_deleted",
        _ => unreachable!("unsupported api key audit action: {action}"),
    };
    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("api_key_id", api_key_id),
        AdminAuditField::new("api_key_name", api_key_name),
    ];
    fields.extend(AdminAuditField::optional("is_enabled", is_enabled));
    AdminAuditEvent::with_fields(event_name, fields)
}

#[cfg(test)]
mod tests {
    use std::{env, sync::Arc};

    use diesel::{
        PgConnection, QueryableByName, RunQueryDsl, connection::SimpleConnection, prelude::*,
        sql_types::Text,
    };

    use crate::config::SecretEncryptionConfig;
    use crate::controller::BaseError;
    use crate::database::api_key::{
        _postgres_model, _sqlite_model, ApiKey, CreateApiKeyPayload, hash_api_key,
    };
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::schema::enum_def::Action;
    use crate::service::catalog::CatalogService;
    use crate::service::secret_encryption::SecretEncryptionService;

    use super::{AdminMutationRunner, ApiKeyAdminService, UpdateApiKeyMetadataPayload};

    const CURRENT_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const POSTGRES_SMOKE_URL_ENV: &str = "CYDER_R1_POSTGRES_MIGRATION_SMOKE_URL";
    const POSTGRES_SMOKE_DATABASE: &str = "cyder_r1_migration_smoke";

    #[derive(QueryableByName)]
    struct DatabaseNameRow {
        #[diesel(sql_type = Text)]
        name: String,
    }

    type SecretTuple = (
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<i32>,
        Option<String>,
    );

    fn config(mode: &str) -> SecretEncryptionConfig {
        serde_yaml::from_str(&format!(
            "downstream_mode: {mode}\nencryption_key: '{CURRENT_KEY}'\n"
        ))
        .expect("secret encryption test config should parse")
    }

    async fn service(
        mode: &str,
    ) -> (
        ApiKeyAdminService,
        Arc<CatalogService>,
        Arc<SecretEncryptionService>,
    ) {
        let catalog = Arc::new(CatalogService::new(true).await);
        let encryption = Arc::new(SecretEncryptionService::from_config(&config(mode)));
        let mutation_runner = Arc::new(AdminMutationRunner::new(
            Arc::clone(&catalog),
            Arc::new(crate::service::runtime::SourceCircuitService::new_memory()),
        ));
        (
            ApiKeyAdminService::new(mutation_runner, Arc::clone(&encryption)),
            catalog,
            encryption,
        )
    }

    fn payload(name: &str) -> CreateApiKeyPayload {
        CreateApiKeyPayload {
            name: name.to_string(),
            description: Some("R2.6 lifecycle test".to_string()),
            default_action: Some(Action::Allow),
            is_enabled: Some(true),
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
            acl_rules: None,
        }
    }

    fn load_secret_tuple(id_value: i64) -> SecretTuple {
        let connection = &mut get_connection().expect("test database connection should load");
        crate::db_execute!(connection, {
            api_key::table
                .filter(api_key::dsl::id.eq(id_value))
                .select((
                    api_key::dsl::secret_ciphertext,
                    api_key::dsl::secret_nonce,
                    api_key::dsl::secret_format_version,
                    api_key::dsl::secret_key_fingerprint,
                ))
                .first(connection)
                .expect("stored secret tuple should load")
        })
    }

    fn update_fingerprint(id_value: i64, fingerprint: &str) {
        let mut connection = get_connection().expect("test database connection should load");
        let DbConnection::Sqlite(connection) = &mut connection else {
            panic!("lifecycle unit test requires sqlite")
        };
        use crate::database::_sqlite_schema::api_key::dsl as key;
        diesel::update(key::api_key.filter(key::id.eq(id_value)))
            .set(key::secret_key_fingerprint.eq(Some(fingerprint.to_string())))
            .execute(connection)
            .expect("fingerprint fixture should update");
    }

    fn corrupt_ciphertext(id_value: i64) {
        let mut tuple = load_secret_tuple(id_value);
        tuple.0.as_mut().expect("ciphertext should exist")[0] ^= 1;
        let mut connection = get_connection().expect("test database connection should load");
        let DbConnection::Sqlite(connection) = &mut connection else {
            panic!("lifecycle unit test requires sqlite")
        };
        use crate::database::_sqlite_schema::api_key::dsl as key;
        diesel::update(key::api_key.filter(key::id.eq(id_value)))
            .set(key::secret_ciphertext.eq(tuple.0))
            .execute(connection)
            .expect("ciphertext fixture should update");
    }

    fn execute_sql(sql: &str) {
        let mut connection = get_connection().expect("test database connection should load");
        let DbConnection::Sqlite(connection) = &mut connection else {
            panic!("lifecycle unit test requires sqlite")
        };
        connection
            .batch_execute(sql)
            .expect("sqlite test SQL should execute");
    }

    #[tokio::test]
    async fn downstream_api_key_secret_lifecycle_obeys_modes_and_preserves_history() {
        let database = TestDbContext::new_sqlite("downstream-secret-lifecycle.sqlite");
        database
            .run_async(async {
                let (recoverable, _, _) = service("recoverable").await;
                let created = recoverable
                    .create_api_key(payload("recoverable-key"))
                    .await
                    .expect("recoverable key should create");
                let id = created.detail.id;
                let raw = created.reveal.api_key.clone();
                assert!(created.detail.can_reveal);
                assert!(created.reveal.can_reveal);
                assert_eq!(
                    ApiKey::get_by_hash(&hash_api_key(&raw))
                        .expect("created hash should authenticate")
                        .id,
                    id
                );
                let stored_before = load_secret_tuple(id);
                assert!(stored_before.0.is_some());
                assert!(!format!("{created:?}").contains(&raw));
                assert_eq!(
                    recoverable
                        .reveal_api_key(id)
                        .expect("current recoverable key should reveal")
                        .api_key,
                    raw
                );

                let disabled = recoverable
                    .update_api_key(
                        id,
                        UpdateApiKeyMetadataPayload {
                            is_enabled: Some(false),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("key should disable");
                assert!(!disabled.is_enabled);
                assert!(disabled.can_reveal);
                assert_eq!(load_secret_tuple(id), stored_before);
                assert_eq!(
                    recoverable
                        .reveal_api_key(id)
                        .expect("disabled key remains revealable")
                        .api_key,
                    raw
                );

                let (one_time, _, _) = service("one_time").await;
                assert!(
                    !one_time
                        .get_api_key_detail(id)
                        .expect("one-time detail should load")
                        .can_reveal
                );
                assert!(matches!(
                    one_time.reveal_api_key(id),
                    Err(BaseError::ApiKeySecretUnavailable)
                ));
                assert_eq!(load_secret_tuple(id), stored_before);

                let (recoverable_again, _, _) = service("recoverable").await;
                assert!(
                    recoverable_again
                        .get_api_key_detail(id)
                        .expect("recoverable detail should load")
                        .can_reveal
                );
                assert_eq!(
                    recoverable_again
                        .reveal_api_key(id)
                        .expect("same current key should restore reveal")
                        .api_key,
                    raw
                );
                assert!(
                    recoverable_again
                        .list_api_keys()
                        .expect("recoverable list should load")
                        .into_iter()
                        .find(|item| item.id == id)
                        .expect("created key should be listed")
                        .can_reveal
                );

                let one_time_rotated = one_time
                    .rotate_api_key(id)
                    .await
                    .expect("one-time rotation should replace the secret");
                assert_ne!(one_time_rotated.api_key, raw);
                assert!(!one_time_rotated.can_reveal);
                assert_eq!(load_secret_tuple(id), (None, None, None, None));
                assert!(
                    !recoverable_again
                        .get_api_key_detail(id)
                        .expect("rotated detail should load")
                        .can_reveal
                );

                let recoverable_rotated = recoverable_again
                    .rotate_api_key(id)
                    .await
                    .expect("recoverable rotation should replace and retain the secret");
                assert!(recoverable_rotated.can_reveal);
                assert_eq!(
                    recoverable_again
                        .reveal_api_key(id)
                        .expect("recoverable rotation should remain revealable")
                        .api_key,
                    recoverable_rotated.api_key
                );

                let one_time_created = one_time
                    .create_api_key(payload("one-time-key"))
                    .await
                    .expect("one-time key should create");
                assert!(!one_time_created.detail.can_reveal);
                assert!(!one_time_created.reveal.can_reveal);
                assert_eq!(
                    load_secret_tuple(one_time_created.detail.id),
                    (None, None, None, None)
                );
                assert!(matches!(
                    one_time.reveal_api_key(one_time_created.detail.id),
                    Err(BaseError::ApiKeySecretUnavailable)
                ));
            })
            .await;
    }

    #[tokio::test]
    async fn downstream_api_key_secret_lifecycle_degrades_unknown_and_corrupt_records() {
        let database = TestDbContext::new_sqlite("downstream-secret-degraded.sqlite");
        database
            .run_async(async {
                let (service, _, _) = service("recoverable").await;
                let unknown = service
                    .create_api_key(payload("unknown-fingerprint"))
                    .await
                    .expect("unknown fixture should create");
                update_fingerprint(unknown.detail.id, &"f".repeat(64));
                assert!(
                    !service
                        .get_api_key_detail(unknown.detail.id)
                        .expect("unknown detail should load")
                        .can_reveal
                );
                assert!(matches!(
                    service.reveal_api_key(unknown.detail.id),
                    Err(BaseError::ApiKeySecretUnavailable)
                ));

                let corrupt = service
                    .create_api_key(payload("corrupt-ciphertext"))
                    .await
                    .expect("corrupt fixture should create");
                corrupt_ciphertext(corrupt.detail.id);
                assert!(
                    service
                        .get_api_key_detail(corrupt.detail.id)
                        .expect("current metadata should remain reveal eligible")
                        .can_reveal
                );
                assert!(matches!(
                    service.reveal_api_key(corrupt.detail.id),
                    Err(BaseError::ApiKeySecretUnavailable)
                ));
                assert_eq!(
                    ApiKey::get_by_hash(&hash_api_key(&corrupt.reveal.api_key))
                        .expect("corrupt ciphertext must not break hash authentication")
                        .id,
                    corrupt.detail.id
                );
            })
            .await;
    }

    #[tokio::test]
    async fn downstream_api_key_rotation_invalidates_old_cache_and_delete_clears_secret() {
        let database = TestDbContext::new_sqlite("downstream-secret-rotate-delete.sqlite");
        database
            .run_async(async {
                let (service, catalog, _) = service("recoverable").await;
                let created = service
                    .create_api_key(payload("rotate-key"))
                    .await
                    .expect("key should create");
                let id = created.detail.id;
                let old_raw = created.reveal.api_key;
                assert!(
                    catalog
                        .get_api_key(&old_raw)
                        .await
                        .expect("old cache lookup should work")
                        .is_some()
                );

                let rotated = service.rotate_api_key(id).await.expect("key should rotate");
                assert_ne!(rotated.api_key, old_raw);
                assert!(
                    catalog
                        .get_api_key(&old_raw)
                        .await
                        .expect("old cache lookup should complete")
                        .is_none()
                );
                assert!(
                    catalog
                        .get_api_key(&rotated.api_key)
                        .await
                        .expect("new cache lookup should work")
                        .is_some()
                );
                assert!(matches!(
                    ApiKey::get_by_hash(&hash_api_key(&old_raw)),
                    Err(BaseError::NotFound(_))
                ));
                assert_eq!(
                    service
                        .reveal_api_key(id)
                        .expect("rotated key should reveal")
                        .api_key,
                    rotated.api_key
                );

                service
                    .delete_api_key(id)
                    .await
                    .expect("key should soft delete");
                assert!(matches!(ApiKey::get_by_id(id), Err(BaseError::NotFound(_))));
                assert_eq!(load_secret_tuple(id), (None, None, None, None));
            })
            .await;
    }

    #[tokio::test]
    async fn downstream_api_key_secret_writes_roll_back_as_one_transaction() {
        let database = TestDbContext::new_sqlite("downstream-secret-rollback.sqlite");
        database
            .run_async(async {
                let (service, _, _) = service("recoverable").await;
                execute_sql(
                    "CREATE TRIGGER fail_api_key_secret_create BEFORE UPDATE OF secret_ciphertext ON api_key WHEN OLD.secret_ciphertext IS NULL AND NEW.secret_ciphertext IS NOT NULL BEGIN SELECT RAISE(ABORT, 'blocked secret create'); END;",
                );
                assert!(service
                    .create_api_key(payload("create-must-rollback"))
                    .await
                    .is_err());
                execute_sql("DROP TRIGGER fail_api_key_secret_create;");
                let mut connection = get_connection().expect("test database connection should load");
                let DbConnection::Sqlite(connection) = &mut connection else {
                    panic!("rollback unit test requires sqlite")
                };
                use crate::database::_sqlite_schema::api_key::dsl as key;
                assert_eq!(
                    key::api_key
                        .filter(key::name.eq("create-must-rollback"))
                        .count()
                        .get_result::<i64>(connection)
                        .expect("row count should load"),
                    0
                );

                let created = service
                    .create_api_key(payload("rollback-existing"))
                    .await
                    .expect("existing fixture should create");
                let id = created.detail.id;
                let old_hash = hash_api_key(&created.reveal.api_key);
                let old_secret = load_secret_tuple(id);

                execute_sql(
                    "CREATE TRIGGER fail_api_key_rotate BEFORE UPDATE OF api_key_hash ON api_key BEGIN SELECT RAISE(ABORT, 'blocked rotate'); END;",
                );
                assert!(service.rotate_api_key(id).await.is_err());
                execute_sql("DROP TRIGGER fail_api_key_rotate;");
                assert_eq!(ApiKey::get_by_id(id).expect("existing key should remain").api_key_hash, old_hash);
                assert_eq!(load_secret_tuple(id), old_secret);

                execute_sql(
                    "CREATE TRIGGER fail_api_key_delete BEFORE UPDATE OF deleted_at ON api_key BEGIN SELECT RAISE(ABORT, 'blocked delete'); END;",
                );
                assert!(service.delete_api_key(id).await.is_err());
                execute_sql("DROP TRIGGER fail_api_key_delete;");
                assert!(ApiKey::get_by_id(id).is_ok());
                assert_eq!(load_secret_tuple(id), old_secret);
            })
            .await;
    }

    #[tokio::test]
    #[ignore = "requires a dedicated PostgreSQL 17 database"]
    async fn postgres_downstream_api_key_lifecycle_matches_sqlite_contract() {
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

        let database = TestDbContext::new_postgres(&database_url);
        database
            .run_async(async {
                let (recoverable, catalog, _) = service("recoverable").await;
                let created = recoverable
                    .create_api_key(payload("postgres-recoverable"))
                    .await
                    .expect("postgres recoverable key should create");
                let id = created.detail.id;
                let raw = created.reveal.api_key;
                assert!(created.detail.can_reveal);
                assert!(load_secret_tuple(id).0.is_some());
                assert_eq!(
                    recoverable
                        .reveal_api_key(id)
                        .expect("postgres key should reveal")
                        .api_key,
                    raw
                );

                let before_disable = load_secret_tuple(id);
                recoverable
                    .update_api_key(
                        id,
                        UpdateApiKeyMetadataPayload {
                            is_enabled: Some(false),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("postgres key should disable");
                assert_eq!(load_secret_tuple(id), before_disable);

                let rotated = recoverable
                    .rotate_api_key(id)
                    .await
                    .expect("postgres key should rotate");
                assert!(
                    catalog
                        .get_api_key(&raw)
                        .await
                        .expect("old postgres hash lookup should finish")
                        .is_none()
                );
                assert!(ApiKey::get_by_hash(&hash_api_key(&rotated.api_key)).is_ok());

                recoverable
                    .delete_api_key(id)
                    .await
                    .expect("postgres key should delete");
                assert_eq!(load_secret_tuple(id), (None, None, None, None));

                let (one_time, _, _) = service("one_time").await;
                let one_time_created = one_time
                    .create_api_key(payload("postgres-one-time"))
                    .await
                    .expect("postgres one-time key should create");
                assert!(!one_time_created.detail.can_reveal);
                assert_eq!(
                    load_secret_tuple(one_time_created.detail.id),
                    (None, None, None, None)
                );
            })
            .await;
        drop(database);

        let mut cleanup = PgConnection::establish(&database_url)
            .expect("dedicated postgres smoke database should remain reachable");
        cleanup
            .batch_execute("DROP SCHEMA public CASCADE; CREATE SCHEMA public;")
            .expect("postgres smoke schema should clean up");
    }
}
