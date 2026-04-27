use std::sync::Arc;

use chrono::Utc;

use crate::controller::BaseError;
use crate::database::model_route::ModelRoute;
use crate::database::provider::{
    BootstrapProviderInput, BootstrapProviderResult, NewProvider, NewProviderApiKey, Provider,
    ProviderApiKey, UpdateProviderApiKeyData, UpdateProviderData,
};
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
use crate::utils::ID_GENERATOR;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{
    AdminCatalogInvalidation, AdminModelRouteCacheTarget, AdminMutationEffect, AdminMutationRunner,
};

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
        let affected_routes = ModelRoute::list_by_provider_id(id)?;
        let num_deleted_db = Provider::delete_with_dependents(id)?;

        if num_deleted_db == 0 {
            return Ok(());
        }

        let mut effects = vec![
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                id,
                key: Some(provider_to_delete.provider_key.clone()),
            }),
            AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ProviderApiKeys {
                provider_id: id,
            }),
            AdminMutationEffect::audit(provider_audit_event("delete", &provider_to_delete)),
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

#[cfg(test)]
mod tests {
    use diesel::connection::SimpleConnection;

    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteCandidateInput,
    };
    use crate::database::provider::{
        BootstrapProviderInput, NewProvider, NewProviderApiKey, Provider, ProviderApiKey,
        ProviderSummaryItem,
    };
    use crate::database::request_patch::{CreateRequestPatchPayload, RequestPatchRule};
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::schema::enum_def::{
        ProviderApiKeyMode, ProviderType, RequestPatchOperation, RequestPatchPlacement,
    };
    use crate::service::app_state::create_test_app_state;
    use serde_json::json;

    use super::{CreateProviderApiKeyInput, ProviderUpsertInput, UpdateProviderApiKeyInput};

    fn provider_input(name: &str, key: &str, endpoint: &str) -> ProviderUpsertInput {
        ProviderUpsertInput {
            name: name.to_string(),
            key: key.to_string(),
            endpoint: endpoint.to_string(),
            use_proxy: false,
            provider_type: Some(ProviderType::Openai),
            provider_api_key_mode: Some(ProviderApiKeyMode::Queue),
        }
    }

    fn seed_provider(id: i64, name: &str, key: &str, endpoint: &str) -> Provider {
        Provider::create(&NewProvider {
            id,
            provider_key: key.to_string(),
            name: name.to_string(),
            endpoint: endpoint.to_string(),
            use_proxy: false,
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
        })
        .expect("provider seed should succeed")
    }

    fn seed_provider_api_key(id: i64, provider_id: i64, api_key: &str) -> ProviderApiKey {
        ProviderApiKey::insert(&NewProviderApiKey {
            id,
            provider_id,
            api_key: api_key.to_string(),
            description: Some("seed".to_string()),
            is_enabled: true,
            created_at: 1,
            updated_at: 1,
        })
        .expect("provider api key seed should succeed")
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

    fn create_provider_request_patch_payload() -> CreateRequestPatchPayload {
        CreateRequestPatchPayload {
            placement: RequestPatchPlacement::Body,
            target: "/temperature".to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(Some(json!(0.2))),
            description: Some("provider patch".to_string()),
            is_enabled: Some(true),
            confirm_dangerous_target: None,
        }
    }

    fn install_sqlite_provider_delete_failure_trigger(provider_id: i64) {
        let mut connection = get_connection().expect("test connection should open");
        let DbConnection::Sqlite(conn) = &mut connection else {
            panic!("provider delete rollback test requires sqlite");
        };

        conn.batch_execute(&format!(
            "
            CREATE TRIGGER fail_provider_delete_keys_{provider_id}
            BEFORE UPDATE ON provider_api_key
            WHEN NEW.provider_id = {provider_id} AND NEW.deleted_at IS NOT NULL
            BEGIN
                SELECT RAISE(ABORT, 'forced provider delete failure');
            END;
            "
        ))
        .expect("provider delete failure trigger should install");
    }

    #[tokio::test]
    async fn create_provider_refreshes_models_catalog() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-create.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let catalog_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                assert!(catalog_before.providers.is_empty());

                let created = app_state
                    .admin
                    .provider
                    .create_provider(provider_input(
                        "OpenAI api.example.com",
                        "openai-api-example-com",
                        "https://api.example.com/v1",
                    ))
                    .await
                    .expect("provider create should succeed");

                let provider = app_state
                    .catalog
                    .get_provider_by_id(created.id)
                    .await
                    .expect("provider cache should load")
                    .expect("provider should exist");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");

                assert_eq!(provider.endpoint, "https://api.example.com/v1");
                assert!(
                    catalog_after
                        .providers
                        .iter()
                        .any(|item| item.id == created.id)
                );
                assert!(
                    Provider::list_summary()
                        .expect("provider summary should load")
                        .iter()
                        .any(|item: &ProviderSummaryItem| item.id == created.id)
                );
            })
            .await;
    }

    #[tokio::test]
    async fn update_provider_preserves_stable_key_and_refreshes_cached_provider_and_catalog() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-update.sqlite");

        test_db_context
            .run_async(async {
                let seeded_provider = seed_provider(
                    7001,
                    "OpenAI api.example.com",
                    "openai-api-example-com",
                    "https://api.example.com/v1",
                );
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let old_provider_key = seeded_provider.provider_key.clone();
                let payload_provider_key = "openai-production".to_string();
                let provider_before = app_state
                    .catalog
                    .get_provider_by_key(&old_provider_key)
                    .await
                    .expect("provider cache should load")
                    .expect("provider should exist");
                let catalog_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                assert_eq!(provider_before.endpoint, "https://api.example.com/v1");
                assert!(catalog_before.providers.iter().any(|item| {
                    item.id == seeded_provider.id && item.endpoint == "https://api.example.com/v1"
                }));

                let updated = app_state
                    .admin
                    .provider
                    .update_provider(
                        seeded_provider.id,
                        provider_input(
                            "OpenAI Production",
                            &payload_provider_key,
                            "https://api-updated.example.com/v1",
                        ),
                    )
                    .await
                    .expect("provider update should succeed");

                let provider_after = app_state
                    .catalog
                    .get_provider_by_id(seeded_provider.id)
                    .await
                    .expect("provider cache should reload")
                    .expect("provider should exist");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");
                let stable_key_after = app_state
                    .catalog
                    .get_provider_by_key(&old_provider_key)
                    .await
                    .expect("stable provider key cache should reload")
                    .expect("stable provider key should resolve");
                let payload_key_after = app_state
                    .catalog
                    .get_provider_by_key(&payload_provider_key)
                    .await
                    .expect("payload provider key cache should reload");

                assert_eq!(updated.endpoint, "https://api-updated.example.com/v1");
                assert_eq!(updated.provider_key, old_provider_key);
                assert_eq!(
                    provider_after.endpoint,
                    "https://api-updated.example.com/v1"
                );
                assert_eq!(provider_after.provider_key, old_provider_key);
                assert_eq!(stable_key_after.id, seeded_provider.id);
                assert!(payload_key_after.is_none());
                assert!(catalog_after.providers.iter().any(|item| {
                    item.id == seeded_provider.id
                        && item.endpoint == "https://api-updated.example.com/v1"
                        && item.provider_key == old_provider_key
                }));
            })
            .await;
    }

    #[tokio::test]
    async fn create_provider_api_key_refreshes_cached_key_list() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-key-create.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(
                    7101,
                    "OpenAI api.example.com",
                    "openai-api-example-com",
                    "https://api.example.com/v1",
                );
                seed_provider_api_key(7102, provider.id, "sk-seed");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let before = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider keys should load");
                assert_eq!(before.len(), 1);

                let created = app_state
                    .admin
                    .provider
                    .create_provider_api_key(
                        provider.id,
                        CreateProviderApiKeyInput {
                            api_key: "sk-created".to_string(),
                            description: Some("created".to_string()),
                            is_enabled: Some(true),
                        },
                    )
                    .await
                    .expect("provider api key create should succeed");

                let after = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider keys should reload");

                assert_eq!(created.provider_id, provider.id);
                assert_eq!(after.len(), 2);
                assert!(after.iter().any(|item| item.id == created.id));
                assert!(after.iter().any(|item| item.api_key == "sk-created"));
            })
            .await;
    }

    #[tokio::test]
    async fn update_provider_api_key_refreshes_cached_key_list() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-key-update.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(
                    7201,
                    "OpenAI api.example.com",
                    "openai-api-example-com",
                    "https://api.example.com/v1",
                );
                let seeded_key = seed_provider_api_key(7202, provider.id, "sk-before");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let before = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider keys should load");
                assert!(before.iter().any(|item| item.api_key == "sk-before"));

                let updated = app_state
                    .admin
                    .provider
                    .update_provider_api_key(
                        provider.id,
                        seeded_key.id,
                        UpdateProviderApiKeyInput {
                            api_key: Some("sk-after".to_string()),
                            description: Some("updated".to_string()),
                            is_enabled: Some(false),
                        },
                    )
                    .await
                    .expect("provider api key update should succeed");

                let after = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider keys should reload");

                assert_eq!(updated.api_key, "sk-after");
                assert!(after.iter().any(|item| item.id == seeded_key.id));
                assert!(after.iter().any(|item| item.api_key == "sk-after"));
                assert!(!after.iter().any(|item| item.api_key == "sk-before"));
            })
            .await;
    }

    #[tokio::test]
    async fn delete_provider_api_key_refreshes_cached_key_list() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-key-delete.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(
                    7301,
                    "OpenAI api.example.com",
                    "openai-api-example-com",
                    "https://api.example.com/v1",
                );
                let seeded_key = seed_provider_api_key(7302, provider.id, "sk-delete");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let before = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider keys should load");
                assert_eq!(before.len(), 1);

                app_state
                    .admin
                    .provider
                    .delete_provider_api_key(provider.id, seeded_key.id)
                    .await
                    .expect("provider api key delete should succeed");

                let after = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider keys should reload");

                assert!(after.is_empty());
                assert!(
                    ProviderApiKey::get_by_id(seeded_key.id).is_err(),
                    "deleted provider api key should no longer be readable"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn delete_provider_prefetches_route_caches_before_candidate_cleanup() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-delete.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(
                    7401,
                    "OpenAI api.example.com",
                    "openai-api-example-com",
                    "https://api.example.com/v1",
                );
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                seed_provider_api_key(7402, provider.id, "sk-delete-provider");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let route_before = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should load")
                    .expect("route should exist");
                let provider_before = app_state
                    .catalog
                    .get_provider_by_id(provider.id)
                    .await
                    .expect("provider cache should load")
                    .expect("provider should exist");
                let keys_before = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider key cache should load");
                assert_eq!(route_before.candidates.len(), 1);
                assert_eq!(provider_before.provider_key, provider.provider_key);
                assert_eq!(keys_before.len(), 1);

                app_state
                    .admin
                    .provider
                    .delete_provider(provider.id)
                    .await
                    .expect("provider delete should succeed");

                let route_after = app_state
                    .catalog
                    .get_model_route_by_id(route.id)
                    .await
                    .expect("route cache should reload")
                    .expect("route should still exist");
                let provider_after = app_state
                    .catalog
                    .get_provider_by_id(provider.id)
                    .await
                    .expect("provider cache should reload");
                let keys_after = app_state
                    .catalog
                    .get_provider_api_keys(provider.id)
                    .await
                    .expect("provider key cache should reload");

                assert!(Provider::get_by_id(provider.id).is_err());
                assert!(provider_after.is_none());
                assert!(keys_after.is_empty());
                assert!(route_after.candidates.is_empty());
            })
            .await;
    }

    #[tokio::test]
    async fn delete_provider_rolls_back_primary_delete_when_dependent_cleanup_fails() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-delete-rollback.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(
                    7411,
                    "OpenAI api.example.com",
                    "openai-api-example-com",
                    "https://api.example.com/v1",
                );
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let route = seed_route("shared-gpt-4o-mini", model.id);
                let key = seed_provider_api_key(7412, provider.id, "sk-delete-provider");
                RequestPatchRule::create_for_provider(
                    provider.id,
                    &create_provider_request_patch_payload(),
                )
                .expect("provider request patch seed should succeed");
                install_sqlite_provider_delete_failure_trigger(provider.id);

                let app_state = create_test_app_state(test_db_context.clone()).await;

                let err = app_state
                    .admin
                    .provider
                    .delete_provider(provider.id)
                    .await
                    .expect_err("provider delete should fail");
                let message = format!("{err:?}");
                assert!(
                    message.contains("forced provider delete failure"),
                    "unexpected error: {message}"
                );

                let provider_after = Provider::get_by_id(provider.id)
                    .expect("provider should still exist after rollback");
                let key_after =
                    ProviderApiKey::get_by_id(key.id).expect("provider key should still exist");
                let route_after =
                    ModelRoute::get_detail(route.id).expect("route should still load");
                let patches_after = RequestPatchRule::list_by_provider_id(provider.id)
                    .expect("provider request patches should still load");

                assert!(provider_after.deleted_at.is_none());
                assert!(provider_after.is_enabled);
                assert!(key_after.deleted_at.is_none());
                assert!(key_after.is_enabled);
                assert_eq!(route_after.candidates.len(), 1);
                assert!(route_after.candidates[0].candidate.deleted_at.is_none());
                assert!(route_after.candidates[0].candidate.is_enabled);
                assert_eq!(patches_after.len(), 1);
                assert!(patches_after[0].is_enabled);
            })
            .await;
    }

    #[tokio::test]
    async fn bootstrap_provider_persist_refreshes_catalog_without_runtime_check() {
        let test_db_context = TestDbContext::new_sqlite("admin-provider-bootstrap.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let catalog_before = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should load");
                assert!(catalog_before.providers.is_empty());

                let created = app_state
                    .admin
                    .provider
                    .bootstrap_provider_persist(BootstrapProviderInput {
                        provider_id: 7501,
                        provider_key: "openai-api-example-com".to_string(),
                        name: "OpenAI api.example.com".to_string(),
                        endpoint: "https://api.example.com/v1".to_string(),
                        use_proxy: false,
                        provider_type: ProviderType::Openai,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                        api_key: "sk-bootstrap".to_string(),
                        api_key_description: Some("bootstrap key".to_string()),
                        model_name: "gpt-4o-mini".to_string(),
                        real_model_name: Some("gpt-4o".to_string()),
                    })
                    .await
                    .expect("bootstrap persistence should succeed");

                let provider_after = app_state
                    .catalog
                    .get_provider_by_id(created.provider.id)
                    .await
                    .expect("provider cache should load")
                    .expect("provider should exist");
                let keys_after = app_state
                    .catalog
                    .get_provider_api_keys(created.provider.id)
                    .await
                    .expect("provider key cache should load");
                let catalog_after = app_state
                    .catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload");

                assert_eq!(created.created_key.provider_id, created.provider.id);
                assert_eq!(created.created_model.provider_id, created.provider.id);
                assert_eq!(provider_after.endpoint, "https://api.example.com/v1");
                assert_eq!(keys_after.len(), 1);
                assert_eq!(keys_after[0].id, created.created_key.id);
                assert!(
                    catalog_after
                        .providers
                        .iter()
                        .any(|item| item.id == created.provider.id)
                );
                assert!(
                    catalog_after
                        .models
                        .iter()
                        .any(|item| item.id == created.created_model.id)
                );
            })
            .await;
    }
}
