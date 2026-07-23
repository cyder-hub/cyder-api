use std::{
    collections::BTreeMap,
    future::Future,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use axum::{
    body::{Body, Bytes},
    extract::Request,
    http::{HeaderMap, Method, StatusCode, header::CONTENT_TYPE},
    response::Response,
    routing::any,
    serve,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    net::TcpListener,
    sync::{Mutex as AsyncMutex, Notify, oneshot},
    task::JoinHandle,
    time::timeout,
};
use tower::ServiceExt;

use super::create_proxy_router;
use crate::{
    database::{
        TestDbContext,
        api_key::{ApiKey, CreateApiKeyPayload},
        request_log::{RequestLog, RequestLogQueryPayload, RequestLogRecord},
        request_patch::CreateRequestPatchPayload,
    },
    schema::enum_def::{
        Action, LlmApiType, ProviderApiKeyMode, ProviderType, RequestPatchOperation,
        RequestPatchPlacement, RequestStatus,
    },
    service::{
        admin::model::UpdateModelInput,
        admin::provider::BootstrapProviderCommand,
        app_state::{AppState, create_test_app_state},
    },
    utils::{ID_GENERATOR, sse::SseParser},
};

const DOWNSTREAM_SECRET_MARKER: &str = "cyder-";
const PROVIDER_SECRET: &str = "provider-baseline-secret";
const UPSTREAM_MODEL: &str = "baseline-upstream-model";
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

const FIXTURE_SOURCES: [(&str, &str); 5] = [
    (
        "openai",
        include_str!("../service/transform/testdata/direct_execution/openai.json"),
    ),
    (
        "responses",
        include_str!("../service/transform/testdata/direct_execution/responses.json"),
    ),
    (
        "anthropic",
        include_str!("../service/transform/testdata/direct_execution/anthropic.json"),
    ),
    (
        "gemini",
        include_str!("../service/transform/testdata/direct_execution/gemini.json"),
    ),
    (
        "ollama",
        include_str!("../service/transform/testdata/direct_execution/ollama.json"),
    ),
];

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct GoldenEvent {
    #[serde(default)]
    event: Option<String>,
    data: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DownstreamAuth {
    Bearer,
    XApiKey,
    GeminiQuery,
}

#[derive(Clone, Debug, Deserialize)]
struct RequestGolden {
    downstream: Value,
    upstream: Value,
    upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct NonStreamGolden {
    upstream_response: Value,
    downstream_status: u16,
    downstream_content_type: String,
    downstream_response: Value,
}

#[derive(Clone, Debug, Deserialize)]
struct StreamGolden {
    downstream_request: Value,
    upstream_request: Value,
    upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
    upstream_events: Vec<GoldenEvent>,
    downstream_events: Vec<GoldenEvent>,
    downstream_content_type: String,
    expected_text: String,
}

#[derive(Clone, Debug, Deserialize)]
struct UsageGolden {
    input: i32,
    output: i32,
    total: i32,
}

#[derive(Clone, Debug, Deserialize)]
struct ErrorGolden {
    downstream_request: Value,
    upstream_status: u16,
    upstream_response: Value,
    downstream_status: u16,
    downstream_code: String,
    downstream_message: String,
}

#[derive(Clone, Debug, Deserialize)]
struct CancellationGolden {
    downstream_request: Value,
    upstream_request: Value,
    upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
    first_upstream_event: GoldenEvent,
    expected_status: RequestStatus,
}

#[derive(Clone, Debug, Deserialize)]
struct DirectExecutionFixture {
    protocol: LlmApiType,
    provider_type: ProviderType,
    downstream_path: String,
    downstream_stream_path: String,
    downstream_auth: DownstreamAuth,
    upstream_headers: BTreeMap<String, String>,
    request: RequestGolden,
    non_stream: NonStreamGolden,
    stream: StreamGolden,
    usage: UsageGolden,
    error: ErrorGolden,
    cancellation: CancellationGolden,
}

fn fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
    FIXTURE_SOURCES
        .iter()
        .map(|(name, source)| {
            let fixture = serde_json::from_str(source).unwrap_or_else(|error| {
                panic!("{name} direct execution fixture should parse: {error}")
            });
            (*name, fixture)
        })
        .collect()
}

fn validate_fixture(name: &str, fixture: &DirectExecutionFixture) {
    assert!(
        !fixture.downstream_path.is_empty(),
        "{name}: downstream path"
    );
    assert!(
        !fixture.downstream_stream_path.is_empty(),
        "{name}: stream path"
    );
    assert!(
        fixture.request.downstream.is_object(),
        "{name}: downstream request"
    );
    assert!(
        fixture.request.upstream.is_object(),
        "{name}: upstream request"
    );
    assert!(
        fixture.non_stream.upstream_response.is_object(),
        "{name}: non-stream upstream"
    );
    assert!(
        fixture.non_stream.downstream_response.is_object(),
        "{name}: non-stream downstream"
    );
    assert!(
        fixture.stream.downstream_request.is_object(),
        "{name}: stream request"
    );
    assert!(
        !fixture.stream.upstream_events.is_empty(),
        "{name}: upstream events"
    );
    assert!(
        !fixture.stream.downstream_events.is_empty(),
        "{name}: downstream events"
    );
    assert_eq!(
        fixture.stream.expected_text, "baseline pong",
        "{name}: stream text"
    );
    assert_eq!(
        (
            fixture.usage.input,
            fixture.usage.output,
            fixture.usage.total
        ),
        (11, 7, 18)
    );
    assert_eq!(fixture.error.upstream_status, 429, "{name}: error sample");
    assert_eq!(
        fixture.cancellation.expected_status,
        RequestStatus::Cancelled
    );
}

fn run_case<F, Fut>(name: &str, test: F)
where
    F: FnOnce(TestDbContext) -> Fut,
    Fut: Future<Output = ()> + 'static,
{
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("direct execution runtime should build");
    let context = TestDbContext::new_sqlite(&format!(
        "direct-execution-{name}-{}.sqlite",
        ID_GENERATOR.generate_id()
    ));
    runtime.block_on(async move {
        let test = Box::pin(test(context.clone()));
        context.run_async(test).await;
    });
}

#[derive(Clone, Debug)]
struct CapturedRequest {
    method: Method,
    path: String,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
}

#[derive(Debug, Default)]
struct DropSignal {
    dropped: AtomicBool,
    notify: Notify,
}

impl DropSignal {
    fn mark(&self) {
        self.dropped.store(true, Ordering::SeqCst);
        self.notify.notify_waiters();
    }

    async fn wait(&self) {
        timeout(WAIT_TIMEOUT, async {
            while !self.dropped.load(Ordering::SeqCst) {
                self.notify.notified().await;
            }
        })
        .await
        .expect("upstream response body should close after downstream cancellation");
    }
}

struct ResponseBodyDropGuard(Arc<DropSignal>);

impl Drop for ResponseBodyDropGuard {
    fn drop(&mut self) {
        self.0.mark();
    }
}

#[derive(Clone)]
enum ScriptedReply {
    Json {
        status: StatusCode,
        body: Value,
    },
    Sse {
        events: Vec<GoldenEvent>,
    },
    HangingSse {
        first_event: GoldenEvent,
        dropped: Arc<DropSignal>,
    },
}

struct TestUpstream {
    base_url: String,
    captured: Arc<AsyncMutex<Vec<CapturedRequest>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestUpstream {
    async fn spawn(reply: ScriptedReply) -> Self {
        let captured = Arc::new(AsyncMutex::new(Vec::new()));
        let router = axum::Router::new().fallback(any({
            let captured = Arc::clone(&captured);
            move |request: Request<Body>| {
                let captured = Arc::clone(&captured);
                let reply = reply.clone();
                async move {
                    let (parts, body) = request.into_parts();
                    let body = axum::body::to_bytes(body, usize::MAX)
                        .await
                        .expect("upstream request body should be readable");
                    captured.lock().await.push(CapturedRequest {
                        method: parts.method,
                        path: parts.uri.path().to_string(),
                        query: parts.uri.query().map(ToString::to_string),
                        headers: parts.headers,
                        body,
                    });

                    match reply {
                        ScriptedReply::Json { status, body } => Response::builder()
                            .status(status)
                            .header(CONTENT_TYPE, "application/json")
                            .body(Body::from(serde_json::to_vec(&body).unwrap()))
                            .unwrap(),
                        ScriptedReply::Sse { events } => Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(events_to_sse_bytes(&events)))
                            .unwrap(),
                        ScriptedReply::HangingSse { first_event, dropped } => {
                            let stream = async_stream::stream! {
                                let _guard = ResponseBodyDropGuard(dropped);
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(events_to_sse_bytes(&[first_event])));
                                std::future::pending::<()>().await;
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream")
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                    }
                }
            }
        }));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("scripted upstream should bind");
        let addr = listener.local_addr().expect("upstream address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let task = tokio::spawn(async move {
            serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("scripted upstream should run");
        });
        Self {
            base_url: format!("http://{addr}"),
            captured,
            shutdown_tx: Some(shutdown_tx),
            task,
        }
    }

    async fn requests(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }

    async fn shutdown(mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        timeout(WAIT_TIMEOUT, &mut self.task)
            .await
            .expect("scripted upstream should stop before deadline")
            .expect("scripted upstream task should join");
    }
}

impl Drop for TestUpstream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.task.abort();
    }
}

