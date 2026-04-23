use super::error::ProxyLogLevel;
use super::logging::{
    LogBodyKind, LoggedBody, RequestLogContext, StreamingBodyWriter,
    record_request_completion_and_log,
};
use super::util::{
    json_top_level_field_count_from_bytes, serialize_upstream_response_headers_for_log, sha256_hex,
};
use super::{
    ProxyError,
    cancellation::{CancellationDropGuard, ProxyCancellationContext},
    classify_reqwest_error, classify_upstream_status, protocol_transform_error,
    provider_governance::{record_provider_failure, record_provider_success},
};

use crate::config::CONFIG;
use crate::cost::UsageNormalization;
use crate::schema::enum_def::{LlmApiType, RequestStatus};
use crate::service::app_state::{ApiKeyConcurrencyGuard, AppState};
use crate::service::cache::types::CacheCostCatalogVersion;
use crate::service::transform::{
    StreamTransformer, transform_result_with_cost_and_diagnostics,
    unified::UnifiedTransformDiagnostic,
};
use crate::utils::sse::SseParser;
use crate::utils::storage::LogBodyCaptureState;
use crate::utils::storage::RequestLogBundleTransformDiagnosticPhase;
use crate::utils::usage::UsageInfo;

use axum::{
    body::{Body, Bytes},
    http::response::Builder as HttpResponseBuilder,
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error};
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::{
    Method, StatusCode,
    header::{
        CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderName, TRANSFER_ENCODING,
    },
};
use serde_json::Value;
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex as TokioMutex, mpsc};
use tokio::time::timeout;

#[derive(Clone, Copy, Debug)]
pub(super) enum ProxyResponseMode {
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

pub(super) struct ProxyRequestOutcome {
    pub response: Response<Body>,
    pub log_context: RequestLogContext,
}

pub(super) struct ProxyRequestFailure {
    pub error: ProxyError,
    pub log_context: RequestLogContext,
    pub response_headers: Option<HeaderMap>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProxyLogMode {
    RecordAll,
    DeferNonStreaming,
}

impl ProxyLogMode {
    fn should_record_immediate(self) -> bool {
        matches!(self, Self::RecordAll)
    }

