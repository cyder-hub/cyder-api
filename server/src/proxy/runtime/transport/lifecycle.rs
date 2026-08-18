use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use tokio::{
    select,
    time::{Instant, sleep_until},
};
use tokio_util::sync::CancellationToken;

use crate::proxy::{ProxyError, ProxyErrorCode, TimeoutPhase};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxyTerminationCause {
    ClientCancelled,
    Timeout { phase: TimeoutPhase },
    UpstreamError,
    DownstreamError,
    Success,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(test)]
pub(crate) enum ReadyTerminationSignal {
    ClientCancelled,
    PhaseTimeout(TimeoutPhase),
    TotalTimeout,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TotalWatchdogResult {
    Expired,
    Cancelled,
}

#[derive(Clone, Debug)]
pub(crate) struct TotalWatchdog {
    token: CancellationToken,
    deadline: Instant,
}

impl TotalWatchdog {
    fn new(deadline: Instant) -> Self {
        Self {
            token: CancellationToken::new(),
            deadline,
        }
    }

    pub(crate) fn cancel(&self) {
        self.token.cancel();
    }

    pub(crate) async fn wait(&self) -> TotalWatchdogResult {
        select! {
            biased;
            _ = self.token.cancelled() => TotalWatchdogResult::Cancelled,
            _ = sleep_until(self.deadline) => TotalWatchdogResult::Expired,
        }
    }

    #[cfg(test)]
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
}

#[derive(Debug, Default)]
struct CoordinatorState {
    terminal: Option<ProxyTerminationCause>,
    request_log_claimed: bool,
    lease_release_claimed: bool,
    total_watchdog: Option<TotalWatchdog>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ProxyTerminationCoordinator {
    state: Arc<Mutex<CoordinatorState>>,
}

impl ProxyTerminationCoordinator {
    pub(crate) fn try_terminate(&self, cause: ProxyTerminationCause) -> bool {
        let watchdog = {
            let mut state = self.lock();
            if state.terminal.is_some() {
                return false;
            }
            state.terminal = Some(cause);
            state.total_watchdog.clone()
        };
        if let Some(watchdog) = watchdog {
            watchdog.cancel();
        }
        true
    }

    pub(crate) fn terminal(&self) -> Option<ProxyTerminationCause> {
        self.lock().terminal
    }

    pub(crate) fn try_terminate_error(&self, error: &ProxyError) -> bool {
        let cause = match error.code() {
            ProxyErrorCode::ClientCancelledError => ProxyTerminationCause::ClientCancelled,
            ProxyErrorCode::UpstreamTimeoutError => ProxyTerminationCause::Timeout {
                phase: error.timeout_phase().unwrap_or(TimeoutPhase::Total),
            },
            ProxyErrorCode::DownstreamSendError => ProxyTerminationCause::DownstreamError,
            _ => ProxyTerminationCause::UpstreamError,
        };
        self.try_terminate(cause)
    }

    pub(crate) fn claim_request_log(&self) -> bool {
        let mut state = self.lock();
        if state.request_log_claimed {
            return false;
        }
        state.request_log_claimed = true;
        true
    }

    pub(crate) fn claim_lease_release(&self) -> bool {
        let mut state = self.lock();
        if state.lease_release_claimed {
            return false;
        }
        state.lease_release_claimed = true;
        true
    }

