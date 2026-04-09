use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

use super::ProxyError;

#[derive(Clone, Debug)]
pub(super) struct ProxyCancellationContext {
    token: CancellationToken,
    reason: Arc<Mutex<Option<String>>>,
}

impl ProxyCancellationContext {
    pub(super) fn new() -> Self {
        Self {
            token: CancellationToken::new(),
            reason: Arc::new(Mutex::new(None)),
        }
    }

    pub(super) async fn cancel(&self, reason: impl Into<String>) {
        self.cancel_now(reason);
    }

    pub(super) fn cancel_now(&self, reason: impl Into<String>) {
        let mut guard = self
            .reason
            .lock()
            .expect("cancellation reason lock poisoned");
        if guard.is_none() {
            *guard = Some(reason.into());
        }
        drop(guard);
        self.token.cancel();
    }

    pub(super) async fn cancellation_error(&self) -> ProxyError {
        let reason = self
            .reason
            .lock()
            .expect("cancellation reason lock poisoned")
            .clone()
            .unwrap_or_else(|| "Client disconnected before proxy request completed.".to_string());
        ProxyError::ClientCancelled(reason)
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
    reason: String,
    armed: bool,
}

impl CancellationDropGuard {
    pub(super) fn new(cancellation: ProxyCancellationContext, reason: impl Into<String>) -> Self {
        Self {
            cancellation,
            reason: reason.into(),
            armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CancellationDropGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel_now(self.reason.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CancellationDropGuard, ProxyCancellationContext};
    use crate::proxy::ProxyError;

    #[tokio::test]
    async fn cancellation_context_returns_client_cancelled_error() {
        let cancellation = ProxyCancellationContext::new();
        cancellation.cancel("client closed socket").await;

        assert!(matches!(
            cancellation.cancellation_error().await,
            ProxyError::ClientCancelled(message) if message == "client closed socket"
        ));
    }

    #[tokio::test]
    async fn cancellation_drop_guard_cancels_when_armed() {
        let cancellation = ProxyCancellationContext::new();
        {
            let _guard = CancellationDropGuard::new(cancellation.clone(), "request future dropped");
        }

        cancellation.cancelled().await;
        assert!(matches!(
            cancellation.cancellation_error().await,
            ProxyError::ClientCancelled(message) if message == "request future dropped"
        ));
    }
}
