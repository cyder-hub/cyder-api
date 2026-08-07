use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;

use crate::logging::event_message_with_fields;
use crate::service::app_state::AppStoreError;
use crate::service::catalog::CatalogService;
use crate::service::runtime::SourceCircuitService;
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
    ReasoningProviderConfig {
        provider_id: i64,
    },
    ReasoningModelConfig {
        model_id: i64,
    },
    RuntimeFeatureProviderConfig {
        provider_id: i64,
    },
    RuntimeFeatureModelConfig {
        model_id: i64,
    },
    Model {
        id: i64,
        name: Option<AdminModelCacheName>,
        previous_name: Option<AdminModelCacheName>,
    },
    ApiKeyId {
        id: i64,
    },
    ApiKeyHash {
        api_key_hash: String,
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
            Self::ReasoningProviderConfig { .. } => "reasoning_provider_config",
            Self::ReasoningModelConfig { .. } => "reasoning_model_config",
            Self::RuntimeFeatureProviderConfig { .. } => "runtime_feature_provider_config",
            Self::RuntimeFeatureModelConfig { .. } => "runtime_feature_model_config",
            Self::Model { .. } => "model",
            Self::ApiKeyId { .. } => "api_key_id",
            Self::ApiKeyHash { .. } => "api_key_hash",
            Self::ModelRequestPatchRules { .. } => "model_request_patch_rules",
            Self::CostCatalogVersions { .. } => "cost_catalog_versions",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminMutationEffect {
    CatalogInvalidation(AdminCatalogInvalidation),
    SourceCircuitClear { source_id: i64 },
    Audit(AdminAuditEvent),
}

impl AdminMutationEffect {
    pub fn catalog_invalidation(invalidation: AdminCatalogInvalidation) -> Self {
        Self::CatalogInvalidation(invalidation)
    }

    pub fn audit(event: AdminAuditEvent) -> Self {
        Self::Audit(event)
    }

    pub fn source_circuit_clear(source_id: i64) -> Self {
        Self::SourceCircuitClear { source_id }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminCatalogInvalidationFailure {
    pub invalidation: AdminCatalogInvalidation,
    pub error_message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminSourceCircuitClearFailure {
    pub source_id: i64,
    pub error_message: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AdminMutationReport {
    pub catalog_invalidation_failures: Vec<AdminCatalogInvalidationFailure>,
    pub source_circuit_clear_failures: Vec<AdminSourceCircuitClearFailure>,
}

impl AdminMutationReport {
    pub fn has_catalog_failures(&self) -> bool {
        !self.catalog_invalidation_failures.is_empty()
    }

    pub fn has_source_circuit_clear_failures(&self) -> bool {
        !self.source_circuit_clear_failures.is_empty()
    }
}

pub(crate) struct AdminMutationRunner {
    catalog: Arc<CatalogService>,
    source_circuit: Arc<SourceCircuitService>,
    audit_logger: AdminAuditLogger,
    #[cfg(test)]
    emitted_audit_events: Mutex<Vec<AdminAuditEvent>>,
}

impl AdminMutationRunner {
    pub(crate) fn new(
        catalog: Arc<CatalogService>,
        source_circuit: Arc<SourceCircuitService>,
    ) -> Self {
        Self {
            catalog,
            source_circuit,
            audit_logger: AdminAuditLogger,
            #[cfg(test)]
            emitted_audit_events: Mutex::new(Vec::new()),
        }
    }

    pub(crate) async fn execute(&self, effects: &[AdminMutationEffect]) -> AdminMutationReport {
        let mut report = AdminMutationReport::default();

        // Post-commit effects always run in the same order:
        // 1. cache invalidation
        // 2. runtime state cleanup
        // 3. management audit events
        for effect in effects {
            match effect {
                AdminMutationEffect::CatalogInvalidation(invalidation) => {
                    if let Err(err) = self.apply_catalog_invalidation(invalidation).await {
                        self.record_invalidation_failure(&mut report, invalidation, err);
                    }
                }
                AdminMutationEffect::SourceCircuitClear { source_id } => {
                    if let Err(err) = self.source_circuit.clear_source(*source_id).await {
                        self.record_source_circuit_clear_failure(&mut report, *source_id, err);
                    }
                }
                AdminMutationEffect::Audit(_) => {}
            }
        }

        for effect in effects {
            if let AdminMutationEffect::Audit(event) = effect {
                self.audit_logger.emit(event);
                #[cfg(test)]
                self.record_audit_event_for_test(event.clone());
            }
        }

        report
    }

    #[cfg(test)]
    fn record_audit_event_for_test(&self, event: AdminAuditEvent) {
        self.emitted_audit_events
            .lock()
            .expect("admin mutation audit test sink should lock")
            .push(event);
    }

    #[cfg(test)]
    pub(crate) fn drain_audit_events(&self) -> Vec<AdminAuditEvent> {
        let mut events = self
            .emitted_audit_events
            .lock()
            .expect("admin mutation audit test sink should lock");
        std::mem::take(&mut *events)
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
            AdminCatalogInvalidation::ReasoningProviderConfig { provider_id } => {
                self.catalog
                    .invalidate_reasoning_provider_config(*provider_id)
                    .await
            }
            AdminCatalogInvalidation::ReasoningModelConfig { model_id } => {
                self.catalog
                    .invalidate_reasoning_model_config(*model_id)
                    .await
            }
            AdminCatalogInvalidation::RuntimeFeatureProviderConfig { provider_id } => {
                self.catalog
                    .invalidate_runtime_feature_provider_config(*provider_id)
                    .await
            }
            AdminCatalogInvalidation::RuntimeFeatureModelConfig { model_id } => {
                self.catalog
                    .invalidate_runtime_feature_model_config(*model_id)
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
            AdminCatalogInvalidation::ApiKeyId { id } => {
                self.catalog.invalidate_api_key_id(*id).await
            }
            AdminCatalogInvalidation::ApiKeyHash { api_key_hash } => {
                self.catalog.invalidate_api_key_hash(api_key_hash).await
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

    fn record_source_circuit_clear_failure(
        &self,
        report: &mut AdminMutationReport,
        source_id: i64,
        err: crate::service::runtime::SourceCircuitError,
    ) {
        let error_message = err.to_string();
        warn!(
            "{}",
            event_message_with_fields(
                "manager.source_circuit_clear_failed",
                &[
                    ("source_id", Some(source_id.to_string())),
                    ("error", Some(error_message.clone())),
                ],
            )
        );
        report
            .source_circuit_clear_failures
            .push(AdminSourceCircuitClearFailure {
                source_id,
                error_message,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::{AdminMutationEffect, AdminMutationRunner};
    use crate::service::catalog::CatalogService;
    use crate::service::runtime::{SourceCircuitService, SourceHealthStatus};
    use std::sync::Arc;

    #[tokio::test]
    async fn source_circuit_clear_effect_runs_after_commit_and_is_reported() {
        let catalog = Arc::new(CatalogService::new(true).await);
        let source_circuit = Arc::new(SourceCircuitService::new_memory());
        for _ in 0..5 {
            source_circuit
                .record_source_failure(810, "timeout".to_string(), None)
                .await
                .expect("source failure should record");
        }
        assert_eq!(
            source_circuit
                .get_source_health_snapshot(810)
                .await
                .expect("open snapshot should load")
                .status,
            SourceHealthStatus::Open
        );

        let runner = AdminMutationRunner::new(catalog, Arc::clone(&source_circuit));
        let report = runner
            .execute(&[AdminMutationEffect::source_circuit_clear(810)])
            .await;

        assert!(!report.has_source_circuit_clear_failures());
        assert_eq!(
            source_circuit
                .get_source_health_snapshot(810)
                .await
                .expect("cleared snapshot should load")
                .status,
            SourceHealthStatus::Healthy
        );
    }
}
