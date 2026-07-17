use std::sync::Arc;

use chrono::Utc;

use crate::controller::BaseError;
use crate::database::provider::{
    BootstrapProviderInput, BootstrapProviderResult, NewProvider, NewProviderApiKey, Provider,
    ProviderApiKey, UpdateProviderApiKeyData, UpdateProviderData,
};
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
use crate::utils::ID_GENERATOR;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone)]
pub struct ProviderUpsertInput {
    pub name: String,
    pub key: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub provider_type: Option<ProviderType>,
    pub provider_api_key_mode: Option<ProviderApiKeyMode>,
}

#[derive(Debug, Clone)]
pub struct CreateProviderApiKeyInput {
    pub api_key: String,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct UpdateProviderApiKeyInput {
    pub api_key: Option<String>,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
}

pub struct ProviderAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ProviderAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_provider(&self, input: ProviderUpsertInput) -> Result<Provider, BaseError> {
        let current_time = Utc::now().timestamp_millis();
        let new_provider_data = NewProvider {
            id: ID_GENERATOR.generate_id(),
            provider_key: input.key,
            name: input.name,
            endpoint: input.endpoint,
            use_proxy: input.use_proxy,
            is_enabled: true,
            created_at: current_time,
            updated_at: current_time,
            provider_type: input.provider_type.unwrap_or(ProviderType::Openai),
            provider_api_key_mode: input
                .provider_api_key_mode
                .unwrap_or(ProviderApiKeyMode::Queue),
        };
        let created_provider = Provider::create(&new_provider_data)?;

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
    ) -> Result<Provider, BaseError> {
        let update_data = UpdateProviderData {
            provider_key: None,
            name: Some(input.name),
            endpoint: Some(input.endpoint),
            use_proxy: Some(input.use_proxy),
            is_enabled: None,
            provider_type: input.provider_type,
            provider_api_key_mode: input.provider_api_key_mode,
        };
        let updated_provider = Provider::update(id, &update_data)?;

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
    ) -> Result<ProviderApiKey, BaseError> {
        let _provider = Provider::get_by_id(provider_id)?;
        let current_time = Utc::now().timestamp_millis();
        let new_key_data = NewProviderApiKey {
            id: ID_GENERATOR.generate_id(),
            provider_id,
            api_key: input.api_key,
            description: input.description,
            is_enabled: input.is_enabled.unwrap_or(true),
            created_at: current_time,
            updated_at: current_time,
        };
        let created_key = ProviderApiKey::insert(&new_key_data)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("create", &created_key)),
        ])
        .await;

        Ok(created_key)
    }

    pub async fn update_provider_api_key(
        &self,
        provider_id: i64,
        key_id: i64,
        input: UpdateProviderApiKeyInput,
    ) -> Result<ProviderApiKey, BaseError> {
        let key_to_update = self.validate_provider_key_membership(provider_id, key_id)?;
        let update_data = UpdateProviderApiKeyData {
            api_key: input.api_key,
            description: input.description,
            is_enabled: input.is_enabled,
        };
        let updated_key = ProviderApiKey::update(key_id, &update_data)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("update", &updated_key)),
        ])
        .await;

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
        ProviderApiKey::delete(key_id)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id,
            }),
            AdminMutationEffect::audit(provider_api_key_audit_event("delete", &key_to_delete)),
        ])
        .await;

        Ok(())
    }

    pub async fn delete_provider(&self, id: i64) -> Result<(), BaseError> {
        let provider_to_delete = Provider::get_by_id(id)?;
        let num_deleted_db = Provider::delete_with_dependents(id)?;

        if num_deleted_db == 0 {
            return Ok(());
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

        self.run_post_commit_effects(effects).await;

        Ok(())
    }

    pub async fn bootstrap_provider_persist(
        &self,
        input: BootstrapProviderInput,
    ) -> Result<BootstrapProviderResult, BaseError> {
        let created = Provider::bootstrap(&input)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id: created.provider.id,
                key: Some(created.provider.provider_key.clone()),
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id: created.provider.id,
            }),
        ])
        .await;

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
    ) -> Result<ProviderApiKey, BaseError> {
        let _provider = Provider::get_by_id(provider_id)?;
        let key = ProviderApiKey::get_by_id(key_id)?;
        if key.provider_id != provider_id {
            return Err(BaseError::ParamInvalid(Some(format!(
                "API key {} does not belong to provider {}",
                key_id, provider_id
            ))));
        }

        Ok(key)
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn provider_audit_event(action: &'static str, provider: &Provider) -> AdminAuditEvent {
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
        AdminAuditField::new("provider_api_key_id", created.created_key.id),
        AdminAuditField::new("model_id", created.created_model.id),
        AdminAuditField::new("model_name", &created.created_model.model_name),
        AdminAuditField::new("check_performed", check_success.is_some()),
    ];
    fields.extend(AdminAuditField::optional("check_success", check_success));
    AdminAuditEvent::with_fields("manager.provider_bootstrapped", fields)
}

fn provider_api_key_audit_event(action: &'static str, key: &ProviderApiKey) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.provider_api_key_created",
        "update" => "manager.provider_api_key_updated",
        "delete" => "manager.provider_api_key_deleted",
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
