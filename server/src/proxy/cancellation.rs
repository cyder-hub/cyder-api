use std::sync::{Arc, Mutex};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::proxy::runtime::transport::{
    lifecycle::{ProviderOutcome, ProxyTerminationCause, ProxyTerminationCoordinator},
    timing::TransportTimingState,
};

use super::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
    request_context::RequestId,
};

#[derive(Clone, Debug)]
pub(crate) struct ProxyCancellationContext {
    token: CancellationToken,
    reason: Arc<Mutex<Option<String>>>,
    coordinator: ProxyTerminationCoordinator,
    timing: TransportTimingState,
    total_deadline: Arc<Mutex<Option<Instant>>>,
}

impl ProxyCancellationContext {
    pub(crate) fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            reason: Arc::new(Mutex::new(None)),
            coordinator: ProxyTerminationCoordinator::default(),
            timing: TransportTimingState::default(),
            total_deadline: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) fn cancel_now(&self, reason: impl Into<String>) {
        let reason = reason.into();
        let mut guard = self
            .reason
            .lock()
            .expect("cancellation reason lock poisoned");
        if guard.is_none() {
            *guard = Some(reason.clone());
        }
        drop(guard);
        self.coordinator
            .try_terminate(ProxyTerminationCause::ClientCancelled);
        self.token.cancel();
    }

    pub(crate) fn coordinator(&self) -> ProxyTerminationCoordinator {
        self.coordinator.clone()
    }

    pub(crate) fn timing(&self) -> TransportTimingState {
        self.timing.clone()
    }

    pub(crate) fn set_total_deadline(&self, deadline: Instant) -> bool {
        let mut guard = self
            .total_deadline
            .lock()
            .expect("total deadline lock poisoned");
        if guard.is_some() {
            return false;
        }
        *guard = Some(deadline);
        true
    }

    pub(crate) fn total_deadline(&self) -> Option<Instant> {
        *self
            .total_deadline
            .lock()
            .expect("total deadline lock poisoned")
    }

    pub(crate) fn try_terminate_error(&self, error: &ProxyError) -> bool {
        self.coordinator.try_terminate_error(error)
    }

    pub(crate) fn try_provider_success(&self) -> bool {
        self.coordinator
            .try_record_provider_outcome(ProviderOutcome::Success)
    }

    pub(crate) fn try_provider_failure(&self) -> bool {
        self.coordinator
            .try_record_provider_outcome(ProviderOutcome::Failure)
    }

    pub(super) async fn cancellation_error(
        &self,
        stage: ExecutionStage,
        response_visibility: ResponseVisibility,
    ) -> ProxyError {
        let reason = self
            .reason
            .lock()
            .expect("cancellation reason lock poisoned")
            .clone()
            .unwrap_or_else(|| "Client disconnected before proxy request completed.".to_string());
        ProxyError::gateway(
            ProxyErrorCode::ClientCancelledError,
            stage,
            response_visibility,
            None,
            reason,
        )
    }

    pub(super) async fn cancelled(&self) {
        self.token.cancelled().await;
    }

    pub(super) fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

pub(super) struct CancellationDropGuard {
    cancellation: ProxyCancellationContext,
    request_id: RequestId,
    log_id: i64,
    response_visibility: ResponseVisibilityTracker,
    stage: ExecutionStage,
    reason: String,
    armed: bool,
}

impl CancellationDropGuard {
    pub(super) fn new(
        cancellation: ProxyCancellationContext,
        request_id: RequestId,
        log_id: i64,
        response_visibility: ResponseVisibilityTracker,
        stage: ExecutionStage,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            cancellation,
            request_id,
            log_id,
            response_visibility,
            stage,
            reason: reason.into(),
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }

    pub(super) fn set_stage(&mut self, stage: ExecutionStage) {
        self.stage = stage;
    }

    fn phase(&self) -> &'static str {
        match self.stage {
            ExecutionStage::Connect => "upstream_request",
            ExecutionStage::UpstreamResponse => "upstream_response",
            stage => stage.as_str(),
        }
    }

    fn error_fact(&self) -> ProxyError {
        ProxyError::gateway(
            ProxyErrorCode::ClientCancelledError,
            self.stage,
            self.response_visibility.current(),
            None,
            self.reason.clone(),
        )
    }
}

impl Drop for CancellationDropGuard {
    fn drop(&mut self) {
        if self.armed {
            let error = self.error_fact();
            crate::debug_event!(
                "proxy.client_disconnect_detected",
                request_id = &self.request_id,
                log_id = self.log_id,
                phase = self.phase(),
                error_code = error.code().as_str(),
                stage = error.stage().as_str(),
                response_visibility = error.response_visibility().as_str(),
            );
            self.cancellation.cancel_now(self.reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CancellationDropGuard, ProxyCancellationContext};
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
    };

    #[tokio::test]
    async fn cancellation_context_returns_client_cancelled_error() {
        let cancellation = ProxyCancellationContext::new();
        cancellation.cancel_now("client closed socket");

        let error = cancellation
            .cancellation_error(ExecutionStage::Connect, ResponseVisibility::NotVisible)
            .await;
        assert_eq!(error.code(), ProxyErrorCode::ClientCancelledError);
        assert_eq!(error.stage(), ExecutionStage::Connect);
        assert_eq!(error.operator_message(), "client closed socket");
    }

    #[tokio::test]
    async fn cancellation_drop_guard_cancels_when_armed() {
        let cancellation = ProxyCancellationContext::new();
        let guard = CancellationDropGuard::new(
            cancellation.clone(),
            crate::proxy::request_context::RequestId::new(),
            42,
            ResponseVisibilityTracker::new(),
            ExecutionStage::Connect,
            "request future dropped",
        );
        let fact = guard.error_fact();
        assert_eq!(fact.code(), ProxyErrorCode::ClientCancelledError);
        assert_eq!(fact.stage(), ExecutionStage::Connect);
        assert_eq!(fact.response_visibility(), ResponseVisibility::NotVisible);
        drop(guard);

        cancellation.cancelled().await;
        let error = cancellation
            .cancellation_error(ExecutionStage::Connect, ResponseVisibility::NotVisible)
            .await;
        assert_eq!(error.code(), ProxyErrorCode::ClientCancelledError);
        assert_eq!(error.operator_message(), "request future dropped");
    }

    #[test]
    fn cancellation_drop_guard_tracks_the_current_upstream_phase() {
        let cancellation = ProxyCancellationContext::new();
        let mut guard = CancellationDropGuard::new(
            cancellation,
            crate::proxy::request_context::RequestId::new(),
            43,
            ResponseVisibilityTracker::new(),
            ExecutionStage::Connect,
            "request future dropped",
        );

        assert_eq!(guard.error_fact().stage(), ExecutionStage::Connect);
        assert_eq!(guard.phase(), "upstream_request");
        guard.set_stage(ExecutionStage::UpstreamResponse);
        assert_eq!(guard.error_fact().stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(guard.phase(), "upstream_response");
        guard.disarm();
    }
}