struct RouterFixture {
    app_state: Arc<AppState>,
    downstream_key: String,
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    provider_api_key_id: i64,
    model_id: i64,
    model_name: String,
}

impl RouterFixture {
    async fn new(context: TestDbContext, fixture: &DirectExecutionFixture, base_url: &str) -> Self {
        Self::new_with_default_action(context, fixture, base_url, Action::Allow).await
    }

    async fn new_with_default_action(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
        default_action: Action,
    ) -> Self {
        let nonce = ID_GENERATOR.generate_id();
        let endpoint = match fixture.provider_type {
            ProviderType::Gemini => format!("{base_url}/v1beta/models"),
            _ => format!("{base_url}/v1"),
        };
        let provider_key = format!("baseline-provider-{nonce}");
        let provider_name = format!("Baseline Provider {nonce}");
        let app_state = create_test_app_state(context).await;
        let bootstrapped = app_state
            .admin
            .provider
            .bootstrap_provider_persist(BootstrapProviderCommand {
                provider_id: nonce,
                provider_key: provider_key.clone(),
                name: provider_name.clone(),
                endpoint,
                use_proxy: false,
                provider_type: fixture.provider_type.clone(),
                provider_api_key_mode: ProviderApiKeyMode::Queue,
                api_key: PROVIDER_SECRET.to_string(),
                api_key_description: Some("direct execution regression".to_string()),
                model_name: "baseline-model".to_string(),
                real_model_name: Some(UPSTREAM_MODEL.to_string()),
            })
            .await
            .expect("provider fixture should bootstrap");
        let created_key = ApiKey::create(&CreateApiKeyPayload {
            name: format!("direct-execution-key-{nonce}"),
            description: Some("direct execution regression".to_string()),
            default_action: Some(default_action),
            is_enabled: Some(true),
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
            acl_rules: None,
        })
        .expect("downstream key fixture should create");
        Self {
            app_state,
            downstream_key: created_key.reveal.api_key,
            provider_id: bootstrapped.provider.id,
            provider_key,
            provider_name,
            provider_api_key_id: bootstrapped.created_key.id,
            model_id: bootstrapped.created_model.id,
            model_name: bootstrapped.created_model.model_name,
        }
    }

