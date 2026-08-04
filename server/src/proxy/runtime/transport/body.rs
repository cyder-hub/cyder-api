use std::{
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum::body::Body;
use axum::http::StatusCode;
use chrono::Utc;
use futures::Stream;
use tokio::{
    sync::{Mutex as TokioMutex, mpsc, oneshot},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use super::lifecycle::{ProxyTerminationCause, ProxyTerminationCoordinator, TotalWatchdogResult};
use crate::{
    proxy::{
        ResponseVisibility, ResponseVisibilityTracker, cancellation::ProxyCancellationContext,
        logging::RequestLogContext, runtime::log_writer::finalize_cancelled_log_context,
    },
    service::{app_state::AppState, cache::types::CacheCostCatalogVersion},
};

pub(crate) const BODY_FRAME_CHANNEL_CAPACITY: usize = 1;

pub(crate) struct BodyFrame {
    pub(crate) result: Result<bytes::Bytes, io::Error>,
    ack: oneshot::Sender<()>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FrameDeliveryError {
    ClientCancelled,
    Timeout { phase: crate::proxy::TimeoutPhase },
    DownstreamDropped,
}

pub(crate) async fn send_body_frame(
    sender: &mpsc::Sender<BodyFrame>,
    result: Result<bytes::Bytes, io::Error>,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
) -> Result<(), FrameDeliveryError> {
    let (ack_sender, ack_receiver) = oneshot::channel();
    let frame = BodyFrame {
        result,
        ack: ack_sender,
    };
    let watchdog = total_watchdog(cancellation, coordinator);
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => Err(FrameDeliveryError::ClientCancelled),
        watchdog_result = watchdog.wait() => Err(frame_delivery_error(
            watchdog_result,
            cancellation,
            coordinator,
        )),
        send_result = sender.send(frame) => {
            if send_result.is_err() {
                Err(FrameDeliveryError::DownstreamDropped)
            } else {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(FrameDeliveryError::ClientCancelled),
                    watchdog_result = watchdog.wait() => Err(frame_delivery_error(
                        watchdog_result,
                        cancellation,
                        coordinator,
                    )),
                    ack_result = ack_receiver => {
                        ack_result.map_err(|_| FrameDeliveryError::DownstreamDropped)
                    }
                }
            }
        }
    }
}

fn total_watchdog(
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
) -> super::lifecycle::TotalWatchdog {
    let deadline = cancellation
        .total_deadline()
        .expect("proxy total deadline must be initialized before Body supervision");
    coordinator.arm_total_watchdog(deadline)
}

fn frame_delivery_error(
    watchdog_result: TotalWatchdogResult,
    cancellation: &ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
) -> FrameDeliveryError {
    match watchdog_result {
        TotalWatchdogResult::Expired => FrameDeliveryError::Timeout {
            phase: crate::proxy::TimeoutPhase::Total,
        },
        TotalWatchdogResult::Cancelled => match coordinator.terminal() {
            Some(ProxyTerminationCause::ClientCancelled) => FrameDeliveryError::ClientCancelled,
            Some(ProxyTerminationCause::Timeout { phase }) => FrameDeliveryError::Timeout { phase },
            _ if cancellation.is_cancelled() => FrameDeliveryError::ClientCancelled,
            _ => FrameDeliveryError::Timeout {
                phase: crate::proxy::TimeoutPhase::Total,
            },
        },
    }
}

struct BodyCancellationFinalizer {
    stop: CancellationToken,
    detached: bool,
}

impl BodyCancellationFinalizer {
    fn new(
        app_state: Arc<AppState>,
        cancellation: ProxyCancellationContext,
        context: Arc<TokioMutex<RequestLogContext>>,
        url: String,
        status_code: StatusCode,
        cost_catalog_version: Option<CacheCostCatalogVersion>,
        response_visibility: ResponseVisibilityTracker,
    ) -> Self {
        let stop = CancellationToken::new();
        let task_stop = stop.clone();
        let task_app_state = Arc::clone(&app_state);
        app_state.infra.spawn_background_task(async move {
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => {
                    let proxy_error = cancellation
                        .cancellation_error(
                            crate::proxy::ExecutionStage::DownstreamSend,
                            response_visibility.current(),
                        )
                        .await;
                    finalize_cancelled_log_context(
                        &task_app_state,
                        &context,
                        &url,
                        Some(status_code),
                        cost_catalog_version.as_ref(),
                        &proxy_error,
                    )
                    .await;
                }
                _ = task_stop.cancelled() => {}
            }
        });
        Self {
            stop,
            detached: false,
        }
    }

    fn disarm(&mut self) {
        self.stop.cancel();
    }

    fn detach(&mut self) {
        self.detached = true;
    }
}

