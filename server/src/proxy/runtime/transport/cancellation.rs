use std::sync::Arc;

use axum::http::StatusCode;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibilityTracker,
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
    response_visibility: ResponseVisibilityTracker,
    reason: String,
    armed: bool,
}

fn response_stream_cancellation_error(
    response_visibility: &ResponseVisibilityTracker,
    reason: impl Into<String>,
) -> ProxyError {
    ProxyError::gateway(
        ProxyErrorCode::ClientCancelledError,
        ExecutionStage::DownstreamSend,
        response_visibility.current(),
        None,
        reason,
    )
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
        response_visibility: ResponseVisibilityTracker,
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
            response_visibility,
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
            let proxy_error =
                response_stream_cancellation_error(&self.response_visibility, self.reason.clone());
            crate::debug_event!(
                "proxy.client_disconnect_detected",
                request_id = &self.request_id,
                log_id = self.log_id,
                phase = "response_stream",
                error_code = proxy_error.code().as_str(),
                stage = proxy_error.stage().as_str(),
                response_visibility = proxy_error.response_visibility().as_str(),
            );
            self.cancellation.cancel_now(self.reason.clone());
            let app_state = Arc::clone(&self.app_state);
            let context = Arc::clone(&self.context);
            let url = self.url.clone();
            let status_code = self.status_code;
            let cost_catalog_version = self.cost_catalog_version.clone();
            let proxy_error = proxy_error.clone();
            let task_app_state = Arc::clone(&app_state);
            app_state.infra.spawn_background_task(async move {
                finalize_cancelled_log_context(
                    &task_app_state,
                    &context,
                    &url,
                    Some(status_code),
                    cost_catalog_version.as_ref(),
                    &proxy_error,
                )
                .await;
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::response_stream_cancellation_error;
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
    };

    #[test]
    fn response_stream_drop_fact_uses_current_visibility() {
        let tracker = ResponseVisibilityTracker::new();
        tracker.advance_to(ResponseVisibility::HeadersCommitted);
        let before_body = response_stream_cancellation_error(&tracker, "client disconnected");
        assert_eq!(before_body.code(), ProxyErrorCode::ClientCancelledError);
        assert_eq!(before_body.stage(), ExecutionStage::DownstreamSend);
        assert_eq!(
            before_body.response_visibility(),
            ResponseVisibility::HeadersCommitted
        );

        tracker.advance_to(ResponseVisibility::BodyStarted);
        let after_body = response_stream_cancellation_error(&tracker, "client disconnected");
        assert_eq!(
            after_body.response_visibility(),
            ResponseVisibility::BodyStarted
        );
    }
}
