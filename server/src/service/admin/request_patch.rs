use std::sync::Arc;

use crate::controller::BaseError;
use crate::database::request_patch::{
    RequestPatchVariantAggregate, RequestPatchVariantInput, RequestPatchVariantPreview,
    RequestPatchVariantRepository,
};
use crate::database::upstream_source::UpstreamSource;

use super::audit::{AdminAuditEvent, AdminAuditField};
use super::mutation::{AdminCatalogInvalidation, AdminMutationEffect, AdminMutationRunner};

#[derive(Clone, Copy)]
enum RequestPatchAdminOwner {
    Source { source_id: i64 },
    ModelSource { model_id: i64, source_id: i64 },
}

impl RequestPatchAdminOwner {
    fn invalidation(self) -> AdminCatalogInvalidation {
        match self {
            Self::Source { source_id } => {
                AdminCatalogInvalidation::RequestPatchSource { source_id }
            }
            Self::ModelSource { model_id, .. } => {
                AdminCatalogInvalidation::RequestPatchModel { model_id }
            }
        }
    }

    fn fields(self) -> [AdminAuditField; 2] {
        match self {
            Self::Source { source_id } => [
                AdminAuditField::new("source_id", source_id),
                AdminAuditField::new("model_id", "none"),
            ],
            Self::ModelSource {
                model_id,
                source_id,
            } => [
                AdminAuditField::new("source_id", source_id),
                AdminAuditField::new("model_id", model_id),
            ],
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

    pub fn list_source_variants(
        &self,
        source_id: i64,
    ) -> Result<Vec<RequestPatchVariantAggregate>, BaseError> {
        RequestPatchVariantRepository::list_by_source(source_id)
    }

    pub fn validate_source_route(&self, provider_id: i64, source_id: i64) -> Result<(), BaseError> {
        UpstreamSource::get_active_by_id_for_provider(source_id, provider_id).map(|_| ())
    }

    pub fn list_model_source_variants(
        &self,
        model_id: i64,
        source_id: i64,
    ) -> Result<Vec<RequestPatchVariantAggregate>, BaseError> {
        RequestPatchVariantRepository::list_by_model_source(model_id, source_id)
    }

    pub fn list_model_variants(
        &self,
        model_id: i64,
    ) -> Result<Vec<RequestPatchVariantAggregate>, BaseError> {
        RequestPatchVariantRepository::list_by_model_ids(&[model_id])
    }

    pub fn preview_source_variant(
        &self,
        source_id: i64,
        input: RequestPatchVariantInput,
        exclude_variant_id: Option<i64>,
    ) -> Result<RequestPatchVariantPreview, BaseError> {
        validate_owner_input(RequestPatchAdminOwner::Source { source_id }, &input)?;
        RequestPatchVariantRepository::preview(&input, exclude_variant_id)
    }

    pub fn preview_model_source_variant(
        &self,
        model_id: i64,
        source_id: i64,
        input: RequestPatchVariantInput,
        exclude_variant_id: Option<i64>,
    ) -> Result<RequestPatchVariantPreview, BaseError> {
        validate_owner_input(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            &input,
        )?;
        RequestPatchVariantRepository::preview(&input, exclude_variant_id)
    }

    pub async fn create_source_variant(
        &self,
        source_id: i64,
        input: RequestPatchVariantInput,
    ) -> Result<RequestPatchVariantAggregate, BaseError> {
        validate_owner_input(RequestPatchAdminOwner::Source { source_id }, &input)?;
        let aggregate = RequestPatchVariantRepository::create(&input)?;
        self.run_saved_effects(
            RequestPatchAdminOwner::Source { source_id },
            "create",
            &aggregate,
        )
        .await;
        Ok(aggregate)
    }

    pub async fn update_source_variant(
        &self,
        source_id: i64,
        variant_id: i64,
        input: RequestPatchVariantInput,
    ) -> Result<RequestPatchVariantAggregate, BaseError> {
        validate_owner_input(RequestPatchAdminOwner::Source { source_id }, &input)?;
        let aggregate = RequestPatchVariantRepository::replace(variant_id, &input)?;
        self.run_saved_effects(
            RequestPatchAdminOwner::Source { source_id },
            "update",
            &aggregate,
        )
        .await;
        Ok(aggregate)
    }

    pub async fn delete_source_variant(
        &self,
        source_id: i64,
        variant_id: i64,
    ) -> Result<RequestPatchVariantAggregate, BaseError> {
        let aggregate = RequestPatchVariantRepository::get(variant_id)?;
        validate_owner_input(
            RequestPatchAdminOwner::Source { source_id },
            &RequestPatchVariantInput {
                source_id: aggregate.variant.source_id,
                model_id: aggregate.variant.model_id,
                suffix: aggregate.variant.suffix.clone(),
                enabled: aggregate.variant.enabled,
                expose_in_models: aggregate.variant.expose_in_models,
                rules: Vec::new(),
            },
        )?;
        let deleted = RequestPatchVariantRepository::soft_delete(variant_id)?;
        self.run_saved_effects(
            RequestPatchAdminOwner::Source { source_id },
            "delete",
            &deleted,
        )
        .await;
        Ok(deleted)
    }

    pub async fn create_model_source_variant(
        &self,
        model_id: i64,
        source_id: i64,
        input: RequestPatchVariantInput,
    ) -> Result<RequestPatchVariantAggregate, BaseError> {
        validate_owner_input(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            &input,
        )?;
        let aggregate = RequestPatchVariantRepository::create(&input)?;
        self.run_saved_effects(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            "create",
            &aggregate,
        )
        .await;
        Ok(aggregate)
    }

    pub async fn update_model_source_variant(
        &self,
        model_id: i64,
        source_id: i64,
        variant_id: i64,
        input: RequestPatchVariantInput,
    ) -> Result<RequestPatchVariantAggregate, BaseError> {
        validate_owner_input(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            &input,
        )?;
        let aggregate = RequestPatchVariantRepository::replace(variant_id, &input)?;
        self.run_saved_effects(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            "update",
            &aggregate,
        )
        .await;
        Ok(aggregate)
    }

    pub async fn delete_model_source_variant(
        &self,
        model_id: i64,
        source_id: i64,
        variant_id: i64,
    ) -> Result<RequestPatchVariantAggregate, BaseError> {
        let aggregate = RequestPatchVariantRepository::get(variant_id)?;
        validate_owner_input(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            &RequestPatchVariantInput {
                source_id: aggregate.variant.source_id,
                model_id: aggregate.variant.model_id,
                suffix: aggregate.variant.suffix.clone(),
                enabled: aggregate.variant.enabled,
                expose_in_models: aggregate.variant.expose_in_models,
                rules: Vec::new(),
            },
        )?;
        let deleted = RequestPatchVariantRepository::soft_delete(variant_id)?;
        self.run_saved_effects(
            RequestPatchAdminOwner::ModelSource {
                model_id,
                source_id,
            },
            "delete",
            &deleted,
        )
        .await;
        Ok(deleted)
    }

    async fn run_saved_effects(
        &self,
        owner: RequestPatchAdminOwner,
        action: &'static str,
        aggregate: &RequestPatchVariantAggregate,
    ) {
        let mut fields = owner.fields().to_vec();
        fields.push(AdminAuditField::new("variant_id", aggregate.variant.id));
        fields.extend(AdminAuditField::optional(
            "suffix",
            aggregate.variant.suffix.as_deref(),
        ));
        fields.push(AdminAuditField::new("enabled", aggregate.variant.enabled));
        fields.push(AdminAuditField::new(
            "expose_in_models",
            aggregate.variant.expose_in_models,
        ));
        fields.push(AdminAuditField::new("rule_count", aggregate.rules.len()));
        self.run_post_commit_effects(vec![
            AdminMutationEffect::catalog_invalidation(owner.invalidation()),
            AdminMutationEffect::audit(AdminAuditEvent::with_fields(
                request_patch_event_name(owner, action),
                fields,
            )),
        ])
        .await;
    }

    async fn run_post_commit_effects(&self, effects: Vec<AdminMutationEffect>) {
        let _ = self.mutation_runner.execute(&effects).await;
    }
}

fn validate_owner_input(
    owner: RequestPatchAdminOwner,
    input: &RequestPatchVariantInput,
) -> Result<(), BaseError> {
    match owner {
        RequestPatchAdminOwner::Source { source_id }
            if input.source_id == source_id && input.model_id.is_none() =>
        {
            Ok(())
        }
        RequestPatchAdminOwner::ModelSource {
            model_id,
            source_id,
        } if input.source_id == source_id && input.model_id == Some(model_id) => Ok(()),
        _ => Err(BaseError::ParamInvalid(Some(
            "request patch Variant owner does not match route".to_string(),
        ))),
    }
}

fn request_patch_event_name(owner: RequestPatchAdminOwner, action: &'static str) -> &'static str {
    match (owner, action) {
        (RequestPatchAdminOwner::Source { .. }, "create") => {
            "manager.source_request_patch_variant_created"
        }
        (RequestPatchAdminOwner::Source { .. }, "update") => {
            "manager.source_request_patch_variant_updated"
        }
        (RequestPatchAdminOwner::Source { .. }, "delete") => {
            "manager.source_request_patch_variant_deleted"
        }
        (RequestPatchAdminOwner::ModelSource { .. }, "create") => {
            "manager.model_source_request_patch_variant_created"
        }
        (RequestPatchAdminOwner::ModelSource { .. }, "update") => {
            "manager.model_source_request_patch_variant_updated"
        }
        (RequestPatchAdminOwner::ModelSource { .. }, "delete") => {
            "manager.model_source_request_patch_variant_deleted"
        }
        _ => unreachable!("unsupported request patch Variant audit action"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::RequestPatchAdminService;
    use crate::database::TestDbContext;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantInput, RequestPatchVariantRepository,
    };
    use crate::database::upstream_source::NewUpstreamSource;
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use crate::service::admin::mutation::AdminMutationRunner;
    use crate::service::catalog::CatalogService;

    fn variant_input(source_id: i64, target: &str) -> RequestPatchVariantInput {
        RequestPatchVariantInput {
            source_id,
            model_id: None,
            suffix: Some("fast".to_string()),
            enabled: true,
            expose_in_models: true,
            rules: vec![RequestPatchRuleInput {
                placement: RequestPatchPlacement::Body,
                target: target.to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some(Some(serde_json::json!(0.2))),
                description: Some("operator temperature override".to_string()),
                confirm_dangerous_target: false,
            }],
        }
    }

    #[tokio::test]
    async fn aggregate_mutations_invalidate_catalog_and_audit_without_rule_values() {
        let database = TestDbContext::new_sqlite("admin-request-patch-aggregate.sqlite");
        database
            .run_async(async {
                Provider::create(
                    &NewProvider {
                        id: 9101,
                        provider_key: "request-patch-admin".to_string(),
                        name: "Request Patch Admin".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &NewUpstreamSource {
                        id: 9102,
                        provider_id: 9101,
                        profile_type: UpstreamProfileType::Openai,
                        endpoint: "https://request-patch-admin.example/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                    },
                )
                .expect("provider should be seeded");
                let catalog = Arc::new(CatalogService::new(true).await);
                let runner = Arc::new(AdminMutationRunner::new(Arc::clone(&catalog)));
                let service = RequestPatchAdminService::new(Arc::clone(&runner));

                let preview = service
                    .preview_source_variant(
                        9102,
                        RequestPatchVariantInput {
                            rules: vec![RequestPatchRuleInput {
                                placement: RequestPatchPlacement::Header,
                                target: "authorization".to_string(),
                                operation: RequestPatchOperation::Set,
                                value_json: Some(Some(serde_json::json!("Bearer preview"))),
                                description: None,
                                confirm_dangerous_target: false,
                            }],
                            ..variant_input(9102, "/options/temperature")
                        },
                        None,
                    )
                    .expect("preview should return a confirmation result");
                assert!(!preview.valid);
                assert_eq!(preview.dangerous_targets.len(), 1);
                assert!(
                    RequestPatchVariantRepository::list_by_source(9102)
                        .expect("preview should not write")
                        .is_empty()
                );
                assert!(runner.drain_audit_events().is_empty());

                let created = service
                    .create_source_variant(9102, variant_input(9102, "/options/temperature"))
                    .await
                    .expect("variant create should commit");
                let events = runner.drain_audit_events();
                let event = events
                    .iter()
                    .find(|event| {
                        event.event_name() == "manager.source_request_patch_variant_created"
                    })
                    .expect("create should emit an audit event");
                assert!(
                    event
                        .fields()
                        .iter()
                        .any(|field| { field.key() == "rule_count" && field.value() == "1" })
                );
                assert!(
                    event
                        .fields()
                        .iter()
                        .all(|field| { field.key() != "value_json" && field.key() != "target" })
                );
                let catalog_snapshot = catalog
                    .get_models_catalog()
                    .await
                    .expect("catalog should reload after invalidation");
                assert_eq!(catalog_snapshot.request_patch_variants.len(), 1);

                let updated = service
                    .update_source_variant(
                        9102,
                        created.variant.id,
                        variant_input(9102, "/options/top_p"),
                    )
                    .await
                    .expect("variant update should commit");
                assert_eq!(updated.rules[0].target, "/options/top_p");
                assert!(runner.drain_audit_events().iter().any(|event| {
                    event.event_name() == "manager.source_request_patch_variant_updated"
                }));

                service
                    .delete_source_variant(9102, created.variant.id)
                    .await
                    .expect("variant delete should commit");
                assert!(
                    service
                        .list_source_variants(9102)
                        .expect("active variants should load")
                        .is_empty()
                );
                assert!(runner.drain_audit_events().iter().any(|event| {
                    event.event_name() == "manager.source_request_patch_variant_deleted"
                }));
            })
            .await;
    }
}
