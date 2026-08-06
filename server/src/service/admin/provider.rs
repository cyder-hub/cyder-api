use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;

use crate::controller::BaseError;
use crate::database::provider::{
    BootstrapProviderInput, BootstrapProviderResult, NewProvider, NewProviderApiKey, Provider,
    ProviderAggregate, ProviderApiKeyRepository, ProviderApiKeySummary,
    UpdateProviderApiKeyMetadata, UpdateProviderData,
};
use crate::database::upstream_source::{
    NewUpstreamSource, PRIMARY_SOURCE_KEY, UpdateUpstreamSourceData,
};
use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
use crate::service::provider_http::normalize_provider_endpoint;
use crate::service::secret_encryption::{SecretDomain, SecretEncryptionService, SensitiveSecret};
use crate::service::upstream_profile::{UpstreamAuthProfile, upstream_runtime_profile};
use crate::service::vertex::{invalidate_vertex_token, validate_vertex_service_account};
use crate::utils::ID_GENERATOR;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone)]
pub struct ProviderUpsertInput {
    pub name: String,
    pub key: String,
    pub upstream_source: UpstreamSourceUpsertInput,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

#[derive(Debug, Clone)]
pub struct UpstreamSourceUpsertInput {
    pub endpoint: String,
    pub use_proxy: bool,
    pub profile_type: Option<UpstreamProfileType>,
}

#[derive(Clone)]
pub struct CreateProviderApiKeyInput {
    pub api_key: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateProviderApiKeyInput {
    pub description: Option<String>,
    pub is_enabled: bool,
}

#[derive(Clone)]
pub struct ReplaceProviderApiKeyInput {
    pub api_key: String,
}

#[derive(Serialize)]
pub struct ProviderApiKeyReveal {
    #[serde(flatten)]
    pub summary: ProviderApiKeySummary,
    pub api_key: String,
}

#[derive(Clone)]
pub struct BootstrapProviderCommand {
    pub provider_id: i64,
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub profile_type: UpstreamProfileType,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub api_key: String,
    pub api_key_description: Option<String>,
    pub model_name: String,
    pub real_model_name: Option<String>,
}

pub struct ProviderAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
    secret_encryption: Arc<SecretEncryptionService>,
}

impl ProviderAdminService {
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