    pub(crate) fn arm_total_watchdog(&self, deadline: Instant) -> TotalWatchdog {
        let (watchdog, terminal) = {
            let mut state = self.lock();
            if let Some(watchdog) = &state.total_watchdog {
                return watchdog.clone();
            }
            let watchdog = TotalWatchdog::new(deadline);
            let terminal = state.terminal.is_some();
            state.total_watchdog = Some(watchdog.clone());
            (watchdog, terminal)
        };
        if terminal {
            watchdog.cancel();
        }
        watchdog
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, CoordinatorState> {
        self.state
            .lock()
            .expect("proxy termination coordinator lock poisoned")
    }
}

/// Keep terminal cleanup alive when the total watchdog fires. The cleanup
/// owner must observe the deadline and commit its terminal cause, but it must
/// not be aborted: request-log and lease finalizers are one-shot operations.
pub(crate) async fn await_cleanup_with_total_watchdog<F>(
    cancellation: &crate::proxy::cancellation::ProxyCancellationContext,
    coordinator: &ProxyTerminationCoordinator,
    cleanup: F,
) where
    F: Future<Output = ()>,
{
    let watchdog = coordinator.arm_total_watchdog(
        cancellation
            .total_deadline()
            .expect("proxy total deadline must be initialized before cleanup supervision"),
    );
    tokio::pin!(cleanup);
    tokio::select! {
        biased;
        _ = cancellation.cancelled() => {
            coordinator.try_terminate(ProxyTerminationCause::ClientCancelled);
            cleanup.await;
        }
        () = &mut cleanup => {}
        watchdog_result = watchdog.wait() => {
            if matches!(watchdog_result, TotalWatchdogResult::Expired) {
                coordinator.try_terminate(ProxyTerminationCause::Timeout {
                    phase: TimeoutPhase::Total,
                });
            }
            cleanup.await;
        }
    }
}

#[cfg(test)]
pub(crate) fn choose_ready_termination(
    client_cancelled: bool,
    phase_timeout: Option<TimeoutPhase>,
    total_timeout: bool,
) -> Option<ReadyTerminationSignal> {
    if client_cancelled {
        Some(ReadyTerminationSignal::ClientCancelled)
    } else if let Some(phase) = phase_timeout {
        Some(ReadyTerminationSignal::PhaseTimeout(phase))
    } else if total_timeout {
        Some(ReadyTerminationSignal::TotalTimeout)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    use tokio::time::{Instant, sleep};

    use super::{
        ProxyTerminationCause, ProxyTerminationCoordinator, ReadyTerminationSignal,
        TotalWatchdogResult, choose_ready_termination,
    };
    use crate::proxy::TimeoutPhase;

    #[test]
    fn biased_termination_priority_is_client_then_phase_then_total() {
        assert_eq!(
            choose_ready_termination(true, Some(TimeoutPhase::FirstByte), true),
            Some(ReadyTerminationSignal::ClientCancelled)
        );
        assert_eq!(
            choose_ready_termination(false, Some(TimeoutPhase::ResponseIdle), true),
            Some(ReadyTerminationSignal::PhaseTimeout(
                TimeoutPhase::ResponseIdle
            ))
        );
        assert_eq!(
            choose_ready_termination(false, None, true),
            Some(ReadyTerminationSignal::TotalTimeout)
        );
        assert_eq!(choose_ready_termination(false, None, false), None);
    }

    #[test]
    fn all_timeout_phases_are_accepted_as_typed_phase_signals() {
        for phase in TimeoutPhase::ALL {
            assert_eq!(
                choose_ready_termination(false, Some(phase), false),
                Some(ReadyTerminationSignal::PhaseTimeout(phase))
            );
        }
    }

    #[tokio::test]
    async fn coordinator_is_first_writer_wins_and_finalizers_are_idempotent() {
        let coordinator = ProxyTerminationCoordinator::default();
        assert!(coordinator.try_terminate(ProxyTerminationCause::Success));
        assert!(!coordinator.try_terminate(ProxyTerminationCause::Timeout {
            phase: TimeoutPhase::Total,
        }));
        assert_eq!(coordinator.terminal(), Some(ProxyTerminationCause::Success));

        assert!(coordinator.claim_request_log());
        assert!(!coordinator.claim_request_log());
        assert!(coordinator.claim_lease_release());
        assert!(!coordinator.claim_lease_release());
    }

    #[tokio::test]
    async fn terminal_cancels_the_owned_total_watchdog_without_a_background_sleep() {
        let coordinator = ProxyTerminationCoordinator::default();
        let deadline = Instant::now() + Duration::from_secs(60);
        let watchdog = coordinator.arm_total_watchdog(deadline);
        assert_eq!(watchdog.deadline(), deadline);
        assert!(coordinator.try_terminate(ProxyTerminationCause::ClientCancelled));
        assert_eq!(watchdog.wait().await, TotalWatchdogResult::Cancelled);
    }

    #[tokio::test]
    async fn total_deadline_does_not_abort_cleanup_owner() {
        let cancellation = crate::proxy::cancellation::ProxyCancellationContext::new();
        assert!(cancellation.set_total_deadline(Instant::now() + Duration::from_millis(10)));
        let coordinator = ProxyTerminationCoordinator::default();
        let completed = Arc::new(AtomicBool::new(false));
        let completed_by_cleanup = Arc::clone(&completed);

        super::await_cleanup_with_total_watchdog(&cancellation, &coordinator, async move {
            sleep(Duration::from_millis(30)).await;
            completed_by_cleanup.store(true, Ordering::SeqCst);
        })
        .await;

        assert!(completed.load(Ordering::SeqCst));
        assert_eq!(
            coordinator.terminal(),
            Some(ProxyTerminationCause::Timeout {
                phase: TimeoutPhase::Total,
            })
        );
    }
}
