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
use super::await_cancellation_safe;
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Clone)]
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

    pub async fn list_api_keys(&self) -> Result<Vec<ApiKeySummary>, BaseError> {
        let database = self.mutation_runner.database();
        let mut summaries = ApiKey::list_summary(&database).await?;
        let metadata = ApiKey::list_reveal_metadata(&database).await?;
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

    pub async fn get_api_key_detail(&self, id: i64) -> Result<ApiKeyDetail, BaseError> {
        let database = self.mutation_runner.database();
        let mut detail = ApiKey::get_detail(&database, id).await?;
        self.apply_can_reveal(&mut detail).await?;
        Ok(detail)
    }

    pub async fn ensure_api_key_exists(&self, id: i64) -> Result<(), BaseError> {
        let database = self.mutation_runner.database();
        ApiKey::get_by_id(&database, id).await.map(|_| ())
    }

    pub async fn create_api_key(
        &self,
        payload: CreateApiKeyPayload,
    ) -> Result<ApiKeyDetailWithSecret, BaseError> {
        let service = self.clone();
        await_cancellation_safe(async move { service.create_api_key_owned(payload).await })
            .await
            .map_err(api_key_mutation_task_error)?
    }

    async fn create_api_key_owned(
        &self,
        payload: CreateApiKeyPayload,
    ) -> Result<ApiKeyDetailWithSecret, BaseError> {
        let secret = SensitiveSecret::new(generate_api_key_secret());
        let id = ID_GENERATOR.generate_id();
        let encrypted = self.encrypt_for_current_mode(id, &secret)?;
        let issuance = ApiKeyIssuance::new(id, secret.expose(), encrypted);
        let database = self.mutation_runner.database();
        let mut detail = ApiKey::create_issued(&database, &payload, &issuance).await?;
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
        let service = self.clone();
        await_cancellation_safe(async move { service.update_api_key_owned(id, payload).await })
            .await
            .map_err(api_key_mutation_task_error)?
    }

    async fn update_api_key_owned(
        &self,
        id: i64,
        payload: UpdateApiKeyMetadataPayload,
    ) -> Result<ApiKeyDetail, BaseError> {
        let database = self.mutation_runner.database();
        let mut updated = ApiKey::update_metadata(&database, id, &payload).await?;
        self.apply_can_reveal(&mut updated).await?;
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
        let service = self.clone();
        await_cancellation_safe(async move { service.rotate_api_key_owned(id).await })
            .await
            .map_err(api_key_mutation_task_error)?
    }

    async fn rotate_api_key_owned(&self, id: i64) -> Result<ApiKeyReveal, BaseError> {
        let database = self.mutation_runner.database();
        let existing = ApiKey::get_by_id(&database, id).await?;
        let old_hash = existing.api_key_hash.clone();
        let secret = SensitiveSecret::new(generate_api_key_secret());
        let encrypted = self.encrypt_for_current_mode(id, &secret)?;
        let issuance = ApiKeyIssuance::new(id, secret.expose(), encrypted);
        let rotated = ApiKey::rotate_issued(&database, id, &issuance).await?;
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

    pub async fn reveal_api_key(&self, id: i64) -> Result<ApiKeyReveal, BaseError> {
        let database = self.mutation_runner.database();
        let metadata = ApiKey::get_reveal_metadata(&database, id).await?;
        if !self.secret_encryption.can_reveal(
            metadata.secret_tuple_complete,
            metadata.secret_key_fingerprint.as_deref(),
        ) {
            self.log_reveal_unavailable(id, "not_current_or_recoverable");
            return Err(BaseError::ApiKeySecretUnavailable);
        }

        let stored = ApiKey::get_stored_secret(&database, id).await?;
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
        let service = self.clone();
        await_cancellation_safe(async move { service.delete_api_key_owned(id).await })
            .await
            .map_err(api_key_mutation_task_error)?
    }

    async fn delete_api_key_owned(&self, id: i64) -> Result<(), BaseError> {
        let database = self.mutation_runner.database();
        let existing = ApiKey::get_by_id(&database, id).await?;
        let api_key_hash = existing.api_key_hash.clone();
        ApiKey::delete(&database, id).await?;

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

    async fn apply_can_reveal(&self, detail: &mut ApiKeyDetail) -> Result<(), BaseError> {
        let database = self.mutation_runner.database();
        let metadata = ApiKey::get_reveal_metadata(&database, detail.id).await?;
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

fn api_key_mutation_task_error(_error: tokio::task::JoinError) -> BaseError {
    BaseError::InternalServerError(Some(
        "API key mutation task failed before publishing its state".to_string(),
    ))
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
    use std::{env, sync::Arc, time::Duration};

    use crate::config::SecretEncryptionConfig;
    use crate::controller::BaseError;
    use crate::database::TestDatabase;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload, hash_api_key};
    use crate::schema::enum_def::Action;
    use crate::service::catalog::CatalogService;
    use crate::service::secret_encryption::SecretEncryptionService;

    use super::{AdminMutationRunner, ApiKeyAdminService, UpdateApiKeyMetadataPayload};

    const CURRENT_KEY: &str = "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    const POSTGRES_SMOKE_URL_ENV: &str = "CYDER_R1_POSTGRES_MIGRATION_SMOKE_URL";
    const POSTGRES_SMOKE_DATABASE: &str = "cyder_r1_migration_smoke";

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
        database: &TestDatabase,
    ) -> (
        ApiKeyAdminService,
        Arc<CatalogService>,
        Arc<SecretEncryptionService>,
    ) {
        let catalog = Arc::new(CatalogService::new(database.runtime(), true).await);
        let encryption = Arc::new(SecretEncryptionService::from_config(&config(mode)));
        let mutation_runner = Arc::new(AdminMutationRunner::new(Arc::clone(&catalog)));
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

    async fn load_secret_tuple(database: &TestDatabase, id_value: i64) -> SecretTuple {
        ApiKey::secret_tuple_for_test(database, id_value)
            .await
            .expect("stored secret tuple should load")
    }

    async fn update_fingerprint(database: &TestDatabase, id_value: i64, fingerprint: &str) {
        ApiKey::update_secret_fingerprint_for_test(database, id_value, fingerprint.to_string())
            .await
            .expect("fingerprint fixture should update");
    }

    async fn corrupt_ciphertext(database: &TestDatabase, id_value: i64) {
        ApiKey::corrupt_secret_ciphertext_for_test(database, id_value)
            .await
            .expect("ciphertext fixture should update");
    }

    async fn execute_sql(database: &TestDatabase, sql: &str) {
        database
            .execute_sqlite_batch(sql)
            .await
            .expect("sqlite test SQL should execute");
    }

    #[tokio::test]
    async fn downstream_api_key_secret_lifecycle_obeys_modes_and_preserves_history() {
        let database = TestDatabase::new_sqlite_default("downstream-secret-lifecycle.sqlite").await;
        (async {
            let (recoverable, _, _) = service("recoverable", &database).await;
            let created = recoverable
                .create_api_key(payload("recoverable-key"))
                .await
                .expect("recoverable key should create");
            let id = created.detail.id;
            let raw = created.reveal.api_key.clone();
            assert!(created.detail.can_reveal);
            assert!(created.reveal.can_reveal);
            assert_eq!(
                ApiKey::get_by_hash(&database, &hash_api_key(&raw))
                    .await
                    .expect("created hash should authenticate")
                    .id,
                id
            );
            let stored_before = load_secret_tuple(&database, id).await;
            assert!(stored_before.0.is_some());
            assert!(!format!("{created:?}").contains(&raw));
            assert_eq!(
                recoverable
                    .reveal_api_key(id)
                    .await
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
            assert_eq!(load_secret_tuple(&database, id).await, stored_before);
            assert_eq!(
                recoverable
                    .reveal_api_key(id)
                    .await
                    .expect("disabled key remains revealable")
                    .api_key,
                raw
            );

            let (one_time, _, _) = service("one_time", &database).await;
            assert!(
                !one_time
                    .get_api_key_detail(id)
                    .await
                    .expect("one-time detail should load")
                    .can_reveal
            );
            assert!(matches!(
                one_time.reveal_api_key(id).await,
                Err(BaseError::ApiKeySecretUnavailable)
            ));
            assert_eq!(load_secret_tuple(&database, id).await, stored_before);

            let (recoverable_again, _, _) = service("recoverable", &database).await;
            assert!(
                recoverable_again
                    .get_api_key_detail(id)
                    .await
                    .expect("recoverable detail should load")
                    .can_reveal
            );
            assert_eq!(
                recoverable_again
                    .reveal_api_key(id)
                    .await
                    .expect("same current key should restore reveal")
                    .api_key,
                raw
            );
            assert!(
                recoverable_again
                    .list_api_keys()
                    .await
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
            assert_eq!(
                load_secret_tuple(&database, id).await,
                (None, None, None, None)
            );
            assert!(
                !recoverable_again
                    .get_api_key_detail(id)
                    .await
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
                    .await
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
                load_secret_tuple(&database, one_time_created.detail.id).await,
                (None, None, None, None)
            );
            assert!(matches!(
                one_time.reveal_api_key(one_time_created.detail.id).await,
                Err(BaseError::ApiKeySecretUnavailable)
            ));
        })
        .await;
    }

    #[tokio::test]
    async fn downstream_api_key_secret_lifecycle_degrades_unknown_and_corrupt_records() {
        let database = TestDatabase::new_sqlite_default("downstream-secret-degraded.sqlite").await;
        (async {
            let (service, _, _) = service("recoverable", &database).await;
            let unknown = service
                .create_api_key(payload("unknown-fingerprint"))
                .await
                .expect("unknown fixture should create");
            update_fingerprint(&database, unknown.detail.id, &"f".repeat(64)).await;
            assert!(
                !service
                    .get_api_key_detail(unknown.detail.id)
                    .await
                    .expect("unknown detail should load")
                    .can_reveal
            );
            assert!(matches!(
                service.reveal_api_key(unknown.detail.id).await,
                Err(BaseError::ApiKeySecretUnavailable)
            ));

            let corrupt = service
                .create_api_key(payload("corrupt-ciphertext"))
                .await
                .expect("corrupt fixture should create");
            corrupt_ciphertext(&database, corrupt.detail.id).await;
            assert!(
                service
                    .get_api_key_detail(corrupt.detail.id)
                    .await
                    .expect("current metadata should remain reveal eligible")
                    .can_reveal
            );
            assert!(matches!(
                service.reveal_api_key(corrupt.detail.id).await,
                Err(BaseError::ApiKeySecretUnavailable)
            ));
            assert_eq!(
                ApiKey::get_by_hash(&database, &hash_api_key(&corrupt.reveal.api_key))
                    .await
                    .expect("corrupt ciphertext must not break hash authentication")
                    .id,
                corrupt.detail.id
            );
        })
        .await;
    }

    #[tokio::test]
    async fn downstream_api_key_rotation_invalidates_old_cache_and_delete_clears_secret() {
        let database =
            TestDatabase::new_sqlite_default("downstream-secret-rotate-delete.sqlite").await;
        (async {
            let (service, catalog, _) = service("recoverable", &database).await;
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
                ApiKey::get_by_hash(&database, &hash_api_key(&old_raw)).await,
                Err(BaseError::NotFound(_))
            ));
            assert_eq!(
                service
                    .reveal_api_key(id)
                    .await
                    .expect("rotated key should reveal")
                    .api_key,
                rotated.api_key
            );

            service
                .delete_api_key(id)
                .await
                .expect("key should soft delete");
            assert!(matches!(
                ApiKey::get_by_id(&database, id).await,
                Err(BaseError::NotFound(_))
            ));
            assert_eq!(
                load_secret_tuple(&database, id).await,
                (None, None, None, None)
            );
        })
        .await;
    }

    #[tokio::test]
    async fn cancelled_api_key_update_still_invalidates_the_committed_cache_entry() {
        let database =
            TestDatabase::new_sqlite_default("downstream-api-key-cancelled-update.sqlite").await;
        let (service, catalog, _) = service("recoverable", &database).await;
        let service = Arc::new(service);
        let created = service
            .create_api_key(payload("cancelled-update"))
            .await
            .expect("key should create");
        let id = created.detail.id;
        let raw = created.reveal.api_key;
        assert!(
            catalog
                .get_api_key(&raw)
                .await
                .expect("cache should warm")
                .is_some()
        );
        service.mutation_runner().drain_audit_events();

        let gate = service.mutation_runner().pause_before_effects_for_test();
        let updating_service = Arc::clone(&service);
        let caller = tokio::spawn(async move {
            updating_service
                .update_api_key(
                    id,
                    UpdateApiKeyMetadataPayload {
                        is_enabled: Some(false),
                        ..Default::default()
                    },
                )
                .await
        });
        tokio::time::timeout(Duration::from_secs(2), gate.wait_until_reached())
            .await
            .expect("update should commit and reach post-commit effects");
        caller.abort();
        let _ = caller.await;

        assert!(
            !ApiKey::get_by_id(&database, id)
                .await
                .expect("committed key should load")
                .is_enabled
        );
        assert!(
            catalog
                .get_api_key(&raw)
                .await
                .expect("pre-invalidation cache lookup should work")
                .is_some(),
            "the gate should prove the stale cache entry existed after commit"
        );

        gate.resume();
        tokio::time::timeout(Duration::from_secs(2), gate.wait_until_completed())
            .await
            .expect("detached mutation owner should finish post-commit effects");
        assert!(
            catalog
                .get_api_key(&raw)
                .await
                .expect("invalidated cache lookup should work")
                .is_none()
        );
        assert!(
            service
                .mutation_runner()
                .drain_audit_events()
                .iter()
                .any(|event| event.event_name() == "manager.api_key_updated")
        );
    }

    #[tokio::test]
    async fn downstream_api_key_secret_writes_roll_back_as_one_transaction() {
        let database = TestDatabase::new_sqlite_default("downstream-secret-rollback.sqlite").await;
        (async {
                let (service, _, _) = service("recoverable", &database).await;
                execute_sql(&database,
                    "CREATE TRIGGER fail_api_key_secret_create BEFORE UPDATE OF secret_ciphertext ON api_key WHEN OLD.secret_ciphertext IS NULL AND NEW.secret_ciphertext IS NOT NULL BEGIN SELECT RAISE(ABORT, 'blocked secret create'); END;",
                ).await;
                assert!(service
                    .create_api_key(payload("create-must-rollback"))
                    .await
                    .is_err());
                execute_sql(&database, "DROP TRIGGER fail_api_key_secret_create;").await;
                assert_eq!(
                    ApiKey::list_all_active(&database)
                        .await
                        .expect("API keys should list")
                        .into_iter()
                        .filter(|api_key| api_key.name == "create-must-rollback")
                        .count(),
                    0,
                );

                let created = service
                    .create_api_key(payload("rollback-existing"))
                    .await
                    .expect("existing fixture should create");
                let id = created.detail.id;
                let old_hash = hash_api_key(&created.reveal.api_key);
                let old_secret = load_secret_tuple(&database, id).await;

                execute_sql(&database,
                    "CREATE TRIGGER fail_api_key_rotate BEFORE UPDATE OF api_key_hash ON api_key BEGIN SELECT RAISE(ABORT, 'blocked rotate'); END;",
                ).await;
                assert!(service.rotate_api_key(id).await.is_err());
                execute_sql(&database, "DROP TRIGGER fail_api_key_rotate;").await;
                assert_eq!(ApiKey::get_by_id(&database, id).await.expect("existing key should remain").api_key_hash, old_hash);
                assert_eq!(load_secret_tuple(&database, id).await, old_secret);

                execute_sql(&database,
                    "CREATE TRIGGER fail_api_key_delete BEFORE UPDATE OF deleted_at ON api_key BEGIN SELECT RAISE(ABORT, 'blocked delete'); END;",
                ).await;
                assert!(service.delete_api_key(id).await.is_err());
                execute_sql(&database, "DROP TRIGGER fail_api_key_delete;").await;
                assert!(ApiKey::get_by_id(&database, id).await.is_ok());
                assert_eq!(load_secret_tuple(&database, id).await, old_secret);
            })
            .await;
    }

    #[tokio::test]
    #[ignore = "requires a dedicated PostgreSQL 17 database"]
    async fn postgres_downstream_api_key_lifecycle_matches_sqlite_contract() {
        let database_url = env::var(POSTGRES_SMOKE_URL_ENV).unwrap_or_else(|_| {
            panic!("{POSTGRES_SMOKE_URL_ENV} must point to the dedicated PostgreSQL smoke database")
        });
        TestDatabase::reset_dedicated_postgres_schema(&database_url, POSTGRES_SMOKE_DATABASE);

        let database = TestDatabase::new_postgres(&database_url).await;
        (async {
            let (recoverable, catalog, _) = service("recoverable", &database).await;
            let created = recoverable
                .create_api_key(payload("postgres-recoverable"))
                .await
                .expect("postgres recoverable key should create");
            let id = created.detail.id;
            let raw = created.reveal.api_key;
            assert!(created.detail.can_reveal);
            assert!(load_secret_tuple(&database, id).await.0.is_some());
            assert_eq!(
                recoverable
                    .reveal_api_key(id)
                    .await
                    .expect("postgres key should reveal")
                    .api_key,
                raw
            );

            let before_disable = load_secret_tuple(&database, id).await;
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
            assert_eq!(load_secret_tuple(&database, id).await, before_disable);

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
            assert!(
                ApiKey::get_by_hash(&database, &hash_api_key(&rotated.api_key))
                    .await
                    .is_ok()
            );

            recoverable
                .delete_api_key(id)
                .await
                .expect("postgres key should delete");
            assert_eq!(
                load_secret_tuple(&database, id).await,
                (None, None, None, None)
            );

            let (one_time, _, _) = service("one_time", &database).await;
            let one_time_created = one_time
                .create_api_key(payload("postgres-one-time"))
                .await
                .expect("postgres one-time key should create");
            assert!(!one_time_created.detail.can_reveal);
            assert_eq!(
                load_secret_tuple(&database, one_time_created.detail.id).await,
                (None, None, None, None)
            );
        })
        .await;
        drop(database);

        TestDatabase::reset_dedicated_postgres_schema(&database_url, POSTGRES_SMOKE_DATABASE);
    }
}