    pub async fn create_provider(
        &self,
        input: ProviderUpsertInput,
    ) -> Result<ProviderAggregate, BaseError> {
        let endpoint =
            normalize_provider_endpoint(&input.upstream_source.endpoint).map_err(|error| {
                BaseError::ParamInvalid(Some(format!("upstream source endpoint {error}")))
            })?;
        let current_time = Utc::now().timestamp_millis();
        let provider_id = ID_GENERATOR.generate_id();
        let new_provider_data = NewProvider {
            id: provider_id,
            provider_key: input.key,
            name: input.name,
            is_enabled: true,
            created_at: current_time,
            updated_at: current_time,
            provider_api_key_mode: input
                .provider_api_key_mode
                .unwrap_or(ProviderApiKeyMode::Queue),
        };
        let new_source_data = NewUpstreamSource {
            id: ID_GENERATOR.generate_id(),
            provider_id,
            source_key: PRIMARY_SOURCE_KEY.to_string(),
            profile_type: input
                .upstream_source
                .profile_type
                .unwrap_or(UpstreamProfileType::Openai),
            endpoint,
            use_proxy: input.upstream_source.use_proxy,
            created_at: current_time,
            updated_at: current_time,
        };
        let created_provider = Provider::create(&new_provider_data, &new_source_data)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: created_provider.id,
                key: Some(created_provider.provider_key.clone()),
            }),
            AdminMutationEffect::audit(provider_audit_event("create", &created_provider)),
        ])
        .await;

        Ok(created_provider)
    }

    pub async fn update_provider(
        &self,
        id: i64,
        input: ProviderUpsertInput,
    ) -> Result<ProviderAggregate, BaseError> {
        let endpoint =
            normalize_provider_endpoint(&input.upstream_source.endpoint).map_err(|error| {
                BaseError::ParamInvalid(Some(format!("upstream source endpoint {error}")))
            })?;
        let update_data = UpdateProviderData {
            provider_key: None,
            name: Some(input.name),
            is_enabled: None,
            provider_api_key_mode: input.provider_api_key_mode,
        };
        let source_update = UpdateUpstreamSourceData {
            profile_type: input.upstream_source.profile_type,
            endpoint: Some(endpoint),
            use_proxy: Some(input.upstream_source.use_proxy),
            updated_at: 0,
        };
        let updated_provider = Provider::update(id, &update_data, &source_update)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: updated_provider.id,
                key: Some(updated_provider.provider_key.clone()),
            }),
            AdminMutationEffect::audit(provider_audit_event("update", &updated_provider)),
        ])
        .await;

        Ok(updated_provider)
    }

    pub async fn create_provider_api_key(
        &self,
        provider_id: i64,
        input: CreateProviderApiKeyInput,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let provider = Provider::get_by_id(provider_id)?;
        let current_time = Utc::now().timestamp_millis();
        let key_id = ID_GENERATOR.generate_id();
        let secret = validate_provider_secret(input.api_key)?;
        validate_provider_secret_for_type(&provider.upstream_source.profile_type, &secret)?;
        let (key_prefix, key_last4) = secret_mask_parts(secret.expose());
        let encrypted_secret = self
            .secret_encryption
            .encrypt_current(SecretDomain::ProviderApiKey(key_id), &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let secret_hmac = self
            .secret_encryption
            .provider_secret_fingerprint(provider_id, &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let new_key_data = NewProviderApiKey {
            id: key_id,
            provider_id,
            description: input.description,
            key_prefix,
            key_last4,
            encrypted_secret,
            secret_hmac,
            is_enabled: true,
            created_at: current_time,
            updated_at: current_time,
        };
        let created_key = ProviderApiKeyRepository::insert(&new_key_data)?;

        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("create", &created_key)),
        ])
        .await?;

        Ok(created_key)
    }

    pub async fn update_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
        input: UpdateProviderApiKeyInput,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let key_to_update = self.validate_provider_key_membership(provider_id, key_id)?;
        let update_data = UpdateProviderApiKeyMetadata {
            description: input.description,
            is_enabled: input.is_enabled,
        };
        let updated_key =
            ProviderApiKeyRepository::update_metadata(provider_id, key_id, &update_data)?;

        invalidate_vertex_token(key_id);
        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("update", &updated_key)),
        ])
        .await?;

        // Keep the fetched row in scope so membership validation remains explicit.
        let _ = key_to_update;

        Ok(updated_key)
    }

    pub async fn delete_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<(), BaseError> {
        let key_to_delete = self.validate_provider_key_membership(provider_id, key_id)?;
        ProviderApiKeyRepository::soft_delete(provider_id, key_id)?;

        invalidate_vertex_token(key_id);
        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("delete", &key_to_delete)),
        ])
        .await?;

        Ok(())
    }

    pub(crate) fn decrypt_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<SensitiveSecret, BaseError> {
        let _provider = Provider::get_by_id(provider_id)?;
        let stored = ProviderApiKeyRepository::get_stored_by_id(provider_id, key_id)?;
        let encrypted = stored
            .encrypted_secret()
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        self.secret_encryption
            .decrypt_current(SecretDomain::ProviderApiKey(key_id), &encrypted)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)
    }

    pub fn list_provider_api_keys(
        &self,
        provider_id: i64,
    ) -> Result<Vec<ProviderApiKeySummary>, BaseError> {
        let _provider = Provider::get_by_id(provider_id)?;
        ProviderApiKeyRepository::list_summaries_by_provider_id(provider_id)
    }

    pub fn get_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        self.validate_provider_key_membership(provider_id, key_id)
    }

    pub async fn replace_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
        input: ReplaceProviderApiKeyInput,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let provider = Provider::get_by_id(provider_id)?;
        let _existing = self.validate_provider_key_membership(provider_id, key_id)?;
        let secret = validate_provider_secret(input.api_key)?;
        validate_provider_secret_for_type(&provider.upstream_source.profile_type, &secret)?;
        let (key_prefix, key_last4) = secret_mask_parts(secret.expose());
        let encrypted = self
            .secret_encryption
            .encrypt_current(SecretDomain::ProviderApiKey(key_id), &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let hmac = self
            .secret_encryption
            .provider_secret_fingerprint(provider_id, &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let updated = ProviderApiKeyRepository::replace_secret(
            provider_id,
            key_id,
            key_prefix,
            key_last4,
            &encrypted,
            &hmac,
        )?;
        invalidate_vertex_token(key_id);
        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("replace", &updated)),
        ])
        .await?;
        Ok(updated)
    }

    pub async fn reveal_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<ProviderApiKeyReveal, BaseError> {
        let summary = self.validate_provider_key_membership(provider_id, key_id)?;
        let secret = self.decrypt_provider_api_key(provider_id, key_id)?;
        self.run_post_commit_effects(vec![AdminMutationEffect::audit(
            provider_api_key_audit_event("reveal", &summary),
        )])
        .await;
        Ok(ProviderApiKeyReveal {
            summary,
            api_key: secret.to_unprotected_string(),
        })
    }

    pub async fn delete_provider(&self, id: i64) -> Result<(), BaseError> {
        let provider_to_delete = Provider::get_by_id(id)?;
        let provider_key_ids = ProviderApiKeyRepository::list_summaries_by_provider_id(id)?
            .into_iter()
            .map(|key| key.id)
            .collect::<Vec<_>>();
        let num_deleted_db = Provider::delete_with_dependents(id)?;

        if num_deleted_db == 0 {
            return Ok(());
        }
        for key_id in provider_key_ids {
            invalidate_vertex_token(key_id);
        }

        let effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id,
                key: Some(provider_to_delete.provider_key.clone()),
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id: id,
            }),
            AdminMutationEffect::audit(provider_audit_event("delete", &provider_to_delete)),
        ];

        let report = self.mutation_runner.execute(&effects).await;
        if report.has_catalog_failures() {
            return Err(BaseError::ProviderRuntimeRefreshFailed);
        }

        Ok(())
    }

    pub async fn bootstrap_provider_persist(
        &self,
        input: BootstrapProviderCommand,
    ) -> Result<BootstrapProviderResult, BaseError> {
        let endpoint = normalize_provider_endpoint(&input.endpoint)
            .map_err(|error| BaseError::ParamInvalid(Some(format!("provider endpoint {error}"))))?;
        let key_id = ID_GENERATOR.generate_id();
        let secret = validate_provider_secret(input.api_key)?;
        validate_provider_secret_for_type(&input.profile_type, &secret)?;
        let (key_prefix, key_last4) = secret_mask_parts(secret.expose());
        let encrypted_secret = self
            .secret_encryption
            .encrypt_current(SecretDomain::ProviderApiKey(key_id), &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let secret_hmac = self
            .secret_encryption
            .provider_secret_fingerprint(input.provider_id, &secret)
            .map_err(|_| BaseError::ProviderApiKeySecretUnavailable)?;
        let created = Provider::bootstrap(&BootstrapProviderInput {
            provider_id: input.provider_id,
            provider_key: input.provider_key,
            name: input.name,
            source_id: ID_GENERATOR.generate_id(),
            endpoint,
            use_proxy: input.use_proxy,
            profile_type: input.profile_type,
            provider_api_key_mode: input.provider_api_key_mode,
            provider_api_key_id: key_id,
            api_key_description: input.api_key_description,
            key_prefix,
            key_last4,
            encrypted_secret,
            secret_hmac,
            model_name: input.model_name,
            real_model_name: input.real_model_name,
        })?;

        self.run_provider_key_post_commit(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: created.provider.id,
                key: Some(created.provider.provider_key.clone()),
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id: created.provider.id,
            }),
        ])
        .await?;

        Ok(created)
    }

    pub async fn record_bootstrap_audit(
        &self,
        created: &BootstrapProviderResult,
        check_success: Option<bool>,
    ) {
        self.run_post_commit_effects(vec![AdminMutationEffect::audit(
            provider_bootstrap_audit_event(created, check_success),
        )])
        .await;
    }

    fn validate_provider_key_membership(
        &self,
        provider_id: i64,
        key_id: i64,
    ) -> Result<ProviderApiKeySummary, BaseError> {
        let _provider = Provider::get_by_id(provider_id)?;
        ProviderApiKeyRepository::get_summary_by_id(provider_id, key_id)
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }

    async fn run_provider_key_post_commit(
        &self,
        effects: Vec<AdminMutationEffect>,
    ) -> Result<(), BaseError> {
        let report = self.mutation_runner.execute(&effects).await;
        if report.has_catalog_failures() {
            return Err(BaseError::ProviderRuntimeRefreshFailed);
        }
        Ok(())
    }
}