impl Drop for BodyCancellationFinalizer {
    fn drop(&mut self) {
        if !self.detached {
            self.stop.cancel();
        }
    }
}

pub(crate) struct GuardedBodyStream {
    receiver: mpsc::Receiver<BodyFrame>,
    pending_ack: Option<oneshot::Sender<()>>,
    cancellation: ProxyCancellationContext,
    coordinator: ProxyTerminationCoordinator,
    worker: Option<JoinHandle<()>>,
    cancellation_finalizer: Option<BodyCancellationFinalizer>,
    response_visibility: ResponseVisibilityTracker,
    timing: super::timing::TransportTimingState,
    terminal_error_emitted: bool,
    terminal_frame_delivered: bool,
    completed: bool,
    drop_reason: String,
}

impl GuardedBodyStream {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        app_state: Arc<AppState>,
        cancellation: ProxyCancellationContext,
        coordinator: ProxyTerminationCoordinator,
        receiver: mpsc::Receiver<BodyFrame>,
        worker: JoinHandle<()>,
        log_context: Arc<TokioMutex<RequestLogContext>>,
        url: String,
        status_code: axum::http::StatusCode,
        cost_catalog_version: Option<CacheCostCatalogVersion>,
        response_visibility: ResponseVisibilityTracker,
        drop_reason: String,
    ) -> Self {
        let timing = cancellation.timing();
        let cancellation_finalizer = BodyCancellationFinalizer::new(
            app_state,
            cancellation.clone(),
            log_context.clone(),
            url,
            status_code,
            cost_catalog_version,
            response_visibility.clone(),
        );
        Self {
            receiver,
            pending_ack: None,
            cancellation,
            coordinator,
            worker: Some(worker),
            cancellation_finalizer: Some(cancellation_finalizer),
            response_visibility,
            timing,
            terminal_error_emitted: false,
            terminal_frame_delivered: false,
            completed: false,
            drop_reason,
        }
    }

    fn poll_terminal_error(&mut self) -> Option<io::Error> {
        if self.terminal_frame_delivered {
            return None;
        }
        let cause = self.coordinator.terminal()?;
        if self.terminal_error_emitted {
            return None;
        }
        let (kind, message) = match cause {
            ProxyTerminationCause::ClientCancelled => (
                io::ErrorKind::ConnectionAborted,
                "client cancelled the guarded upstream response",
            ),
            ProxyTerminationCause::Timeout { phase } => (io::ErrorKind::TimedOut, phase.as_str()),
            ProxyTerminationCause::UpstreamError => (
                io::ErrorKind::Other,
                "upstream response terminated with an error",
            ),
            ProxyTerminationCause::DownstreamError => (
                io::ErrorKind::Other,
                "downstream response terminated with an error",
            ),
            ProxyTerminationCause::Success => return None,
        };
        self.terminal_error_emitted = true;
        self.receiver.close();
        self.completed = true;
        if let Some(finalizer) = self.cancellation_finalizer.as_mut() {
            if matches!(cause, ProxyTerminationCause::ClientCancelled) {
                finalizer.detach();
            } else {
                finalizer.disarm();
            }
        }
        // Dropping a JoinHandle detaches the cleanup owner. Aborting it here
        // can strand a claimed request log or API-key lease.
        self.worker.take();
        Some(io::Error::new(kind, message))
    }

    fn observe_downstream_body(&self, bytes: &bytes::Bytes) {
        if bytes.is_empty() {
            return;
        }
        self.response_visibility
            .advance_to(ResponseVisibility::BodyStarted);
        let now = Utc::now().timestamp_millis();
        self.timing
            .mark_first_response_body(now, tokio::time::Instant::now());
    }
}

