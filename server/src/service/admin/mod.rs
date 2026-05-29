use std::sync::Arc;

use crate::service::catalog::CatalogService;

use self::api_key::ApiKeyAdminService;
use self::auth::ManagerAuthService;
use self::cost::CostAdminService;
use self::model::ModelAdminService;
use self::model_route::ModelRouteAdminService;
use self::mutation::AdminMutationRunner;
use self::portable_config::PortableConfigAdminService;
use self::provider::ProviderAdminService;
use self::reasoning_config::ReasoningConfigAdminService;
use self::request_patch::RequestPatchAdminService;
use self::runtime_feature_config::RuntimeFeatureConfigAdminService;

pub mod api_key;
pub mod audit;
pub mod auth;
pub mod cost;
pub mod model;
pub mod model_route;
pub mod mutation;
pub mod portable_config;
pub mod provider;
pub mod reasoning_config;
pub mod request_patch;
pub mod runtime_feature_config;

// Management write paths must be owned here. Controllers may parse HTTP payloads and
// shape responses, but cache invalidation, audit emission, and write orchestration
// must stay inside service/admin to avoid owner drift back into handlers.
pub struct AdminServices {
    pub auth: Arc<ManagerAuthService>,
    pub provider: Arc<ProviderAdminService>,
    pub api_key: Arc<ApiKeyAdminService>,
    pub model: Arc<ModelAdminService>,
    pub model_route: Arc<ModelRouteAdminService>,
    pub request_patch: Arc<RequestPatchAdminService>,
    pub cost: Arc<CostAdminService>,
    pub reasoning_config: Arc<ReasoningConfigAdminService>,
    pub runtime_feature_config: Arc<RuntimeFeatureConfigAdminService>,
    pub portable_config: Arc<PortableConfigAdminService>,
}

impl AdminServices {
    pub fn new(catalog: Arc<CatalogService>) -> Self {
        let mutation_runner = Arc::new(AdminMutationRunner::new(catalog));

        Self {
            auth: Arc::new(ManagerAuthService::new()),
            provider: Arc::new(ProviderAdminService::new(Arc::clone(&mutation_runner))),
            api_key: Arc::new(ApiKeyAdminService::new(Arc::clone(&mutation_runner))),
            model: Arc::new(ModelAdminService::new(Arc::clone(&mutation_runner))),
            model_route: Arc::new(ModelRouteAdminService::new(Arc::clone(&mutation_runner))),
            request_patch: Arc::new(RequestPatchAdminService::new(Arc::clone(&mutation_runner))),
            cost: Arc::new(CostAdminService::new(Arc::clone(&mutation_runner))),
            reasoning_config: Arc::new(ReasoningConfigAdminService::new(Arc::clone(
                &mutation_runner,
            ))),
            runtime_feature_config: Arc::new(RuntimeFeatureConfigAdminService::new(Arc::clone(
                &mutation_runner,
            ))),
            portable_config: Arc::new(PortableConfigAdminService::new(Arc::clone(
                &mutation_runner,
            ))),
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
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.reasoning_config.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.runtime_feature_config.mutation_runner(),
        ));
        assert!(Arc::ptr_eq(
            services.provider.mutation_runner(),
            services.portable_config.mutation_runner(),
        ));
        assert_eq!(Arc::strong_count(&catalog), 2);
    }
}
