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
pub(crate) use response::process_success_response_body;

use self::{
    cancellation::RequestLogContextGuard, non_stream::handle_non_streaming_response,
    stream::handle_streaming_response,
};
use crate::{
    proxy::{
        ProxyError,
        cancellation::{CancellationDropGuard, ProxyCancellationContext},
        logging::RequestLogContext,
        provider_governance::record_provider_failure,
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::record_immediate_completion_if_allowed,
            policy::{RuntimeExecutionPolicy, RuntimeLogMode},
        },
        util::serialize_upstream_response_headers_for_log,
    },
    schema::enum_def::{LlmApiType, RequestStatus},
    service::runtime::ProviderCircuitProbePermit,
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
    pub response_headers: Option<HeaderMap>,
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
    log_mode: RuntimeLogMode,
    execution_policy: RuntimeExecutionPolicy,
) -> Result<ProxyRequestOutcome, ProxyRequestFailure> {
    let provider_id = log_context.provider_id;
    let log_context = Arc::new(TokioMutex::new(log_context));

    let client_bundle = app_state.infra.client_bundle().await;
    let first_byte_timeout = client_bundle.proxy_request.first_byte_timeout();
    let client = if use_proxy {
        Arc::clone(&client_bundle.proxy_client)
    } else {
        Arc::clone(&client_bundle.client)
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
            cancellation_guard.disarm();
            if execution_policy.records_provider_runtime()
                && !matches!(proxy_error, ProxyError::ClientCancelled(_))
            {
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
            context.request_url = Some(url.clone());
            context.completion_ts = Some(completed_at);
            context.cost_catalog_version = cost_catalog_version.clone();
            context.overall_status = if matches!(proxy_error, ProxyError::ClientCancelled(_)) {
                RequestStatus::Cancelled
            } else {
                RequestStatus::Error
            };
            record_immediate_completion_if_allowed(
                &app_state,
                &context,
                log_mode,
                execution_policy,
            )
            .await;
            api_key_request_lease.release().await;

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
            log_mode,
            execution_policy,
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
            api_key_request_lease,
            provider_circuit_permit,
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

#[cfg(test)]
mod tests {
    use super::{
        client::send_with_first_byte_timeout,
        response::{
            build_response_builder, decode_response_body, process_success_response_body,
            should_forward_response_header,
        },
        stream::{
            mark_stream_response_started_to_client, next_stream_chunk_timeout_duration,
            sync_stream_usage_to_log_context,
        },
    };
    use crate::{
        cost::UsageNormalization,
        proxy::{
            ProxyError,
            cancellation::ProxyCancellationContext,
            logging::{LoggedBody, RequestLogContext},
            runtime::log_writer::{
                finalize_cancelled_log_context, finalize_non_streaming_log_context,
            },
            runtime::policy::RuntimeExecutionPolicy,
        },
        schema::enum_def::{LlmApiType, ProviderApiKeyMode, ProviderType, RequestStatus},
        service::{
            app_state::AppState,
            cache::types::{CacheApiKey, CacheCostCatalogVersion, CacheModel, CacheProvider},
            transform::StreamTransformer,
        },
        utils::{sse::SseParser, usage::UsageInfo},
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
        let api_key = CacheApiKey {
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
            &api_key,
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
        let first_byte_timeout = Some(std::time::Duration::from_secs(7));
        assert_eq!(
            next_stream_chunk_timeout_duration(0, first_byte_timeout).map(|value| value.as_secs()),
            Some(7)
        );
        assert_eq!(
            next_stream_chunk_timeout_duration(1, first_byte_timeout),
            None
        );
        assert_eq!(next_stream_chunk_timeout_duration(0, None), None);
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
        cancellation.cancel_now("client hung up before upstream responded");

        let client = reqwest::Client::new();
        let result = send_with_first_byte_timeout(
            &cancellation,
            client.get("http://127.0.0.1:9"),
            "LLM request",
            Some(std::time::Duration::from_secs(1)),
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

        let persisted = finalize_cancelled_log_context(
            &app_state,
            &log_context,
            "https://example.com/v1/chat",
            Some(StatusCode::OK),
            Some(&cost_catalog_version),
            None,
            None,
            RuntimeExecutionPolicy::Normal,
        )
        .await;

        assert!(persisted);
        app_state.flush_proxy_logs().await;

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
    async fn finalize_cancelled_log_context_respects_replay_execution_policy() {
        let app_state = std::sync::Arc::new(AppState::new().await);
        let log_context = std::sync::Arc::new(tokio::sync::Mutex::new(make_log_context()));

        let persisted = finalize_cancelled_log_context(
            &app_state,
            &log_context,
            "https://example.com/v1/chat",
            Some(StatusCode::OK),
            None,
            None,
            None,
            RuntimeExecutionPolicy::ReplayLive,
        )
        .await;

        assert!(!persisted);
        let context = log_context.lock().await;
        assert_eq!(context.overall_status, RequestStatus::Cancelled);
        assert_eq!(context.llm_status, Some(StatusCode::OK));
        assert_eq!(
            context.request_url.as_deref(),
            Some("https://example.com/v1/chat")
        );
    }

    #[tokio::test]
    async fn streaming_usage_sync_ignores_missing_intermediate_usage() {
        let log_context = std::sync::Arc::new(tokio::sync::Mutex::new(make_log_context()));
        let mut parser = SseParser::new();
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Openai);
        let sse_chunk = concat!(
            "data: {",
            "\"id\":\"chatcmpl-test\",",
            "\"object\":\"chat.completion.chunk\",",
            "\"created\":1776310010,",
            "\"model\":\"deepseek-ai/DeepSeek-V3.2\",",
            "\"choices\":[{\"index\":0,\"delta\":{\"content\":\"hello\",\"role\":\"assistant\"},\"finish_reason\":null}]",
            "}\n\n"
        );

        let events = parser.process(sse_chunk.as_bytes());
        assert_eq!(events.len(), 1);
        let transformed_events = transformer.transform_events(events);
        assert_eq!(transformed_events.len(), 1);

        sync_stream_usage_to_log_context(&log_context, &mut transformer).await;

        assert!(transformer.diagnostics_snapshot().is_empty());
        let context = log_context.lock().await;
        assert_eq!(context.usage, None);
        assert_eq!(context.usage_normalization, None);
        assert!(context.transform_diagnostics.is_empty());
    }

    #[tokio::test]
    async fn streaming_final_usage_parse_records_missing_usage_diagnostic() {
        let mut parser = SseParser::new();
        let mut transformer = StreamTransformer::new(LlmApiType::Openai, LlmApiType::Openai);
        let sse_chunk = concat!(
            "data: {",
            "\"id\":\"chatcmpl-test\",",
            "\"object\":\"chat.completion.chunk\",",
            "\"created\":1776310010,",
            "\"model\":\"deepseek-ai/DeepSeek-V3.2\",",
            "\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]",
            "}\n\n",
            "data: [DONE]\n\n"
        );

        let events = parser.process(sse_chunk.as_bytes());
        assert_eq!(events.len(), 2);
        let transformed_events = transformer.transform_events(events);
        assert_eq!(transformed_events.len(), 2);

        assert_eq!(transformer.parse_usage_info(), None);
        assert_eq!(transformer.diagnostics_snapshot().len(), 1);
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

        let persisted = finalize_cancelled_log_context(
            &app_state,
            &log_context,
            "https://example.com/v1/chat/completions",
            Some(StatusCode::OK),
            Some(&cost_catalog_version),
            None,
            None,
            RuntimeExecutionPolicy::Normal,
        )
        .await;

        assert!(persisted);
        app_state.flush_proxy_logs().await;

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
