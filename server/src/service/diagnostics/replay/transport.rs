use std::sync::Arc;

use bytes::Bytes;
use chrono::Utc;
use reqwest::{Method, header::CONTENT_TYPE, header::HeaderMap};

use crate::{
    controller::BaseError,
    cost::UsageNormalization,
    proxy::{
        ProxyCancellationContext, ProxyError, apply_provider_request_auth_header,
        classify_reqwest_error, classify_upstream_status, process_success_response_body,
        send_with_first_byte_timeout,
    },
    schema::enum_def::{LlmApiType, RequestReplayStatus},
    service::{
        app_state::AppState,
        diagnostics::{
            body::{
                REPLAY_BODY_CAPTURE_NOT_CAPTURED, body_from_bytes, build_replay_request_headers,
                log_capture_state_to_string, read_replay_response_body_bounded,
                replay_body_capture_metadata, replay_response_capture_limit,
                serialize_headers_for_output,
            },
            policy::stripped_response_header_names,
            replay::{
                preview::ReplayResolvedCredential,
                source::AttemptReplaySource,
                types::{
                    RequestReplayBody, RequestReplayBodyCaptureMetadata, RequestReplayNameValue,
                },
            },
        },
        transform::{StreamTransformer, unified::UnifiedTransformDiagnostic},
    },
    utils::sse::SseParser,
};

#[derive(Debug, Clone)]
pub(crate) struct AttemptReplayExecutionOutcome {
    pub(crate) status: RequestReplayStatus,
    pub(crate) http_status: Option<i32>,
    pub(crate) first_byte_at: Option<i64>,
    pub(crate) error_code: Option<String>,
    pub(crate) error_message: Option<String>,
    pub(crate) response_headers: Vec<RequestReplayNameValue>,
    pub(crate) response_body: Option<RequestReplayBody>,
    pub(crate) response_body_bytes: Option<Bytes>,
    pub(crate) response_body_capture_state: Option<String>,
    pub(crate) response_body_capture: Option<RequestReplayBodyCaptureMetadata>,
    pub(crate) usage_normalization: Option<UsageNormalization>,
    pub(crate) transform_diagnostics: Vec<UnifiedTransformDiagnostic>,
    pub(crate) estimated_cost_nanos: Option<i64>,
    pub(crate) estimated_cost_currency: Option<String>,
    pub(crate) total_input_tokens: Option<i32>,
    pub(crate) total_output_tokens: Option<i32>,
    pub(crate) reasoning_tokens: Option<i32>,
    pub(crate) total_tokens: Option<i32>,
}