impl Unpin for GuardedBodyStream {}

impl Stream for GuardedBodyStream {
    type Item = Result<bytes::Bytes, io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        if let Some(ack) = this.pending_ack.take() {
            let _ = ack.send(());
        }
        if let Some(error) = this.poll_terminal_error() {
            return Poll::Ready(Some(Err(error)));
        }
        match this.receiver.poll_recv(cx) {
            Poll::Ready(Some(frame)) => {
                if let Ok(bytes) = &frame.result {
                    this.observe_downstream_body(bytes);
                    this.pending_ack = Some(frame.ack);
                } else {
                    this.terminal_frame_delivered = true;
                    this.completed = true;
                    if let Some(finalizer) = this.cancellation_finalizer.as_mut() {
                        finalizer.disarm();
                    }
                    let _ = frame.ack.send(());
                }
                Poll::Ready(Some(frame.result))
            }
            Poll::Ready(None) => {
                this.completed = true;
                if let Some(finalizer) = this.cancellation_finalizer.as_mut() {
                    finalizer.disarm();
                }
                this.worker.take();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for GuardedBodyStream {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        self.cancellation.cancel_now(self.drop_reason.clone());
        if let Some(worker) = self.worker.take() {
            worker.abort();
        }
    }
}

pub(crate) fn guarded_body(stream: GuardedBodyStream) -> Body {
    Body::from_stream(stream)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::lifecycle::{ProxyTerminationCause, ProxyTerminationCoordinator};
    use super::{
        BODY_FRAME_CHANNEL_CAPACITY, BodyFrame, FrameDeliveryError, GuardedBodyStream,
        send_body_frame,
    };
    use crate::proxy::cancellation::ProxyCancellationContext;
    use tokio::sync::mpsc;
    use tokio::time::Instant;

    #[tokio::test]
    async fn body_frame_waits_for_downstream_ack_before_next_frame() {
        let cancellation = ProxyCancellationContext::new();
        let coordinator = ProxyTerminationCoordinator::default();
        let deadline = Instant::now() + Duration::from_secs(60);
        assert!(cancellation.set_total_deadline(deadline));
        let (sender, mut receiver) = mpsc::channel(BODY_FRAME_CHANNEL_CAPACITY);
        let sender_clone = sender.clone();
        let task = tokio::spawn(async move {
            send_body_frame(
                &sender_clone,
                Ok(bytes::Bytes::from_static(b"one")),
                &cancellation,
                &coordinator,
            )
            .await
        });
        let frame = receiver.recv().await.expect("frame should be buffered");
        assert!(!task.is_finished(), "worker must wait for downstream ack");
        let _ = frame.ack.send(());
        assert_eq!(task.await.unwrap(), Ok(()));
    }

    #[tokio::test]
    async fn total_deadline_still_terminates_when_downstream_ack_is_blocked() {
        let cancellation = ProxyCancellationContext::new();
        let coordinator = ProxyTerminationCoordinator::default();
        assert!(cancellation.set_total_deadline(Instant::now() + Duration::from_millis(20)));
        let (sender, mut receiver) = mpsc::channel(BODY_FRAME_CHANNEL_CAPACITY);
        let task_cancellation = cancellation.clone();
        let task_coordinator = coordinator.clone();
        let task = tokio::spawn(async move {
            send_body_frame(
                &sender,
                Ok(bytes::Bytes::from_static(b"blocked")),
                &task_cancellation,
                &task_coordinator,
            )
            .await
        });

        let _frame = receiver.recv().await.expect("frame should be delivered");
        let result = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("total deadline should terminate blocked downstream delivery")
            .unwrap();
        assert_eq!(
            result,
            Err(FrameDeliveryError::Timeout {
                phase: crate::proxy::TimeoutPhase::Total,
            })
        );
    }

    #[test]
    fn frame_capacity_is_fixed_to_one() {
        assert_eq!(BODY_FRAME_CHANNEL_CAPACITY, 1);
        let _: Option<BodyFrame> = None;
        let _: Option<FrameDeliveryError> = None;
        let _: Option<ProxyTerminationCause> = None;
        let _: Option<GuardedBodyStream> = None;
    }
}
