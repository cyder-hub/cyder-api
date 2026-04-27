use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::api_key::{
    ApiKey, ApiKeyDetail, ApiKeyDetailWithSecret, ApiKeyModelOverrideWriteInput,
    ApiKeyModelOverrideWriteSummary, ApiKeyReveal, CreateApiKeyPayload,
    UpdateApiKeyMetadataPayload, hash_api_key,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Debug, Clone)]
pub struct ApiKeyModelOverrideInput {
    pub source_name: String,
    pub target_route_id: i64,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
}

type ApiKeyOverrideReplaceSummary = ApiKeyModelOverrideWriteSummary;

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
        model_overrides: Vec<ApiKeyModelOverrideInput>,
    ) -> Result<ApiKeyDetailWithSecret, BaseError> {
        let result = ApiKey::create_with_model_overrides(
            &payload,
            &map_model_override_write_inputs(model_overrides),
        )?;
        let created = result.created;
        let override_summary = result.override_summary;

        let mut effects = vec![AdminMutationEffect::audit(api_key_audit_event(
            "create",
            created.detail.id,
            &created.detail.name,
            Some(created.detail.is_enabled),
        ))];
        effects.extend(api_key_override_effects(
            created.detail.id,
            &created.detail.name,
            &override_summary,
        ));
        self.run_post_commit_effects(effects).await;

        Ok(created)
    }

    pub async fn update_api_key(
        &self,
        id: i64,
        payload: UpdateApiKeyMetadataPayload,
        model_overrides: Vec<ApiKeyModelOverrideInput>,
    ) -> Result<ApiKeyDetail, BaseError> {
        let result = ApiKey::update_metadata_with_model_overrides(
            id,
            &payload,
            &map_model_override_write_inputs(model_overrides),
        )?;
        let updated = result.updated;
        let override_summary = result.override_summary;

        let mut effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyId { id }),
            AdminMutationEffect::audit(api_key_audit_event(
                "update",
                updated.id,
                &updated.name,
                Some(updated.is_enabled),
            )),
        ];
        effects.extend(api_key_override_effects(
            updated.id,
            &updated.name,
            &override_summary,
        ));
        self.run_post_commit_effects(effects).await;

        Ok(updated)
    }

    pub async fn replace_api_key_model_overrides(
        &self,
        api_key_id: i64,
        model_overrides: Vec<ApiKeyModelOverrideInput>,
    ) -> Result<(), BaseError> {
        let api_key = ApiKey::get_by_id(api_key_id)?;
        let override_summary = ApiKey::replace_model_overrides(
            api_key_id,
            &map_model_override_write_inputs(model_overrides),
        )?;

        self.run_post_commit_effects(api_key_override_effects(
            api_key_id,
            &api_key.name,
            &override_summary,
        ))
        .await;

        Ok(())
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
        let deleted = ApiKey::delete_with_model_overrides(id)?;
        let existing = deleted.deleted;
        let api_key_hash = deleted.old_api_key_hash;
        let override_summary = deleted.override_summary;

        let mut effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyHash {
                api_key_hash,
            }),
            AdminMutationEffect::audit(api_key_audit_event(
                "delete",
                existing.id,
                &existing.name,
                Some(existing.is_enabled),
            )),
        ];
        effects.extend(api_key_override_invalidation_effects(id, &override_summary));
        self.run_post_commit_effects(effects).await;

        Ok(())
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn map_model_override_write_inputs(
    payloads: Vec<ApiKeyModelOverrideInput>,
) -> Vec<ApiKeyModelOverrideWriteInput> {
    payloads
        .into_iter()
        .map(|payload| ApiKeyModelOverrideWriteInput {
            source_name: payload.source_name,
            target_route_id: payload.target_route_id,
            description: payload.description,
            is_enabled: payload.is_enabled,
        })
        .collect()
}

fn api_key_override_effects(
    api_key_id: i64,
    api_key_name: &str,
    summary: &ApiKeyOverrideReplaceSummary,
) -> Vec<AdminMutationEffect> {
    let mut effects = api_key_override_invalidation_effects(api_key_id, summary);
    effects.push(AdminMutationEffect::audit(
        api_key_override_replace_audit_event(api_key_id, api_key_name, summary),
    ));
    effects
}