pub(crate) async fn perform_attempt_replay_execution(
    app_state: &Arc<AppState>,
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
) -> AttemptReplayExecutionOutcome {
    let headers = match build_attempt_upstream_headers(source, credential) {
        Ok(headers) => headers,
        Err(err) => return invalid_replay_headers_outcome(err),
    };
    let client_bundle = app_state.infra.client_bundle().await;
    let policy = app_state.diagnostics.policy().await;
    let capture_limit_bytes = replay_response_capture_limit(&policy);
    let first_byte_timeout = client_bundle.proxy_request.first_byte_timeout();
    let client = if source.provider.use_proxy {
        std::sync::Arc::clone(&client_bundle.proxy_client)
    } else {
        std::sync::Arc::clone(&client_bundle.client)
    };

    let cancellation = ProxyCancellationContext::new();
    let response = match send_with_first_byte_timeout(
        &cancellation,
        client
            .request(Method::POST, &source.request_uri)
            .headers(headers)
            .body(source.llm_request_body.bytes.clone()),
        "Attempt replay upstream request",
        first_byte_timeout,
    )
    .await
    {
        Ok(response) => response,
        Err(proxy_error) => return execution_outcome_from_proxy_error(proxy_error),
    };

    let status_code = response.status();
    let response_headers =
        serialize_headers_for_output(response.headers(), stripped_response_header_names());
    let is_gzip = response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("gzip"));
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let is_sse = content_type
        .as_deref()
        .is_some_and(|value| value.contains("text/event-stream"));
    let first_byte_at = Some(Utc::now().timestamp_millis());

    let capture = match read_replay_response_body_bounded(
        response.bytes_stream(),
        is_gzip,
        capture_limit_bytes,
        |err| classify_reqwest_error("Reading attempt replay response body", &err),
    )
    .await
    {
        Ok(capture) => capture,
        Err(proxy_error) => return execution_outcome_from_proxy_error(proxy_error),
    };
    let decompressed_body = capture.body.clone();
    let response_body_capture_state = Some(log_capture_state_to_string(&capture.state));
    let response_body_capture = Some(replay_body_capture_metadata(&capture));
    let response_body = Some(body_from_bytes(
        &decompressed_body,
        content_type.clone(),
        response_body_capture_state.clone(),
    ));

    let (usage_normalization, transform_diagnostics) = if is_sse {
        parse_stream_usage_and_diagnostics(&decompressed_body, source.llm_api_type)
    } else if status_code.is_success() {
        let (_, _, usage_normalization, diagnostics) = process_success_response_body(
            &decompressed_body,
            source.llm_api_type,
            source.llm_api_type,
        );
        (usage_normalization, diagnostics)
    } else {
        (None, Vec::new())
    };

    if status_code.is_success() {
        AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Success,
            http_status: Some(i32::from(status_code.as_u16())),
            first_byte_at,
            error_code: None,
            error_message: None,
            response_headers,
            response_body,
            response_body_bytes: Some(decompressed_body),
            response_body_capture_state,
            response_body_capture,
            usage_normalization,
            transform_diagnostics,
            ..empty_cost_fields()
        }
    } else {
        let proxy_error = classify_upstream_status(status_code, &decompressed_body);
        AttemptReplayExecutionOutcome {
            status: RequestReplayStatus::Error,
            http_status: Some(i32::from(status_code.as_u16())),
            first_byte_at,
            error_code: Some(proxy_error.error_code().to_string()),
            error_message: Some(proxy_error.message().to_string()),
            response_headers,
            response_body,
            response_body_bytes: Some(decompressed_body),
            response_body_capture_state,
            response_body_capture,
            usage_normalization,
            transform_diagnostics,
            ..empty_cost_fields()
        }
    }
}

pub(crate) fn build_attempt_upstream_headers(
    source: &AttemptReplaySource,
    credential: &ReplayResolvedCredential,
) -> Result<HeaderMap, BaseError> {
    let mut headers = build_replay_request_headers(&source.request_headers)?;
    apply_provider_request_auth_header(
        &mut headers,
        &source.provider,
        source.llm_api_type,
        &credential.request_key,
    )
    .map_err(|error| BaseError::ParamInvalid(Some(error.to_string())))?;

    Ok(headers)
}

pub(crate) fn parse_stream_usage_and_diagnostics(
    bytes: &Bytes,
    api_type: LlmApiType,
) -> (Option<UsageNormalization>, Vec<UnifiedTransformDiagnostic>) {
    let mut parser = SseParser::new();
    let events = parser.process(bytes);
    if events.is_empty() {
        return (None, Vec::new());
    }

    let mut transformer = StreamTransformer::new(api_type, api_type);
    let _ = transformer.transform_events(events);
    (
        transformer.parse_usage_normalization(),
        transformer.diagnostics_snapshot(),
    )
}

pub(crate) fn execution_outcome_from_proxy_error(
    proxy_error: ProxyError,
) -> AttemptReplayExecutionOutcome {
    AttemptReplayExecutionOutcome {
        status: RequestReplayStatus::Error,
        http_status: None,
        first_byte_at: None,
        error_code: Some(proxy_error.error_code().to_string()),
        error_message: Some(proxy_error.message().to_string()),
        response_headers: Vec::new(),
        response_body: None,
        response_body_bytes: None,
        response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
        response_body_capture: None,
        usage_normalization: None,
        transform_diagnostics: Vec::new(),
        ..empty_cost_fields()
    }
}

fn invalid_replay_headers_outcome(err: BaseError) -> AttemptReplayExecutionOutcome {
    AttemptReplayExecutionOutcome {
        status: RequestReplayStatus::Error,
        http_status: None,
        first_byte_at: None,
        error_code: Some("invalid_replay_preview_headers".to_string()),
        error_message: Some(match err {
            BaseError::ParamInvalid(message) => {
                message.unwrap_or("invalid replay preview headers".to_string())
            }
            other => format!("{other:?}"),
        }),
        response_body_capture_state: Some(REPLAY_BODY_CAPTURE_NOT_CAPTURED.to_string()),
        response_body_capture: None,
        response_headers: Vec::new(),
        response_body: None,
        response_body_bytes: None,
        usage_normalization: None,
        transform_diagnostics: Vec::new(),
        ..empty_cost_fields()
    }
}

