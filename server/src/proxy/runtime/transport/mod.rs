mod cancellation;
mod client;
mod non_stream;
mod response;
mod stream;

use std::sync::Arc;

use axum::{
    body::{Body, Bytes},
    http::{HeaderMap, header::CONTENT_TYPE},
    response::Response,
};
use chrono::Utc;
use reqwest::Method;
use tokio::sync::Mutex as TokioMutex;

pub(crate) use client::send_with_first_byte_timeout;

use self::{non_stream::handle_non_streaming_response, stream::handle_streaming_response};
use crate::{
    proxy::{
        ProxyError,
        cancellation::{CancellationDropGuard, ProxyCancellationContext},
        logging::RequestLogContext,
        provider_governance::record_provider_failure,
        runtime::api_key_lease::ApiKeyRequestLeaseFinalizer,
        util::serialize_upstream_response_headers_for_log,
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::runtime::{ProviderCircuitProbePermit, ReasoningContinuationScope},
    service::{app_state::AppState, cache::types::CacheCostCatalogVersion},
};

#[derive(Clone, Copy, Debug)]
pub(in crate::proxy) enum ProxyResponseMode {
    Generation {
        api_type: LlmApiType,
        target_api_type: LlmApiType,
    },
    Utility {
        api_type: LlmApiType,
    },
}

impl ProxyResponseMode {
    fn api_types(self) -> (LlmApiType, LlmApiType) {
        match self {
            Self::Generation {
                api_type,
                target_api_type,
            } => (api_type, target_api_type),
            Self::Utility { api_type } => (api_type, api_type),
        }
    }
}

pub(in crate::proxy) struct ProxyRequestOutcome {
    pub response: Response<Body>,
    pub log_context: RequestLogContext,
}

pub(in crate::proxy) struct ProxyRequestFailure {
    pub error: ProxyError,
    pub log_context: RequestLogContext,
}

#[derive(Clone, Debug)]
pub(in crate::proxy) struct ReasoningContinuationCaptureContext {
    pub scope: ReasoningContinuationScope,
    pub feature_enabled: bool,
}

fn finalize_send_failure_log_context(
    context: &mut RequestLogContext,
    url: &str,
    completed_at: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    proxy_error: &ProxyError,
) {
    context.request_url = Some(url.to_string());
    context.completion_ts = Some(completed_at);
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
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
    mut api_key_request_lease: ApiKeyRequestLeaseFinalizer,
    provider_circuit_permit: Option<ProviderCircuitProbePermit>,
    response_mode: ProxyResponseMode,
    reasoning_capture: Option<ReasoningContinuationCaptureContext>,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let provider_id = log_context.provider_id;
    let log_context = Arc::new(TokioMutex::new(log_context));

    let client_bundle = app_state.infra.client_bundle().await;
    let first_byte_timeout = client_bundle.proxy_request.first_byte_timeout();
    let client = match client_bundle.provider_client(use_proxy) {
        Ok(client) => client,
        Err(error) => {
            let proxy_error = ProxyError::BadGateway(error.to_string());
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

    let mut drop_cancellation_guard = CancellationDropGuard::new(
        cancellation.clone(),
        format!(
            "Client disconnected during proxy request for log_id {}.",
            log_context.lock().await.id
        ),
    );

    log_context.lock().await.llm_request_sent_at = Some(Utc::now().timestamp_millis());
    let response = match send_with_first_byte_timeout(
        &cancellation,
        client
            .request(Method::POST, &url)
            .headers(headers)
            .body(data),
        "LLM request",
        first_byte_timeout,
    )
    .await
    {
        Ok(resp) => resp,
        Err(proxy_error) => {
            drop_cancellation_guard.disarm();
            if !matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                record_provider_failure(
                    &app_state,
                    provider_id,
                    &model_str,
                    &proxy_error,
                    provider_circuit_permit.as_ref(),
                )
                .await;
            }
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

    {
        let mut context = log_context.lock().await;
        context.response_headers_json =
            serialize_upstream_response_headers_for_log(response.headers());
    }

    let is_sse = response.status().is_success()
        && response.headers().get(CONTENT_TYPE).map_or(false, |value| {
            value.to_str().unwrap_or("").contains("text/event-stream")
        });
    {
        let mut context = log_context.lock().await;
        context.is_stream = is_sse;
    }

    let result = if is_sse {
        let (api_type, target_api_type) = response_mode.api_types();
        match handle_streaming_response(
            &app_state,
            cancellation.clone(),
            provider_id,
            log_context.clone(),
            model_str,
            response,
            &url,
            cost_catalog_version,
            api_key_request_lease,
            provider_circuit_permit,
            api_type,
            target_api_type,
            reasoning_capture.clone(),
            first_byte_timeout,
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
            &app_state,
            &cancellation,
            provider_id,
            log_context,
            model_str,
            response,
            &url,
            cost_catalog_version.as_ref(),
            api_key_request_lease,
            provider_circuit_permit,
            response_mode,
            reasoning_capture.as_ref(),
        )
        .await
    };
    drop_cancellation_guard.disarm();
    result
}