    fn requested_model(&self) -> String {
        format!("{}/{}", self.provider_key, self.model_name)
    }

    async fn send(
        &self,
        fixture: &DirectExecutionFixture,
        stream: bool,
        body: &Value,
    ) -> Response<Body> {
        let path_template = if stream {
            &fixture.downstream_stream_path
        } else {
            &fixture.downstream_path
        };
        let mut uri = path_template.replace("$REQUESTED_MODEL", &self.requested_model());
        let mut builder = Request::builder()
            .method(Method::POST)
            .header(CONTENT_TYPE, "application/json");
        match fixture.downstream_auth {
            DownstreamAuth::Bearer => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            DownstreamAuth::XApiKey => builder = builder.header("x-api-key", &self.downstream_key),
            DownstreamAuth::GeminiQuery => {
                let separator = if uri.contains('?') { '&' } else { '?' };
                uri.push(separator);
                uri.push_str("key=");
                uri.push_str(&self.downstream_key);
            }
        }
        let mut request = builder
            .uri(uri)
            .body(Body::from(
                serde_json::to_vec(&render_value(body, &self.requested_model())).unwrap(),
            ))
            .expect("downstream request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3000,
            ))));
        create_proxy_router()
            .with_state(Arc::clone(&self.app_state))
            .oneshot(request)
            .await
            .expect("proxy router should respond")
    }

    async fn wait_for_log(&self, expected: RequestStatus) -> RequestLogRecord {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            self.app_state.flush_proxy_logs().await;
            let logs = RequestLog::list_full(RequestLogQueryPayload {
                provider_id: Some(self.provider_id),
                model_id: Some(self.model_id),
                page: Some(1),
                page_size: Some(10),
                ..Default::default()
            })
            .expect("request logs should be queryable")
            .list;
            if let Some(log) = logs.into_iter().find(|log| log.overall_status == expected) {
                return log;
            }
            assert!(
                Instant::now() < deadline,
                "request log should reach {expected:?} before deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

fn render_value(value: &Value, requested_model: &str) -> Value {
    match value {
        Value::String(value) if value == "$REQUESTED_MODEL" => {
            Value::String(requested_model.to_string())
        }
        Value::String(value) if value == "$UPSTREAM_MODEL" => {
            Value::String(UPSTREAM_MODEL.to_string())
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| render_value(value, requested_model))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), render_value(value, requested_model)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

fn event_data_text(data: &Value) -> String {
    data.as_str()
        .map(ToString::to_string)
        .unwrap_or_else(|| serde_json::to_string(data).unwrap())
}

fn events_to_sse_bytes(events: &[GoldenEvent]) -> Vec<u8> {
    let mut bytes = Vec::new();
    for event in events {
        if let Some(name) = &event.event {
            bytes.extend_from_slice(format!("event: {name}\n").as_bytes());
        }
        bytes.extend_from_slice(format!("data: {}\n\n", event_data_text(&event.data)).as_bytes());
    }
    bytes
}

fn parse_downstream_events(api_type: LlmApiType, body: &[u8]) -> Vec<GoldenEvent> {
    if api_type == LlmApiType::Ollama {
        return String::from_utf8_lossy(body)
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| GoldenEvent {
                event: None,
                data: serde_json::from_str(line).expect("Ollama line should be JSON"),
            })
            .collect();
    }
    let mut parser = SseParser::new();
    parser
        .process(body)
        .into_iter()
        .map(|event| GoldenEvent {
            event: event.event,
            data: serde_json::from_str(&event.data).unwrap_or(Value::String(event.data)),
        })
        .collect()
}

fn normalize_value(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(normalize_value),
        Value::Object(values) => {
            values.remove("created_at");
            values.remove("completed_at");
            for (key, value) in values.iter_mut() {
                if matches!(key.as_str(), "id" | "item_id")
                    && value
                        .as_str()
                        .is_some_and(|value| value.starts_with("msg_"))
                {
                    *value = Value::String("$DYNAMIC_MESSAGE_ID".to_string());
                } else {
                    normalize_value(value);
                }
            }
        }
        _ => {}
    }
}

fn normalized(mut value: Value) -> Value {
    normalize_value(&mut value);
    value
}

fn normalized_events(events: Vec<GoldenEvent>) -> Vec<GoldenEvent> {
    events
        .into_iter()
        .map(|mut event| {
            normalize_value(&mut event.data);
            event
        })
        .collect()
}

fn stream_text(api_type: LlmApiType, events: &[GoldenEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match api_type {
            LlmApiType::Openai => event
                .data
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str),
            LlmApiType::Responses => (event.data.get("type").and_then(Value::as_str)
                == Some("response.output_text.delta"))
            .then(|| event.data.get("delta").and_then(Value::as_str))
            .flatten(),
            LlmApiType::Anthropic => event.data.pointer("/delta/text").and_then(Value::as_str),
            LlmApiType::Gemini => event
                .data
                .pointer("/candidates/0/content/parts/0/text")
                .and_then(Value::as_str),
            LlmApiType::Ollama => event
                .data
                .pointer("/message/content")
                .and_then(Value::as_str),
            LlmApiType::GeminiOpenai => None,
        })
        .collect()
}

