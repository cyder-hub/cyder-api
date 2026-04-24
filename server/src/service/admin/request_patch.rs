use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::request_patch::{
    CreateRequestPatchPayload, RequestPatchMutationOutcome, RequestPatchRule,
    RequestPatchRuleResponse, UpdateRequestPatchPayload,
};

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Clone, Copy)]
enum RequestPatchAdminScope {
    Provider(i64),
    Model(i64),
}

impl RequestPatchAdminScope {
    fn scope_kind(self) -> &'static str {
        match self {
            Self::Provider(_) => "provider",
            Self::Model(_) => "model",
        }
    }

    fn scope_id(self) -> i64 {
        match self {
            Self::Provider(id) | Self::Model(id) => id,
        }
    }

    fn invalidation(self) -> AdminCatalogInvalidation {
        match self {
            Self::Provider(provider_id) => {
                AdminCatalogInvalidation::ProviderRequestPatchRules { provider_id }
            }
            Self::Model(model_id) => AdminCatalogInvalidation::ModelRequestPatchRules { model_id },
        }
    }
}

pub struct RequestPatchAdminService {
    mutation_runner: Arc<AdminMutationRunner>,
}

impl RequestPatchAdminService {
    pub(crate) fn new(mutation_runner: Arc<AdminMutationRunner>) -> Self {
        Self { mutation_runner }
    }

    #[cfg(test)]
    pub(crate) fn mutation_runner(&self) -> &Arc<AdminMutationRunner> {
        &self.mutation_runner
    }

    pub async fn create_provider_request_patch(
        &self,
        provider_id: i64,
        payload: CreateRequestPatchPayload,
    ) -> Result<RequestPatchMutationOutcome, BaseError> {
        let outcome = RequestPatchRule::create_for_provider(provider_id, &payload)?;
        self.run_saved_outcome_effects(
            RequestPatchAdminScope::Provider(provider_id),
            "create",
            &outcome,
        )
        .await;
        Ok(outcome)
    }

    pub async fn update_provider_request_patch(
        &self,
        provider_id: i64,
        rule_id: i64,
        payload: UpdateRequestPatchPayload,
    ) -> Result<RequestPatchMutationOutcome, BaseError> {
        let outcome = RequestPatchRule::update_for_provider(provider_id, rule_id, &payload)?;
        self.run_saved_outcome_effects(
            RequestPatchAdminScope::Provider(provider_id),
            "update",
            &outcome,
        )
        .await;
        Ok(outcome)
    }

    pub async fn delete_provider_request_patch(
        &self,
        provider_id: i64,
        rule_id: i64,
    ) -> Result<(), BaseError> {
        let rule = RequestPatchRule::get_provider_rule(provider_id, rule_id)?;
        RequestPatchRule::delete_for_provider(provider_id, rule_id)?;
        self.run_delete_effects(RequestPatchAdminScope::Provider(provider_id), &rule)
            .await;
        Ok(())
    }

    pub async fn create_model_request_patch(
        &self,
        model_id: i64,
        payload: CreateRequestPatchPayload,
    ) -> Result<RequestPatchMutationOutcome, BaseError> {
        let outcome = RequestPatchRule::create_for_model(model_id, &payload)?;
        self.run_saved_outcome_effects(RequestPatchAdminScope::Model(model_id), "create", &outcome)
            .await;
        Ok(outcome)
    }

    pub async fn update_model_request_patch(
        &self,
        model_id: i64,
        rule_id: i64,
        payload: UpdateRequestPatchPayload,
    ) -> Result<RequestPatchMutationOutcome, BaseError> {
        let outcome = RequestPatchRule::update_for_model(model_id, rule_id, &payload)?;
        self.run_saved_outcome_effects(RequestPatchAdminScope::Model(model_id), "update", &outcome)
            .await;
        Ok(outcome)
    }

    pub async fn delete_model_request_patch(
        &self,
        model_id: i64,
        rule_id: i64,
    ) -> Result<(), BaseError> {
        let rule = RequestPatchRule::get_model_rule(model_id, rule_id)?;
        RequestPatchRule::delete_for_model(model_id, rule_id)?;
        self.run_delete_effects(RequestPatchAdminScope::Model(model_id), &rule)
            .await;
        Ok(())
    }

    async fn run_saved_outcome_effects(
        &self,
        scope: RequestPatchAdminScope,
        action: &'static str,
        outcome: &RequestPatchMutationOutcome,
    ) {
        if let RequestPatchMutationOutcome::Saved { rule } = outcome {
            self.run_post_commit_effects(vec![
                AdminMutationEffect::catalog_invalidation(scope.invalidation()),
                AdminMutationEffect::audit(request_patch_audit_event(
                    action,
                    scope,
                    rule,
                    rule.is_enabled,
                )),
            ])
            .await;
        }
    }