fn provider_audit_event(action: &'static str, provider: &ProviderAggregate) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.provider_created",
        "update" => "manager.provider_updated",
        "delete" => "manager.provider_deleted",
        _ => unreachable!("unsupported provider audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("provider_id", provider.id),
            AdminAuditField::new("provider_key", &provider.provider_key),
            AdminAuditField::new("provider_name", &provider.name),
            AdminAuditField::new("is_enabled", provider.is_enabled),
            AdminAuditField::new("source_id", provider.upstream_source.id),
            AdminAuditField::new("source_key", &provider.upstream_source.source_key),
            AdminAuditField::new(
                "source_profile_type",
                format!("{:?}", provider.upstream_source.profile_type),
            ),
        ],
    )
}

fn provider_bootstrap_audit_event(
    created: &BootstrapProviderResult,
    check_success: Option<bool>,
) -> AdminAuditEvent {
    let mut fields = vec![
        AdminAuditField::new("action", "bootstrap"),
        AdminAuditField::new("provider_id", created.provider.id),
        AdminAuditField::new("provider_key", &created.provider.provider_key),
        AdminAuditField::new("provider_name", &created.provider.name),
        AdminAuditField::new("is_enabled", created.provider.is_enabled),
        AdminAuditField::new("source_id", created.provider.upstream_source.id),
        AdminAuditField::new("source_key", &created.provider.upstream_source.source_key),
        AdminAuditField::new(
            "source_profile_type",
            format!("{:?}", created.provider.upstream_source.profile_type),
        ),
        AdminAuditField::new("provider_api_key_id", created.created_key.id),
        AdminAuditField::new("model_id", created.created_model.id),
        AdminAuditField::new("model_name", &created.created_model.model_name),
        AdminAuditField::new("check_performed", check_success.is_some()),
    ];
    fields.extend(AdminAuditField::optional("check_success", check_success));
    AdminAuditEvent::with_fields("manager.provider_bootstrapped", fields)
}

