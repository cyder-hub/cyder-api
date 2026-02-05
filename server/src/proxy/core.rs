use super::logging::{get_log_manager, RequestLogContext};
use super::util::serialize_reqwest_headers;

use crate::config::CONFIG;
use crate::schema::enum_def::{LlmApiType, RequestStatus};
use crate::service::cache::types::CacheBillingPlan;
use crate::service::transform::{transform_result, StreamTransformer};
use crate::utils::sse::SseParser;

use axum::{
    body::{Body, Bytes},
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error, info, warn};
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::{
    header::{HeaderMap, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
    Method, Proxy, StatusCode,
};
use serde_json::Value;
use std::io::Read;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex as TokioMutex};

struct RequestLogContextGuard {
    context: Arc<TokioMutex<RequestLogContext>>,
    is_armed: bool,
}

impl RequestLogContextGuard {
    fn new(context: Arc<TokioMutex<RequestLogContext>>) -> Self {
        Self {
            context,
            is_armed: true,
        }
    }

    fn disarm(&mut self) {
        self.is_armed = false;
    }
}

impl Drop for RequestLogContextGuard {
    fn drop(&mut self) {
        if self.is_armed {
            let context_clone = Arc::clone(&self.context);
            tokio::spawn(async move {
                let mut context = context_clone.lock().await;
                warn!(
                    "Request for log_id {} was cancelled by the client.",
                    context.id
                );
                context.overall_status = RequestStatus::Cancelled;
                context.completion_ts = Some(Utc::now().timestamp_millis());
                get_log_manager().log(context.clone()).await;
            });
        }
    }
}

pub(super) fn build_reqwest_client(
    use_proxy: bool,
) -> Result<reqwest::Client, (StatusCode, String)> {
    let mut client_builder = reqwest::Client::builder();
    if use_proxy {
        if let Some(proxy_url) = &CONFIG.proxy {
            let proxy = Proxy::https(proxy_url).map_err(|e| {
                error!("Invalid proxy URL '{}': {}", proxy_url, e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Invalid proxy configuration".to_string(),
                )
            })?;
            client_builder = client_builder.proxy(proxy);
        }
    }
    client_builder.build().map_err(|e| {
        error!("Failed to build reqwest client: {}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to build HTTP client".to_string(),
        )
    })
}

// A simple proxy that sends a request and returns the response, handling streaming and gzip.
// It does not perform logging or response transformation.
pub(super) async fn simple_proxy_request(
    url: String,
    data: String,
    headers: reqwest::header::HeaderMap,
    use_proxy: bool,
) -> Result<Response<Body>, (StatusCode, String)> {
    let client = build_reqwest_client(use_proxy)?;

    debug!(
        "[simple_proxy_request] request header: {:?}",
        serialize_reqwest_headers(&headers)
    );
    debug!("[simple_proxy_request] request data: {}", &data);

    let response = match client
        .request(Method::POST, &url)
        .headers(headers)
        .body(data)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            let error_message = format!("LLM request failed: {}", e);
            error!("{}", error_message);
            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    let status_code = response.status();
    let response_headers = response.headers().clone();
    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

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
                let err_msg = format!("Failed to read LLM response body: {}", e);
                error!("[simple_proxy_request] {}", err_msg);
                return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
            }
        };

        let decompressed_body = if is_gzip {
            if body_bytes.is_empty() {
                Bytes::new()
            } else {
                let mut gz = GzDecoder::new(&body_bytes[..]);
                let mut decompressed_data = Vec::new();
                match gz.read_to_end(&mut decompressed_data) {
                    Ok(_) => Bytes::from(decompressed_data),
                    Err(e) => {
                        error!("Gzip decoding failed in simple_proxy_request: {}", e);
                        body_bytes // return original if decode fails
                    }
                }
            }
        } else {
            body_bytes
        };
        Ok(response_builder
            .body(Body::from(decompressed_body))
            .unwrap())
    }
}

