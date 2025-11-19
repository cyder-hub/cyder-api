use super::logging::log_final_update;
use super::util::serialize_reqwest_headers;

use crate::config::CONFIG;
use crate::controller::llm_types::LlmApiType;
use crate::database::price::PriceRule;
use crate::database::request_log::{RequestLog, UpdateRequestLogData};
use crate::schema::enum_def::RequestStatus;
use crate::service::transform::{transform_result, StreamTransformer};
use crate::utils::billing::parse_usage_info;
use crate::utils::split_chunks;

use axum::{
    body::{Body, Bytes},
    response::Response,
};
use chrono::Utc;
use cyder_tools::log::{debug, error, info, warn};
use flate2::read::GzDecoder;
use futures::StreamExt;
use reqwest::{
    header::{CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_TYPE, TRANSFER_ENCODING},
    Method, Proxy, StatusCode,
};
use serde_json::Value;
use std::io::Read;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

// A guard to log a warning if the request is cancelled before completion.
pub(super) struct CancellationGuard {
    log_id: i64,
    is_armed: bool,
}

impl CancellationGuard {
    pub(super) fn new(log_id: i64) -> Self {
        Self {
            log_id,
            is_armed: true,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.is_armed = false;
    }
}

impl Drop for CancellationGuard {
    fn drop(&mut self) {
        if self.is_armed {
            warn!(
                "Request for log_id {} was cancelled by the client.",
                self.log_id
            );
            let update_data = UpdateRequestLogData {
                status: Some(RequestStatus::Cancelled),
                response_sent_to_client_at: Some(Utc::now().timestamp_millis()),
                ..Default::default()
            };
            if let Err(e) =
                RequestLog::update_request_with_completion_details(self.log_id, &update_data)
            {
                error!(
                    "Failed to update request log status to CANCELLED for log_id {}: {:?}",
                    self.log_id, e
                );
            }
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

    let is_sse = response_headers
        .get(CONTENT_TYPE)
        .map_or(false, |value| {
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
        Ok(response_builder.body(Body::from(decompressed_body)).unwrap())
    }
}

// Builds the HTTP client, sends the request to the LLM, and passes the response to be handled.
pub(super) async fn proxy_request(
    log_id: i64,
    url: String,
    data: String,
    headers: reqwest::header::HeaderMap,
    model_str: String,
    use_proxy: bool,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    info!(
        "Starting proxy request for log_id {}, model {}",
        log_id, model_str
    );

    // 1. Build HTTP client, with proxy if configured
    let client = build_reqwest_client(use_proxy)?;

    let mut cancellation_guard = CancellationGuard::new(log_id);

    debug!(
        "[proxy] proxy request header: {:?}",
        serialize_reqwest_headers(&headers)
    );
    debug!("[proxy] proxy request data: {}", &data);

    // 2. Send request to LLM
    let response = match client
        .request(Method::POST, &url)
        .headers(headers)
        .body(data.clone()) // Clone here for potential retries or logging
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            cancellation_guard.disarm();
            let error_message = format!("LLM request failed: {}", e);
            error!("{}", error_message);
            let completed_at = Utc::now().timestamp_millis();
            log_final_update(
                log_id,
                "LLM send error",
                &url,
                &data,
                None,
                Some(None),
                false,
                None,
                completed_at,
                None,
                &price_rules,
                currency.as_deref(),
                Some(RequestStatus::Error),
            );
            return Err((StatusCode::BAD_GATEWAY, error_message));
        }
    };

    // 3. Process the response stream
    let result = handle_llm_response(
        log_id,
        model_str,
        response,
        &url,
        &data,
        price_rules,
        currency,
        api_type,
        target_api_type,
    )
    .await;
    cancellation_guard.disarm();
    result
}

// Handles a non-streaming response from the LLM.
async fn handle_non_streaming_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
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
        // Do not forward content-length, content-encoding, or transfer-encoding.
        // Axum will set the correct content-length.
        // We handle decompression, so we don't want to forward the original encoding.
        // We are not streaming chunk-by-chunk from the origin, so we don't forward transfer-encoding.
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
            log_final_update(
                log_id,
                "LLM body read error",
                url,
                data,
                Some(status_code),
                Some(Some(err_msg.clone())),
                false,
                None,
                completed_at,
                None,
                &price_rules,
                currency.as_deref(),
                Some(RequestStatus::Error),
            );
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
                    error!("Gzip decoding failed for log_id {}: {}", log_id, e);
                    body_bytes // return original if decode fails
                }
            }
        }
    } else {
        body_bytes
    };
    debug!(
        "[handle_non_streaming_response] decompressed body: {}",
        String::from_utf8_lossy(&decompressed_body)
    );
    let llm_response_completed_at = Utc::now().timestamp_millis();

    if status_code.is_success() {
        let parsed_usage_info = serde_json::from_slice::<Value>(&decompressed_body)
            .ok()
            .and_then(|val| parse_usage_info(&val, target_api_type));

        log_final_update(
            log_id,
            "Non-SSE success",
            url,
            data,
            Some(status_code),
            Some(None),
            false,
            None,
            llm_response_completed_at,
            parsed_usage_info.as_ref(),
            &price_rules,
            currency.as_deref(),
            Some(RequestStatus::Success),
        );
        info!(
            "{}: Non-SSE request completed for log_id {}.",
            model_str, log_id
        );

        let final_body = if api_type != target_api_type {
            // Transformation is needed
            let original_value: Value = match serde_json::from_slice(&decompressed_body) {
                Ok(v) => v,
                Err(e) => {
                    // If we can't parse the body, we can't transform it. Return original.
                    error!(
                        "Failed to parse LLM response for transformation: {}. Returning original body.",
                        e
                    );
                    return Ok(response_builder.body(Body::from(decompressed_body)).unwrap());
                }
            };

            let transformed_value = transform_result(original_value, target_api_type, api_type);

            match serde_json::to_vec(&transformed_value) {
                Ok(b) => Bytes::from(b),
                Err(e) => {
                    error!(
                        "Failed to serialize transformed response: {}. Returning original body.",
                        e
                    );
                    decompressed_body
                }
            }
        } else {
            // No transformation needed
            decompressed_body
        };

        Ok(response_builder.body(Body::from(final_body)).unwrap())
    } else {
        let error_body_str = String::from_utf8_lossy(&decompressed_body).into_owned();
        error!(
            "LLM request failed with status {} for log_id {}: {}",
            status_code, log_id, &error_body_str
        );
        log_final_update(
            log_id,
            "LLM error status",
            url,
            data,
            Some(status_code),
            Some(Some(error_body_str.clone())),
            false,
            None,
            llm_response_completed_at,
            None,
            &price_rules,
            currency.as_deref(),
            Some(RequestStatus::Error),
        );

        Ok(response_builder.body(Body::from(error_body_str)).unwrap())
    }
}