fn provider_api_key_audit_event(
    action: &'static str,
    key: &ProviderApiKeySummary,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.provider_api_key_created",
        "update" => "manager.provider_api_key_updated",
        "delete" => "manager.provider_api_key_deleted",
        "replace" => "manager.provider_api_key_replaced",
        "reveal" => "manager.provider_api_key_revealed",
        _ => unreachable!("unsupported provider api key audit action: {action}"),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("provider_id", key.provider_id),
            AdminAuditField::new("provider_api_key_id", key.id),
            AdminAuditField::new("is_enabled", key.is_enabled),
            AdminAuditField::new("description_present", key.description.is_some()),
        ],
    )
}

fn validate_provider_secret(value: String) -> Result<SensitiveSecret, BaseError> {
    if value.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "provider API key must not be empty".to_string(),
        )));
    }
    Ok(SensitiveSecret::new(value))
}

fn secret_mask_parts(secret: &str) -> (String, String) {
    let characters = secret.chars().collect::<Vec<_>>();
    let visible_budget = characters.len().saturating_sub(1);
    let prefix_len = visible_budget.div_ceil(2).min(4);
    let suffix_len = visible_budget.saturating_sub(prefix_len).min(4);
    let prefix = characters.iter().take(prefix_len).collect::<String>();
    let last4 = characters
        .iter()
        .rev()
        .take(suffix_len)
        .rev()
        .collect::<String>();
    (prefix, last4)
}