// Builds the HTTP client, sends the request to the LLM, and passes the response to be handled.
pub(super) async fn proxy_request(
    log_context: RequestLogContext,
    url: String,
    data: Bytes,
    headers: HeaderMap,
    model_str: String,
    use_proxy: bool,
    billing_plan: Option<CacheBillingPlan>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    info!(
        "Starting proxy request for log_id {}, model {}",
        log_context.id, model_str
    );
    let log_context = Arc::new(TokioMutex::new(log_context));

    // 1. Build HTTP client, with proxy if configured
    let client = build_reqwest_client(use_proxy)?;

    let mut cancellation_guard = RequestLogContextGuard::new(log_context.clone());

    // 2. Send request to LLM
    log_context.lock().await.llm_request_sent_at = Some(Utc::now().timestamp_millis());
    let response = match client
        .request(Method::POST, &url)
        .headers(headers)
        .body(data) // Clone here for potential retries or logging
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            cancellation_guard.disarm();
            let error_message = format!("LLM request failed: {}", e);
            error!("{}", error_message);
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            context.request_url = Some(url.clone());
            context.completion_ts = Some(completed_at);
            context.billing_plan = billing_plan.clone();
            context.overall_status = RequestStatus::Error;
            get_log_manager().log(context.clone()).await;

            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    // 3. Process the response stream
    let is_sse = response
        .headers()
        .get(CONTENT_TYPE)
        .map_or(false, |value| {
            value.to_str().unwrap_or("").contains("text/event-stream")
        });

    {
        let mut context = log_context.lock().await;
        context.is_stream = is_sse;
    }

    let result = if is_sse {
        handle_streaming_response(
            log_context,
            model_str,
            response,
            &url,
            billing_plan,
            api_type,
            target_api_type,
        )
        .await
    } else {
        handle_non_streaming_response(
            log_context,
            model_str,
            response,
            &url,
            billing_plan.as_ref(),
            api_type,
            target_api_type,
        )
        .await
    };
    cancellation_guard.disarm();
    result
}

// Handles a non-streaming response from the LLM.
async fn handle_non_streaming_response(
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    billing_plan: Option<&CacheBillingPlan>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let status_code = response.status();
    let response_headers = response.headers().clone();
    debug!(
        "[handle_non_streaming_response] response headers: {:?}",
        response_headers
    );
    let is_gzip = response_headers
        .get(CONTENT_ENCODING)
        .map_or(false, |value| value.to_str().unwrap_or("").contains("gzip"));

    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let body_bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            let err_msg = format!("Failed to read LLM response body: {}", e);
            error!("[handle_non_streaming_response] {}", err_msg);
            let completed_at = Utc::now().timestamp_millis();

            let mut context = log_context.lock().await;
            context.request_url = Some(url.to_string());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(completed_at);
            context.billing_plan = billing_plan.cloned();
            context.overall_status = RequestStatus::Error;
            context.llm_response_body = Some(Bytes::from(err_msg.clone()));
            get_log_manager().log(context.clone()).await;

            return Err((StatusCode::INTERNAL_SERVER_ERROR, err_msg));
        }
    };

    let decompressed_body = if is_gzip {
        if body_bytes.is_empty() {
            Bytes::new()
        } else {
            let mut gz = GzDecoder::new(&body_bytes[..]);
            let mut decompressed_data = Vec::new();
            match gz.read_to_end(&mut decompressed_data) {
                Ok(_) => Bytes::from(decompressed_data),
                Err(e) => {
                    let log_id = log_context.lock().await.id;
                    error!("Gzip decoding failed for log_id {}: {}", log_id, e);
                    body_bytes // return original if decode fails
                }
            }
        }
    } else {
        body_bytes
    };
    let llm_response_completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        let (final_body, parsed_usage_info) =
            match serde_json::from_slice::<Value>(&decompressed_body) {
                Ok(original_value) => {
                    let (transformed_value, usage_info) =
                        transform_result(original_value, target_api_type, api_type);

                    // OPTIMIZATION: If the API type is the same, no transformation occurred.
                    // We can return the original, untouched body and avoid re-serializing.
                    let body_bytes = if api_type == target_api_type {
                        decompressed_body.clone()
                    } else {
                        match serde_json::to_vec(&transformed_value) {
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
                    (body_bytes, usage_info)
                }
                Err(e) => {
                    // response is not JSON. No transformation or usage parsing possible.
                    debug!(
                        "Response body is not valid JSON, cannot parse usage or transform: {}. Body: {:?}",
                        e, String::from_utf8_lossy(&decompressed_body)
                    );
                    (decompressed_body.clone(), None)
                }
            };

        let mut context = log_context.lock().await;
        context.request_url = Some(url.to_string());
        context.llm_status = Some(status_code);
        context.completion_ts = Some(llm_response_completed_at);
        context.usage = parsed_usage_info;
        context.billing_plan = billing_plan.cloned();
        context.overall_status = RequestStatus::Success;
        context.llm_response_body = Some(decompressed_body.clone());
        context.user_response_body = Some(final_body.clone());
        get_log_manager().log(context.clone()).await;

        info!(
            "{}: Non-SSE request completed for log_id {}.",
            model_str, context.id
        );

        Ok(response_builder.body(Body::from(final_body)).unwrap())
    } else {
        let error_body_str = String::from_utf8_lossy(&decompressed_body).into_owned();
        let mut context = log_context.lock().await;
        error!(
            "LLM request failed with status {} for log_id {}: {}",
            status_code, context.id, &error_body_str
        );

        context.request_url = Some(url.to_string());
        context.llm_status = Some(status_code);
        context.completion_ts = Some(llm_response_completed_at);
        context.billing_plan = billing_plan.cloned();
        context.overall_status = RequestStatus::Error;
        context.llm_response_body = Some(decompressed_body.clone());
        context.user_response_body = Some(decompressed_body.clone());
        get_log_manager().log(context.clone()).await;

        Ok(response_builder
            .body(Body::from(decompressed_body))
            .unwrap())
    }
}

