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
const ID_WORKER_LEFT: u32 = ID_SEQUENCE_BIT;
const ID_TIMESTAMP_LEFT: u32 = ID_WORKER_LEFT + ID_WORKER_BIT;
const ID_SEQUENCE_MASK: u64 = (1 << (ID_SEQUENCE_BIT + 1)) - 1;

pub struct IdGenerator {
    worker_id: u64,
    sequence: Mutex<u64>,
    last_timestamp: Mutex<u64>,
}

impl IdGenerator {
    const fn new(worker_id: u64) -> Self {
        Self {
            worker_id,
            sequence: Mutex::new(0),
            last_timestamp: Mutex::new(0),
        }
    }

    pub fn generate_id(&self) -> i64 {
        let mut sequence = self.sequence.lock().unwrap();
        let mut last_timestamp = self.last_timestamp.lock().unwrap();
        let mut timestamp = Self::current_timestamp();

        if timestamp < *last_timestamp {
            panic!("system clock moved backwards while generating an id");
        }

        if timestamp == *last_timestamp {
            *sequence = (*sequence + 1) & ID_SEQUENCE_MASK;
            if *sequence == ID_MAX_SEQUENCE {
                timestamp = Self::wait_next_millis(*last_timestamp);
                *sequence = 0;
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

pub static ID_GENERATOR: LazyLock<IdGenerator> = LazyLock::new(|| IdGenerator::new(1));