fn validate_provider_secret_for_type(
    profile_type: &UpstreamProfileType,
    secret: &SensitiveSecret,
) -> Result<(), BaseError> {
    if upstream_runtime_profile(profile_type).auth == UpstreamAuthProfile::VertexOAuth {
        validate_vertex_service_account(secret.expose())
            .map_err(|message| BaseError::ParamInvalid(Some(message)))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_secret_masks_always_hide_at_least_one_character() {
        let cases = [
            ("a", "", ""),
            ("ab", "a", ""),
            ("abc", "a", "c"),
            ("abcd", "ab", "d"),
            ("abcde", "ab", "de"),
            ("abcdef", "abc", "ef"),
            ("abcdefg", "abc", "efg"),
            ("abcdefgh", "abcd", "fgh"),
            ("abcdefghi", "abcd", "fghi"),
            ("abcdefghij", "abcd", "ghij"),
        ];

        for (secret, expected_prefix, expected_suffix) in cases {
            let (prefix, suffix) = secret_mask_parts(secret);
            assert_eq!(prefix, expected_prefix, "unexpected prefix for {secret}");
            assert_eq!(suffix, expected_suffix, "unexpected suffix for {secret}");
            assert!(
                prefix.chars().count() + suffix.chars().count() < secret.chars().count(),
                "mask fragments must not expose the complete secret: {secret}"
            );
        }
    }

    #[test]
    fn provider_secret_masks_count_unicode_characters_without_overlap() {
        let secret = "密钥甲乙丙";
        let (prefix, suffix) = secret_mask_parts(secret);

        assert_eq!(prefix, "密钥");
        assert_eq!(suffix, "乙丙");
    }

    #[test]
    fn provider_secret_validation_accepts_non_empty_secrets_of_any_length() {
        let secret = validate_provider_secret("x".to_string())
            .expect("single-character provider credentials remain valid");

        assert_eq!(secret.expose(), "x");
    }

    #[test]
    fn vertex_credentials_require_safe_service_account_structure_and_rsa_key() {
        let marker = "private-sensitive-marker";
        let unsupported_token_uri = SensitiveSecret::new(format!(
            r#"{{"client_email":"svc@example.com","token_uri":"https://oauth.example.com/token","private_key_id":"kid","private_key":"{marker}"}}"#
        ));
        let error =
            validate_provider_secret_for_type(&UpstreamProfileType::Vertex, &unsupported_token_uri)
                .expect_err("unsupported token URI must be rejected");
        let message = match error {
            BaseError::ParamInvalid(Some(message)) => message,
            other => panic!("unexpected error: {other:?}"),
        };
        assert_eq!(
            message,
            "Vertex credential token_uri must exactly match https://oauth2.googleapis.com/token"
        );
        assert!(!message.contains(marker));

        let malformed = SensitiveSecret::new(format!(
            r#"{{"client_email":"svc@example.com","token_uri":"https://oauth2.googleapis.com/token","private_key_id":"kid","private_key":"{marker}"}}"#
        ));
        let error = validate_provider_secret_for_type(&UpstreamProfileType::Vertex, &malformed)
            .expect_err("invalid RSA key must be rejected");
        let message = match error {
            BaseError::ParamInvalid(Some(message)) => message,
            other => panic!("unexpected error: {other:?}"),
        };
        assert_eq!(
            message,
            "Vertex credential contains an invalid RSA private key"
        );
        assert!(!message.contains(marker));

        validate_provider_secret_for_type(&UpstreamProfileType::Openai, &malformed)
            .expect("ordinary provider credentials are opaque strings");
    }
}
