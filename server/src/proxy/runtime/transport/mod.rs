pub(crate) mod body;
mod client;
pub(crate) mod lifecycle;
mod non_stream;
mod response;
mod stream;
pub(crate) mod timing;

#[cfg(test)]
pub(crate) mod test_support;

use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use chrono::Utc;
use reqwest::Method;
use tokio::sync::Mutex as TokioMutex;

pub(crate) use client::send_with_deadline;

use self::{non_stream::handle_non_streaming_response, stream::handle_streaming_response_guarded};
use crate::{
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility, ResponseVisibilityTracker,
        cancellation::{CancellationDropGuard, ProxyCancellationContext},
        logging::RequestLogContext,
        runtime::api_key_lease::ApiKeyRequestLeaseFinalizer,
    },
    schema::enum_def::{DownstreamProtocol, RequestStatus, UpstreamProtocol},
    service::{
        app_state::AppState, cache::types::CacheCostCatalogVersion,
        upstream_response::normalize_content_type,
    },
};

#[derive(Clone, Copy, Debug)]
pub(in crate::proxy) enum ProxyResponseMode {
    Generation {
        downstream_protocol: DownstreamProtocol,
        upstream_protocol: UpstreamProtocol,
    },
    Utility {
        downstream_protocol: DownstreamProtocol,
        upstream_protocol: UpstreamProtocol,
    },
}

impl ProxyResponseMode {
    fn protocols(self) -> (DownstreamProtocol, UpstreamProtocol) {
        match self {
            Self::Generation {
                downstream_protocol,
                upstream_protocol,
            }
            | Self::Utility {
                downstream_protocol,
                upstream_protocol,
            } => (downstream_protocol, upstream_protocol),
        }
    }
}

fn is_sse_response(status: StatusCode, headers: &HeaderMap) -> bool {
    status.is_success()
        && normalize_content_type(headers)
            .is_some_and(|content_type| content_type.essence == "text/event-stream")
}

pub(in crate::proxy) struct ProxyRequestOutcome {
    pub response: Response<Body>,
    pub log_context: RequestLogContext,
}

pub(in crate::proxy) struct ProxyRequestFailure {
    pub error: ProxyError,
    pub log_context: RequestLogContext,
}

fn finalize_send_failure_log_context(
    context: &mut RequestLogContext,
    url: &str,
    completed_at: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    proxy_error: &ProxyError,
) {
    context.request_url = Some(url.to_string());
    context.completed_at = Some(completed_at);
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = if proxy_error.code() == ProxyErrorCode::ClientCancelledError {
        RequestStatus::Cancelled
    } else {
        RequestStatus::Error
    };
}