fn assert_upstream(
    name: &str,
    fixture: &DirectExecutionFixture,
    captured: &[CapturedRequest],
    expected_path: &str,
    expected_query: Option<&str>,
    expected_body: &Value,
    requested_model: &str,
) {
    assert_eq!(captured.len(), 1, "{name}: exactly one upstream request");
    let request = &captured[0];
    assert_eq!(request.method, Method::POST, "{name}: upstream method");
    assert_eq!(request.path, expected_path, "{name}: upstream path");
    assert_eq!(
        request.query.as_deref(),
        expected_query,
        "{name}: upstream query"
    );
    let body: Value = serde_json::from_slice(&request.body).expect("upstream body should be JSON");
    assert_eq!(
        body,
        render_value(expected_body, requested_model),
        "{name}: upstream body"
    );
    for (header, expected) in &fixture.upstream_headers {
        assert_eq!(
            request
                .headers
                .get(header)
                .and_then(|value| value.to_str().ok()),
            Some(expected.as_str()),
            "{name}: upstream header {header}"
        );
    }
    for value in request.headers.values() {
        let value = value.to_str().unwrap_or_default();
        assert!(
            !value.starts_with(DOWNSTREAM_SECRET_MARKER),
            "{name}: downstream credential leaked upstream"
        );
    }
    assert!(
        !request
            .query
            .as_deref()
            .unwrap_or_default()
            .contains(DOWNSTREAM_SECRET_MARKER)
    );
}