    async fn run_delete_effects(
        &self,
        scope: RequestPatchAdminScope,
        rule: &RequestPatchRuleResponse,
    ) {
        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(scope.invalidation()),
            AdminMutationEffect::audit(request_patch_audit_event("delete", scope, rule, false)),
        ])
        .await;
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn request_patch_audit_event(
    action: &'static str,
    scope: RequestPatchAdminScope,
    rule: &RequestPatchRuleResponse,
    is_enabled: bool,
) -> AdminAuditEvent {
    let event_name = match (scope.scope_kind(), action) {
        ("provider", "create") => "manager.provider_request_patch_created",
        ("provider", "update") => "manager.provider_request_patch_updated",
        ("provider", "delete") => "manager.provider_request_patch_deleted",
        ("model", "create") => "manager.model_request_patch_created",
        ("model", "update") => "manager.model_request_patch_updated",
        ("model", "delete") => "manager.model_request_patch_deleted",
        _ => unreachable!(
            "unsupported request patch audit action: {}:{}",
            scope.scope_kind(),
            action
        ),
    };

    AdminAuditEvent::with_fields(
        event_name,
        [
            AdminAuditField::new("action", action),
            AdminAuditField::new("scope_kind", scope.scope_kind()),
            AdminAuditField::new("scope_id", scope.scope_id()),
            AdminAuditField::new("request_patch_rule_id", rule.id),
            AdminAuditField::new("placement", format!("{:?}", rule.placement)),
            AdminAuditField::new("operation", format!("{:?}", rule.operation)),
            AdminAuditField::new("is_enabled", is_enabled),
        ],
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::database::TestDbContext;
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::RequestPatchRule;
    use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
    use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};
    use crate::service::app_state::create_test_app_state;
    use serde_json::json;

    use super::RequestPatchAdminService;
    use super::{
        CreateRequestPatchPayload, RequestPatchMutationOutcome, UpdateRequestPatchPayload,
    };

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

    fn create_payload(target: &str, value: serde_json::Value) -> CreateRequestPatchPayload {
        CreateRequestPatchPayload {
            placement: RequestPatchPlacement::Body,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(Some(value)),
            description: Some("patch".to_string()),
            is_enabled: Some(true),
            confirm_dangerous_target: None,
        }
    }

    fn update_payload(target: &str, value: serde_json::Value) -> UpdateRequestPatchPayload {
        UpdateRequestPatchPayload {
            target: Some(target.to_string()),
            value_json: Some(Some(value)),
            ..Default::default()
        }
    }

    fn service(app_state: &Arc<crate::service::app_state::AppState>) -> &RequestPatchAdminService {
        app_state.admin.request_patch.as_ref()
    }

    #[tokio::test]
    async fn provider_scope_request_patch_lifecycle_refreshes_cached_rules_and_effective_view() {
        let test_db_context = TestDbContext::new_sqlite("admin-request-patch-provider.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(9101, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let provider_rules_before = app_state
                    .catalog
                    .get_provider_request_patch_rules(provider.id)
                    .await
                    .expect("provider patch cache should load");
                let effective_before = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should load")
                    .expect("effective cache should exist");
                assert!(provider_rules_before.is_empty());
                assert!(effective_before.effective_rules.is_empty());

                let created = service(&app_state)
                    .create_provider_request_patch(
                        provider.id,
                        create_payload("/temperature", json!(0.2)),
                    )
                    .await
                    .expect("provider request patch create should succeed");
                let created_rule = match created {
                    RequestPatchMutationOutcome::Saved { rule } => rule,
                    other => panic!("unexpected create outcome: {other:?}"),
                };

                let provider_rules_after_create = app_state
                    .catalog
                    .get_provider_request_patch_rules(provider.id)
                    .await
                    .expect("provider patch cache should reload");
                let effective_after_create = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should reload")
                    .expect("effective cache should exist");

                assert_eq!(provider_rules_after_create.len(), 1);
                assert_eq!(provider_rules_after_create[0].target, "/temperature");
                assert_eq!(effective_after_create.effective_rules.len(), 1);
                assert_eq!(
                    effective_after_create.effective_rules[0].target,
                    "/temperature"
                );

                let updated = service(&app_state)
                    .update_provider_request_patch(
                        provider.id,
                        created_rule.id,
                        update_payload("/top_p", json!(0.9)),
                    )
                    .await
                    .expect("provider request patch update should succeed");
                let updated_rule = match updated {
                    RequestPatchMutationOutcome::Saved { rule } => rule,
                    other => panic!("unexpected update outcome: {other:?}"),
                };

                let provider_rules_after_update = app_state
                    .catalog
                    .get_provider_request_patch_rules(provider.id)
                    .await
                    .expect("provider patch cache should reload after update");
                let effective_after_update = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should reload after update")
                    .expect("effective cache should exist");

                assert_eq!(updated_rule.target, "/top_p");
                assert_eq!(provider_rules_after_update[0].target, "/top_p");
                assert_eq!(effective_after_update.effective_rules[0].target, "/top_p");

                service(&app_state)
                    .delete_provider_request_patch(provider.id, created_rule.id)
                    .await
                    .expect("provider request patch delete should succeed");

                let provider_rules_after_delete = app_state
                    .catalog
                    .get_provider_request_patch_rules(provider.id)
                    .await
                    .expect("provider patch cache should reload after delete");
                let effective_after_delete = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should reload after delete")
                    .expect("effective cache should exist");

                assert!(provider_rules_after_delete.is_empty());
                assert!(effective_after_delete.effective_rules.is_empty());
                assert!(
                    RequestPatchRule::list_by_provider_id(provider.id)
                        .expect("provider rules should load")
                        .is_empty()
                );
            })
            .await;
    }

    #[tokio::test]
    async fn model_scope_request_patch_lifecycle_refreshes_cached_rules_and_effective_view() {
        let test_db_context = TestDbContext::new_sqlite("admin-request-patch-model.sqlite");

        test_db_context
            .run_async(async {
                let provider = seed_provider(9201, "openai");
                let model = seed_model_for_provider(provider.id, "gpt-4o-mini");
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let model_rules_before = app_state
                    .catalog
                    .get_model_request_patch_rules(model.id)
                    .await
                    .expect("model patch cache should load");
                let effective_before = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should load")
                    .expect("effective cache should exist");
                assert!(model_rules_before.is_empty());
                assert!(effective_before.effective_rules.is_empty());

                let created = service(&app_state)
                    .create_model_request_patch(
                        model.id,
                        create_payload("/temperature", json!(0.4)),
                    )
                    .await
                    .expect("model request patch create should succeed");
                let created_rule = match created {
                    RequestPatchMutationOutcome::Saved { rule } => rule,
                    other => panic!("unexpected create outcome: {other:?}"),
                };

                let model_rules_after_create = app_state
                    .catalog
                    .get_model_request_patch_rules(model.id)
                    .await
                    .expect("model patch cache should reload");
                let effective_after_create = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should reload")
                    .expect("effective cache should exist");

                assert_eq!(model_rules_after_create.len(), 1);
                assert_eq!(model_rules_after_create[0].target, "/temperature");
                assert_eq!(effective_after_create.effective_rules.len(), 1);
                assert_eq!(
                    effective_after_create.effective_rules[0].target,
                    "/temperature"
                );

                let updated = service(&app_state)
                    .update_model_request_patch(
                        model.id,
                        created_rule.id,
                        update_payload("/top_p", json!(0.7)),
                    )
                    .await
                    .expect("model request patch update should succeed");
                let updated_rule = match updated {
                    RequestPatchMutationOutcome::Saved { rule } => rule,
                    other => panic!("unexpected update outcome: {other:?}"),
                };

                let model_rules_after_update = app_state
                    .catalog
                    .get_model_request_patch_rules(model.id)
                    .await
                    .expect("model patch cache should reload after update");
                let effective_after_update = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should reload after update")
                    .expect("effective cache should exist");

                assert_eq!(updated_rule.target, "/top_p");
                assert_eq!(model_rules_after_update[0].target, "/top_p");
                assert_eq!(effective_after_update.effective_rules[0].target, "/top_p");

                service(&app_state)
                    .delete_model_request_patch(model.id, created_rule.id)
                    .await
                    .expect("model request patch delete should succeed");

                let model_rules_after_delete = app_state
                    .catalog
                    .get_model_request_patch_rules(model.id)
                    .await
                    .expect("model patch cache should reload after delete");
                let effective_after_delete = app_state
                    .catalog
                    .get_model_effective_request_patches(model.id)
                    .await
                    .expect("effective cache should reload after delete")
                    .expect("effective cache should exist");

                assert!(model_rules_after_delete.is_empty());
                assert!(effective_after_delete.effective_rules.is_empty());
                assert!(
                    RequestPatchRule::list_by_model_id(model.id)
                        .expect("model rules should load")
                        .is_empty()
                );
            })
            .await;
    }
}
