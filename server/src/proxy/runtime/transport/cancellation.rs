use std::sync::Arc;

use axum::http::StatusCode;
use tokio::sync::Mutex as TokioMutex;

use crate::{
    proxy::{
        cancellation::ProxyCancellationContext,
        logging::RequestLogContext,
        runtime::{
            log_writer::{
                finalize_cancelled_log_context, record_request_drop_cancellation_if_allowed,
            },
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
        },
    },
    service::{app_state::AppState, cache::types::CacheCostCatalogVersion},
};

pub(super) struct RequestLogContextGuard {
    app_state: Arc<AppState>,
    context: Arc<TokioMutex<RequestLogContext>>,
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
    is_armed: bool,
}

impl RequestLogContextGuard {
    pub(super) fn new(
        app_state: Arc<AppState>,
        context: Arc<TokioMutex<RequestLogContext>>,
        log_mode: RuntimeLogMode,
        execution_policy: RuntimeExecutionPolicy,
    ) -> Self {
        Self {
            app_state,
            context,
            log_mode,
            execution_policy,
            is_armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.is_armed = false;
    }
}

impl Drop for RequestLogContextGuard {
    fn drop(&mut self) {
        if self.is_armed
            && self.log_mode.should_record_immediate()
            && self.execution_policy.records_request_log()
        {
            let app_state = Arc::clone(&self.app_state);
            let context_clone = Arc::clone(&self.context);
            let task_app_state = Arc::clone(&app_state);
            let log_mode = self.log_mode;
            let execution_policy = self.execution_policy;
            app_state.infra.spawn_background_task(async move {
                record_request_drop_cancellation_if_allowed(
                    &task_app_state,
                    &context_clone,
                    log_mode,
                    execution_policy,
                )
                .await;
            });
        }
    }
}

pub(super) struct ResponseStreamCancellationGuard {
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    context: Arc<TokioMutex<RequestLogContext>>,
    url: String,
    status_code: StatusCode,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    execution_policy: RuntimeExecutionPolicy,
    reason: String,
    armed: bool,
}

impl ResponseStreamCancellationGuard {
    pub(super) fn new(
        app_state: Arc<AppState>,
        cancellation: ProxyCancellationContext,
        context: Arc<TokioMutex<RequestLogContext>>,
        url: impl Into<String>,
        status_code: StatusCode,
        cost_catalog_version: Option<CacheCostCatalogVersion>,
        execution_policy: RuntimeExecutionPolicy,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            app_state,
            cancellation,
            context,
            url: url.into(),
            status_code,
            cost_catalog_version,
            execution_policy,
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
            self.cancellation.cancel_now(self.reason.clone());
            if !self.execution_policy.records_request_log() {
                return;
            }
            let app_state = Arc::clone(&self.app_state);
            let context = Arc::clone(&self.context);
            let url = self.url.clone();
            let status_code = self.status_code;
            let cost_catalog_version = self.cost_catalog_version.clone();
            let execution_policy = self.execution_policy;
            let task_app_state = Arc::clone(&app_state);
            app_state.infra.spawn_background_task(async move {
                finalize_cancelled_log_context(
                    &task_app_state,
                    &context,
                    &url,
                    Some(status_code),
                    cost_catalog_version.as_ref(),
                    None,
                    None,
                    execution_policy,
                )
                .await;
            });
        }
    }
}
