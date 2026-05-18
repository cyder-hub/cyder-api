use axum::{
    Json,
    body::Bytes,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use std::sync::{LazyLock, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod acl;
pub mod auth;
pub mod sse;
pub mod storage;
pub mod usage;

#[derive(Debug, Serialize)]
pub struct HttpResult<T> {
    pub code: usize,
    pub data: T,
}

impl<T> HttpResult<T> {
    pub fn new(data: T) -> HttpResult<T> {
        HttpResult { code: 0, data }
    }
}

impl<T> IntoResponse for HttpResult<T>
where
    T: Serialize,
{
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

pub fn split_chunks(input: Bytes) -> (Vec<Bytes>, Bytes) {
    let mut lines = Vec::new();
    let mut start = 0;

    while let Some(pos) = input[start..].iter().position(|&b| b == b'\n') {
        let end = start + pos;
        lines.push(input.slice(start..end));
        start = end + 1; // Move past the newline character
    }

    let remainder = if start < input.len() {
        input.slice(start..)
    } else {
        Bytes::new()
    };

    (lines, remainder)
}

const ID_START_TIMESTAMP: u64 = 1_716_257_820_000;
const ID_WORKER_BIT: u32 = 5;
const ID_SEQUENCE_BIT: u32 = 6;
const ID_MAX_SEQUENCE: u64 = 1_u64 << ID_SEQUENCE_BIT;
pub const ID_MAX_WORKER_ID: u64 = (1_u64 << ID_WORKER_BIT) - 1;
const ID_WORKER_LEFT: u32 = ID_SEQUENCE_BIT;
const ID_TIMESTAMP_LEFT: u32 = ID_WORKER_LEFT + ID_WORKER_BIT;
const ID_SEQUENCE_MASK: u64 = ID_MAX_SEQUENCE - 1;

pub struct IdGenerator {
    worker_id: u64,
    sequence: Mutex<u64>,
    last_timestamp: Mutex<u64>,
}

impl IdGenerator {
    fn new(worker_id: u64) -> Self {
        assert!(
            worker_id <= ID_MAX_WORKER_ID,
            "id worker_id must be in 0..={ID_MAX_WORKER_ID}"
        );
        Self {
            worker_id,
            sequence: Mutex::new(0),
            last_timestamp: Mutex::new(0),
        }
    }

    pub fn generate_id(&self) -> i64 {
        self.generate_id_with_timestamp(Self::current_timestamp(), Self::wait_next_millis)
    }

    fn generate_id_with_timestamp(
        &self,
        mut timestamp: u64,
        wait_next_millis: impl FnOnce(u64) -> u64,
    ) -> i64 {
        let mut sequence = self.sequence.lock().unwrap();
        let mut last_timestamp = self.last_timestamp.lock().unwrap();

        if timestamp < *last_timestamp {
            panic!("system clock moved backwards while generating an id");
        }

        if timestamp == *last_timestamp {
            if *sequence == ID_SEQUENCE_MASK {
                timestamp = wait_next_millis(*last_timestamp);
                *sequence = 0;
            } else {
                *sequence += 1;
            }
        } else {
            *sequence = 0;
        }

        *last_timestamp = timestamp;
        (timestamp << ID_TIMESTAMP_LEFT | self.worker_id << ID_WORKER_LEFT | *sequence) as i64
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
            - ID_START_TIMESTAMP
    }

    fn wait_next_millis(last_timestamp: u64) -> u64 {
        let mut timestamp = Self::current_timestamp();
        while timestamp <= last_timestamp {
            timestamp = Self::current_timestamp();
        }
        timestamp
    }
}

pub static ID_GENERATOR: LazyLock<IdGenerator> =
    LazyLock::new(|| IdGenerator::new(crate::config::CONFIG.id.worker_id));

#[cfg(test)]
mod tests {
    use super::{
        ID_MAX_SEQUENCE, ID_MAX_WORKER_ID, ID_SEQUENCE_MASK, ID_TIMESTAMP_LEFT, ID_WORKER_LEFT,
        IdGenerator,
    };

    fn timestamp_part(id: i64) -> u64 {
        (id as u64) >> ID_TIMESTAMP_LEFT
    }

    fn worker_part(id: i64) -> u64 {
        ((id as u64) >> ID_WORKER_LEFT) & ID_MAX_WORKER_ID
    }

    fn sequence_part(id: i64) -> u64 {
        (id as u64) & ID_SEQUENCE_MASK
    }

    #[test]
    fn id_generator_preserves_time_worker_sequence_layout() {
        let generator = IdGenerator::new(7);

        let id =
            generator.generate_id_with_timestamp(42, |_| unreachable!("first id should not wait"));

        assert_eq!(timestamp_part(id), 42);
        assert_eq!(worker_part(id), 7);
        assert_eq!(sequence_part(id), 0);
    }

    #[test]
    fn id_generator_uses_all_sequence_values_before_waiting() {
        let generator = IdGenerator::new(1);
        let timestamp = 100;

        for expected_sequence in 0..ID_MAX_SEQUENCE {
            let id = generator.generate_id_with_timestamp(timestamp, |_| {
                unreachable!("sequence {expected_sequence} should fit in the current millisecond")
            });
            assert_eq!(timestamp_part(id), timestamp);
            assert_eq!(sequence_part(id), expected_sequence);
        }

        let rollover = generator.generate_id_with_timestamp(timestamp, |last_timestamp| {
            assert_eq!(last_timestamp, timestamp);
            last_timestamp + 1
        });

        assert_eq!(timestamp_part(rollover), timestamp + 1);
        assert_eq!(sequence_part(rollover), 0);
    }

    #[test]
    fn id_generator_rejects_out_of_range_worker_id() {
        let result = std::panic::catch_unwind(|| {
            IdGenerator::new(ID_MAX_WORKER_ID + 1);
        });

        assert!(result.is_err());
    }

    #[test]
    fn id_generator_static_uses_configured_worker_id() {
        let id = super::ID_GENERATOR.generate_id();

        assert_eq!(worker_part(id), crate::config::CONFIG.id.worker_id);
    }
}
