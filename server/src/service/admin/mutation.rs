use std::sync::Arc;

use crate::logging::event_message_with_fields;
use crate::service::app_state::AppStoreError;
use crate::service::catalog::CatalogService;
use cyder_tools::log::warn;

use super::audit::{AdminAuditEvent, AdminAuditLogger};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminModelCacheName {
    pub provider_key: String,
    pub model_name: String,
}

impl AdminModelCacheName {
    pub fn new(provider_key: impl Into<String>, model_name: impl Into<String>) -> Self {
        Self {
            provider_key: provider_key.into(),
            model_name: model_name.into(),
        }
    }

    fn as_catalog_name(&self) -> String {
        format!("{}/{}", self.provider_key, self.model_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminModelRouteCacheTarget {
    pub id: i64,
    pub name: Option<String>,
}

impl AdminModelRouteCacheTarget {
    pub fn new(id: i64, name: Option<impl Into<String>>) -> Self {
        Self {
            id,
            name: name.map(Into::into),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminCatalogInvalidation {
    ModelsCatalog,
    Provider {
        id: i64,
        key: Option<String>,
    },
    ProviderApiKeys {
        provider_id: i64,
    },
    ProviderRequestPatchRules {
        provider_id: i64,
    },
    Model {
        id: i64,
        name: Option<AdminModelCacheName>,
        previous_name: Option<AdminModelCacheName>,
    },
    ModelRoute {
        id: i64,
        name: Option<String>,
        previous_name: Option<String>,
    },
    ModelRoutes(Vec<AdminModelRouteCacheTarget>),
    ApiKeyId {
        id: i64,
    },
    ApiKeyHash {
        api_key_hash: String,
    },
    ApiKeyModelOverrides {
        api_key_id: i64,
        source_names: Vec<String>,
    },
    ModelRequestPatchRules {
        model_id: i64,
    },
    CostCatalogVersions {
        ids: Vec<i64>,
    },
}

impl AdminCatalogInvalidation {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ModelsCatalog => "models_catalog",
            Self::Provider { .. } => "provider",
            Self::ProviderApiKeys { .. } => "provider_api_keys",
            Self::ProviderRequestPatchRules { .. } => "provider_request_patch_rules",
            Self::Model { .. } => "model",
            Self::ModelRoute { .. } => "model_route",
            Self::ModelRoutes(_) => "model_routes",
            Self::ApiKeyId { .. } => "api_key_id",
            Self::ApiKeyHash { .. } => "api_key_hash",
            Self::ApiKeyModelOverrides { .. } => "api_key_model_overrides",
            Self::ModelRequestPatchRules { .. } => "model_request_patch_rules",
            Self::CostCatalogVersions { .. } => "cost_catalog_versions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminMutationEffect {
    CatalogInvalidation(AdminCatalogInvalidation),
    Audit(AdminAuditEvent),
}

impl AdminMutationEffect {
    pub fn catalog_invalidation(invalidation: AdminCatalogInvalidation) -> Self {
        Self::CatalogInvalidation(invalidation)
    }

    pub fn audit(event: AdminAuditEvent) -> Self {
        Self::Audit(event)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminCatalogInvalidationFailure {
    pub invalidation: AdminCatalogInvalidation,
    pub error_message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AdminMutationReport {
    pub catalog_invalidation_failures: Vec<AdminCatalogInvalidationFailure>,
}

impl AdminMutationReport {
    pub fn has_catalog_failures(&self) -> bool {
        !self.catalog_invalidation_failures.is_empty()
    }
}

pub(crate) struct AdminMutationRunner {
    catalog: Arc<CatalogService>,
    audit_logger: AdminAuditLogger,
}

impl AdminMutationRunner {
    pub(crate) fn new(catalog: Arc<CatalogService>) -> Self {
        Self {
            catalog,
            audit_logger: AdminAuditLogger,
        }
    }

    pub(crate) async fn execute(&self, effects: &[AdminMutationEffect]) -> AdminMutationReport {
        let mut report = AdminMutationReport::default();

        // Post-commit effects always run in the same order:
        // 1. cache invalidation
        // 2. management audit events
        for effect in effects {
            if let AdminMutationEffect::CatalogInvalidation(invalidation) = effect
                && let Err(err) = self.apply_catalog_invalidation(invalidation).await
            {
                self.record_invalidation_failure(&mut report, invalidation, err);
            }
        }

        for effect in effects {
            if let AdminMutationEffect::Audit(event) = effect {
                self.audit_logger.emit(event);
            }
        }

        report
    }

    async fn apply_catalog_invalidation(
        &self,
        invalidation: &AdminCatalogInvalidation,
    ) -> Result<(), AppStoreError> {
        match invalidation {
            AdminCatalogInvalidation::ModelsCatalog => {
                self.catalog.invalidate_models_catalog().await
            }
            AdminCatalogInvalidation::Provider { id, key } => {
                self.catalog.invalidate_provider(*id, key.as_deref()).await
            }
            AdminCatalogInvalidation::ProviderApiKeys { provider_id } => {
                self.catalog
                    .invalidate_provider_api_keys(*provider_id)
                    .await
            }
            AdminCatalogInvalidation::ProviderRequestPatchRules { provider_id } => {
                self.catalog
                    .invalidate_provider_request_patch_rules(*provider_id)
                    .await
            }
            AdminCatalogInvalidation::Model {
                id,
                name,
                previous_name,
            } => {
                if let Some(previous_name) = previous_name.as_ref() {
                    self.catalog
                        .invalidate_model_by_name(
                            &previous_name.provider_key,
                            &previous_name.model_name,
                        )
                        .await?;
                }
                let composed_name = name.as_ref().map(AdminModelCacheName::as_catalog_name);
                self.catalog
                    .invalidate_model(*id, composed_name.as_deref())
                    .await
            }
            AdminCatalogInvalidation::ModelRoute {
                id,
                name,
                previous_name,
            } => {
                if let Some(previous_name) = previous_name.as_deref() {
                    self.catalog
                        .invalidate_model_route_by_name(previous_name)
                        .await?;
                }
                self.catalog
                    .invalidate_model_route(*id, name.as_deref())
                    .await
            }
            AdminCatalogInvalidation::ModelRoutes(routes) => {
                for route in routes {
                    self.catalog
                        .invalidate_model_route(route.id, route.name.as_deref())
                        .await?;
                }
                Ok(())
            }
            AdminCatalogInvalidation::ApiKeyId { id } => {
                self.catalog.invalidate_api_key_id(*id).await
            }
            AdminCatalogInvalidation::ApiKeyHash { api_key_hash } => {
                self.catalog.invalidate_api_key_hash(api_key_hash).await
            }
            AdminCatalogInvalidation::ApiKeyModelOverrides {
                api_key_id,
                source_names,
            } => {
                for source_name in source_names {
                    self.catalog
                        .invalidate_api_key_model_override(*api_key_id, source_name)
                        .await?;
                }
                Ok(())
            }
            AdminCatalogInvalidation::ModelRequestPatchRules { model_id } => {
                self.catalog
                    .invalidate_model_request_patch_rules(*model_id)
                    .await
            }
            AdminCatalogInvalidation::CostCatalogVersions { ids } => {
                for id in ids {
                    self.catalog.invalidate_cost_catalog_version(*id).await?;
                }
                Ok(())
            }
        }
    }

    fn record_invalidation_failure(
        &self,
        report: &mut AdminMutationReport,
        invalidation: &AdminCatalogInvalidation,
        err: AppStoreError,
    ) {
        let error_message = err.to_string();
        warn!(
            "{}",
            event_message_with_fields(
                "manager.admin_catalog_invalidation_failed",
                &[
                    ("invalidation_kind", Some(invalidation.kind().to_string())),
                    ("invalidation", Some(format!("{invalidation:?}"))),
                    ("error", Some(error_message.clone())),
                ],
            )
        );
        report
            .catalog_invalidation_failures
            .push(AdminCatalogInvalidationFailure {
                invalidation: invalidation.clone(),
                error_message,
            });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::database::TestDbContext;
    use crate::service::catalog::CatalogService;

    use super::{
        AdminCatalogInvalidation, AdminModelCacheName, AdminModelRouteCacheTarget,
        AdminMutationEffect, AdminMutationRunner,
    };
    use crate::service::admin::audit::{AdminAuditEvent, AdminAuditField};

    #[test]
    fn model_cache_name_uses_provider_and_model_segments() {
        let name = AdminModelCacheName::new("openai", "gpt-4.1");
        assert_eq!(name.as_catalog_name(), "openai/gpt-4.1");
    }

    #[tokio::test]
    async fn mutation_runner_supports_all_known_catalog_invalidation_variants() {
        let test_db_context = TestDbContext::new_sqlite("admin-mutation-runner.sqlite");

        test_db_context
            .run_async(async {
                let catalog = Arc::new(CatalogService::new(true).await);
                let runner = AdminMutationRunner::new(catalog);
                let effects = vec![
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ModelsCatalog,
                    ),
                    AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Provider {
                        id: 11,
                        key: Some("provider-a".to_string()),
                    }),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ProviderApiKeys { provider_id: 11 },
                    ),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ProviderRequestPatchRules { provider_id: 11 },
                    ),
                    AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::Model {
                        id: 21,
                        name: Some(AdminModelCacheName::new("provider-a", "model-a")),
                        previous_name: Some(AdminModelCacheName::new("provider-a", "model-legacy")),
                    }),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ModelRoute {
                            id: 31,
                            name: Some("route-a".to_string()),
                            previous_name: Some("route-legacy".to_string()),
                        },
                    ),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ModelRoutes(vec![
                            AdminModelRouteCacheTarget::new(32, Some("route-b")),
                            AdminModelRouteCacheTarget::new(33, Some("route-c")),
                        ]),
                    ),
                    AdminMutationEffect::catalog_invalidation(AdminCatalogInvalidation::ApiKeyId {
                        id: 41,
                    }),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ApiKeyHash {
                            api_key_hash: "hash-a".to_string(),
                        },
                    ),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ApiKeyModelOverrides {
                            api_key_id: 41,
                            source_names: vec!["route-a".to_string(), "route-b".to_string()],
                        },
                    ),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::ModelRequestPatchRules { model_id: 21 },
                    ),
                    AdminMutationEffect::catalog_invalidation(
                        AdminCatalogInvalidation::CostCatalogVersions { ids: vec![51, 52] },
                    ),
                    AdminMutationEffect::audit(AdminAuditEvent::with_fields(
                        "manager.admin_skeleton_verified",
                        [AdminAuditField::new("scope", "mutation_runner")],
                    )),
                ];

                let report = runner.execute(&effects).await;
                assert!(
                    !report.has_catalog_failures(),
                    "unexpected invalidation failures: {:?}",
                    report.catalog_invalidation_failures
                );
            })
            .await;
    }
}
