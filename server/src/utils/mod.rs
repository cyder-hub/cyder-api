use axum::{
    Json,
    body::Bytes,
    response::{IntoResponse, Response},
};
use cyder_tools::snow_flake::Snowflake;
use serde::Serialize;
use std::sync::LazyLock;

pub mod auth;
pub mod billing;
pub mod limit;
pub mod sse;
pub mod storage;

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

pub static ID_GENERATOR: LazyLock<Snowflake> = LazyLock::new(|| Snowflake::new(1));