// Builds the HTTP client, sends the request to the LLM, and passes the response to be handled.
pub(in crate::proxy) async fn send_materialized_request(
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    log_context: RequestLogContext,
    url: String,
    data: Bytes,
    headers: HeaderMap,
    model_str: String,
    use_proxy: bool,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    response_mode: ProxyResponseMode,
    response_visibility: ResponseVisibilityTracker,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let coordinator = cancellation.coordinator();
    let mut api_key_request_lease = api_key_request_lease.with_coordinator(coordinator.clone());
    let log_context = Arc::new(TokioMutex::new(log_context));
    log_context
        .lock()
        .await
        .set_completion_coordinator(coordinator.clone());
    log_context
        .lock()
        .await
        .attach_transport_timing(cancellation.timing());

    let client_bundle = app_state.infra.client_bundle().await;
    let proxy_timeouts = client_bundle.proxy_request.timeouts.clone();
    let upstream_error_body_limit_bytes =
        client_bundle.proxy_request.upstream_error_body_limit_bytes;
    let sse_response_limits = client_bundle.proxy_request.sse_response.clone();
    let client = match client_bundle.provider_client(use_proxy) {
        Ok(client) => client,
        Err(error) => {
            let proxy_error = ProxyError::gateway(
                ProxyErrorCode::ProviderConfigurationError,
                ExecutionStage::Materialize,
                ResponseVisibility::NotVisible,
                None,
                error.to_string(),
            );
            cancellation.try_terminate_error(&proxy_error);
            let completed_at = Utc::now().timestamp_millis();
            let failure_context = {
                let mut context = log_context.lock().await;
                finalize_send_failure_log_context(
                    &mut context,
                    &url,
                    completed_at,
                    cost_catalog_version.as_ref(),
                    &proxy_error,
                );
                context.clone()
            };
            api_key_request_lease.release().await;
            return Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: failure_context,
            });
        }
    };

    let (request_id, log_id) = {
        let context = log_context.lock().await;
        (context.request_id.clone(), context.id)
    };
    let mut drop_cancellation_guard = CancellationDropGuard::new(
        cancellation.clone(),
        request_id,
        log_id,
        response_visibility.clone(),
        ExecutionStage::Connect,
        format!("Client disconnected during proxy request for log_id {log_id}."),
    );

    let request_sent_at = Utc::now().timestamp_millis();
    cancellation
        .timing()
        .mark_upstream_request_sent(request_sent_at, tokio::time::Instant::now());
    let response = match send_with_deadline(
        &cancellation,
        client
            .request(Method::POST, &url)
            .headers(headers)
            .body(data),
        "LLM request",
        &proxy_timeouts,
    )
    .await
    {
        Ok(resp) => resp,
        Err(proxy_error) => {
            drop_cancellation_guard.disarm();
            cancellation.try_terminate_error(&proxy_error);
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            finalize_send_failure_log_context(
                &mut context,
                &url,
                completed_at,
                cost_catalog_version.as_ref(),
                &proxy_error,
            );
            api_key_request_lease.release().await;

            return Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: context.clone(),
            });
        }
    };
    let response_headers_at = Utc::now().timestamp_millis();
    cancellation
        .timing()
        .mark_response_headers_received(response_headers_at, tokio::time::Instant::now());
    drop_cancellation_guard.set_stage(ExecutionStage::UpstreamResponse);

    let is_sse = is_sse_response(response.status(), response.headers());
    {
        let mut context = log_context.lock().await;
        context.is_stream = is_sse;
    }

    let result = if is_sse {
        let (downstream_protocol, upstream_protocol) = response_mode.protocols();
        match handle_streaming_response_guarded(
            &app_state,
            cancellation.clone(),
            log_context.clone(),
            model_str,
            response,
            &url,
            cost_catalog_version,
            api_key_request_lease,
            downstream_protocol,
            upstream_protocol,
            proxy_timeouts.clone(),
            sse_response_limits,
            response_visibility.clone(),
        )
        .await
        {
            Ok(response) => {
                let log_context = log_context.lock().await.clone();
                Ok(ProxyRequestOutcome {
                    response,
                    log_context,
                })
            }
            Err(error) => Err(ProxyRequestFailure {
                error,
                log_context: log_context.lock().await.clone(),
            }),
        }
    } else {
        handle_non_streaming_response(
            &cancellation,
            log_context,
            model_str,
            response,
            &url,
            cost_catalog_version.as_ref(),
            api_key_request_lease,
            response_mode,
            upstream_error_body_limit_bytes,
            &client_bundle.proxy_request.non_stream_response,
            response_visibility,
            &proxy_timeouts,
        )
        .await
    };
    drop_cancellation_guard.disarm();
    result
}

#[cfg(test)]
mod tests {
    use axum::http::{HeaderMap, HeaderValue, StatusCode, header::CONTENT_TYPE};

    use super::is_sse_response;

    #[test]
    fn sse_detection_requires_success_and_exact_normalized_media_essence() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("Text/Event-Stream; Charset=UTF-8"),
        );
        assert!(is_sse_response(StatusCode::OK, &headers));
        assert!(!is_sse_response(StatusCode::BAD_REQUEST, &headers));

        for value in [
            "text/event-streamish",
            "application/text/event-stream",
            "text/event-stream; charset=invalid charset",
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(CONTENT_TYPE, HeaderValue::from_str(value).unwrap());
            assert!(!is_sse_response(StatusCode::OK, &headers), "{value}");
        }

        let mut duplicate = HeaderMap::new();
        duplicate.append(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
        duplicate.append(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
        assert!(!is_sse_response(StatusCode::OK, &duplicate));
    }
}
