use std::sync::Arc;

use crate::service::catalog::CatalogService;

use self::api_key::ApiKeyAdminService;
use self::cost::CostAdminService;
use self::model::ModelAdminService;
use self::model_route::ModelRouteAdminService;
use self::mutation::AdminMutationRunner;
use self::provider::ProviderAdminService;
use self::request_patch::RequestPatchAdminService;

pub mod api_key;
pub mod audit;
pub mod cost;
pub mod model;
pub mod model_route;
pub mod mutation;
pub mod provider;
pub mod request_patch;

// Management write paths must be owned here. Controllers may parse HTTP payloads and
// shape responses, but cache invalidation, audit emission, and write orchestration
// must stay inside service/admin to avoid owner drift back into handlers.
pub struct AdminServices {
    pub provider: Arc<ProviderAdminService>,
    pub api_key: Arc<ApiKeyAdminService>,
    pub model: Arc<ModelAdminService>,
    pub model_route: Arc<ModelRouteAdminService>,
    pub request_patch: Arc<RequestPatchAdminService>,
    pub cost: Arc<CostAdminService>,
}

impl AdminServices {
    pub fn new(catalog: Arc<CatalogService>) -> Self {
        let mutation_runner = Arc::new(AdminMutationRunner::new(catalog));

        Self {
            provider: Arc::new(ProviderAdminService::new(Arc::clone(&mutation_runner))),
            api_key: Arc::new(ApiKeyAdminService::new(Arc::clone(&mutation_runner))),
            model: Arc::new(ModelAdminService::new(Arc::clone(&mutation_runner))),
            model_route: Arc::new(ModelRouteAdminService::new(Arc::clone(&mutation_runner))),
            request_patch: Arc::new(RequestPatchAdminService::new(Arc::clone(&mutation_runner))),
            cost: Arc::new(CostAdminService::new(Arc::clone(&mutation_runner))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::service::catalog::CatalogService;

    use super::AdminServices;

    #[tokio::test]
    async fn admin_services_share_one_mutation_runner() {
        let catalog = Arc::new(CatalogService::new(true).await);
        let services = AdminServices::new(Arc::clone(&catalog));

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
            services.model_route.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.request_patch.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.cost.mutation_runner(),
        ));
        assert_eq!(Arc::strong_count(&catalog), 2);
    }
}