fn assert_log_common(
    router: &RouterFixture,
    fixture: &DirectExecutionFixture,
    log: &RequestLogRecord,
) {
    let target_api_type = match fixture.provider_type {
        ProviderType::Openai | ProviderType::VertexOpenai => LlmApiType::Openai,
        ProviderType::Gemini | ProviderType::Vertex => LlmApiType::Gemini,
        ProviderType::Ollama => LlmApiType::Ollama,
        ProviderType::Anthropic => LlmApiType::Anthropic,
        ProviderType::Responses => LlmApiType::Responses,
        ProviderType::GeminiOpenai => LlmApiType::GeminiOpenai,
    };
    assert_eq!(log.user_api_type, fixture.protocol);
    assert_eq!(log.llm_api_type, Some(target_api_type));
    assert_eq!(log.provider_id, Some(router.provider_id));
    assert_eq!(log.provider_api_key_id, Some(router.provider_api_key_id));
    assert_eq!(log.model_id, Some(router.model_id));
    assert_eq!(
        log.provider_key_snapshot.as_deref(),
        Some(router.provider_key.as_str())
    );
    assert_eq!(
        log.provider_name_snapshot.as_deref(),
        Some(router.provider_name.as_str())
    );
    assert_eq!(
        log.model_name_snapshot.as_deref(),
        Some(router.model_name.as_str())
    );
    assert_eq!(
        log.real_model_name_snapshot.as_deref(),
        Some(UPSTREAM_MODEL)
    );
}

fn assert_usage(log: &RequestLogRecord, usage: &UsageGolden) {
    assert_eq!(log.total_input_tokens, Some(usage.input));
    assert_eq!(log.total_output_tokens, Some(usage.output));
    assert_eq!(log.total_tokens, Some(usage.total));
}

#[test]
fn direct_execution_regression_fixtures_define_five_complete_protocols() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 5);
    for (name, fixture) in fixtures {
        validate_fixture(name, &fixture);
    }
}

