use serde::Serialize;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ResponseVisibility {
    NotVisible = 0,
    HeadersCommitted = 1,
    BodyStarted = 2,
    Unknown = 3,
}

impl ResponseVisibility {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 4] = [
        Self::NotVisible,
        Self::HeadersCommitted,
        Self::BodyStarted,
        Self::Unknown,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::NotVisible => "not_visible",
            Self::HeadersCommitted => "headers_committed",
            Self::BodyStarted => "body_started",
            Self::Unknown => "unknown",
        }
    }

    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::NotVisible,
            1 => Self::HeadersCommitted,
            2 => Self::BodyStarted,
            3 => Self::Unknown,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ResponseVisibilityTracker {
    state: Arc<AtomicU8>,
}

impl Default for ResponseVisibilityTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ResponseVisibilityTracker {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(AtomicU8::new(ResponseVisibility::NotVisible as u8)),
        }
    }

    pub(crate) fn current(&self) -> ResponseVisibility {
        ResponseVisibility::from_u8(self.state.load(Ordering::Acquire))
    }

    pub(crate) fn advance_to(&self, target: ResponseVisibility) -> ResponseVisibility {
        let target = target as u8;
        let mut current = self.state.load(Ordering::Acquire);
        while current < target {
            match self.state.compare_exchange_weak(
                current,
                target,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return ResponseVisibility::from_u8(target),
                Err(actual) => current = actual,
            }
        }
        ResponseVisibility::from_u8(current)
    }
}

#[cfg(test)]
mod tests {
    use super::{ResponseVisibility, ResponseVisibilityTracker};
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn visibility_values_are_stable_and_monotonic() {
        assert_eq!(ResponseVisibility::ALL.len(), 4);
        for (index, visibility) in ResponseVisibility::ALL.into_iter().enumerate() {
            assert_eq!(visibility as usize, index);
            assert_eq!(
                serde_json::to_value(visibility).expect("visibility should serialize"),
                visibility.as_str()
            );
        }
    }

    #[test]
    fn tracker_advances_idempotently_and_never_regresses() {
        let tracker = ResponseVisibilityTracker::new();
        assert_eq!(tracker.current(), ResponseVisibility::NotVisible);
        assert_eq!(
            tracker.advance_to(ResponseVisibility::HeadersCommitted),
            ResponseVisibility::HeadersCommitted
        );
        assert_eq!(
            tracker.advance_to(ResponseVisibility::NotVisible),
            ResponseVisibility::HeadersCommitted
        );
        assert_eq!(
            tracker.advance_to(ResponseVisibility::BodyStarted),
            ResponseVisibility::BodyStarted
        );
        assert_eq!(
            tracker.advance_to(ResponseVisibility::BodyStarted),
            ResponseVisibility::BodyStarted
        );
    }

    #[test]
    fn concurrent_advances_converge_on_the_highest_visibility() {
        let tracker = Arc::new(ResponseVisibilityTracker::new());
        let targets = [
            ResponseVisibility::NotVisible,
            ResponseVisibility::HeadersCommitted,
            ResponseVisibility::BodyStarted,
            ResponseVisibility::HeadersCommitted,
        ];
        let handles = targets.map(|target| {
            let tracker = Arc::clone(&tracker);
            thread::spawn(move || tracker.advance_to(target))
        });
        for handle in handles {
            handle.join().expect("visibility worker should finish");
        }
        assert_eq!(tracker.current(), ResponseVisibility::BodyStarted);
    }
}
