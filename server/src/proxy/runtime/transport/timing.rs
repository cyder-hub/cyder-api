use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use bytes::Bytes;
use tokio::time::Instant;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TimingSnapshot {
    pub upstream_request_sent_at: Option<i64>,
    pub response_headers_at: Option<i64>,
    pub upstream_first_raw_body_at: Option<i64>,
    pub first_response_body_at: Option<i64>,
    pub first_token_at: Option<i64>,
    pub max_upstream_response_idle_ms: Option<i64>,
}

#[derive(Debug, Default)]
struct TimingState {
    snapshot: TimingSnapshot,
    upstream_request_sent_mono: Option<Instant>,
    response_headers_mono: Option<Instant>,
    upstream_first_raw_body_mono: Option<Instant>,
    first_response_body_mono: Option<Instant>,
    first_token_mono: Option<Instant>,
    active_idle_wait_ms: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct TransportTimingState {
    state: Arc<Mutex<TimingState>>,
}

impl TransportTimingState {
    pub(crate) fn mark_upstream_request_sent(&self, wall_clock_ms: i64, now: Instant) -> bool {
        let mut state = self.lock();
        if state.snapshot.upstream_request_sent_at.is_some() {
            return false;
        }
        state.snapshot.upstream_request_sent_at = Some(wall_clock_ms);
        state.upstream_request_sent_mono = Some(now);
        true
    }

    pub(crate) fn mark_response_headers_received(&self, wall_clock_ms: i64, now: Instant) -> bool {
        let mut state = self.lock();
        if state.snapshot.response_headers_at.is_some() {
            return false;
        }
        state.snapshot.response_headers_at = Some(wall_clock_ms);
        state.response_headers_mono = Some(now);
        true
    }

    pub(crate) fn observe_upstream_raw_chunk(
        &self,
        chunk: &Bytes,
        wall_clock_ms: i64,
        wait_started_at: Instant,
        received_at: Instant,
    ) -> bool {
        let mut state = self.lock();
        let active_wait_ms = duration_ms(received_at.saturating_duration_since(wait_started_at));
        if state.snapshot.upstream_first_raw_body_at.is_some() {
            state.active_idle_wait_ms = state.active_idle_wait_ms.saturating_add(active_wait_ms);
        }
        if chunk.is_empty() {
            return false;
        }

        let is_first = state.snapshot.upstream_first_raw_body_at.is_none();
        if is_first {
            state.snapshot.upstream_first_raw_body_at = Some(wall_clock_ms);
            state.upstream_first_raw_body_mono = Some(received_at);
        } else {
            state.snapshot.max_upstream_response_idle_ms = Some(
                state
                    .snapshot
                    .max_upstream_response_idle_ms
                    .unwrap_or_default()
                    .max(state.active_idle_wait_ms),
            );
        }
        state.active_idle_wait_ms = 0;
        is_first
    }

    pub(crate) fn mark_first_response_body(&self, wall_clock_ms: i64, now: Instant) -> bool {
        let mut state = self.lock();
        if state.snapshot.first_response_body_at.is_some() {
            return false;
        }
        state.snapshot.first_response_body_at = Some(wall_clock_ms);
        state.first_response_body_mono = Some(now);
        true
    }

    pub(crate) fn mark_first_token(&self, wall_clock_ms: i64, now: Instant) -> bool {
        let mut state = self.lock();
        if state.snapshot.first_token_at.is_some() {
            return false;
        }
        state.snapshot.first_token_at = Some(wall_clock_ms);
        state.first_token_mono = Some(now);
        true
    }

    pub(crate) fn time_to_first_token_ms(&self) -> Option<i64> {
        let state = self.lock();
        state
            .upstream_request_sent_mono
            .zip(state.first_token_mono)
            .map(|(sent, token)| duration_ms(token.saturating_duration_since(sent)))
    }

    pub(crate) fn snapshot(&self) -> TimingSnapshot {
        self.lock().snapshot.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, TimingState> {
        self.state
            .lock()
            .expect("transport timing state lock poisoned")
    }
}

fn duration_ms(duration: Duration) -> i64 {
    duration.as_millis().min(i64::MAX as u128) as i64
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bytes::Bytes;
    use tokio::time::Instant;

    use super::TransportTimingState;

    #[test]
    fn timing_state_is_set_once_and_tracks_the_largest_active_read_gap() {
        let state = TransportTimingState::default();
        let base = Instant::now();

        assert!(state.mark_upstream_request_sent(100, base));
        assert!(!state.mark_upstream_request_sent(200, base + Duration::from_secs(1)));
        assert!(state.mark_response_headers_received(110, base + Duration::from_millis(10)));
        let chunk = Bytes::from_static(b"chunk");
        assert!(state.observe_upstream_raw_chunk(
            &chunk,
            120,
            base + Duration::from_millis(10),
            base + Duration::from_millis(20),
        ));
        assert!(!state.observe_upstream_raw_chunk(
            &chunk,
            130,
            base + Duration::from_millis(30),
            base + Duration::from_millis(45),
        ));
        assert!(!state.observe_upstream_raw_chunk(
            &chunk,
            140,
            base + Duration::from_millis(60),
            base + Duration::from_millis(70),
        ));

        let snapshot = state.snapshot();
        assert_eq!(snapshot.upstream_request_sent_at, Some(100));
        assert_eq!(snapshot.response_headers_at, Some(110));
        assert_eq!(snapshot.upstream_first_raw_body_at, Some(120));
        assert_eq!(snapshot.max_upstream_response_idle_ms, Some(15));
    }

    #[test]
    fn active_idle_excludes_processing_between_raw_chunks_and_empty_chunks_accumulate() {
        let state = TransportTimingState::default();
        let base = Instant::now();
        let chunk = Bytes::from_static(b"chunk");
        let empty = Bytes::new();

        assert!(state.observe_upstream_raw_chunk(
            &chunk,
            100,
            base,
            base + Duration::from_millis(1),
        ));
        assert!(!state.observe_upstream_raw_chunk(
            &empty,
            101,
            base + Duration::from_millis(101),
            base + Duration::from_millis(111),
        ));
        assert!(!state.observe_upstream_raw_chunk(
            &chunk,
            120,
            base + Duration::from_millis(211),
            base + Duration::from_millis(221),
        ));

        assert_eq!(state.snapshot().max_upstream_response_idle_ms, Some(20));
    }

    #[test]
    fn first_response_body_and_token_are_independent_set_once_facts() {
        let state = TransportTimingState::default();
        let base = Instant::now();

        assert!(state.mark_upstream_request_sent(1_000, base));
        assert!(state.mark_first_response_body(1_120, base + Duration::from_millis(120)));
        assert!(!state.mark_first_response_body(1_130, base + Duration::from_millis(130)));
        assert!(state.mark_first_token(1_150, base + Duration::from_millis(150)));
        assert!(!state.mark_first_token(1_160, base + Duration::from_millis(160)));

        let snapshot = state.snapshot();
        assert_eq!(snapshot.first_response_body_at, Some(1_120));
        assert_eq!(snapshot.first_token_at, Some(1_150));
        assert_eq!(state.time_to_first_token_ms(), Some(150));
    }
}
