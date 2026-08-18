use std::sync::Arc;
#[cfg(test)]
use std::sync::Mutex;
#[cfg(test)]
use tokio::sync::Notify;

use crate::database::runtime::DatabaseRuntime;
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
pub enum AdminCatalogInvalidation {
    ModelsCatalog,
    Provider {
        id: i64,
        key: Option<String>,
    },
    ProviderApiKeys {
        provider_id: i64,
    },
    RequestPatchSource {
        source_id: i64,
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
    RequestPatchModel {
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
            Self::RequestPatchSource { .. } => "request_patch_source",
            Self::Model { .. } => "model",
            Self::ApiKeyId { .. } => "api_key_id",
            Self::ApiKeyHash { .. } => "api_key_hash",
            Self::RequestPatchModel { .. } => "request_patch_model",
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
    #[cfg(test)]
    emitted_audit_events: Mutex<Vec<AdminAuditEvent>>,
    #[cfg(test)]
    before_effects_gate: Mutex<Option<AdminMutationTestGate>>,
}

#[cfg(test)]
#[derive(Clone)]
pub(crate) struct AdminMutationTestGate {
    reached: Arc<Notify>,
    resume: Arc<Notify>,
    completed: Arc<Notify>,
}

#[cfg(test)]
impl AdminMutationTestGate {
    pub(crate) async fn wait_until_reached(&self) {
        self.reached.notified().await;
    }

    pub(crate) fn resume(&self) {
        self.resume.notify_one();
    }

    pub(crate) async fn wait_until_completed(&self) {
        self.completed.notified().await;
    }
}

impl AdminMutationRunner {
    pub(crate) fn new(catalog: Arc<CatalogService>) -> Self {
        Self {
            catalog,
            audit_logger: AdminAuditLogger,
            #[cfg(test)]
            emitted_audit_events: Mutex::new(Vec::new()),
            #[cfg(test)]
            before_effects_gate: Mutex::new(None),
        }
    }

    pub(crate) fn database(&self) -> Arc<DatabaseRuntime> {
        self.catalog.database()
    }

    pub(crate) async fn execute(&self, effects: &[AdminMutationEffect]) -> AdminMutationReport {
        #[cfg(test)]
        let test_gate = self.wait_before_effects_for_test().await;
        let mut report = AdminMutationReport::default();

        // Post-commit effects always run in the same order:
        // 1. cache invalidation
        // 2. management audit events
        for effect in effects {
            match effect {
                AdminMutationEffect::CatalogInvalidation(invalidation) => {
                    if let Err(err) = self.apply_catalog_invalidation(invalidation).await {
                        self.record_invalidation_failure(&mut report, invalidation, err);
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

        #[cfg(test)]
        if let Some(gate) = test_gate {
            gate.completed.notify_one();
        }

        report
    }

    #[cfg(test)]
    pub(crate) fn pause_before_effects_for_test(&self) -> AdminMutationTestGate {
        let gate = AdminMutationTestGate {
            reached: Arc::new(Notify::new()),
            resume: Arc::new(Notify::new()),
            completed: Arc::new(Notify::new()),
        };
        *self
            .before_effects_gate
            .lock()
            .expect("admin mutation test gate should lock") = Some(gate.clone());
        gate
    }

    #[cfg(test)]
    async fn wait_before_effects_for_test(&self) -> Option<AdminMutationTestGate> {
        let gate = self
            .before_effects_gate
            .lock()
            .expect("admin mutation test gate should lock")
            .take();
        if let Some(gate) = &gate {
            gate.reached.notify_one();
            gate.resume.notified().await;
        }
        gate
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
            AdminCatalogInvalidation::RequestPatchSource { source_id } => {
                self.catalog
                    .invalidate_request_patch_source(*source_id)
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
            AdminCatalogInvalidation::RequestPatchModel { model_id } => {
                self.catalog.invalidate_request_patch_model(*model_id).await
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
mod tests {}