fn empty_cost_fields() -> AttemptReplayExecutionOutcome {
    AttemptReplayExecutionOutcome {
        status: RequestReplayStatus::Error,
        http_status: None,
        first_byte_at: None,
        error_code: None,
        error_message: None,
        response_headers: Vec::new(),
        response_body: None,
        response_body_bytes: None,
        response_body_capture_state: None,
        response_body_capture: None,
        usage_normalization: None,
        transform_diagnostics: Vec::new(),
        estimated_cost_nanos: None,
        estimated_cost_currency: None,
        total_input_tokens: None,
        total_output_tokens: None,
        reasoning_tokens: None,
        total_tokens: None,
    }
}

#[cfg(test)]
mod tests {
    use std::{io::Write, sync::Arc};

    use axum::{
        Router,
        body::Bytes as AxumBytes,
        http::{HeaderMap as AxumHeaderMap, StatusCode, header::CONTENT_TYPE},
        routing::post,
    };
    use bytes::Bytes;
    use flate2::{Compression, write::GzEncoder};
    use reqwest::header::{AUTHORIZATION, HeaderValue};

    use crate::{
        database::request_attempt::RequestAttemptDetail,
        schema::enum_def::{ProviderApiKeyMode, ProviderType, RequestAttemptStatus},
        service::{
            app_state::AppState,
            cache::types::CacheProvider,
            diagnostics::{
                body::{REPLAY_BODY_CAPTURE_INCOMPLETE, build_header_map_from_name_values},
                replay::{
                    preview::ReplayResolvedCredential,
                    source::{AttemptReplaySource, DecodedBundleBody},
                    types::RequestReplayNameValue,
                },
            },
        },
        utils::storage::LogBodyCaptureState,
    };

    use super::*;

    #[derive(Debug, Clone, Default)]
    struct CapturedReplayRequest {
        authorization: Option<String>,
        x_api_key: Option<String>,
        x_goog_api_key: Option<String>,
        body: String,
    }

    fn credential(request_key: &str) -> ReplayResolvedCredential {
        ReplayResolvedCredential {
            provider_api_key_id: 3,
            request_key: request_key.to_string(),
            used_override: false,
        }
    }

    fn llm_api_type_for_provider(provider_type: &ProviderType) -> LlmApiType {
        match provider_type {
            ProviderType::Gemini | ProviderType::Vertex => LlmApiType::Gemini,
            ProviderType::Anthropic => LlmApiType::Anthropic,
            ProviderType::Responses => LlmApiType::Responses,
            ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
            ProviderType::Ollama => LlmApiType::Ollama,
            ProviderType::Openai | ProviderType::VertexOpenai => LlmApiType::Openai,
        }
    }

    fn provider_key_and_name(provider_type: &ProviderType) -> (&'static str, &'static str) {
        match provider_type {
            ProviderType::Openai => ("openai", "OpenAI"),
            ProviderType::Gemini => ("gemini", "Gemini"),
            ProviderType::Vertex => ("vertex", "Vertex"),
            ProviderType::VertexOpenai => ("vertex-openai", "Vertex OpenAI"),
            ProviderType::Ollama => ("ollama", "Ollama"),
            ProviderType::Anthropic => ("anthropic", "Anthropic"),
            ProviderType::Responses => ("responses", "Responses"),
            ProviderType::GeminiOpenai => ("gemini-openai", "Gemini OpenAI"),
        }
    }

    fn source(request_uri: String, provider_type: ProviderType) -> AttemptReplaySource {
        let llm_api_type = llm_api_type_for_provider(&provider_type);
        let (provider_key, provider_name) = provider_key_and_name(&provider_type);
        let sanitized_request_headers = vec![
            RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            },
            RequestReplayNameValue {
                name: "x-trace-id".to_string(),
                value: Some("trace-1".to_string()),
            },
        ];
        let request_headers =
            build_header_map_from_name_values(&sanitized_request_headers).expect("headers");