    fn should_record_streaming(self) -> bool {
        matches!(self, Self::RecordAll | Self::DeferNonStreaming)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ProxyExecutionPolicy {
    Normal,
    ReplayDryRun,
    ReplayLive,
}

impl ProxyExecutionPolicy {
    pub(crate) fn records_request_log(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn records_provider_runtime(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn admits_api_key_requests(self) -> bool {
        matches!(self, Self::Normal)
    }
}

struct RequestLogContextGuard {
    app_state: Arc<AppState>,
    context: Arc<TokioMutex<RequestLogContext>>,
    log_mode: ProxyLogMode,
    execution_policy: ProxyExecutionPolicy,
    is_armed: bool,
}

impl RequestLogContextGuard {
    fn new(
        app_state: Arc<AppState>,
        context: Arc<TokioMutex<RequestLogContext>>,
        log_mode: ProxyLogMode,
        execution_policy: ProxyExecutionPolicy,
    ) -> Self {
        Self {
            app_state,
            context,
            log_mode,
            execution_policy,
            is_armed: true,
        }
    }

    fn disarm(&mut self) {
        self.is_armed = false;
    }
}

impl Drop for RequestLogContextGuard {
    fn drop(&mut self) {
        if self.is_armed
            && self.log_mode.should_record_immediate()
            && self.execution_policy.records_request_log()
        {
            let app_state = Arc::clone(&self.app_state);
            let context_clone = Arc::clone(&self.context);
            tokio::spawn(async move {
                let mut context = context_clone.lock().await;
                crate::debug_event!("proxy.client_cancelled", log_id = context.id);
                context.overall_status = RequestStatus::Cancelled;
                context.completion_ts = Some(Utc::now().timestamp_millis());
                record_request_completion_and_log(&app_state, context.clone()).await;
            });
        }
    }
}

struct ResponseStreamCancellationGuard {
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    context: Arc<TokioMutex<RequestLogContext>>,
    url: String,
    status_code: StatusCode,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    execution_policy: ProxyExecutionPolicy,
    reason: String,
    armed: bool,
}

impl ResponseStreamCancellationGuard {
    fn new(
        app_state: Arc<AppState>,
        cancellation: ProxyCancellationContext,
        context: Arc<TokioMutex<RequestLogContext>>,
        url: impl Into<String>,
        status_code: StatusCode,
        cost_catalog_version: Option<CacheCostCatalogVersion>,
        execution_policy: ProxyExecutionPolicy,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            app_state,
            cancellation,
            context,
            url: url.into(),
            status_code,
            cost_catalog_version,
            execution_policy,
            reason: reason.into(),
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for ResponseStreamCancellationGuard {
    fn drop(&mut self) {
        if self.armed {
            self.cancellation.cancel_now(self.reason.clone());
            if !self.execution_policy.records_request_log() {
                return;
            }
            let app_state = Arc::clone(&self.app_state);
            let context = Arc::clone(&self.context);
            let url = self.url.clone();
            let status_code = self.status_code;
            let cost_catalog_version = self.cost_catalog_version.clone();
            tokio::spawn(async move {
                finalize_cancelled_log_context(
                    &app_state,
                    &context,
                    &url,
                    Some(status_code),
                    cost_catalog_version.as_ref(),
                    None,
                    None,
                )
                .await;
            });
        }
    }
}

fn log_simple_request_failed(url: &str, stage: Option<&str>, proxy_error: &ProxyError) {
    match proxy_error.operator_log_level() {
        ProxyLogLevel::Debug => crate::debug_event!(
            "proxy.simple_request_failed",
            url = url,
            error_code = proxy_error.error_code(),
            stage = stage,
        ),
        ProxyLogLevel::Warn => crate::warn_event!(
            "proxy.simple_request_failed",
            url = url,
            error_code = proxy_error.error_code(),
            stage = stage,
        ),
        ProxyLogLevel::Error => crate::error_event!(
            "proxy.simple_request_failed",
            url = url,
            error_code = proxy_error.error_code(),
            stage = stage,
        ),
    }
}

fn should_forward_response_header(name: &HeaderName) -> bool {
    name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING
}

fn response_content_type(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
}

pub(super) fn build_response_builder(
    status_code: StatusCode,
    response_headers: &HeaderMap,
) -> HttpResponseBuilder {
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if should_forward_response_header(name) {
            response_builder = response_builder.header(name, value);
        }
    }
    response_builder
}

pub(crate) fn decode_response_body(body_bytes: Bytes, is_gzip: bool) -> Bytes {
    if !is_gzip {
        return body_bytes;
    }

    if body_bytes.is_empty() {
        return Bytes::new();
    }

    let mut gz = GzDecoder::new(&body_bytes[..]);
    let mut decompressed_data = Vec::new();
    match gz.read_to_end(&mut decompressed_data) {
        Ok(_) => Bytes::from(decompressed_data),
        Err(e) => {
            error!("Gzip decoding failed: {}", e);
            body_bytes
        }
    }
}

pub(crate) fn process_success_response_body(
    decompressed_body: &Bytes,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (
    Bytes,
    Option<UsageInfo>,
    Option<UsageNormalization>,
    Vec<UnifiedTransformDiagnostic>,
) {
    match serde_json::from_slice::<Value>(decompressed_body) {
        Ok(original_value) => {
            let output = transform_result_with_cost_and_diagnostics(
                original_value,
                target_api_type,
                api_type,
            );

            let body_bytes = if api_type == target_api_type {
                decompressed_body.clone()
            } else {
                match serde_json::to_vec(&output.value) {
                    Ok(b) => Bytes::from(b),
                    Err(e) => {
                        error!(
                            "Failed to serialize transformed response: {}. Returning original body.",
                            e
                        );
                        decompressed_body.clone()
                    }
                }
            };
            (
                body_bytes,
                output.usage_info,
                output.usage_normalization,
                output.diagnostics,
            )
        }
        Err(e) => {
            crate::debug_event!(
                "proxy.response_non_json_passthrough",
                response_body_bytes = decompressed_body.len(),
                response_body_sha256 = sha256_hex(decompressed_body),
                parse_error = e,
                json_top_level_fields = json_top_level_field_count_from_bytes(decompressed_body),
            );
            (decompressed_body.clone(), None, None, Vec::new())
        }
    }
}

pub(super) fn finalize_non_streaming_log_context(
    context: &mut RequestLogContext,
    url: &str,
    status_code: StatusCode,
    completion_ts: i64,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    overall_status: RequestStatus,
    usage: Option<UsageInfo>,
    usage_normalization: Option<UsageNormalization>,
    llm_response_body: Bytes,
    user_response_body: Bytes,
) {
    context.request_url = Some(url.to_string());
    context.llm_status = Some(status_code);
    context.completion_ts = Some(completion_ts);
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = overall_status;
    context.llm_response_body = Some(LoggedBody::from_bytes(llm_response_body));
    context.user_response_body = Some(LoggedBody::from_bytes(user_response_body));
}

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

async fn finalize_cancelled_log_context(
    app_state: &Arc<AppState>,
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    url: &str,
    status_code: Option<StatusCode>,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    llm_response_body: Option<LoggedBody>,
    user_response_body: Option<LoggedBody>,
) {
    let mut context = log_context.lock().await;
    context.request_url = Some(url.to_string());
    context.llm_status = status_code;
    context.completion_ts = Some(Utc::now().timestamp_millis());
    context.cost_catalog_version = cost_catalog_version.cloned();
    context.overall_status = RequestStatus::Cancelled;
    context.llm_response_body = llm_response_body;
    context.user_response_body = user_response_body;
    record_request_completion_and_log(app_state, context.clone()).await;
}

async fn sync_stream_usage_to_log_context(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformer: &mut StreamTransformer,
) {
    let usage = transformer.parse_usage_info();
    let usage_normalization = transformer.parse_usage_normalization();
    let diagnostics = transformer.diagnostics_snapshot();

    if usage.is_none() && usage_normalization.is_none() && diagnostics.is_empty() {
        return;
    }

    let mut context = log_context.lock().await;
    context.usage = usage;
    context.usage_normalization = usage_normalization;
    context.replace_transform_diagnostics_phase(
        RequestLogBundleTransformDiagnosticPhase::Stream,
        &diagnostics,
    );
}

async fn mark_stream_response_started_to_client(
    log_context: &Arc<TokioMutex<RequestLogContext>>,
    transformed_chunk: &Bytes,
) {
    if transformed_chunk.is_empty() {
        return;
    }

    let mut context = log_context.lock().await;
    if context.first_chunk_ts.is_none() {
        context.first_chunk_ts = Some(Utc::now().timestamp_millis());
    }
}

fn next_stream_chunk_timeout_duration(first_chunk_received_at_proxy: i64) -> Option<Duration> {
    if first_chunk_received_at_proxy == 0 {
        CONFIG.proxy_request.first_byte_timeout()
    } else {
        None
    }
}

// A simple proxy that sends a request and returns the response, handling streaming and gzip.
// It does not perform logging or response transformation.
pub(super) async fn simple_proxy_request(
    app_state: &AppState,
    url: String,
    data: String,
    headers: reqwest::header::HeaderMap,
    use_proxy: bool,
) -> Result<Response<Body>, ProxyError> {
    let cancellation = ProxyCancellationContext::new();
    let client = if use_proxy {
        &app_state.proxy_client
    } else {
        &app_state.client
    };

    crate::debug_event!(
        "proxy.simple_request_dispatch",
        url = &url,
        request_header_count = headers.len(),
        request_body_bytes = data.len(),
        request_body_sha256 = sha256_hex(data.as_bytes()),
        json_top_level_fields = json_top_level_field_count_from_bytes(data.as_bytes()),
    );

    let response = match send_with_first_byte_timeout(
        &cancellation,
        client
            .request(Method::POST, &url)
            .headers(headers)
            .body(data),
        "LLM request",
    )
    .await
    {
        Ok(resp) => resp,
        Err(proxy_error) => {
            log_simple_request_failed(&url, None, &proxy_error);
            return Err(proxy_error);
        }
    };

    let status_code = response.status();
    let response_headers = response.headers().clone();
    let response_builder = build_response_builder(status_code, &response_headers);

    let is_sse = response_headers.get(CONTENT_TYPE).map_or(false, |value| {
        value.to_str().unwrap_or("").contains("text/event-stream")
    });

    if is_sse {
        let body = Body::from_stream(
            response
                .bytes_stream()
                .map(|r| r.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))),
        );
        Ok(response_builder.body(body).unwrap())
    } else {
        let is_gzip = response_headers
            .get(CONTENT_ENCODING)
            .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

        let body_bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                let proxy_error = classify_reqwest_error("Reading upstream response body", &e);
                log_simple_request_failed(&url, Some("read_response_body"), &proxy_error);
                return Err(proxy_error);
            }
        };

        let decompressed_body = decode_response_body(body_bytes, is_gzip);
        Ok(response_builder
            .body(Body::from(decompressed_body))
            .unwrap())
    }
}

// Builds the HTTP client, sends the request to the LLM, and passes the response to be handled.
pub(super) async fn proxy_request(
    app_state: Arc<AppState>,
    cancellation: ProxyCancellationContext,
    log_context: RequestLogContext,
    url: String,
    data: Bytes,
    headers: HeaderMap,
    model_str: String,
    use_proxy: bool,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    api_key_concurrency_guard: Option<ApiKeyConcurrencyGuard>,
    response_mode: ProxyResponseMode,
    log_mode: ProxyLogMode,
    execution_policy: ProxyExecutionPolicy,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let provider_id = log_context.provider_id;
    let log_context = Arc::new(TokioMutex::new(log_context));

    // 1. Get HTTP client from AppState, with proxy if configured
    let client = if use_proxy {
        &app_state.proxy_client
    } else {
        &app_state.client
    };

    let mut cancellation_guard = RequestLogContextGuard::new(
        Arc::clone(&app_state),
        log_context.clone(),
        log_mode,
        execution_policy,
    );
    let mut drop_cancellation_guard = CancellationDropGuard::new(
        cancellation.clone(),
        format!(
            "Client disconnected during proxy request for log_id {}.",
            log_context.lock().await.id
        ),
    );

    // 2. Send request to LLM
    log_context.lock().await.llm_request_sent_at = Some(Utc::now().timestamp_millis());
    let response = match send_with_first_byte_timeout(
        &cancellation,
        client
            .request(Method::POST, &url)
            .headers(headers)
            .body(data),
        "LLM request",
    )
    .await
    {
        Ok(resp) => resp,
        Err(proxy_error) => {
            drop_cancellation_guard.disarm();
            cancellation_guard.disarm();
            if execution_policy.records_provider_runtime()
                && !matches!(proxy_error, ProxyError::ClientCancelled(_))
            {
                record_provider_failure(&app_state, provider_id, &model_str, &proxy_error).await;
            }
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            context.request_url = Some(url.clone());
            context.completion_ts = Some(completed_at);
            context.cost_catalog_version = cost_catalog_version.clone();
            context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                RequestStatus::Cancelled
            } else {
                RequestStatus::Error
            };
            if log_mode.should_record_immediate() && execution_policy.records_request_log() {
                record_request_completion_and_log(&app_state, context.clone()).await;
            }

            return Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: context.clone(),
                response_headers: None,
            });
        }
    };

