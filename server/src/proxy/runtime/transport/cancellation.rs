use std::sync::Arc;

use axum::http::StatusCode;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    proxy::{
        cancellation::ProxyCancellationContext, logging::RequestLogContext,
        request_context::RequestId, runtime::log_writer::finalize_cancelled_log_context,
    },
    service::{app_state::AppState, cache::types::CacheCostCatalogVersion},
};

pub(super) struct ResponseStreamCancellationGuard {
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    context: Arc<TokioMutex<RequestLogContext>>,
    request_id: RequestId,
    log_id: i64,
    url: String,
    status_code: StatusCode,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    reason: String,
    armed: bool,
}

impl ResponseStreamCancellationGuard {
    pub(super) fn new(
        app_state: Arc<AppState>,
        cancellation: ProxyCancellationContext,
        context: Arc<TokioMutex<RequestLogContext>>,
        request_id: RequestId,
        log_id: i64,
        url: impl Into<String>,
        status_code: StatusCode,
        cost_catalog_version: Option<CacheCostCatalogVersion>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            app_state,
            cancellation,
            context,
            request_id,
            log_id,
            url: url.into(),
            status_code,
            cost_catalog_version,
            reason: reason.into(),
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ResponseStreamCancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            crate::debug_event!(
                "proxy.client_disconnect_detected",
                request_id = &self.request_id,
                log_id = self.log_id,
                phase = "response_stream",
            );
            self.cancellation.cancel_now(self.reason.clone());
            let app_state = Arc::clone(&self.app_state);
            let context = Arc::clone(&self.context);
            let url = self.url.clone();
            let status_code = self.status_code;
            let cost_catalog_version = self.cost_catalog_version.clone();
            let task_app_state = Arc::clone(&app_state);
            app_state.infra.spawn_background_task(async move {
                finalize_cancelled_log_context(
                    &task_app_state,
                    &context,
                    &url,
                    Some(status_code),
                    cost_catalog_version.as_ref(),
                )
                .await;
            });
        }
    }
}
