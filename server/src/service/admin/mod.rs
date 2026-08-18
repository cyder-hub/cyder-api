use std::{future::Future, sync::Arc};

use crate::service::catalog::CatalogService;
use crate::service::secret_encryption::SecretEncryptionService;

use self::api_key::ApiKeyAdminService;
use self::auth::ManagerAuthService;
use self::cost::CostAdminService;
use self::model::ModelAdminService;
use self::mutation::AdminMutationRunner;
use self::provider::ProviderAdminService;
use self::request_patch::RequestPatchAdminService;

pub mod api_key;
pub mod audit;
pub mod auth;
pub mod cost;
pub mod model;
pub mod mutation;
pub mod provider;
pub mod request_patch;

async fn await_cancellation_safe<T, F>(future: F) -> Result<T, tokio::task::JoinError>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    tokio::spawn(future).await
}

// Management write paths must be owned here. Controllers may parse HTTP payloads and
// shape responses, but cache invalidation, audit emission, and write orchestration
// must stay inside service/admin to avoid owner drift back into handlers.
pub struct AdminServices {
    pub auth: Arc<ManagerAuthService>,
    pub provider: Arc<ProviderAdminService>,
    pub api_key: Arc<ApiKeyAdminService>,
    pub model: Arc<ModelAdminService>,
    pub request_patch: Arc<RequestPatchAdminService>,
    pub cost: Arc<CostAdminService>,
    pub secret_encryption: Arc<SecretEncryptionService>,
}

impl AdminServices {
    pub async fn new(
        catalog: Arc<CatalogService>,
        secret_encryption: Arc<SecretEncryptionService>,
    ) -> Self {
        let database = catalog.database();
        let mutation_runner = Arc::new(AdminMutationRunner::new(catalog));

        Self {
            auth: Arc::new(ManagerAuthService::new(database, Arc::clone(&secret_encryption)).await),
            provider: Arc::new(ProviderAdminService::new(
                Arc::clone(&mutation_runner),
                Arc::clone(&secret_encryption),
            )),
            api_key: Arc::new(ApiKeyAdminService::new(
                Arc::clone(&mutation_runner),
                Arc::clone(&secret_encryption),
            )),
            model: Arc::new(ModelAdminService::new(Arc::clone(&mutation_runner))),
            request_patch: Arc::new(RequestPatchAdminService::new(Arc::clone(&mutation_runner))),
            cost: Arc::new(CostAdminService::new(Arc::clone(&mutation_runner))),
            secret_encryption,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::config::{DatabaseIoConfig, SecretEncryptionConfig};
    use crate::database::test_support::TestDatabase;
    use crate::service::catalog::CatalogService;
    use crate::service::secret_encryption::SecretEncryptionService;

    use super::AdminServices;

    #[tokio::test]
    async fn admin_services_share_one_mutation_runner() {
        let database =
            TestDatabase::new_sqlite("admin-services.sqlite", 2, DatabaseIoConfig::default()).await;
        let catalog = Arc::new(CatalogService::new(database.runtime(), true).await);
        let secret_encryption = Arc::new(SecretEncryptionService::from_config(
            &SecretEncryptionConfig::default(),
        ));
        let services =
            AdminServices::new(Arc::clone(&catalog), Arc::clone(&secret_encryption)).await;

        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.api_key.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.model.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.request_patch.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.cost.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(&services.secret_encryption, &secret_encryption,));
        assert!(Arc::ptr_eq(
            services.api_key.secret_encryption(),
            &secret_encryption,
        ));
        assert!(Arc::ptr_eq(
            services.auth.secret_encryption(),
            &secret_encryption,
        ));
        assert_eq!(Arc::strong_count(&catalog), 2);
    }
}