    {
        let mut context = log_context.lock().await;
        context.response_headers_json =
            serialize_upstream_response_headers_for_log(response.headers());
    }

    // 3. Process the response stream
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
            api_key_concurrency_guard,
            api_type,
            target_api_type,
            log_mode,
            execution_policy,
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
                response_headers: None,
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
            api_key_concurrency_guard,
            response_mode,
            log_mode,
            execution_policy,
        )
        .await
    };
    drop_cancellation_guard.disarm();
    cancellation_guard.disarm();
    result
}

// Handles a non-streaming response from the LLM.
async fn handle_non_streaming_response(
    app_state: &Arc<AppState>,
    cancellation: &ProxyCancellationContext,
    provider_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<&CacheCostCatalogVersion>,
    _api_key_concurrency_guard: Option<ApiKeyConcurrencyGuard>,
    response_mode: ProxyResponseMode,
    log_mode: ProxyLogMode,
    execution_policy: ProxyExecutionPolicy,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    crate::debug_event!(
        "proxy.response_headers_received",
        status_code = status_code.as_u16(),
        response_header_count = response_headers.len(),
        content_type = response_content_type(&response_headers),
    );
    let is_gzip = response_headers
        .get(CONTENT_ENCODING)
        .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

    let response_builder = build_response_builder(status_code, &response_headers);

    let body_bytes = match read_response_bytes_with_cancellation(
        response,
        "Reading upstream response body",
        cancellation,
    )
    .await
    {
        Ok(b) => b,
        Err(proxy_error) => {
            if execution_policy.records_provider_runtime()
                && !matches!(proxy_error, ProxyError::ClientCancelled(_))
            {
                record_provider_failure(app_state, provider_id, &model_str, &proxy_error).await;
            }
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            context.request_url = Some(url.to_string());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(completed_at);
            context.cost_catalog_version = cost_catalog_version.cloned();
            context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                RequestStatus::Cancelled
            } else {
                RequestStatus::Error
            };
            context.llm_response_body =
                Some(LoggedBody::from_bytes(Bytes::from(proxy_error.to_string())));
            if log_mode.should_record_immediate() && execution_policy.records_request_log() {
                record_request_completion_and_log(app_state, context.clone()).await;
            }

            return Err(ProxyRequestFailure {
                error: proxy_error,
                log_context: context.clone(),
                response_headers: Some(response_headers),
            });
        }
    };

    let decompressed_body = decode_response_body(body_bytes, is_gzip);
    let llm_response_completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        let (final_body, parsed_usage_info, parsed_usage_normalization, transform_diagnostics) =
            match response_mode {
                ProxyResponseMode::Generation {
                    api_type,
                    target_api_type,
                } => process_success_response_body(&decompressed_body, api_type, target_api_type),
                ProxyResponseMode::Utility { .. } => {
                    let usage_normalization = serde_json::from_slice::<Value>(&decompressed_body)
                        .ok()
                        .and_then(|val| super::util::parse_utility_usage_normalization(&val));
                    (
                        decompressed_body.clone(),
                        None,
                        usage_normalization,
                        Vec::new(),
                    )
                }
            };

        let mut context = log_context.lock().await;
        finalize_non_streaming_log_context(
            &mut context,
            url,
            status_code,
            llm_response_completed_at,
            cost_catalog_version,
            RequestStatus::Success,
            parsed_usage_info,
            parsed_usage_normalization,
            decompressed_body.clone(),
            final_body.clone(),
        );
        context.append_transform_diagnostics(
            RequestLogBundleTransformDiagnosticPhase::Response,
            &transform_diagnostics,
        );
        if log_mode.should_record_immediate() && execution_policy.records_request_log() {
            record_request_completion_and_log(app_state, context.clone()).await;
        }
        if execution_policy.records_provider_runtime() {
            record_provider_success(app_state, provider_id, &model_str).await;
        }
        crate::debug_event!(
            "proxy.request_succeeded_debug",
            log_id = context.id,
            model = &model_str,
            status_code = status_code.as_u16(),
            is_stream = false,
            latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
        );

        let response = response_builder.body(Body::from(final_body)).unwrap();
        Ok(ProxyRequestOutcome {
            response,
            log_context: context.clone(),
        })
    } else {
        let mut context = log_context.lock().await;
        crate::error_event!(
            "proxy.upstream_error_body",
            status_code = status_code.as_u16(),
            log_id = context.id,
            response_body_bytes = decompressed_body.len(),
            response_body_sha256 = sha256_hex(&decompressed_body),
            json_top_level_fields = json_top_level_field_count_from_bytes(&decompressed_body),
            content_type = response_content_type(&response_headers),
        );

        finalize_non_streaming_log_context(
            &mut context,
            url,
            status_code,
            llm_response_completed_at,
            cost_catalog_version,
            RequestStatus::Error,
            None,
            None,
            decompressed_body.clone(),
            decompressed_body.clone(),
        );
        if log_mode.should_record_immediate() && execution_policy.records_request_log() {
            record_request_completion_and_log(app_state, context.clone()).await;
        }
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        if execution_policy.records_provider_runtime() {
            record_provider_failure(app_state, provider_id, &model_str, &proxy_error).await;
        }
        Err(ProxyRequestFailure {
            error: proxy_error,
            log_context: context.clone(),
            response_headers: Some(response_headers),
        })
    }
}