fn api_key_override_invalidation_effects(
    api_key_id: i64,
    summary: &ApiKeyOverrideReplaceSummary,
) -> Vec<AdminMutationEffect> {
    let mut effects = vec![AdminMutationEffect::catalog_invalidation(
        AdminCatalogInvalidation::ModelsCatalog,
    )];
    let source_names = summary.invalidation_source_names();

    if !source_names.is_empty() {
        effects.push(AdminMutationEffect::catalog_invalidation(
            AdminCatalogInvalidation::ApiKeyModelOverrides {
                api_key_id,
                source_names,
            },
        ));
    }

    effects
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

fn api_key_override_replace_audit_event(
    api_key_id: i64,
    api_key_name: &str,
    summary: &ApiKeyOverrideReplaceSummary,
) -> AdminAuditEvent {
    AdminAuditEvent::with_fields(
        "manager.api_key_model_overrides_replaced",
        [
            AdminAuditField::new("action", "replace"),
            AdminAuditField::new("api_key_id", api_key_id),
            AdminAuditField::new("api_key_name", api_key_name),
            AdminAuditField::new("override_count", summary.override_count),
            AdminAuditField::new("enabled_override_count", summary.enabled_override_count),
        ],
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::controller::BaseError;
    use crate::database::TestDbContext;
    use crate::database::api_key::{
        ApiKey, CreateApiKeyPayload, UpdateApiKeyMetadataPayload, hash_api_key,
    };
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::model_route::ApiKeyModelOverride;
    use crate::database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteCandidateInput,
    };
    use crate::database::provider::{NewProvider, Provider};
    use crate::schema::enum_def::{Action, ProviderApiKeyMode, ProviderType};
    use crate::service::app_state::create_test_app_state;

    use super::{ApiKeyAdminService, ApiKeyModelOverrideInput};

    fn action_allow() -> Action {
        Action::Allow
    }

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

    fn override_input(source_name: &str, route_id: i64) -> ApiKeyModelOverrideInput {
        ApiKeyModelOverrideInput {
            source_name: source_name.to_string(),
            target_route_id: route_id,
            description: Some("override".to_string()),
            is_enabled: Some(true),
        }
    }

    fn create_payload(name: &str) -> CreateApiKeyPayload {
        CreateApiKeyPayload {
            name: name.to_string(),
            description: Some("seed".to_string()),
            default_action: Some(action_allow()),
            is_enabled: Some(true),
            expires_at: None,
            rate_limit_rpm: Some(10),
            max_concurrent_requests: Some(2),
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

    fn update_payload(name: &str) -> UpdateApiKeyMetadataPayload {
        UpdateApiKeyMetadataPayload {
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    fn service(app_state: &Arc<crate::service::app_state::AppState>) -> &ApiKeyAdminService {
        app_state.admin.api_key.as_ref()
    }

    #[tokio::test]
    async fn create_api_key_refreshes_models_catalog_and_persists_overrides() {
        let test_db_context = TestDbContext::new_sqlite("admin-api-key-create.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(8101, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let catalog_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                assert!(catalog_before.api_key_overrides.is_empty());

                let created = service(&app_state)
                    .create_api_key(
                        create_payload("created"),
                        vec![override_input("alias-a", route.id)],
                    )
                    .await
                    .expect("api key create should succeed");

                let created_hash = hash_api_key(&created.reveal.api_key);
                let cached = app_state
                    .catalog
                    .get_api_key_by_hash(&created_hash)
                    .await
                    .expect("api key cache should load")
                    .expect("api key should exist");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");
                let overrides = ApiKeyModelOverride::list_by_api_key_id(created.detail.id)
                    .expect("overrides should load");

                assert_eq!(cached.id, created.detail.id);
                assert_eq!(cached.name, "created");
                assert_eq!(overrides.len(), 1);
                assert_eq!(overrides[0].source_name, "alias-a");
                assert!(
                    catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id
                            && item.source_name == "alias-a")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn create_api_key_rolls_back_when_override_target_is_invalid() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-api-key-create-override-rollback.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let err = service(&app_state)
                    .create_api_key(
                        create_payload("create-rollback"),
                        vec![override_input("alias-invalid", -1)],
                    )
                    .await
                    .expect_err("invalid override should fail create");

                assert!(matches!(err, BaseError::NotFound(_)));
                let keys = ApiKey::list_summary().expect("api keys should load");
                assert!(!keys.iter().any(|api_key| api_key.name == "create-rollback"));
            })
            .await;
    }

    #[tokio::test]
    async fn update_api_key_replaces_overrides_and_invalidates_api_key_cache() {
        let test_db_context = TestDbContext::new_sqlite("admin-api-key-update.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(8201, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route_a = seed_route("route-a", model.id);
                let route_b = seed_route("route-b", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = service(&app_state)
                    .create_api_key(
                        create_payload("before"),
                        vec![override_input("alias-a", route_a.id)],
                    )
                    .await
                    .expect("api key create should succeed");
                let created_hash = hash_api_key(&created.reveal.api_key);
                let cached_before = app_state
                    .catalog
                    .get_api_key_by_hash(&created_hash)
                    .await
                    .expect("api key cache should load")
                    .expect("api key should exist");
                assert_eq!(cached_before.name, "before");

                let updated = service(&app_state)
                    .update_api_key(
                        created.detail.id,
                        update_payload("after"),
                        vec![override_input("alias-b", route_b.id)],
                    )
                    .await
                    .expect("api key update should succeed");

                let cached_after = app_state
                    .catalog
                    .get_api_key_by_hash(&created_hash)
                    .await
                    .expect("api key cache should reload")
                    .expect("api key should exist");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");
                let overrides =
                    ApiKeyModelOverride::list_by_api_key_id(created.detail.id).expect("overrides");

                assert_eq!(updated.name, "after");
                assert_eq!(cached_after.name, "after");
                assert_eq!(overrides.len(), 1);
                assert_eq!(overrides[0].source_name, "alias-b");
                assert_eq!(overrides[0].target_route_id, route_b.id);
                assert!(
                    catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id
                            && item.source_name == "alias-b")
                );
                assert!(
                    !catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id
                            && item.source_name == "alias-a")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn update_api_key_rolls_back_metadata_and_overrides_when_override_target_is_invalid() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-api-key-update-override-rollback.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(8251, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("route-rollback-a", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = service(&app_state)
                    .create_api_key(
                        create_payload("before-rollback"),
                        vec![override_input("alias-a", route.id)],
                    )
                    .await
                    .expect("api key create should succeed");

                let err = service(&app_state)
                    .update_api_key(
                        created.detail.id,
                        update_payload("after-rollback"),
                        vec![override_input("alias-invalid", -1)],
                    )
                    .await
                    .expect_err("invalid override should fail update");

                assert!(matches!(err, BaseError::NotFound(_)));
                let api_key = ApiKey::get_by_id(created.detail.id).expect("api key should exist");
                let overrides =
                    ApiKeyModelOverride::list_by_api_key_id(created.detail.id).expect("overrides");

                assert_eq!(api_key.name, "before-rollback");
                assert_eq!(overrides.len(), 1);
                assert_eq!(overrides[0].source_name, "alias-a");
                assert_eq!(overrides[0].target_route_id, route.id);
            })
            .await;
    }

    #[tokio::test]
    async fn rotate_api_key_invalidates_old_hash_and_exposes_new_hash() {
        let test_db_context = TestDbContext::new_sqlite("admin-api-key-rotate.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let created = service(&app_state)
                    .create_api_key(create_payload("rotating"), Vec::new())
                    .await
                    .expect("api key create should succeed");
                let old_hash = hash_api_key(&created.reveal.api_key);

                let cached_before = app_state
                    .catalog
                    .get_api_key_by_hash(&old_hash)
                    .await
                    .expect("old hash cache should load");
                assert!(cached_before.is_some());

                let rotated = service(&app_state)
                    .rotate_api_key(created.detail.id)
                    .await
                    .expect("api key rotate should succeed");

                let new_hash = hash_api_key(&rotated.api_key);
                let old_cached_after = app_state
                    .catalog
                    .get_api_key_by_hash(&old_hash)
                    .await
                    .expect("old hash cache should reload");
                let new_cached_after = app_state
                    .catalog
                    .get_api_key_by_hash(&new_hash)
                    .await
                    .expect("new hash cache should load")
                    .expect("new hash should resolve");

                assert!(old_cached_after.is_none());
                assert_eq!(new_cached_after.id, created.detail.id);
            })
            .await;
    }

    #[tokio::test]
    async fn delete_api_key_invalidates_old_hash_and_clears_models_catalog_overrides() {
        let test_db_context = TestDbContext::new_sqlite("admin-api-key-delete.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(8301, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("route-delete", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = service(&app_state)
                    .create_api_key(
                        create_payload("delete-me"),
                        vec![override_input("alias-delete", route.id)],
                    )
                    .await
                    .expect("api key create should succeed");
                let old_hash = hash_api_key(&created.reveal.api_key);

                let cached_before = app_state
                    .catalog
                    .get_api_key_by_hash(&old_hash)
                    .await
                    .expect("old hash cache should load");
                let catalog_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                assert!(cached_before.is_some());
                assert!(
                    catalog_before
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id)
                );

                service(&app_state)
                    .delete_api_key(created.detail.id)
                    .await
                    .expect("api key delete should succeed");

                let cached_after = app_state
                    .catalog
                    .get_api_key_by_hash(&old_hash)
                    .await
                    .expect("old hash cache should reload");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");
                let overrides =
                    ApiKeyModelOverride::list_by_api_key_id(created.detail.id).expect("overrides");

                assert!(cached_after.is_none());
                assert!(ApiKey::get_by_id(created.detail.id).is_err());
                assert!(overrides.is_empty());
                assert!(
                    !catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn replace_api_key_model_overrides_rolls_back_when_target_route_is_invalid() {
        let test_db_context =
            TestDbContext::new_sqlite("admin-api-key-override-replace-rollback.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(8451, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("route-override-rollback-a", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = service(&app_state)
                    .create_api_key(
                        create_payload("override-rollback"),
                        vec![override_input("alias-a", route.id)],
                    )
                    .await
                    .expect("api key create should succeed");

                let err = service(&app_state)
                    .replace_api_key_model_overrides(
                        created.detail.id,
                        vec![override_input("alias-invalid", -1)],
                    )
                    .await
                    .expect_err("invalid override should fail replace");

                assert!(matches!(err, BaseError::NotFound(_)));
                let overrides =
                    ApiKeyModelOverride::list_by_api_key_id(created.detail.id).expect("overrides");

                assert_eq!(overrides.len(), 1);
                assert_eq!(overrides[0].source_name, "alias-a");
                assert_eq!(overrides[0].target_route_id, route.id);
            })
            .await;
    }

    #[tokio::test]
    async fn replace_api_key_model_overrides_replaces_rows_and_refreshes_models_catalog() {
        let test_db_context = TestDbContext::new_sqlite("admin-api-key-override-replace.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(8401, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route_a = seed_route("route-override-a", model.id);
                let route_b = seed_route("route-override-b", model.id);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let created = service(&app_state)
                    .create_api_key(
                        create_payload("override-replace"),
                        vec![override_input("alias-a", route_a.id)],
                    )
                    .await
                    .expect("api key create should succeed");
                let catalog_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                assert!(
                    catalog_before
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id
                            && item.source_name == "alias-a")
                );

                service(&app_state)
                    .replace_api_key_model_overrides(
                        created.detail.id,
                        vec![override_input("alias-b", route_b.id)],
                    )
                    .await
                    .expect("override replace should succeed");

                let overrides =
                    ApiKeyModelOverride::list_by_api_key_id(created.detail.id).expect("overrides");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");

                assert_eq!(overrides.len(), 1);
                assert_eq!(overrides[0].source_name, "alias-b");
                assert_eq!(overrides[0].target_route_id, route_b.id);
                assert!(
                    catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id
                            && item.source_name == "alias-b")
                );
                assert!(
                    !catalog_after
                        .api_key_overrides
                        .iter()
                        .any(|item| item.api_key_id == created.detail.id
                            && item.source_name == "alias-a")
                );
            })
            .await;
    }
}