        AttemptReplaySource {
            request_log_id: 42,
            attempt: RequestAttemptDetail {
                id: 101,
                request_log_id: 42,
                attempt_index: 1,
                candidate_position: 1,
                provider_id: Some(2),
                provider_api_key_id: Some(3),
                model_id: Some(4),
                provider_key_snapshot: Some(provider_key.to_string()),
                provider_name_snapshot: Some(provider_name.to_string()),
                model_name_snapshot: Some("gpt-test".to_string()),
                real_model_name_snapshot: Some("gpt-4o-mini".to_string()),
                llm_api_type: Some(llm_api_type),
                attempt_status: RequestAttemptStatus::Success,
                http_status: Some(200),
                total_tokens: Some(7),
                estimated_cost_nanos: Some(100),
                estimated_cost_currency: Some("USD".to_string()),
                ..Default::default()
            },
            requested_model_name: Some("primary-high".to_string()),
            base_requested_model_name: Some("primary".to_string()),
            resolved_reasoning_suffix: Some("high".to_string()),
            resolved_reasoning_preset: Some("high".to_string()),
            resolved_route_id: Some(8),
            resolved_route_name: Some("primary".to_string()),
            provider: Arc::new(CacheProvider {
                id: 2,
                provider_key: provider_key.to_string(),
                name: provider_name.to_string(),
                endpoint: "https://upstream.example/v1".to_string(),
                use_proxy: false,
                provider_type,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                is_enabled: true,
            }),
            llm_api_type,
            request_uri,
            sanitized_request_headers,
            request_headers,
            llm_request_body: DecodedBundleBody {
                bytes: Bytes::from_static(
                    br#"{"model":"gpt-4o-mini","messages":[{"role":"user","content":"ping"}]}"#,
                ),
                media_type: Some("application/json".to_string()),
                capture_state: Some("complete".to_string()),
            },
            baseline_response_headers: vec![RequestReplayNameValue {
                name: "content-type".to_string(),
                value: Some("application/json".to_string()),
            }],
            baseline_response_body: None,
            cost_catalog_version: None,
        }
    }

    fn replay_path(provider_type: &ProviderType) -> &'static str {
        match provider_type {
            ProviderType::Gemini | ProviderType::Vertex => {
                "/v1beta/models/gemini-2.5-pro:generateContent"
            }
            ProviderType::Anthropic => "/v1/messages",
            ProviderType::Ollama => "/api/chat",
            ProviderType::Openai
            | ProviderType::Responses
            | ProviderType::GeminiOpenai
            | ProviderType::VertexOpenai => "/v1/chat/completions",
        }
    }

    fn replay_request_uri(base_url: &str, provider_type: &ProviderType) -> String {
        format!("{}{}", base_url, replay_path(provider_type))
    }

    async fn spawn_upstream(
        path: &'static str,
        status: StatusCode,
        response_content_type: &'static str,
        response_body: impl Into<String>,
    ) -> (
        String,
        Arc<tokio::sync::Mutex<Option<CapturedReplayRequest>>>,
    ) {
        let captured = Arc::new(tokio::sync::Mutex::new(None));
        let captured_for_handler = Arc::clone(&captured);
        let response_body = Arc::new(response_body.into());
        let app = Router::new().route(
            path,
            post(move |headers: AxumHeaderMap, body: AxumBytes| {
                let captured_for_handler = Arc::clone(&captured_for_handler);
                let response_body = Arc::clone(&response_body);
                async move {
                    *captured_for_handler.lock().await = Some(CapturedReplayRequest {
                        authorization: headers
                            .get("authorization")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        x_api_key: headers
                            .get("x-api-key")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        x_goog_api_key: headers
                            .get("x-goog-api-key")
                            .and_then(|value| value.to_str().ok())
                            .map(str::to_string),
                        body: String::from_utf8_lossy(&body).to_string(),
                    });
                    (
                        status,
                        [(CONTENT_TYPE, response_content_type)],
                        response_body.as_str().to_string(),
                    )
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener should bind");
        let addr = listener.local_addr().expect("listener should have address");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock upstream should serve");
        });

        (format!("http://{}", addr), captured)
    }

    #[tokio::test]
    async fn attempt_replay_transport_reuses_request_snapshot_and_parses_usage() {
        let response_body = r#"{"id":"chatcmpl-1","object":"chat.completion","created":1,"model":"gpt-4o-mini","choices":[{"index":0,"message":{"role":"assistant","content":"pong"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":3,"total_tokens":7}}"#;
        let provider_type = ProviderType::Openai;
        let (base_url, captured) = spawn_upstream(
            replay_path(&provider_type),
            StatusCode::OK,
            "application/json",
            response_body,
        )
        .await;
        let source = source(replay_request_uri(&base_url, &provider_type), provider_type);
        let app_state = Arc::new(AppState::new().await);

        let outcome =
            perform_attempt_replay_execution(&app_state, &source, &credential("sk-live")).await;

        assert_eq!(outcome.status, RequestReplayStatus::Success);
        assert_eq!(outcome.http_status, Some(200));
        assert_eq!(
            outcome
                .usage_normalization
                .as_ref()
                .map(|usage| usage.total_input_tokens + usage.total_output_tokens),
            Some(7)
        );
        assert_eq!(
            outcome.response_body_capture_state.as_deref(),
            Some("complete")
        );
        assert!(
            outcome
                .response_body
                .as_ref()
                .and_then(|body| body.json.as_ref())
                .is_some()
        );

        let captured = captured
            .lock()
            .await
            .clone()
            .expect("request should be captured");
        assert_eq!(captured.authorization.as_deref(), Some("Bearer sk-live"));
        assert!(captured.x_api_key.is_none());
        assert!(captured.x_goog_api_key.is_none());
        assert!(captured.body.contains("\"ping\""));
    }

    #[tokio::test]
    async fn attempt_replay_transport_rebuilds_provider_specific_auth_headers() {
        let cases = [
            (
                ProviderType::Gemini,
                "gk-live",
                Some(("x_goog_api_key", "gk-live")),
            ),
            (
                ProviderType::Anthropic,
                "ak-live",
                Some(("x_api_key", "ak-live")),
            ),
            (
                ProviderType::Vertex,
                "ya29.vertex-live",
                Some(("authorization", "Bearer ya29.vertex-live")),
            ),
            (
                ProviderType::VertexOpenai,
                "ya29.vertex-openai-live",
                Some(("authorization", "Bearer ya29.vertex-openai-live")),
            ),
        ];

        for (provider_type, request_key, expected_auth) in cases {
            let (base_url, captured) = spawn_upstream(
                replay_path(&provider_type),
                StatusCode::OK,
                "text/plain",
                "pong",
            )
            .await;
            let source = source(
                replay_request_uri(&base_url, &provider_type),
                provider_type.clone(),
            );
            let app_state = Arc::new(AppState::new().await);

            let outcome =
                perform_attempt_replay_execution(&app_state, &source, &credential(request_key))
                    .await;

            assert_eq!(outcome.status, RequestReplayStatus::Success);
            assert_eq!(outcome.http_status, Some(200));

            let captured = captured
                .lock()
                .await
                .clone()
                .expect("request should be captured");
            match expected_auth {
                Some(("authorization", value)) => {
                    assert_eq!(captured.authorization.as_deref(), Some(value));
                    assert!(captured.x_api_key.is_none());
                    assert!(captured.x_goog_api_key.is_none());
                }
                Some(("x_api_key", value)) => {
                    assert_eq!(captured.x_api_key.as_deref(), Some(value));
                    assert!(captured.authorization.is_none());
                    assert!(captured.x_goog_api_key.is_none());
                }
                Some(("x_goog_api_key", value)) => {
                    assert_eq!(captured.x_goog_api_key.as_deref(), Some(value));
                    assert!(captured.authorization.is_none());
                    assert!(captured.x_api_key.is_none());
                }
                Some((other, _)) => panic!("unexpected auth capture field: {other}"),
                None => panic!("auth expectation must be present"),
            }
        }
    }

    #[test]
    fn attempt_upstream_headers_strip_historical_auth_and_apply_current_provider_auth() {
        let provider_type = ProviderType::Openai;
        let mut source = source(
            replay_request_uri("https://upstream.example", &provider_type),
            provider_type,
        );
        source
            .request_headers
            .insert(AUTHORIZATION, HeaderValue::from_static("Bearer stale"));
        source
            .request_headers
            .insert("x-api-key", HeaderValue::from_static("stale-ak"));
        source
            .request_headers
            .insert("x-goog-api-key", HeaderValue::from_static("stale-gk"));
        source
            .request_headers
            .insert("host", HeaderValue::from_static("stale.example"));

        let headers = build_attempt_upstream_headers(&source, &credential("sk-current"))
            .expect("headers should build");

        assert_eq!(
            headers
                .get(AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer sk-current")
        );
        assert!(headers.get("x-api-key").is_none());
        assert!(headers.get("x-goog-api-key").is_none());
        assert!(headers.get("host").is_none());
        assert_eq!(
            headers
                .get("x-trace-id")
                .and_then(|value| value.to_str().ok()),
            Some("trace-1")
        );
    }

    #[tokio::test]
    async fn attempt_replay_transport_maps_upstream_errors_and_preserves_error_body() {
        let response_body = r#"{"error":{"message":"slow down"}}"#;
        let provider_type = ProviderType::Openai;
        let (base_url, _captured) = spawn_upstream(
            replay_path(&provider_type),
            StatusCode::TOO_MANY_REQUESTS,
            "application/json",
            response_body,
        )
        .await;
        let source = source(replay_request_uri(&base_url, &provider_type), provider_type);
        let app_state = Arc::new(AppState::new().await);

        let outcome =
            perform_attempt_replay_execution(&app_state, &source, &credential("sk-live")).await;

        assert_eq!(outcome.status, RequestReplayStatus::Error);
        assert_eq!(outcome.http_status, Some(429));
        assert_eq!(
            outcome.error_code.as_deref(),
            Some("upstream_rate_limit_error")
        );
        assert_eq!(
            outcome.response_body_capture_state.as_deref(),
            Some("complete")
        );
        assert!(
            outcome
                .response_body
                .as_ref()
                .and_then(|body| body.json.as_ref())
                .is_some()
        );
    }

    #[tokio::test]
    async fn attempt_replay_transport_marks_large_response_incomplete_without_failing() {
        let policy = crate::service::diagnostics::policy::DiagnosticsPolicy::default();
        let limit = replay_response_capture_limit(&policy);
        let response_body = "x".repeat(limit + 1024);
        let provider_type = ProviderType::Openai;
        let (base_url, _captured) = spawn_upstream(
            replay_path(&provider_type),
            StatusCode::OK,
            "text/plain",
            response_body,
        )
        .await;
        let source = source(replay_request_uri(&base_url, &provider_type), provider_type);
        let app_state = Arc::new(AppState::new().await);

        let outcome =
            perform_attempt_replay_execution(&app_state, &source, &credential("sk-live")).await;

        assert_eq!(outcome.status, RequestReplayStatus::Success);
        assert_eq!(outcome.http_status, Some(200));
        assert_eq!(
            outcome.response_body_capture_state.as_deref(),
            Some(REPLAY_BODY_CAPTURE_INCOMPLETE)
        );
        assert_eq!(
            outcome
                .response_body_capture
                .as_ref()
                .map(|capture| capture.bytes_captured),
            Some(limit as i64)
        );
        assert_eq!(
            outcome.response_body_bytes.as_ref().map(Bytes::len),
            Some(limit)
        );
    }

    #[tokio::test]
    async fn attempt_replay_transport_degrades_truncated_sse_usage_parse() {
        let limit = 80usize;
        let sse = format!(
            "data: {}\n\ndata: {}\n\n",
            serde_json::json!({"choices":[{"delta":{"content":"a".repeat(limit)}}]}),
            serde_json::json!({"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}})
        );
        let stream = futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(sse))]);

        let capture = read_replay_response_body_bounded(stream, false, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");
        let (usage, diagnostics) =
            parse_stream_usage_and_diagnostics(&capture.body, LlmApiType::Openai);

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert!(usage.is_none());
        assert!(diagnostics.is_empty());
    }

    #[tokio::test]
    async fn attempt_replay_transport_decodes_gzip_capture_before_mapping_body() {
        let limit = 64usize;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&vec![b'x'; limit * 4])
            .expect("gzip write should succeed");
        let compressed = encoder.finish().expect("gzip finish should succeed");
        let stream =
            futures::stream::iter(vec![Ok::<Bytes, std::io::Error>(Bytes::from(compressed))]);

        let capture = read_replay_response_body_bounded(stream, true, limit, |err| {
            ProxyError::BadGateway(err.to_string())
        })
        .await
        .expect("capture should succeed");

        assert_eq!(capture.state, LogBodyCaptureState::Incomplete);
        assert_eq!(capture.body.len(), limit);
        assert_eq!(capture.body, Bytes::from(vec![b'x'; limit]));
        assert_eq!(capture.body_encoding, "decoded:gzip");
    }
}
