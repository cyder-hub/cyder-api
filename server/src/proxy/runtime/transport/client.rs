use std::time::Duration;

use axum::body::Bytes;
use tokio::time::timeout;

use crate::proxy::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
    cancellation::ProxyCancellationContext, classify_reqwest_error,
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

pub(super) async fn read_response_bytes_with_cancellation(
    response: reqwest::Response,
    context: &str,
    cancellation: &ProxyCancellationContext,
) -> Result<Bytes, ProxyError> {
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
        result = response.bytes() => result.map_err(|err| classify_reqwest_error(
            context,
            &err,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::NotVisible,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::send_with_first_byte_timeout;
    use crate::proxy::{
        ExecutionStage, ProxyErrorCode, ResponseVisibility, cancellation::ProxyCancellationContext,
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
}
