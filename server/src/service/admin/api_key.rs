use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::api_key::{
    ApiKey, ApiKeyDetail, ApiKeyDetailWithSecret, ApiKeyReveal, CreateApiKeyPayload,
    UpdateApiKeyMetadataPayload, hash_api_key,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

pub struct ApiKeyAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ApiKeyAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_api_key(
        &self,
        payload: CreateApiKeyPayload,
    ) -> Result<ApiKeyDetailWithSecret, BaseError> {
        let created = ApiKey::create(&payload)?;
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
        let updated = ApiKey::update_metadata(id, &payload)?;
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
        let old_hash = existing
            .api_key_hash
            .clone()
            .unwrap_or_else(|| hash_api_key(&existing.api_key));
        let rotated = ApiKey::rotate_key(id)?;

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
        Ok(rotated)
    }

    pub async fn delete_api_key(&self, id: i64) -> Result<(), BaseError> {
        let existing = ApiKey::get_by_id(id)?;
        let api_key_hash = existing
            .api_key_hash
            .clone()
            .unwrap_or_else(|| hash_api_key(&existing.api_key));
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
