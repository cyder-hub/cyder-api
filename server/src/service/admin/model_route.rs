use std::collections::BTreeMap;
use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::model_route::{
    ApiKeyModelOverride, CreateModelRoutePayload, ModelRoute, ModelRouteDetail,
    UpdateModelRoutePayload,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

pub struct ModelRouteAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl ModelRouteAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_model_route(
        &self,
        payload: CreateModelRoutePayload,
    ) -> Result<ModelRouteDetail, BaseError> {
        let detail = ModelRoute::create(&payload)?;

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ModelRoute {
                id: detail.route.id,
                name: Some(detail.route.route_name.clone()),
                previous_name: None,
            }),
            AdminMutationEffect::audit(model_route_audit_event(
                "create",
                &detail.route,
                Some(detail.candidates.len()),
            )),
        ])
        .await;

        Ok(detail)
    }

    pub async fn update_model_route(
        &self,
        id: i64,
        payload: UpdateModelRoutePayload,
    ) -> Result<ModelRouteDetail, BaseError> {
        let original_route = ModelRoute::get_by_id(id)?;
        let detail = ModelRoute::update(id, &payload)?;
        let previous_name = if original_route.route_name != detail.route.route_name {
            Some(original_route.route_name.clone())
        } else {
            None
        };

        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ModelRoute {
                id: detail.route.id,
                name: Some(detail.route.route_name.clone()),
                previous_name,
            }),
            AdminMutationEffect::audit(model_route_audit_event(
                "update",
                &detail.route,
                Some(detail.candidates.len()),
            )),
        ])
        .await;

        Ok(detail)
    }

    pub async fn delete_model_route(&self, id: i64) -> Result<(), BaseError> {
        let route = ModelRoute::get_by_id(id)?;
        let overrides = ApiKeyModelOverride::list_by_target_route_id(id)?;
        ModelRoute::delete_with_dependents(id)?;

        let mut effects = vec![AdminMutationEffect::catalog_invalidation(
            AdminCatalogInvalidation::ModelRoute {
                id,
                name: Some(route.route_name.clone()),
                previous_name: None,
            },
        )];
        effects.extend(api_key_override_invalidation_effects(&overrides));
        effects.push(AdminMutationEffect::catalog_invalidation(
            AdminCatalogInvalidation::ModelsCatalog,
        ));
        effects.push(AdminMutationEffect::audit(model_route_audit_event(
            "delete", &route, None,
        )));

        self.run_post_commit_effects(effects).await;

        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn api_key_override_invalidation_effects(
    overrides: &[ApiKeyModelOverride],
) -> Vec<AdminMutationEffect> {
    let mut source_names_by_api_key = BTreeMap::<i64, Vec<String>>::new();
    for override_row in overrides {
        source_names_by_api_key
            .entry(override_row.api_key_id)
            .or_default()
            .push(override_row.source_name.clone());
    }

    source_names_by_api_key
        .into_iter()
        .map(|(api_key_id, source_names)| {
            AdminMutationEffect::catalog_invalidation(
                AdminCatalogInvalidation::ApiKeyModelOverrides {
                    api_key_id,
                    source_names,
                },
            )
        })
        .collect()
}

fn model_route_audit_event(
    action: &'static str,
    route: &ModelRoute,
    candidate_count: Option<usize>,
) -> AdminAuditEvent {
    let event_name = match action {
        "create" => "manager.model_route_created",
        "update" => "manager.model_route_updated",
        "delete" => "manager.model_route_deleted",
        _ => unreachable!("unsupported model route audit action: {action}"),
    };

    let mut fields = vec![
        AdminAuditField::new("action", action),
        AdminAuditField::new("route_id", route.id),
        AdminAuditField::new("route_name", &route.route_name),
        AdminAuditField::new("is_enabled", route.is_enabled),
        AdminAuditField::new("expose_in_models", route.expose_in_models),
    ];
    fields.extend(AdminAuditField::optional(
        "candidate_count",
        candidate_count,
    ));
    AdminAuditEvent::with_fields(event_name, fields)
}

#[cfg(test)]
mod tests {
    use diesel::connection::SimpleConnection;

    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::model_route::{
        ApiKeyModelOverride, CreateApiKeyModelOverridePayload, CreateModelRoutePayload, ModelRoute,
        ModelRouteCandidateInput, UpdateModelRoutePayload,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::{DbConnection, get_connection};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::create_test_app_state;
    use std::sync::Arc;

    use super::ModelRouteAdminService;

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
        Model::create(
            provider_id,
            model_name,
            None,
            true,
            ModelCapabilityFlags {
                supports_streaming: true,
                supports_tools: true,
                supports_reasoning: true,
                supports_image_input: true,
                supports_embeddings: true,
                supports_rerank: true,
            },
        )
        .expect("model seed should succeed")
    }

    fn create_route_payload(route_name: &str, model_id: i64) -> CreateModelRoutePayload {
        CreateModelRoutePayload {
            route_name: route_name.to_string(),
            description: Some("route".to_string()),
            is_enabled: Some(true),
            expose_in_models: Some(true),
            candidates: vec![ModelRouteCandidateInput {
                model_id,
                priority: 0,
                is_enabled: Some(true),
            }],
        }
    }

    fn seed_route(route_name: &str, model_id: i64) -> ModelRoute {
        ModelRoute::create(&create_route_payload(route_name, model_id))
            .expect("route seed should succeed")
            .route
    }

    fn create_api_key() -> crate::database::api_key::ApiKeyDetailWithSecret {
        ApiKey::create(&CreateApiKeyPayload {
            name: "route-delete".to_string(),
            description: Some("seed".to_string()),
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
        })
        .expect("api key seed should succeed")
    }

    fn install_sqlite_model_route_delete_override_failure_trigger(route_id: i64) {
        let mut connection = get_connection().expect("test connection should open");
        let DbConnection::Sqlite(conn) = &mut connection else {
            panic!("model route delete rollback test requires sqlite");
        };

        conn.batch_execute(&format!(
            "
            CREATE TRIGGER fail_model_route_delete_overrides_{route_id}
            BEFORE UPDATE ON api_key_model_override
            WHEN NEW.target_route_id = {route_id} AND NEW.deleted_at IS NOT NULL
            BEGIN
                SELECT RAISE(ABORT, 'forced model route delete failure');
            END;
            "
        ))
        .expect("model route delete failure trigger should install");
    }

    fn service(app_state: &Arc<crate::service::app_state::AppState>) -> &ModelRouteAdminService {
        app_state.admin.model_route.as_ref()
    }

    #[tokio::test]
    async fn create_model_route_refreshes_negative_name_cache_and_catalog() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-route-create.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(11101, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_before = app_state
                    .catalog
                    .get_model_route_by_name("shared-gpt-4o-mini")
                    .await
                    .expect("route cache should load");
                assert!(cached_before.is_none());

                let created = service(&app_state)
                    .create_model_route(create_route_payload("shared-gpt-4o-mini", model.id))
                    .await
                    .expect("route create should succeed");

                let cached_after = app_state
                    .catalog
                    .get_model_route_by_name("shared-gpt-4o-mini")
                    .await
                    .expect("route cache should reload")
                    .expect("route should exist");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");

                assert_eq!(cached_after.id, created.route.id);
                assert!(
                    catalog_after
                        .routes
                        .iter()
                        .any(|route| route.id == created.route.id)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn update_model_route_renames_and_invalidates_old_name_cache() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-route-update.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(11201, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let cached_before = app_state
                    .catalog
                    .get_model_route_by_name("shared-gpt-4o-mini")
                    .await
                    .expect("old route cache should load")
                    .expect("old route should exist");
                assert_eq!(cached_before.id, route.id);

                let updated = service(&app_state)
                    .update_model_route(
                        route.id,
                        UpdateModelRoutePayload {
                            route_name: Some("shared-gpt-4.1-mini".to_string()),
                            ..Default::default()
                        },
                    )
                    .await
                    .expect("route update should succeed");

                let old_name_after = app_state
                    .catalog
                    .get_model_route_by_name("shared-gpt-4o-mini")
                    .await
                    .expect("old route cache should reload");
                let new_name_after = app_state
                    .catalog
                    .get_model_route_by_name("shared-gpt-4.1-mini")
                    .await
                    .expect("new route cache should load")
                    .expect("new route should exist");

                assert_eq!(updated.route.route_name, "shared-gpt-4.1-mini");
                assert!(old_name_after.is_none());
                assert_eq!(new_name_after.id, route.id);
            })
            .await;
    }

    #[tokio::test]
    async fn delete_model_route_clears_preloaded_override_and_route_caches() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-route-delete.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(11301, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                let api_key = create_api_key();
                ApiKeyModelOverride::create(&CreateApiKeyModelOverridePayload {
                    api_key_id: api_key.detail.id,
                    source_name: "alias-a".to_string(),
                    target_route_id: route.id,
                    description: Some("override".to_string()),
                    is_enabled: Some(true),
                })
                .expect("override seed should succeed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let override_before = app_state
                    .catalog
                    .get_api_key_override_route(api_key.detail.id, "alias-a")
                    .await
                    .expect("override route cache should load")
                    .expect("override route should exist");
                let route_before = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should load")
                    .expect("route should exist");

                assert_eq!(override_before.id, route.id);
                assert_eq!(route_before.id, route.id);

                service(&app_state)
                    .delete_model_route(route.id)
                    .await
                    .expect("route delete should succeed");

                let override_after = app_state
                    .catalog
                    .get_api_key_override_route(api_key.detail.id, "alias-a")
                    .await
                    .expect("override route cache should reload");
                let route_after = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should reload");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");

                assert!(override_after.is_none());
                assert!(route_after.is_none());
                assert!(
                    ApiKeyModelOverride::list_by_api_key_id(api_key.detail.id)
                        .expect("override list should load")
                        .is_empty()
                );
                assert!(!catalog_after.routes.iter().any(|item| item.id == route.id));
                assert!(
                    !catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == api_key.detail.id
                            && item.source_name == "alias-a")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn delete_model_route_rolls_back_when_override_cleanup_fails() {
        let test_db_context = TestDbContext::new_sqlite("admin-model-route-delete-rollback.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(11351, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini-rollback", model.id);
                let api_key = create_api_key();
                ApiKeyModelOverride::create(&CreateApiKeyModelOverridePayload {
                    api_key_id: api_key.detail.id,
                    source_name: "alias-rollback".to_string(),
                    target_route_id: route.id,
                    description: Some("override".to_string()),
                    is_enabled: Some(true),
                })
                .expect("override seed should succeed");
                install_sqlite_model_route_delete_override_failure_trigger(route.id);

                let app_state = create_test_app_state(test_db_context.clone()).await;

                let err = service(&app_state)
                    .delete_model_route(route.id)
                    .await
                    .expect_err("route delete should fail");
                let message = format!("{err:?}");
                assert!(
                    message.contains("forced model route delete failure"),
                    "unexpected error: {message}"
                );

                let route_after = ModelRoute::get_by_id(route.id).expect("route should remain");
                let detail_after =
                    ModelRoute::get_detail(route.id).expect("route detail should remain");
                let overrides_after = ApiKeyModelOverride::list_by_api_key_id(api_key.detail.id)
                    .expect("overrides should load");

                assert!(route_after.deleted_at.is_none());
                assert!(route_after.is_enabled);
                assert_eq!(detail_after.candidates.len(), 1);
                assert!(detail_after.candidates[0].candidate.deleted_at.is_none());
                assert!(detail_after.candidates[0].candidate.is_enabled);
                assert_eq!(overrides_after.len(), 1);
                assert_eq!(overrides_after[0].target_route_id, route.id);
                assert_eq!(overrides_after[0].source_name, "alias-rollback");
                assert!(overrides_after[0].deleted_at.is_none());
                assert!(overrides_after[0].is_enabled);
            })
            .await;
    }
}