// Handles a streaming (SSE) response from the LLM.
async fn handle_streaming_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let status_code = response.status();
    let response_headers = response.headers().clone();

    let mut response_builder = Response::builder().status(status_code);
    for (name, value) in response_headers.iter() {
        // Do not forward content-length, content-encoding, or transfer-encoding.
        // Axum will set the correct content-length for the stream.
        // We are not forwarding the original encoding.
        // We are re-streaming, so we don't forward transfer-encoding.
        if name != CONTENT_LENGTH && name != CONTENT_ENCODING && name != TRANSFER_ENCODING {
            response_builder = response_builder.header(name, value);
        }
    }

    let mut first_chunk_received_at_proxy: i64 = 0;
    let latest_chunk_arc: Arc<Mutex<Option<Bytes>>> = Arc::new(Mutex::new(None));
    let logged_sse_success = Arc::new(Mutex::new(false));
    let (tx, mut rx) = mpsc::channel::<Result<bytes::Bytes, reqwest::Error>>(10);

    let url_owned = url.to_string();
    let data_owned = data.to_string();
    let logged_sse_success_clone = logged_sse_success.clone();

    tokio::spawn(async move {
        let mut stream = response.bytes_stream();
        while let Some(chunk_result) = stream.next().await {
            if tx.send(chunk_result).await.is_err() {
                break;
            }
        }
    });

    let mut transformer = StreamTransformer::new(target_api_type, api_type);

    let monitored_stream = async_stream::stream! {
        let mut remainder = Bytes::new();
        while let Some(chunk_result) = rx.recv().await {
            match chunk_result {
                Ok(chunk) => {
                    if first_chunk_received_at_proxy == 0 {
                        first_chunk_received_at_proxy = Utc::now().timestamp_millis();
                    }

                    let current_chunk = if remainder.is_empty() {
                        chunk
                    } else {
                        Bytes::from([remainder.as_ref(), chunk.as_ref()].concat())
                    };
                    let (lines, new_remainder) = split_chunks(current_chunk);
                    remainder = new_remainder;

                    if lines.is_empty() {
                        continue;
                    }

                    let mut transformed_sub_chunks_as_strings: Vec<String> = Vec::new();
                    for sub_chunk in &lines {
                        // This part is for logging usage from the final chunk of an OpenAI stream.
                        if target_api_type == LlmApiType::OpenAI {
                            let line_str = String::from_utf8_lossy(sub_chunk);
                            if line_str.trim() == "data: [DONE]" {
                                if let Some(final_data_chunk) = latest_chunk_arc.lock().unwrap().take() {
                                    let final_line_str = String::from_utf8_lossy(&final_data_chunk);
                                    if let Some(data_json_str) = final_line_str.strip_prefix("data:").map(str::trim) {
                                        if let Ok(data_value) = serde_json::from_str::<Value>(data_json_str) {
                                            let parsed_usage_info = parse_usage_info(&data_value, target_api_type);
                                            let completed_at = Utc::now().timestamp_millis();
                                            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
                                            log_final_update(log_id, "SSE DONE", &url_owned, &data_owned, Some(status_code), Some(None), true, first_chunk_ts, completed_at, parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestStatus::Success));
                                            *logged_sse_success_clone.lock().unwrap() = true;
                                            info!("{}: SSE stream completed.", model_str);
                                        }
                                    }
                                }
                            } else if line_str.starts_with("data:") {
                                *latest_chunk_arc.lock().unwrap() = Some(sub_chunk.clone());
                            }
                        }

                        let transformed_bytes_opt = transformer.transform_chunk(sub_chunk.clone());

                        if let Some(transformed_bytes) = transformed_bytes_opt {
                            let s = if transformed_bytes.is_empty() {
                                String::new()
                            } else {
                                if api_type == LlmApiType::OpenAI && target_api_type == LlmApiType::OpenAI {
                                    format!("{}\n\n", String::from_utf8_lossy(&transformed_bytes))
                                } else {
                                    String::from_utf8_lossy(&transformed_bytes).to_string()
                                }
                            };
                            transformed_sub_chunks_as_strings.push(s);
                        }
                    }

                    let transformed_chunk = Bytes::from(transformed_sub_chunks_as_strings.concat());

                    // Forward the transformed chunk to the client
                    if !transformed_chunk.is_empty() {
                        yield Ok::<_, std::io::Error>(transformed_chunk);
                    }
                }
                Err(e) => {
                    let stream_error_message = format!("LLM stream error: {}", e);
                    error!("{}", stream_error_message);
                    let completed_at = Utc::now().timestamp_millis();
                    let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

                    log_final_update(
                        log_id, "LLM stream error", &url_owned, &data_owned, Some(status_code),
                        Some(None), true, first_chunk_ts, completed_at, None, &price_rules, currency.as_deref(), Some(RequestStatus::Error),
                    );
                    yield Err(std::io::Error::new(std::io::ErrorKind::Other, stream_error_message));
                    break;
                }
            }
        }

        if status_code.is_success() && !remainder.is_empty() {
            let line_str = String::from_utf8_lossy(&remainder);
            if line_str.starts_with("data:") {
                *latest_chunk_arc.lock().unwrap() = Some(remainder);
            }
        }

        // After the upstream is closed, check if we need to send a [DONE] message.
        if status_code.is_success() && api_type == LlmApiType::OpenAI && target_api_type == LlmApiType::Gemini {
            // The client expects an OpenAI stream, which must end with [DONE].
            // The Gemini stream we just consumed doesn't have this, so we add it.
            debug!("[handle_streaming_response] Appending [DONE] chunk for OpenAI client.");
            let done_chunk = Bytes::from("data: [DONE]\n\n");
            yield Ok::<_, std::io::Error>(done_chunk);
        }

        let llm_response_completed_at = Utc::now().timestamp_millis();

        if status_code.is_success() && !*logged_sse_success.lock().unwrap() {
            if let Some(final_data_chunk) = latest_chunk_arc.lock().unwrap().take() {
                let final_line_str = String::from_utf8_lossy(&final_data_chunk);
                if let Some(data_json_str) = final_line_str.strip_prefix("data:").map(str::trim) {
                    if let Ok(data_value) = serde_json::from_str::<Value>(data_json_str) {
                        debug!("[proxy] final response from stream end {:?}", data_value);
                        let parsed_usage_info = parse_usage_info(&data_value, target_api_type);
                        let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };

                        log_final_update(
                            log_id, "SSE stream end", &url_owned, &data_owned, Some(status_code),
                            Some(None), true, first_chunk_ts, llm_response_completed_at,
                            parsed_usage_info.as_ref(), &price_rules, currency.as_deref(), Some(RequestStatus::Success),
                        );
                        info!("{}: SSE stream completed at stream end.", model_str);
                    } else {
                        error!("Failed to parse final SSE data JSON at stream end for log_id {}: {}", log_id, data_json_str);
                    }
                }
            } else {
                info!("{}: SSE stream completed without a final data chunk to parse.", model_str);
                let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
                log_final_update(
                    log_id, "SSE stream end (no final chunk)", &url_owned, &data_owned, Some(status_code),
                    Some(None), true, first_chunk_ts, llm_response_completed_at,
                    None, &price_rules, currency.as_deref(), Some(RequestStatus::Success),
                );
            }
        } else if !status_code.is_success() {
            let error_body_str = "Error during stream".to_string();
            let first_chunk_ts = if first_chunk_received_at_proxy == 0 { None } else { Some(first_chunk_received_at_proxy) };
            log_final_update(
                log_id, "LLM error status", &url_owned, &data_owned, Some(status_code),
                Some(Some(error_body_str)), true, first_chunk_ts,
                llm_response_completed_at, None, &price_rules, currency.as_deref(), Some(RequestStatus::Error),
            );
        }
    };

    match response_builder.body(Body::from_stream(monitored_stream)) {
        Ok(final_response) => Ok(final_response),
        Err(e) => {
            let error_message =
                format!("Failed to build client response for log_id {}: {}", log_id, e);
            error!("{}", error_message);
            Err((StatusCode::INTERNAL_SERVER_ERROR, error_message))
        }
    }
}

// Dispatches to the correct response handler based on whether the response is a stream.
pub(super) async fn handle_llm_response(
    log_id: i64,
    model_str: String,
    response: reqwest::Response,
    url: &str,
    data: &str,
    price_rules: Vec<PriceRule>,
    currency: Option<String>,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> Result<Response<Body>, (StatusCode, String)> {
    let is_sse = response
        .headers()
        .get(CONTENT_TYPE)
        .map_or(false, |value| {
            value.to_str().unwrap_or("").contains("text/event-stream")
        });

    if let Err(e) = RequestLog::update_request_with_completion_details(
        log_id,
        &UpdateRequestLogData {
            is_stream: Some(is_sse),
            ..Default::default()
        },
    ) {
        error!(
            "Failed to update request log (is_stream) for log_id {}: {:?}",
            log_id, e
        );
    }

    if is_sse {
        handle_streaming_response(
            log_id,
            model_str,
            response,
            url,
            data,
            price_rules,
            currency,
            api_type,
            target_api_type,
        )
        .await
    } else {
        handle_non_streaming_response(
            log_id,
            model_str,
            response,
            url,
            data,
            price_rules,
            currency,
            api_type,
            target_api_type,
        )
        .await
    }
}
