use std::sync::Arc;

use crate::service::{
    app_state::AppState,
    runtime::{ApiKeyGovernanceService, ApiKeyRequestLease},
};

pub(crate) struct ApiKeyRequestLeaseFinalizer {
    governance: Arc<ApiKeyGovernanceService>,
    lease: Option<ApiKeyRequestLease>,
}

impl ApiKeyRequestLeaseFinalizer {
    pub(crate) fn new(app_state: &Arc<AppState>, lease: Option<ApiKeyRequestLease>) -> Self {
        Self {
            governance: Arc::clone(&app_state.api_key_governance),
            lease,
        }
    }

    pub(crate) async fn release(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        let api_key_id = lease.api_key_id();
        let lease_id = lease.lease_id().to_string();
        if let Err(err) = self.governance.release_api_key_request_lease(lease).await {
            crate::warn_event!(
                "auth.request_lease_release_failed",
                api_key_id = api_key_id,
                lease_id = lease_id,
                error = err.to_string(),
            );
        }
    }
}

impl Drop for ApiKeyRequestLeaseFinalizer {
    fn drop(&mut self) {
        let Some(lease) = self.lease.take() else {
            return;
        };
        let governance = Arc::clone(&self.governance);
        let api_key_id = lease.api_key_id();
        let lease_id = lease.lease_id().to_string();

        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            crate::warn_event!(
                "auth.request_lease_drop_without_runtime",
                api_key_id = api_key_id,
                lease_id = lease_id,
            );
            return;
        };

        handle.spawn(async move {
            if let Err(err) = governance.release_api_key_request_lease(lease).await {
                crate::warn_event!(
                    "auth.request_lease_drop_release_failed",
                    api_key_id = api_key_id,
                    lease_id = lease_id,
                    error = err.to_string(),
                );
            }
        });
    }
}
