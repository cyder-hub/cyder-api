use std::time::Duration;

use bytes::Bytes;
use reqwest::{RequestBuilder, Response};
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::time::{Instant, timeout_at};

use super::upstream_response::{
    CompleteResponseBody, UpstreamHttpErrorKind, UpstreamResponseReadError,
    read_complete_response_body,
};

#[derive(Debug, Error)]
pub(crate) enum AuxiliaryHttpError {
    #[error("auxiliary request timed out")]
    Timeout,
    #[error("auxiliary request failed ({kind})")]
    Send { kind: &'static str },
    #[error("auxiliary response body failed: {0}")]
    Body(UpstreamResponseReadError),
    #[error("auxiliary response JSON was invalid")]
    Parse,
}

impl AuxiliaryHttpError {
    pub(crate) fn safe_kind(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Send { kind } => kind,
            Self::Body(error) if error.is_timeout() => "timeout",
            Self::Body(_) => "body",
            Self::Parse => "invalid_json",
        }
    }

    pub(crate) fn with_context(&self, context: &'static str) -> String {
        match self {
            Self::Body(error) => format!("{context}: {error}"),
            _ => format!("{context} ({})", self.safe_kind()),
        }
    }
}

#[derive(Debug)]
pub(crate) struct AuxiliaryResponse {
    response: Response,
    deadline: Instant,
}

impl AuxiliaryResponse {
    pub(crate) fn from_parts(response: Response, deadline: Instant) -> Self {
        Self { response, deadline }
    }

    pub(crate) fn into_parts(self) -> (Response, Instant) {
        (self.response, self.deadline)
    }

    pub(crate) fn status(&self) -> reqwest::StatusCode {
        self.response.status()
    }

    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Send an auxiliary request with one total deadline shared by the send and
/// the subsequent response-body read.
pub(crate) async fn send_auxiliary_request(
    request: RequestBuilder,
    total_timeout: Duration,
) -> Result<AuxiliaryResponse, AuxiliaryHttpError> {
    let deadline = Instant::now() + total_timeout;
    let response = match timeout_at(deadline, request.send()).await {
        Ok(Ok(response)) => response,
        Ok(Err(error)) => {
            return Err(AuxiliaryHttpError::Send {
                kind: UpstreamHttpErrorKind::from_reqwest(&error).as_str(),
            });
        }
        Err(_) => return Err(AuxiliaryHttpError::Timeout),
    };

    Ok(AuxiliaryResponse { response, deadline })
}

/// Read every auxiliary response body under the deadline started by
/// `send_auxiliary_request`.
pub(crate) async fn read_auxiliary_response_body(
    auxiliary_response: AuxiliaryResponse,
    limits: &crate::config::NonStreamResponseConfig,
) -> Result<CompleteResponseBody, AuxiliaryHttpError> {
    let (response, deadline) = auxiliary_response.into_parts();
    match timeout_at(deadline, read_complete_response_body(response, limits)).await {
        Ok(Ok(body)) => Ok(body),
        Ok(Err(error)) => Err(AuxiliaryHttpError::Body(error)),
        Err(_) => Err(AuxiliaryHttpError::Timeout),
    }
}

/// Parse an auxiliary JSON body without allowing synchronous deserialization
/// to run past the same request deadline used for send and body read.
pub(crate) async fn parse_auxiliary_json<T>(
    body: Bytes,
    deadline: Instant,
) -> Result<T, AuxiliaryHttpError>
where
    T: DeserializeOwned + Send + 'static,
{
    let parse_task = tokio::task::spawn_blocking(move || serde_json::from_slice::<T>(&body));
    match timeout_at(deadline, parse_task).await {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(_))) | Ok(Err(_)) => Err(AuxiliaryHttpError::Parse),
        Err(_) => Err(AuxiliaryHttpError::Timeout),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::{
        io::AsyncWriteExt,
        net::TcpListener,
        time::{sleep, timeout},
    };

    use super::{AuxiliaryHttpError, read_auxiliary_response_body, send_auxiliary_request};
    use crate::config::NonStreamResponseConfig;

    #[tokio::test]
    async fn auxiliary_total_timeout_covers_a_body_that_stalls_after_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = tokio::io::AsyncReadExt::read(&mut socket, &mut request).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\nfirst\r\n")
                .await
                .unwrap();
            sleep(Duration::from_secs(2)).await;
        });

        let auxiliary_response = send_auxiliary_request(
            reqwest::Client::new().get(format!("http://{address}/models")),
            Duration::from_secs(1),
        )
        .await
        .expect("headers should arrive before the auxiliary deadline");
        let result = timeout(
            Duration::from_secs(2),
            read_auxiliary_response_body(
                auxiliary_response,
                &NonStreamResponseConfig {
                    raw_body_limit_bytes: 1024,
                    decoded_body_limit_bytes: 1024,
                },
            ),
        )
        .await
        .expect("body read should be bounded");

        assert!(matches!(result, Err(AuxiliaryHttpError::Timeout)));
    }
}
