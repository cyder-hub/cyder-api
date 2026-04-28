use axum::body::Bytes;
use tokio::time::timeout;

use crate::{
    config::CONFIG,
    proxy::{ProxyError, cancellation::ProxyCancellationContext, classify_reqwest_error},
};

pub(crate) async fn send_with_first_byte_timeout(
    cancellation: &ProxyCancellationContext,
    request: reqwest::RequestBuilder,
    context: &str,
) -> Result<reqwest::Response, ProxyError> {
    if cancellation.is_cancelled() {
        return Err(cancellation.cancellation_error().await);
    }
    match CONFIG.proxy_request.first_byte_timeout() {
        Some(timeout_duration) => {
            tokio::select! {
                _ = cancellation.cancelled() => Err(cancellation.cancellation_error().await),
                result = timeout(timeout_duration, request.send()) => match result {
                    Ok(result) => result.map_err(|err| classify_reqwest_error(context, &err)),
                    Err(_) => Err(ProxyError::UpstreamTimeout(format!(
                        "{context} timed out waiting for the first upstream byte after {:?}",
                        timeout_duration
                    ))),
                }
            }
        }
        None => {
            tokio::select! {
                _ = cancellation.cancelled() => Err(cancellation.cancellation_error().await),
                result = request.send() => result.map_err(|err| classify_reqwest_error(context, &err)),
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
        return Err(cancellation.cancellation_error().await);
    }
    tokio::select! {
        _ = cancellation.cancelled() => Err(cancellation.cancellation_error().await),
        result = response.bytes() => result.map_err(|err| classify_reqwest_error(context, &err)),
    }
}