// Handles a streaming (SSE) response from the LLM.
async fn handle_streaming_response(
    app_state: &Arc<AppState>,
    cancellation: ProxyCancellationContext,
    provider_id: i64,
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    cost_catalog_version: Option<CacheCostCatalogVersion>,
    api_key_concurrency_guard: Option<ApiKeyConcurrencyGuard>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    log_mode: ProxyLogMode,
    execution_policy: ProxyExecutionPolicy,
) -> Result<Response<Body>, ProxyError> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    let log_id = log_context.lock().await.id;

    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    let url_owned = url.to_string();
    let cost_catalog_version_clone = cost_catalog_version.clone();
    let app_state_clone = Arc::clone(app_state);

    let cancellation_for_reader = cancellation.clone();
    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        loop {
            tokio::select! {
                _ = cancellation_for_reader.cancelled() => break,
                maybe_chunk = stream.next() => {
                    let Some(chunk_result) = maybe_chunk else {
                        break;
                    };
                    if tx.send(chunk_result).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let mut transformer = StreamTransformer::new(target_api_type, api_type);
    let mut parser = SseParser::new();
    let log_context_clone = log_context.clone();
    let llm_body_writer = StreamingBodyWriter::new(LogBodyKind::LlmResponse, log_id)
        .await
        .map_err(|e| {
            ProxyError::InternalError(format!("Failed to create LLM stream spool writer: {e}"))
        })?;
    let user_body_writer = StreamingBodyWriter::new(LogBodyKind::UserResponse, log_id)
        .await
        .map_err(|e| {
            ProxyError::InternalError(format!("Failed to create user stream spool writer: {e}"))
        })?;

    let monitored_stream = async_stream::stream! {
        let _api_key_concurrency_guard = api_key_concurrency_guard;
        let mut response_drop_guard = ResponseStreamCancellationGuard::new(
            Arc::clone(&app_state_clone),
            cancellation.clone(),
            log_context_clone.clone(),
            url_owned.clone(),
            status_code,
            cost_catalog_version_clone.clone(),
            execution_policy,
            format!("Client disconnected while receiving streaming response for log_id {}.", log_id),
        );
        let mut first_chunk_received_at_proxy: i64 = 0;
        let mut llm_body_writer = Some(llm_body_writer);
        let mut user_body_writer = Some(user_body_writer);

        loop {
            let chunk_result = match next_stream_chunk_timeout_duration(first_chunk_received_at_proxy) {
                Some(timeout_duration) => match tokio::select! {
                    _ = cancellation.cancelled() => Err(cancellation.cancellation_error().await),
                    result = timeout(timeout_duration, rx.recv()) => Ok(result),
                } {
                    Err(proxy_error) => {
                        response_drop_guard.disarm();
                        if let Some(writer) = llm_body_writer.take() {
                            let _ = writer.abort().await;
                        }
                        if let Some(writer) = user_body_writer.take() {
                            let _ = writer.abort().await;
                        }
                        if execution_policy.records_request_log() {
                            finalize_cancelled_log_context(
                                &app_state_clone,
                                &log_context_clone,
                                &url_owned,
                                Some(status_code),
                                cost_catalog_version_clone.as_ref(),
                                None,
                                None,
                            ).await;
                        }
                        yield Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, proxy_error.to_string()));
                        return;
                    }
                    Ok(result) => match result {
                    Ok(result) => result,
                    Err(_) => {
                        response_drop_guard.disarm();
                        let stream_error_message = format!(
                            "LLM stream timed out waiting for the first chunk after {:?}",
                            timeout_duration
                        );
                        error!("{}", stream_error_message);
                        let completed_at = Utc::now().timestamp_millis();

                        let mut context = log_context_clone.lock().await;
                        context.request_url = Some(url_owned.clone());
                        context.llm_status = Some(status_code);
                        context.completion_ts = Some(completed_at);
                        context.cost_catalog_version = cost_catalog_version_clone.clone();
                        context.overall_status = RequestStatus::Error;
                        context.llm_response_body = match llm_body_writer.take() {
                            Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                            None => None,
                        };
                        context.user_response_body = match user_body_writer.take() {
                            Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                            None => None,
                        };
                        let proxy_error = ProxyError::UpstreamTimeout(stream_error_message.clone());
                        context.final_error_code = Some(proxy_error.error_code().to_string());
                        context.final_error_message = Some(proxy_error.message().to_string());
                        if log_mode.should_record_streaming()
                            && execution_policy.records_request_log()
                        {
                            record_request_completion_and_log(&app_state_clone, context.clone()).await;
                        }
                        if execution_policy.records_provider_runtime() {
                            record_provider_failure(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                &proxy_error,
                            )
                            .await;
                        }

                        yield Err(std::io::Error::new(std::io::ErrorKind::TimedOut, stream_error_message));
                        return;
                    }
                }},
                None => {
                    tokio::select! {
                        _ = cancellation.cancelled() => {
                            response_drop_guard.disarm();
                            if let Some(writer) = llm_body_writer.take() {
                                let _ = writer.abort().await;
                            }
                            if let Some(writer) = user_body_writer.take() {
                                let _ = writer.abort().await;
                            }
                            if execution_policy.records_request_log() {
                                finalize_cancelled_log_context(
                                    &app_state_clone,
                                    &log_context_clone,
                                    &url_owned,
                                    Some(status_code),
                                    cost_catalog_version_clone.as_ref(),
                                    None,
                                    None,
                                ).await;
                            }
                            yield Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, cancellation.cancellation_error().await.to_string()));
                            return;
                        }
                        result = rx.recv() => result,
                    }
                }
            };

            let Some(chunk_result) = chunk_result else {
                break;
            };

            match chunk_result {
                Ok(chunk) => {
                    if let Err(e) = llm_body_writer.as_mut().expect("llm stream writer should exist").append(&chunk).await {
                        response_drop_guard.disarm();
                        let stream_error_message = format!("Failed to persist LLM stream chunk: {}", e);
                        error!("{}", stream_error_message);
                        let completed_at = Utc::now().timestamp_millis();

                        let mut context = log_context_clone.lock().await;
                        context.request_url = Some(url_owned.clone());
                        context.llm_status = Some(status_code);
                        context.completion_ts = Some(completed_at);
                        context.cost_catalog_version = cost_catalog_version_clone.clone();
                        context.overall_status = RequestStatus::Error;
                        context.llm_response_body = match llm_body_writer.take() {
                            Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                            None => None,
                        };
                        context.user_response_body = match user_body_writer.take() {
                            Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                            None => None,
                        };
                        let proxy_error = ProxyError::InternalError(stream_error_message.clone());
                        context.final_error_code = Some(proxy_error.error_code().to_string());
                        context.final_error_message = Some(proxy_error.message().to_string());
                        if log_mode.should_record_streaming()
                            && execution_policy.records_request_log()
                        {
                            record_request_completion_and_log(&app_state_clone, context.clone()).await;
                        }
                        if execution_policy.records_provider_runtime() {
                            record_provider_failure(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                &proxy_error,
                            )
                            .await;
                        }

                        yield Err(std::io::Error::other(stream_error_message));
                        return;
                    }

                    {
                        let mut context = log_context_clone.lock().await;
                        context.llm_response_body =
                            Some(llm_body_writer.as_ref().expect("llm stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
                    }

                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }

                    let events = parser.process(&chunk);
                    if events.is_empty() {
                        continue;
                    }

                    let transformed_events = transformer.transform_events(events);
                    sync_stream_usage_to_log_context(&log_context_clone, &mut transformer).await;
                    let mut transformed_chunk_bytes: Vec<u8> = Vec::new();

                    for transformed_event in transformed_events {
                        if target_api_type == LlmApiType::Ollama {
                            transformed_chunk_bytes
                                .extend_from_slice(transformed_event.data.as_bytes());
                            transformed_chunk_bytes.push(b'\n');
                        } else {
                            transformed_chunk_bytes.extend_from_slice(&transformed_event.to_bytes());
                        }
                    }

                    let transformed_chunk = Bytes::from(transformed_chunk_bytes);
                    if let Err(e) = user_body_writer.as_mut().expect("user stream writer should exist").append(&transformed_chunk).await {
                        response_drop_guard.disarm();
                        let stream_error_message =
                            format!("Failed to persist transformed stream chunk: {}", e);
                        error!("{}", stream_error_message);
                        let completed_at = Utc::now().timestamp_millis();

                        let mut context = log_context_clone.lock().await;
                        context.request_url = Some(url_owned.clone());
                        context.llm_status = Some(status_code);
                        context.completion_ts = Some(completed_at);
                        context.cost_catalog_version = cost_catalog_version_clone.clone();
                        context.overall_status = RequestStatus::Error;
                        context.llm_response_body = match llm_body_writer.take() {
                            Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                            None => None,
                        };
                        context.user_response_body = match user_body_writer.take() {
                            Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                            None => None,
                        };
                        let proxy_error = ProxyError::InternalError(stream_error_message.clone());
                        context.final_error_code = Some(proxy_error.error_code().to_string());
                        context.final_error_message = Some(proxy_error.message().to_string());
                        if log_mode.should_record_streaming()
                            && execution_policy.records_request_log()
                        {
                            record_request_completion_and_log(&app_state_clone, context.clone()).await;
                        }
                        if execution_policy.records_provider_runtime() {
                            record_provider_failure(
                                &app_state_clone,
                                provider_id,
                                &model_str,
                                &proxy_error,
                            )
                            .await;
                        }

                        yield Err(std::io::Error::other(stream_error_message));
                        return;
                    }

                    {
                        let mut context = log_context_clone.lock().await;
                        context.user_response_body =
                            Some(user_body_writer.as_ref().expect("user stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
                    }

                    if !transformed_chunk.is_empty() {
                        mark_stream_response_started_to_client(
                            &log_context_clone,
                            &transformed_chunk,
                        )
                        .await;
                        yield Ok::<_, std::io::Error>(transformed_chunk);
                    }
                }
                Err(e) => {
                    response_drop_guard.disarm();
                    let stream_error_message = format!("LLM stream error: {}", e);
                    error!("{}", stream_error_message);
                    let completed_at = Utc::now().timestamp_millis();

                    let mut context = log_context_clone.lock().await;
                    context.request_url = Some(url_owned.clone());
                    context.llm_status = Some(status_code);
                    context.completion_ts = Some(completed_at);
                    context.cost_catalog_version = cost_catalog_version_clone.clone();
                    context.overall_status = RequestStatus::Error;
                    context.llm_response_body = match llm_body_writer.take() {
                        Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                        None => None,
                    };
                    context.user_response_body = match user_body_writer.take() {
                        Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                        None => None,
                    };
                    let proxy_error = ProxyError::BadGateway(stream_error_message.clone());
                    context.final_error_code = Some(proxy_error.error_code().to_string());
                    context.final_error_message = Some(proxy_error.message().to_string());
                    if log_mode.should_record_streaming()
                        && execution_policy.records_request_log()
                    {
                        record_request_completion_and_log(&app_state_clone, context.clone()).await;
                    }
                    if execution_policy.records_provider_runtime() {
                        record_provider_failure(
                            &app_state_clone,
                            provider_id,
                            &model_str,
                            &proxy_error,
                        )
                        .await;
                    }

                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, stream_error_message));
                    return;
                }
            }
        }

        if status_code.is_success() && api_type == LlmApiType::Openai && target_api_type == LlmApiType::Gemini {
            debug!("[handle_streaming_response] Appending [DONE] chunk for OpenAI client.");
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            if let Err(e) = user_body_writer.as_mut().expect("user stream writer should exist").append(&done_chunk).await {
                response_drop_guard.disarm();
                let stream_error_message = format!("Failed to persist terminal DONE chunk: {}", e);
                error!("{}", stream_error_message);
                let completed_at = Utc::now().timestamp_millis();

                let mut context = log_context_clone.lock().await;
                context.request_url = Some(url_owned.clone());
                context.llm_status = Some(status_code);
                context.completion_ts = Some(completed_at);
                context.cost_catalog_version = cost_catalog_version_clone.clone();
                context.overall_status = RequestStatus::Error;
                context.llm_response_body = match llm_body_writer.take() {
                    Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                    None => None,
                };
                context.user_response_body = match user_body_writer.take() {
                    Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                    None => None,
                };
                let proxy_error = ProxyError::InternalError(stream_error_message.clone());
                context.final_error_code = Some(proxy_error.error_code().to_string());
                context.final_error_message = Some(proxy_error.message().to_string());
                if log_mode.should_record_streaming()
                    && execution_policy.records_request_log()
                {
                    record_request_completion_and_log(&app_state_clone, context.clone()).await;
                }
                if execution_policy.records_provider_runtime() {
                    record_provider_failure(
                        &app_state_clone,
                        provider_id,
                        &model_str,
                        &proxy_error,
                    )
                    .await;
                }

                yield Err(std::io::Error::other(stream_error_message));
                return;
            }
            {
                let mut context = log_context_clone.lock().await;
                context.user_response_body =
                    Some(user_body_writer.as_ref().expect("user stream writer should exist").snapshot(LogBodyCaptureState::Incomplete));
            }
            mark_stream_response_started_to_client(&log_context_clone, &done_chunk).await;
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() {
            let mut context = log_context_clone.lock().await;
            context.request_url = Some(url_owned.clone());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(llm_response_completed_at);
            context.cost_catalog_version = cost_catalog_version_clone.clone();
            context.overall_status = RequestStatus::Success;
            context.llm_response_body = match llm_body_writer.take() {
                Some(writer) => writer.finish(LogBodyCaptureState::Complete).await.ok(),
                None => None,
            };
            context.user_response_body = match user_body_writer.take() {
                Some(writer) => writer.finish(LogBodyCaptureState::Complete).await.ok(),
                None => None,
            };

            context.usage = transformer.parse_usage_info();
            context.usage_normalization = transformer.parse_usage_normalization();
            context.replace_transform_diagnostics_phase(
                RequestLogBundleTransformDiagnosticPhase::Stream,
                &transformer.diagnostics_snapshot(),
            );
            if log_mode.should_record_streaming() && execution_policy.records_request_log() {
                record_request_completion_and_log(&app_state_clone, context.clone()).await;
            }
            if execution_policy.records_provider_runtime() {
                record_provider_success(&app_state_clone, provider_id, &model_str).await;
            }
            if context.usage.is_none() {
                crate::debug_event!(
                    "proxy.stream_usage_missing_debug",
                    log_id = context.id,
                    model = &model_str,
                    status_code = status_code.as_u16(),
                );
            }
            crate::debug_event!(
                "proxy.request_succeeded_debug",
                log_id = context.id,
                model = &model_str,
                status_code = status_code.as_u16(),
                is_stream = true,
                latency_ms = llm_response_completed_at.saturating_sub(context.request_received_at),
            );
            response_drop_guard.disarm();
        } else { // !status_code.is_success()
            let mut context = log_context_clone.lock().await;
            context.request_url = Some(url_owned.clone());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(llm_response_completed_at);
            context.cost_catalog_version = cost_catalog_version_clone.clone();
            context.overall_status = RequestStatus::Error;
            context.llm_response_body = match llm_body_writer.take() {
                Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                None => None,
            };
            context.user_response_body = match user_body_writer.take() {
                Some(writer) => writer.finish(LogBodyCaptureState::Incomplete).await.ok(),
                None => None,
            };
            let proxy_error = classify_upstream_status(status_code, &[]);
            context.final_error_code = Some(proxy_error.error_code().to_string());
            context.final_error_message = Some(proxy_error.message().to_string());
            if log_mode.should_record_streaming() && execution_policy.records_request_log() {
                record_request_completion_and_log(&app_state_clone, context.clone()).await;
            }
            if execution_policy.records_provider_runtime() {
                record_provider_failure(&app_state_clone, provider_id, &model_str, &proxy_error).await;
            }
            response_drop_guard.disarm();
        }
    };

    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => Ok(final_response),
        Err(e) => {
            let log_id = log_context.lock().await.id;
            let proxy_error = protocol_transform_error(
                &format!("Failed to build client response for log_id {log_id}"),
                e,
            );
            error!("{}", proxy_error);
            Err(proxy_error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        build_response_builder, decode_response_body, finalize_cancelled_log_context,
        finalize_non_streaming_log_context, mark_stream_response_started_to_client,
        next_stream_chunk_timeout_duration, process_success_response_body,
        send_with_first_byte_timeout, should_forward_response_header,
        sync_stream_usage_to_log_context,
    };
    use crate::{
        cost::UsageNormalization,
        proxy::logging::get_log_manager,
        proxy::logging::{LoggedBody, RequestLogContext},
        proxy::{ProxyError, cancellation::ProxyCancellationContext},
        schema::enum_def::{LlmApiType, ProviderApiKeyMode, ProviderType, RequestStatus},
        service::app_state::AppState,
        service::cache::types::{CacheApiKey, CacheCostCatalogVersion, CacheModel, CacheProvider},
        service::transform::StreamTransformer,
        utils::sse::SseParser,
        utils::usage::UsageInfo,
    };
    use axum::body::{Body, to_bytes};
    use flate2::{Compression, write::GzEncoder};
    use reqwest::{
        StatusCode,
        header::{
            CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, HeaderMap, HeaderValue,
            TRANSFER_ENCODING,
        },
    };
    use serde_json::{Value, json};
    use std::io::Write;

    fn gzip_bytes(input: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(input).unwrap();
        encoder.finish().unwrap()
    }

    fn make_log_context() -> RequestLogContext {
        let system_api_key = CacheApiKey {
            id: 1,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "system".to_string(),
            description: None,
            default_action: crate::schema::enum_def::Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: vec![],
        };
        let provider = CacheProvider {
            id: 2,
            provider_key: "provider".to_string(),
            name: "Provider".to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        };
        let model = CacheModel {
            id: 3,
            provider_id: 2,
            model_name: "gpt-test".to_string(),
            real_model_name: None,
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        };

        RequestLogContext::new(
            &system_api_key,
            &provider,
            &model,
            Some(4),
            "provider/gpt-test",
            "direct",
            None,
            None,
            1234,
            &None,
            LlmApiType::Openai,
            LlmApiType::Openai,
        )
    }

    #[test]
    fn build_response_builder_filters_hop_by_hop_headers() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-request-id", HeaderValue::from_static("req-1"));
        headers.insert(CONTENT_LENGTH, HeaderValue::from_static("42"));
        headers.insert(CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        headers.insert(TRANSFER_ENCODING, HeaderValue::from_static("chunked"));

        let response = build_response_builder(StatusCode::OK, &headers)
            .body(Body::empty())
            .unwrap();

        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "application/json"
        );
        assert_eq!(response.headers().get("x-request-id").unwrap(), "req-1");
        assert!(response.headers().get(CONTENT_LENGTH).is_none());
        assert!(response.headers().get(CONTENT_ENCODING).is_none());
        assert!(response.headers().get(TRANSFER_ENCODING).is_none());
        assert!(should_forward_response_header(&CONTENT_TYPE));
        assert!(!should_forward_response_header(&CONTENT_LENGTH));
    }

    #[test]
    fn next_stream_chunk_timeout_only_applies_before_first_chunk() {
        assert_eq!(
            next_stream_chunk_timeout_duration(0).map(|value| value.as_secs()),
            crate::config::CONFIG
                .proxy_request
                .first_byte_timeout()
                .map(|value| value.as_secs())
        );
        assert_eq!(next_stream_chunk_timeout_duration(1), None);
    }

    #[tokio::test]
    async fn visible_stream_timestamp_is_set_only_for_non_empty_transformed_chunks() {
        let log_context = std::sync::Arc::new(tokio::sync::Mutex::new(make_log_context()));

        mark_stream_response_started_to_client(&log_context, &bytes::Bytes::new()).await;
        assert!(log_context.lock().await.first_chunk_ts.is_none());

        mark_stream_response_started_to_client(
            &log_context,
            &bytes::Bytes::from_static(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
            ),
        )
        .await;
        let first_visible_ts = log_context
            .lock()
            .await
            .first_chunk_ts
            .expect("visible chunk should set timestamp");

        mark_stream_response_started_to_client(
            &log_context,
            &bytes::Bytes::from_static(b"data: [DONE]\n\n"),
        )
        .await;
        assert_eq!(
            log_context.lock().await.first_chunk_ts,
            Some(first_visible_ts)
        );
    }

    #[test]
    fn decode_response_body_decompresses_valid_gzip() {
        let compressed = gzip_bytes(br#"{"ok":true}"#);

        let decoded = decode_response_body(bytes::Bytes::from(compressed), true);

        assert_eq!(decoded, bytes::Bytes::from_static(br#"{"ok":true}"#));
    }

    #[test]
    fn decode_response_body_returns_original_on_invalid_gzip() {
        let original = bytes::Bytes::from_static(b"not-gzip");

        let decoded = decode_response_body(original.clone(), true);

        assert_eq!(decoded, original);
    }

    #[test]
    fn process_success_response_body_transforms_json_and_extracts_usage() {
        let gemini_result = bytes::Bytes::from(
            json!({
                "candidates": [{
                    "index": 0,
                    "content": {
                        "parts": [{"text": "This is a test response from Gemini."}],
                        "role": "model"
                    },
                    "finishReason": "STOP"
                }],
                "usageMetadata": {
                    "promptTokenCount": 10,
                    "candidatesTokenCount": 8,
                    "totalTokenCount": 18
                }
            })
            .to_string(),
        );

        let (final_body, usage, normalization, diagnostics) =
            process_success_response_body(&gemini_result, LlmApiType::Openai, LlmApiType::Gemini);
        let final_value: Value = serde_json::from_slice(&final_body).unwrap();

        assert_eq!(final_value["object"], "chat.completion");
        assert_eq!(
            final_value["choices"][0]["message"]["content"],
            "This is a test response from Gemini."
        );
        assert_eq!(final_value["usage"]["prompt_tokens"], 10);
        assert_eq!(final_value["usage"]["completion_tokens"], 8);
        assert_eq!(
            usage,
            Some(UsageInfo {
                input_tokens: 10,
                output_tokens: 8,
                total_tokens: 18,
                ..Default::default()
            })
        );
        assert_eq!(
            normalization,
            Some(UsageNormalization {
                total_input_tokens: 10,
                total_output_tokens: 8,
                input_text_tokens: 10,
                output_text_tokens: 8,
                ..Default::default()
            })
        );
        assert!(diagnostics.is_empty());
    }

    #[tokio::test]
    async fn send_with_first_byte_timeout_returns_client_cancelled_when_cancelled() {
        let cancellation = ProxyCancellationContext::new();
        cancellation
            .cancel("client hung up before upstream responded")
            .await;

        let client = reqwest::Client::new();
        let result = send_with_first_byte_timeout(
            &cancellation,
            client.get("http://127.0.0.1:9"),
            "LLM request",
        )
        .await;

        assert!(matches!(
            result,
            Err(ProxyError::ClientCancelled(message))
                if message == "client hung up before upstream responded"
        ));
    }

    #[test]
    fn process_success_response_body_passes_through_non_json() {
        let body = bytes::Bytes::from_static(b"plain text response");

        let (final_body, usage, normalization, diagnostics) =
            process_success_response_body(&body, LlmApiType::Openai, LlmApiType::Gemini);

        assert_eq!(final_body, body);
        assert!(usage.is_none());
        assert!(normalization.is_none());
        assert!(diagnostics.is_empty());
    }

    #[tokio::test]
    async fn finalize_non_streaming_log_context_records_error_response() {
        let mut context = make_log_context();
        let cost_catalog_version = CacheCostCatalogVersion {
            id: 5,
            catalog_id: 4,
            version: "v1".to_string(),
            currency: "USD".to_string(),
            source: None,
            effective_from: 0,
            effective_until: None,
            is_enabled: true,
            components: vec![],
        };
        let body = bytes::Bytes::from_static(br#"{"error":"upstream failed"}"#);

        finalize_non_streaming_log_context(
            &mut context,
            "https://example.com/v1/chat",
            StatusCode::BAD_GATEWAY,
            5678,
            Some(&cost_catalog_version),
            RequestStatus::Error,
            None,
            None,
            body.clone(),
            body.clone(),
        );

        assert_eq!(
            context.request_url.as_deref(),
            Some("https://example.com/v1/chat")
        );
        assert_eq!(context.llm_status, Some(StatusCode::BAD_GATEWAY));
        assert_eq!(context.completion_ts, Some(5678));
        assert_eq!(context.overall_status, RequestStatus::Error);
        assert_eq!(context.cost_catalog_version.as_ref().map(|v| v.id), Some(5));
        match context.llm_response_body.as_ref() {
            Some(LoggedBody::InMemory { bytes, .. }) => assert_eq!(bytes, &body),
            other => panic!("unexpected llm_response_body: {other:?}"),
        }
        match context.user_response_body.as_ref() {
            Some(LoggedBody::InMemory { bytes, .. }) => assert_eq!(bytes, &body),
            other => panic!("unexpected user_response_body: {other:?}"),
        }
        assert!(context.usage.is_none());

        let response = axum::response::Response::builder()
            .status(StatusCode::BAD_GATEWAY)
            .body(Body::from(body.clone()))
            .unwrap();
        let returned_body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(returned_body, body);
    }

    #[tokio::test]
    async fn finalize_cancelled_log_context_preserves_existing_usage_and_cost_fields() {
        let app_state = std::sync::Arc::new(AppState::new().await);
        let log_context = std::sync::Arc::new(tokio::sync::Mutex::new(make_log_context()));
        let cost_catalog_version = CacheCostCatalogVersion {
            id: 5,
            catalog_id: 4,
            version: "v1".to_string(),
            currency: "USD".to_string(),
            source: None,
            effective_from: 0,
            effective_until: None,
            is_enabled: true,
            components: vec![],
        };

        {
            let mut context = log_context.lock().await;
            context.usage = Some(UsageInfo {
                input_tokens: 7,
                output_tokens: 16,
                total_tokens: 23,
                reasoning_tokens: 16,
                ..Default::default()
            });
            context.usage_normalization = Some(UsageNormalization {
                total_input_tokens: 7,
                total_output_tokens: 16,
                input_text_tokens: 7,
                output_text_tokens: 16,
                reasoning_tokens: 16,
                ..Default::default()
            });
        }

        finalize_cancelled_log_context(
            &app_state,
            &log_context,
            "https://example.com/v1/chat",
            Some(StatusCode::OK),
            Some(&cost_catalog_version),
            None,
            None,
        )
        .await;

        get_log_manager().flush().await;

        let context = log_context.lock().await;
        assert_eq!(context.overall_status, RequestStatus::Cancelled);
        assert_eq!(context.llm_status, Some(StatusCode::OK));
        assert_eq!(context.cost_catalog_version.as_ref().map(|v| v.id), Some(5));
        assert_eq!(context.usage.as_ref().map(|u| u.input_tokens), Some(7));
        assert_eq!(context.usage.as_ref().map(|u| u.output_tokens), Some(16));
        assert_eq!(
            context
                .usage_normalization
                .as_ref()
                .map(|u| u.total_output_tokens),
            Some(16)
        );
    }

    #[tokio::test]
    async fn streaming_usage_survives_cancellation_after_sse_usage_chunk() {
        let app_state = std::sync::Arc::new(AppState::new().await);
        let log_context = std::sync::Arc::new(tokio::sync::Mutex::new(make_log_context()));
        let cost_catalog_version = CacheCostCatalogVersion {
            id: 5,
            catalog_id: 4,
            version: "v1".to_string(),
            currency: "USD".to_string(),
            source: None,
            effective_from: 0,
            effective_until: None,
            is_enabled: true,
            components: vec![],
        };
        let mut parser = SseParser::new();
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Openai);
        let sse_chunk = concat!(
            "data: {",
            "\"id\":\"chatcmpl-test\",",
            "\"object\":\"chat.completion.chunk\",",
            "\"created\":1776310010,",
            "\"model\":\"deepseek-ai/DeepSeek-V3.2\",",
            "\"choices\":[{\"index\":0,\"delta\":{\"content\":null,\"reasoning_content\":\"分类\",\"role\":\"assistant\"},\"finish_reason\":null}],",
            "\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":16,\"total_tokens\":23,",
            "\"completion_tokens_details\":{\"reasoning_tokens\":16},",
            "\"prompt_tokens_details\":{\"cached_tokens\":0},",
            "\"prompt_cache_hit_tokens\":0,",
            "\"prompt_cache_miss_tokens\":7}",
            "}\n\n"
        );

        let events = parser.process(sse_chunk.as_bytes());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, None);

        let transformed_events = transformer.transform_events(events);
        assert_eq!(transformed_events.len(), 1);

        sync_stream_usage_to_log_context(&log_context, &mut transformer).await;

        finalize_cancelled_log_context(
            &app_state,
            &log_context,
            "https://example.com/v1/chat/completions",
            Some(StatusCode::OK),
            Some(&cost_catalog_version),
            None,
            None,
        )
        .await;

        get_log_manager().flush().await;

        let context = log_context.lock().await;
        assert_eq!(context.overall_status, RequestStatus::Cancelled);
        assert_eq!(context.llm_status, Some(StatusCode::OK));
        assert_eq!(
            context.request_url.as_deref(),
            Some("https://example.com/v1/chat/completions")
        );
        assert_eq!(context.cost_catalog_version.as_ref().map(|v| v.id), Some(5));
        assert_eq!(
            context.usage,
            Some(UsageInfo {
                input_tokens: 7,
                output_tokens: 16,
                total_tokens: 23,
                reasoning_tokens: 16,
                ..Default::default()
            })
        );
        assert_eq!(
            context.usage_normalization,
            Some(UsageNormalization {
                total_input_tokens: 7,
                total_output_tokens: 16,
                input_text_tokens: 7,
                output_text_tokens: 0,
                reasoning_tokens: 16,
                ..Default::default()
            })
        );
    }
}