// Handles a streaming (SSE) response from the LLM.
async fn handle_streaming_response(
    log_context: Arc<TokioMutex<RequestLogContext>>,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    billing_plan: Option<CacheBillingPlan>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let status_code = response.status();
    let response_headers = response.headers().clone();

    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    let url_owned = url.to_string();
    let billing_plan_clone = billing_plan.clone();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            if tx.send(chunk_result).await.is_err() {
                break;
            }
        }
    });

    let mut transformer = StreamTransformer::new(target_api_type, api_type);
    let mut parser = SseParser::new();
    let log_context_clone = log_context.clone();

    let monitored_stream = async_stream::stream! {
        let mut first_chunk_received_at_proxy: i64 = 0;
        let mut llm_body_aggregator: Vec<u8> = Vec::new();
        let mut user_body_aggregator: Vec<u8> = Vec::new();

        while let Some(chunk_result) = rx.recv().await {
            match chunk_result {
                Ok(chunk) => {
                    llm_body_aggregator.extend_from_slice(&chunk);

                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                        let mut context = log_context_clone.lock().await;
                        context.first_chunk_ts = Some(first_chunk_received_at_proxy);
                    }

                    let events = parser.process(&chunk);
                    if events.is_empty() {
                        continue;
                    }

                    let transformed_events = transformer.transform_events(events);
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
                    user_body_aggregator.extend_from_slice(&transformed_chunk);

                    if !transformed_chunk.is_empty() {
                        yield Ok::<_, std::io::Error>(transformed_chunk);
                    }
                }
                Err(e) => {
                    let stream_error_message = format!("LLM stream error: {}", e);
                    error!("{}", stream_error_message);
                    let completed_at = Utc::now().timestamp_millis();

                    let mut context = log_context_clone.lock().await;
                    context.request_url = Some(url_owned.clone());
                    context.llm_status = Some(status_code);
                    context.completion_ts = Some(completed_at);
                    context.billing_plan = billing_plan_clone.clone();
                    context.overall_status = RequestStatus::Error;
                    context.llm_response_body = Some(Bytes::from(llm_body_aggregator.clone()));
                    context.user_response_body = Some(Bytes::from(user_body_aggregator.clone()));
                    get_log_manager().log(context.clone()).await;

                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, stream_error_message));
                    break;
                }
            }
        }

        if status_code.is_success() && api_type == LlmApiType::Openai && target_api_type == LlmApiType::Gemini {
            debug!("[handle_streaming_response] Appending [DONE] chunk for OpenAI client.");
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            user_body_aggregator.extend_from_slice(&done_chunk);
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() {
            let mut context = log_context_clone.lock().await;
            context.request_url = Some(url_owned.clone());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(llm_response_completed_at);
            context.billing_plan = billing_plan_clone.clone();
            context.overall_status = RequestStatus::Success;
            context.llm_response_body = Some(Bytes::from(llm_body_aggregator));
            context.user_response_body = Some(Bytes::from(user_body_aggregator));

            context.usage = transformer.parse_usage_info();
            if context.usage.is_none() {
                info!("{}: SSE stream completed without usage info.", model_str);
            }

            get_log_manager().log(context.clone()).await;
            info!("{}: SSE stream completed.", model_str);

        } else { // !status_code.is_success()
            let mut context = log_context_clone.lock().await;
            context.request_url = Some(url_owned.clone());
            context.llm_status = Some(status_code);
            context.completion_ts = Some(llm_response_completed_at);
            context.billing_plan = billing_plan_clone.clone();
            context.overall_status = RequestStatus::Error;
            context.llm_response_body = Some(Bytes::from(llm_body_aggregator));
            context.user_response_body = Some(Bytes::from(user_body_aggregator));
            get_log_manager().log(context.clone()).await;
        }
    };

    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => Ok(final_response),
        Err(e) => {
            let log_id = log_context.lock().await.id;
            let error_message = format!(
                "Failed to build client response for log_id {}: {}",
                log_id, e
            );
            error!("{}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}