#[test]
fn capability_rejection_does_not_decrypt_provider_credential() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let model = crate::database::model::Model::get_by_id(router.model_id).unwrap();
        router
            .app_state
            .admin
            .model
            .update_model(
                model.id,
                UpdateModelInput {
                    model_name: model.model_name,
                    real_model_name: model.real_model_name,
                    is_enabled: true,
                    cost_catalog_id: model.cost_catalog_id,
                    supports_streaming: Some(model.supports_streaming),
                    supports_tools: Some(false),
                    supports_reasoning: Some(model.supports_reasoning),
                    supports_image_input: Some(model.supports_image_input),
                    supports_embeddings: Some(model.supports_embeddings),
                    supports_rerank: Some(model.supports_rerank),
                },
            )
            .await
            .expect("model capability should update");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();
        let mut body = fixture.request.downstream.clone();
        body.as_object_mut().unwrap().insert(
            "tools".to_string(),
            json!([{"type":"function","function":{"name":"probe"}}]),
        );

        let response = router.send(&fixture, false, &body).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn acl_rejection_does_not_decrypt_provider_credential() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new_with_default_action(
            context,
            &fixture,
            &upstream.base_url,
            Action::Deny,
        )
        .await;
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn request_patch_conflict_rejection_does_not_decrypt_provider_credential() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let patch = |target: &str| CreateRequestPatchPayload {
            placement: RequestPatchPlacement::Body,
            target: target.to_string(),
            operation: RequestPatchOperation::Set,
            value_json: Some(Some(json!({"temperature": 0.2}))),
            description: Some("decrypt ordering regression".to_string()),
            is_enabled: Some(true),
            confirm_dangerous_target: None,
        };
        router
            .app_state
            .admin
            .request_patch
            .create_provider_request_patch(router.provider_id, patch("/generation_config"))
            .await
            .expect("provider patch should create");
        router
            .app_state
            .admin
            .request_patch
            .create_model_request_patch(router.model_id, patch("/generation_config/temperature"))
            .await
            .expect("model patch should create");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_regression_non_stream_request_response_usage_and_log_golden() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: fixture.non_stream.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(
                response.status().as_u16(),
                fixture.non_stream.downstream_status,
                "{name}: downstream status"
            );
            assert!(
                response
                    .headers()
                    .get(CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .is_some_and(
                        |value| value.starts_with(&fixture.non_stream.downstream_content_type)
                    ),
                "{name}: downstream content type"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let actual: Value = serde_json::from_slice(&body).unwrap_or_else(|error| {
                panic!(
                    "{name}: downstream JSON: {error}; body={}",
                    String::from_utf8_lossy(&body)
                )
            });
            assert_eq!(
                normalized(actual),
                normalized(fixture.non_stream.downstream_response.clone()),
                "{name}: downstream response"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(200));
            assert_usage(&log, &fixture.usage);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_stream_events_usage_and_single_call_golden() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Sse {
                events: fixture.stream.upstream_events.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}: stream status");
            assert!(response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).is_some_and(|value| value.starts_with(&fixture.stream.downstream_content_type)), "{name}: stream content type");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let actual_events = parse_downstream_events(fixture.protocol, &body);
            assert_eq!(
                normalized_events(actual_events.clone()),
                normalized_events(fixture.stream.downstream_events.clone()),
                "{name}: ordered stream events; body={}",
                String::from_utf8_lossy(&body)
            );
            assert_eq!(
                stream_text(fixture.protocol, &actual_events),
                fixture.stream.expected_text,
                "{name}: stream text"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.stream.upstream_path,
                fixture.stream.upstream_query.as_deref(),
                &fixture.stream.upstream_request,
                &router.requested_model(),
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_log_common(&router, &fixture, &log);
            assert!(log.is_stream, "{name}: log should be streaming");
            assert_usage(&log, &fixture.usage);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_upstream_429_is_safe_logged_and_never_retried() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::from_u16(fixture.error.upstream_status).unwrap(),
                body: fixture.error.upstream_response.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, false, &fixture.error.downstream_request)
                .await;
            assert_eq!(
                response.status().as_u16(),
                fixture.error.downstream_status,
                "{name}: error status"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                body,
                json!({"code": fixture.error.downstream_code, "message": fixture.error.downstream_message}),
                "{name}: safe error body"
            );
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.request.upstream_path,
                fixture.request.upstream_query.as_deref(),
                &fixture.request.upstream,
                &router.requested_model(),
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(429));
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            assert_eq!(
                log.final_error_message.as_deref(),
                Some("Upstream returned 429: baseline throttled")
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_client_cancellation_closes_upstream_and_logs_cancelled() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
                first_event: fixture.cancellation.first_upstream_event.clone(),
                dropped: Arc::clone(&dropped),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{name}: cancellation stream status"
            );
            let mut body = response.into_body().into_data_stream();
            let first = timeout(WAIT_TIMEOUT, body.next())
                .await
                .expect("first downstream frame deadline")
                .expect("first downstream frame")
                .expect("first downstream frame should be readable");
            assert!(!first.is_empty(), "{name}: first downstream frame");
            drop(body);
            dropped.wait().await;
            let captured = upstream.requests().await;
            assert_upstream(
                name,
                &fixture,
                &captured,
                &fixture.cancellation.upstream_path,
                fixture.cancellation.upstream_query.as_deref(),
                &fixture.cancellation.upstream_request,
                &router.requested_model(),
            );
            let log = router
                .wait_for_log(fixture.cancellation.expected_status.clone())
                .await;
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.overall_status, RequestStatus::Cancelled);
            assert_eq!(captured.len(), 1, "{name}: cancellation must not retry");
            upstream.shutdown().await;
        });
    }
}
