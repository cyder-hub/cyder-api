use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::model::{Model, ModelCapabilityFlags, UpdateModelData};
use crate::database::provider::Provider;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{
    AdminCatalogInvalidation, AdminModelCacheName, AdminMutationEffect, AdminMutationRunner,
};

#[derive(Debug, Clone)]
pub struct CreateModelInput {
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub capabilities: ModelCapabilityFlags,
}

#[derive(Debug, Clone)]
pub struct UpdateModelInput {
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub cost_catalog_id: Option<i64>,
    pub supports_streaming: Option<bool>,
    pub supports_tools: Option<bool>,
    pub supports_reasoning: Option<bool>,
    pub supports_image_input: Option<bool>,
    pub supports_embeddings: Option<bool>,
    pub supports_rerank: Option<bool>,
}

pub struct ModelAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ModelAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_model(&self, input: CreateModelInput) -> Result<Model, BaseError> {
        let provider = Provider::get_by_id(input.provider_id)?;
        let created = Model::create(
            input.provider_id,
            &input.model_name,
            input.real_model_name.as_deref(),
            input.is_enabled,
            input.capabilities,
        )?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id: created.id,
                name: Some(model_cache_name(&provider, &created.model_name)),
                previous_name: None,
            }),
            AdminMutationEffect::audit(model_audit_event("create", &created)),
        ])
        .await;

        Ok(created)
    }

    pub async fn update_model(&self, id: i64, input: UpdateModelInput) -> Result<Model, BaseError> {
        let existing = Model::get_by_id(id)?;
        let provider = Provider::get_by_id(existing.provider_id)?;
        let updated = Model::update(
            id,
            &UpdateModelData {
                model_name: Some(input.model_name),
                real_model_name: Some(input.real_model_name),
                is_enabled: Some(input.is_enabled),
                cost_catalog_id: Some(input.cost_catalog_id),
                supports_streaming: input.supports_streaming,
                supports_tools: input.supports_tools,
                supports_reasoning: input.supports_reasoning,
                supports_image_input: input.supports_image_input,
                supports_embeddings: input.supports_embeddings,
                supports_rerank: input.supports_rerank,
            },
        )?;

        let previous_name = if existing.model_name != updated.model_name {
            Some(model_cache_name(&provider, &existing.model_name))
        } else {
            None
        };

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id: updated.id,
                name: Some(model_cache_name(&provider, &updated.model_name)),
                previous_name,
            }),
            AdminMutationEffect::audit(model_audit_event("update", &updated)),
        ])
        .await;

        Ok(updated)
    }

    pub async fn delete_model(&self, id: i64) -> Result<(), BaseError> {
        let model = Model::get_by_id(id)?;
        let provider = Provider::get_by_id(model.provider_id)?;
        let num_deleted = Model::delete_with_dependents(id)?;

        if num_deleted == 0 {
            return Ok(());
        }

        let effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id,
                name: Some(model_cache_name(&provider, &model.model_name)),
                previous_name: None,
            }),
            AdminMutationEffect::audit(model_audit_event("delete", &model)),
        ];

        self.run_post_commit_effects(effects).await;

        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn model_cache_name(provider: &Provider, model_name: &str) -> AdminModelCacheName {
    AdminModelCacheName::new(provider.provider_key.clone(), model_name.to_string())
}

fn model_audit_event(action: &'static str, model: &Model) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.model_created",
        "update" => "manager.model_updated",
        "delete" => "manager.model_deleted",
        _ => unreachable!("unsupported model audit action: {action}"),
    };

    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("model_id", model.id),
        AdminAuditField::new("provider_id", model.provider_id),
        AdminAuditField::new("model_name", &model.model_name),
        AdminAuditField::new("is_enabled", model.is_enabled),
    ];
    fields.extend(AdminAuditField::optional(
        "real_model_name",
        model.real_model_name.as_deref(),
    ));
    AdminAuditEvent::with_fields(event_name, fields)
}
