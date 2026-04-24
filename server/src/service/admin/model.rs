use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::model::{Model, ModelCapabilityFlags, UpdateModelData};
use crate::database::model_route::ModelRoute;
use crate::database::provider::Provider;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{
    AdminCatalogInvalidation, AdminModelCacheName, AdminModelRouteCacheTarget, AdminMutationEffect,
    AdminMutationRunner,
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
        let affected_routes = ModelRoute::list_by_model_id(id)?;
        let num_deleted = Model::delete_with_dependents(id)?;

        if num_deleted == 0 {
            return Ok(());
        }

        let mut effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                id,
                name: Some(model_cache_name(&provider, &model.model_name)),
                previous_name: None,
            }),
            AdminMutationEffect::audit(model_audit_event("delete", &model)),
        ];

        if !affected_routes.is_empty() {
            effects.insert(
                0,
                AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ModelRoutes(
                    affected_routes
                        .into_iter()
                        .map(|route| {
                            AdminModelRouteCacheTarget::new(route.id, Some(route.route_name))
                        })
                        .collect(),
                )),
            );
        }

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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use diesel::connection::SimpleConnection;

    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteCandidateInput,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{CreateRequestPatchPayload, RequestPatchRule};
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::schema::enum_def::{
        ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    };
    use crate::service::app_state::create_test_app_state;
    use serde_json::json;

    use super::{CreateModelInput, ModelAdminService, UpdateModelInput};

    fn seed_provider(id: i64, provider_key: &str) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: provider_key.to_string(),
            name: provider_key.to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_model_for_provider(provider_id: i64, model_name: &str) -> Model {
        Model::create(provider_id, model_name, None, true, default_capabilities())
            .expect("model seed should succeed")
    }

    fn seed_route(route_name: &str, model_id: i64) -> ModelRoute {
        ModelRoute::create(&CreateModelRoutePayload {
            route_name: route_name.to_string(),
            description: Some("seed route".to_string()),
            is_enabled: Some(true),
            expose_in_models: Some(true),
            candidates: vec![ModelRouteCandidateInput {
                model_id,
                priority: 0,
                is_enabled: Some(true),
            }],
        })
        .expect("route seed should succeed")
        .route
    }

    fn create_request_patch_payload() -> CreateRequestPatchPayload {
        CreateRequestPatchPayload {
            placement: RequestPatchPlacement::Body,
            target: "/temperature".to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(Some(json!(0.2))),
            description: Some("patch".to_string()),
            is_enabled: Some(true),
            confirm_dangerous_target: None,
        }
    }

    fn install_sqlite_model_delete_failure_trigger(model_id: i64) {
        let mut connection = get_connection().expect("test connection should open");
        let DbConnection::Sqlite(conn) = &mut connection else {
            panic!("model delete rollback test requires sqlite");
        };

        conn.batch_execute(&format!(
            "
            CREATE TRIGGER fail_model_delete_patches_{model_id}
            BEFORE UPDATE ON request_patch_rule
            WHEN NEW.model_id = {model_id} AND NEW.deleted_at IS NOT NULL
            BEGIN
                SELECT RAISE(ABORT, 'forced model delete failure');
            END;
            "
        ))
        .expect("model delete failure trigger should install");
    }

    fn default_capabilities() -> ModelCapabilityFlags {
        ModelCapabilityFlags {
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
        }
    }

    fn create_input(provider_id: i64, model_name: &str) -> CreateModelInput {
        CreateModelInput {
            provider_id,
            model_name: model_name.to_string(),
            real_model_name: None,
            is_enabled: true,
            capabilities: default_capabilities(),
        }
    }

    fn update_input(model_name: &str) -> UpdateModelInput {
        UpdateModelInput {
            model_name: model_name.to_string(),
            real_model_name: None,
            is_enabled: true,
            cost_catalog_id: None,
            supports_streaming: Some(true),
            supports_tools: Some(true),
            supports_reasoning: Some(true),
            supports_image_input: Some(true),
            supports_embeddings: Some(true),
            supports_rerank: Some(true),
        }
    }

    fn service(app_state: &Arc<crate::service::app_state::AppState>) -> &ModelAdminService {
        app_state.admin.model.as_ref()
    }

    #[tokio::test]
    async fn create_model_refreshes_negative_name_cache_and_catalog() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-create.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(10101, "openai");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_before = app_state
                    .catalog
                    .get_model_by_name(&provider.provider_key, "gpt-4o-mini")
                    .await
                    .expect("model cache should load");
                assert!(cached_before.is_none());

                let created = service(&app_state)
                    .create_model(create_input(provider.id, "gpt-4o-mini"))
                    .await
                    .expect("model create should succeed");

                let cached_after = app_state
                    .catalog
                    .get_model_by_name(&provider.provider_key, "gpt-4o-mini")
                    .await
                    .expect("model cache should reload")
                    .expect("model should exist");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");

                assert_eq!(cached_after.id, created.id);
                assert!(
                    catalog_after
                        .models
                        .iter()
                        .any(|item| item.id == created.id && item.model_name == "gpt-4o-mini")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn update_model_renames_and_invalidates_old_name_cache() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-update.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(10201, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_before = app_state
                    .catalog
                    .get_model_by_name(&provider.provider_key, "gpt-4o-mini")
                    .await
                    .expect("old name cache should load")
                    .expect("old name should exist");
                assert_eq!(cached_before.id, model.id);

                let updated = service(&app_state)
                    .update_model(model.id, update_input("gpt-4.1-mini"))
                    .await
                    .expect("model update should succeed");

                let old_name_after = app_state
                    .catalog
                    .get_model_by_name(&provider.provider_key, "gpt-4o-mini")
                    .await
                    .expect("old name cache should reload");
                let new_name_after = app_state
                    .catalog
                    .get_model_by_name(&provider.provider_key, "gpt-4.1-mini")
                    .await
                    .expect("new name cache should load")
                    .expect("new name should exist");
                let cached_by_id = app_state
                    .catalog
                    .get_model_by_id(model.id)
                    .await
                    .expect("id cache should reload")
                    .expect("model id should exist");

                assert_eq!(updated.model_name, "gpt-4.1-mini");
                assert!(old_name_after.is_none());
                assert_eq!(new_name_after.id, model.id);
                assert_eq!(cached_by_id.model_name, "gpt-4.1-mini");
            })
            .await;
    }

    #[tokio::test]
    async fn delete_model_refreshes_preloaded_routes_and_request_patch_caches() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-delete.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(10301, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                RequestPatchRule::create_for_model(model.id, &create_request_patch_payload())
                    .expect("request patch seed should succeed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let route_before = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should load")
                    .expect("route should exist");
                let rules_before = app_state
                    .catalog
                    .get_model_request_patch_rules(model.id)
                    .await
                    .expect("request patch cache should load");
                let effective_before = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective patch cache should load")
                    .expect("effective patch cache should exist");

                assert_eq!(route_before.candidates.len(), 1);
                assert_eq!(rules_before.len(), 1);
                assert_eq!(effective_before.effective_rules.len(), 1);

                service(&app_state)
                    .delete_model(model.id)
                    .await
                    .expect("model delete should succeed");

                let model_after = app_state
                    .catalog
                    .get_model_by_id(model.id)
                    .await
                    .expect("model cache should reload");
                let route_after = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should reload")
                    .expect("route should still exist");
                let effective_after = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective patch cache should reload");

                assert!(model_after.is_none());
                assert!(route_after.candidates.is_empty());
                assert!(effective_after.is_none());
                assert!(
                    RequestPatchRule::list_all()
                        .expect("request patch list should load")
                        .iter()
                        .all(|rule| rule.model_id != Some(model.id))
                );
            })
            .await;
    }

    #[tokio::test]
    async fn delete_model_rolls_back_primary_delete_when_dependent_cleanup_fails() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-delete-rollback.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(10311, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                RequestPatchRule::create_for_model(model.id, &create_request_patch_payload())
                    .expect("request patch seed should succeed");
                install_sqlite_model_delete_failure_trigger(model.id);

                let app_state = create_test_app_state(test_db_context.clone()).await;

                let err = service(&app_state)
                    .delete_model(model.id)
                    .await
                    .expect_err("model delete should fail");
                let message = format!("{err:?}");
                assert!(
                    message.contains("forced model delete failure"),
                    "unexpected error: {message}"
                );

                let model_after = Model::get_by_id(model.id).expect("model should still exist");
                let route_after =
                    ModelRoute::get_detail(route.id).expect("route should still load");
                let patches_after =
                    RequestPatchRule::list_by_model_id(model.id).expect("patches should load");

                assert!(model_after.deleted_at.is_none());
                assert!(model_after.is_enabled);
                assert_eq!(route_after.candidates.len(), 1);
                assert!(route_after.candidates[0].candidate.deleted_at.is_none());
                assert!(route_after.candidates[0].candidate.is_enabled);
                assert_eq!(patches_after.len(), 1);
                assert!(patches_after[0].is_enabled);
            })
            .await;
    }
}
