use std::time::Duration;

use tokio::time::timeout;

use crate::{
    config::NonStreamResponseConfig,
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
        cancellation::ProxyCancellationContext, classify_reqwest_error,
    },
    service::upstream_response::{
        CapturedErrorBody, CompleteResponseBody, UpstreamResponseReadError,
        capture_error_response_body, read_complete_response_body,
    },
};

pub(crate) async fn send_with_first_byte_timeout(
    cancellation: &ProxyCancellationContext,
    request: reqwest::RequestBuilder,
    context: &str,
    first_byte_timeout: Option<Duration>,
) -> Result<reqwest::Response, ProxyError> {
    if cancellation.is_cancelled() {
        return Err(cancellation
            .cancellation_error(ExecutionStage::Connect, ResponseVisibility::NotVisible)
            .await);
    }
    match first_byte_timeout {
        Some(timeout_duration) => {
            tokio::select! {
                _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
                    ExecutionStage::Connect,
                    ResponseVisibility::NotVisible,
                ).await),
                result = timeout(timeout_duration, request.send()) => match result {
                    Ok(result) => result.map_err(|err| classify_reqwest_error(
                        context,
                        &err,
                        ExecutionStage::Connect,
                        ResponseVisibility::NotVisible,
                    )),
                    Err(_) => Err(ProxyError::gateway(
                        ProxyErrorCode::UpstreamTimeoutError,
                        ExecutionStage::Connect,
                        ResponseVisibility::NotVisible,
                        None,
                        format!(
                            "{context} timed out waiting for the first upstream byte after {:?}",
                            timeout_duration
                        ),
                    )),
                }
            }
        }
        None => {
            tokio::select! {
                _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
                    ExecutionStage::Connect,
                    ResponseVisibility::NotVisible,
                ).await),
                result = request.send() => result.map_err(|err| classify_reqwest_error(
                    context,
                    &err,
                    ExecutionStage::Connect,
                    ResponseVisibility::NotVisible,
                )),
            }
        }
    }
}

fn classify_response_read_error(
    context: &'static str,
    error: UpstreamResponseReadError,
) -> ProxyError {
    let code = if error.is_timeout() {
        ProxyErrorCode::UpstreamTimeoutError
    } else {
        ProxyErrorCode::UpstreamResponseError
    };
    ProxyError::gateway(
        code,
        ExecutionStage::UpstreamResponse,
        ResponseVisibility::NotVisible,
        None,
        format!("{context}: {error}"),
    )
}

pub(super) async fn read_complete_response_with_cancellation(
    response: reqwest::Response,
    cancellation: &ProxyCancellationContext,
    limits: &NonStreamResponseConfig,
) -> Result<CompleteResponseBody, ProxyError> {
    if cancellation.is_cancelled() {
        return Err(cancellation
            .cancellation_error(
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
            )
            .await);
    }
    tokio::select! {
        _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
        ).await),
        result = read_complete_response_body(response, limits) => result.map_err(|error| {
            classify_response_read_error("Failed to read complete upstream response", error)
        }),
    }
}

pub(super) async fn capture_error_response_with_cancellation(
    response: reqwest::Response,
    cancellation: &ProxyCancellationContext,
    limits: &NonStreamResponseConfig,
    disclosure_limit: usize,
) -> Result<CapturedErrorBody, ProxyError> {
    if cancellation.is_cancelled() {
        return Err(cancellation
            .cancellation_error(
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
            )
            .await);
    }
    tokio::select! {
        _ = cancellation.cancelled() => Err(cancellation.cancellation_error(
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
        ).await),
        result = capture_error_response_body(response, limits, disclosure_limit) => {
            result.map_err(|error| {
                classify_response_read_error("Failed to capture upstream error response", error)
            })
        },
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{read_complete_response_with_cancellation, send_with_first_byte_timeout};
    use crate::config::NonStreamResponseConfig;
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, cancellation::ProxyCancellationContext,
    };
    use axum::http::StatusCode;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        sync::oneshot,
        time::timeout,
    };

    #[tokio::test]
    async fn malformed_reqwest_request_is_classified_as_upstream_request_error() {
        let cancellation = ProxyCancellationContext::new();
        let request = reqwest::Client::new().post("http://[::1");

        let error = send_with_first_byte_timeout(
            &cancellation,
            request,
            "malformed upstream request",
            None,
        )
        .await
        .expect_err("malformed request must fail before connecting");

        assert_eq!(error.code(), ProxyErrorCode::UpstreamRequestError);
        assert_eq!(error.stage(), ExecutionStage::Connect);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert!(error.upstream_error().is_none());
        assert_eq!(
            error.client_payload().public_message(),
            "The gateway could not send the request to the upstream provider."
        );
    }

    #[tokio::test]
    async fn bounded_response_read_preserves_reqwest_total_timeout_classification() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (release_tx, release_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\na")
                .await
                .unwrap();
            let _ = release_rx.await;
            drop(socket);
        });
        let response = reqwest::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .unwrap()
            .get(format!("http://{address}/slow-body"))
            .send()
            .await
            .unwrap();

        let error = read_complete_response_with_cancellation(
            response,
            &ProxyCancellationContext::new(),
            &NonStreamResponseConfig {
                raw_body_limit_bytes: 1024,
                decoded_body_limit_bytes: 1024,
            },
        )
        .await
        .err()
        .expect("the configured reqwest total timeout must interrupt the body read");
        assert_eq!(error.code(), ProxyErrorCode::UpstreamTimeoutError);
        assert_eq!(error.code().as_str(), "upstream_timeout_error");
        assert_eq!(error.status_code(), StatusCode::GATEWAY_TIMEOUT);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);

        let _ = release_tx.send(());
    }

    #[tokio::test]
    async fn cancellation_drops_in_progress_bounded_response_read_and_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let (socket_dropped_tx, socket_dropped_rx) = oneshot::channel();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n3\r\nabc\r\n",
                )
                .await
                .unwrap();
            let mut byte = [0u8; 1];
            let closed = socket.read(&mut byte).await.unwrap() == 0;
            let _ = socket_dropped_tx.send(closed);
        });
        let response = reqwest::Client::new()
            .get(format!("http://{address}/hanging"))
            .send()
            .await
            .unwrap();
        let cancellation = ProxyCancellationContext::new();
        let cancellation_trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::task::yield_now().await;
            cancellation_trigger.cancel_now("client cancelled bounded read");
        });

        let error = read_complete_response_with_cancellation(
            response,
            &cancellation,
            &NonStreamResponseConfig {
                raw_body_limit_bytes: 1024,
                decoded_body_limit_bytes: 1024,
            },
        )
        .await
        .err()
        .expect("cancelled read must fail");
        assert_eq!(error.code(), ProxyErrorCode::ClientCancelledError);
        assert_eq!(error.stage(), ExecutionStage::UpstreamResponse);
        assert_eq!(error.response_visibility(), ResponseVisibility::NotVisible);
        assert!(
            timeout(Duration::from_secs(2), socket_dropped_rx)
                .await
                .expect("socket close should be observed")
                .expect("socket close signal should be sent")
        );
    }
}
