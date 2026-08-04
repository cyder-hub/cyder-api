use std::{
    collections::BTreeMap,
    future::Future,
    io::Write as _,
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
    http::{
        HeaderMap, Method, StatusCode,
        header::{CONTENT_TYPE, LOCATION},
    },
    response::Response,
    routing::any,
    serve,
};
use flate2::{Compression, write::GzEncoder};
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

use super::{
    ExecutionStage, ResponseVisibility, create_proxy_router,
    logging::{RequestLogPersistedContext, RequestLogPersistedSink},
    request_context::{X_CLIENT_REQUEST_ID, X_REQUEST_ID},
};
use crate::{
    config::{ClientIdentityConfig, OutboundHttpConfig, ProxyRequestConfig},
    database::{
        TestDbContext,
        api_key::{ApiKey, CreateApiKeyPayload},
        provider::{Provider, UpdateProviderData},
        request_log::{RequestLog, RequestLogQueryPayload, RequestLogRecord},
        request_patch::CreateRequestPatchPayload,
    },
    ingress::client_identity::ClientIdentityResolver,
    schema::enum_def::{
        Action, DownstreamProtocol, ProviderApiKeyMode, ProviderType, RequestPatchOperation,
        RequestPatchPlacement, RequestStatus,
    },
    service::{
        admin::model::UpdateModelInput,
        admin::provider::BootstrapProviderCommand,
        app_state::{AppState, create_test_app_state},
        infra::AppInfra,
        provider_profile::provider_runtime_profile,
    },
    utils::{
        ID_GENERATOR,
        sse::{SseFrame, SseParser},
    },
};

const DOWNSTREAM_SECRET_MARKER: &str = "cyder-";
const PROVIDER_SECRET: &str = "provider-baseline-secret";
const UPSTREAM_MODEL: &str = "baseline-upstream-model";
const DIRECT_CLIENT_REQUEST_ID: &str = "direct-execution.client-1";
const WAIT_TIMEOUT: Duration = Duration::from_secs(5);

const FIXTURE_SOURCES: [(&str, &str); 4] = [
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
];

#[derive(Clone, Debug, Deserialize, PartialEq)]
struct GoldenEvent {
    #[serde(default)]
    event: Option<String>,
    data: Value,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum DownstreamAuth {
    Bearer,
    XApiKey,
    GeminiQuery,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct RequestGolden {
    pub(super) downstream: Value,
    pub(super) upstream: Value,
    pub(super) upstream_path: String,
    #[serde(default)]
    upstream_query: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub(super) struct NonStreamGolden {
    pub(super) upstream_response: Value,
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
pub(super) struct ErrorGolden {
    pub(super) downstream_request: Value,
    pub(super) upstream_status: u16,
    pub(super) upstream_response: Value,
    pub(super) downstream_status: u16,
    pub(super) downstream_response: Value,
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
pub(super) struct DirectExecutionFixture {
    pub(super) protocol: DownstreamProtocol,
    pub(super) provider_type: ProviderType,
    pub(super) downstream_path: String,
    downstream_stream_path: String,
    pub(super) downstream_auth: DownstreamAuth,
    upstream_headers: BTreeMap<String, String>,
    pub(super) request: RequestGolden,
    pub(super) non_stream: NonStreamGolden,
    stream: StreamGolden,
    usage: UsageGolden,
    pub(super) error: ErrorGolden,
    cancellation: CancellationGolden,
}

pub(super) fn fixtures() -> Vec<(&'static str, DirectExecutionFixture)> {
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
    assert!(
        fixture.error.downstream_response.is_object(),
        "{name}: error downstream response"
    );
    assert_eq!(
        fixture.cancellation.expected_status,
        RequestStatus::Cancelled
    );
}

pub(super) fn run_case<F, Fut>(name: &str, test: F)
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
pub(super) struct CapturedRequest {
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
    Raw {
        status: StatusCode,
        content_type: Option<String>,
        content_encoding: Option<String>,
        body: Vec<u8>,
    },
    Sse {
        events: Vec<GoldenEvent>,
    },
    ChunkedSse {
        content_encoding: Option<String>,
        chunks: Vec<Vec<u8>>,
        hang_after_chunks: bool,
        dropped: Option<Arc<DropSignal>>,
    },
    HangingSse {
        first_event: GoldenEvent,
        dropped: Arc<DropSignal>,
    },
    InterruptedSse {
        first_event: GoldenEvent,
    },
    InterruptedSseBeforeFirstEvent,
    InterruptedBody {
        content_type: String,
        first_chunk: Vec<u8>,
    },
    Redirect {
        status: StatusCode,
        location: String,
    },
}

pub(super) struct TestUpstream {
    pub(super) base_url: String,
    captured: Arc<AsyncMutex<Vec<CapturedRequest>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    task: JoinHandle<()>,
}

impl TestUpstream {
    pub(super) async fn spawn_json(status: StatusCode, body: Value) -> Self {
        Self::spawn(ScriptedReply::Json { status, body }).await
    }

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
                            .header(&X_REQUEST_ID, "upstream-forged-request-id")
                            .header(&X_CLIENT_REQUEST_ID, "upstream-forged-client-id")
                            .body(Body::from(serde_json::to_vec(&body).unwrap()))
                            .unwrap(),
                        ScriptedReply::Raw {
                            status,
                            content_type,
                            content_encoding,
                            body,
                        } => {
                            let mut builder = Response::builder().status(status);
                            if let Some(content_type) = content_type {
                                builder = builder.header(CONTENT_TYPE, content_type);
                            }
                            if let Some(content_encoding) = content_encoding {
                                builder = builder.header("content-encoding", content_encoding);
                            }
                            builder.body(Body::from(body)).unwrap()
                        }
                        ScriptedReply::Sse { events } => Response::builder()
                            .status(StatusCode::OK)
                            .header(CONTENT_TYPE, "text/event-stream")
                            .body(Body::from(events_to_sse_bytes(&events)))
                            .unwrap(),
                        ScriptedReply::ChunkedSse {
                            content_encoding,
                            chunks,
                            hang_after_chunks,
                            dropped,
                        } => {
                            let stream = async_stream::stream! {
                                let _guard = dropped.map(ResponseBodyDropGuard);
                                for chunk in chunks {
                                    yield Ok::<Bytes, std::io::Error>(Bytes::from(chunk));
                                    tokio::time::sleep(Duration::from_millis(10)).await;
                                }
                                if hang_after_chunks {
                                    std::future::pending::<()>().await;
                                }
                            };
                            let mut builder = Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream");
                            if let Some(content_encoding) = content_encoding {
                                builder = builder.header("content-encoding", content_encoding);
                            }
                            builder.body(Body::from_stream(stream)).unwrap()
                        }
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
                        ScriptedReply::InterruptedSse { first_event } => {
                            let stream = async_stream::stream! {
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(events_to_sse_bytes(&[first_event])));
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                yield Err(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionReset,
                                    "scripted upstream stream interruption",
                                ));
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream")
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::InterruptedSseBeforeFirstEvent => {
                            let stream = async_stream::stream! {
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                yield Err::<Bytes, std::io::Error>(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionReset,
                                    "scripted upstream interruption before first event",
                                ));
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, "text/event-stream")
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::InterruptedBody {
                            content_type,
                            first_chunk,
                        } => {
                            let stream = async_stream::stream! {
                                yield Ok::<Bytes, std::io::Error>(Bytes::from(first_chunk));
                                tokio::time::sleep(Duration::from_millis(25)).await;
                                yield Err(std::io::Error::new(
                                    std::io::ErrorKind::ConnectionReset,
                                    "scripted upstream body interruption",
                                ));
                            };
                            Response::builder()
                                .status(StatusCode::OK)
                                .header(CONTENT_TYPE, content_type)
                                .body(Body::from_stream(stream))
                                .unwrap()
                        }
                        ScriptedReply::Redirect { status, location } => Response::builder()
                            .status(status)
                            .header(LOCATION, location)
                            .body(Body::from("redirect"))
                            .unwrap(),
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

    pub(super) async fn requests(&self) -> Vec<CapturedRequest> {
        self.captured.lock().await.clone()
    }

    pub(super) async fn shutdown(mut self) {
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

pub(super) struct RouterFixture {
    pub(super) app_state: Arc<AppState>,
    pub(super) downstream_key: String,
    pub(super) downstream_key_name: String,
    pub(super) downstream_api_key_id: i64,
    provider_id: i64,
    provider_key: String,
    provider_name: String,
    provider_api_key_id: i64,
    model_id: i64,
    model_name: String,
}

#[derive(Default)]
struct RecordingPersistedSink {
    contexts: AsyncMutex<Vec<RequestLogPersistedContext>>,
}

#[async_trait::async_trait]
impl RequestLogPersistedSink for RecordingPersistedSink {
    async fn on_request_log_persisted(&self, context: RequestLogPersistedContext) {
        self.contexts.lock().await.push(context);
    }
}

impl RouterFixture {
    pub(super) async fn new(
        context: TestDbContext,
        fixture: &DirectExecutionFixture,
        base_url: &str,
    ) -> Self {
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
        let downstream_key_name = format!("direct-execution-key-{nonce}");
        let created_key = ApiKey::create(&CreateApiKeyPayload {
            name: downstream_key_name.clone(),
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
        let downstream_api_key_id = created_key.reveal.id;
        let downstream_key = created_key.reveal.api_key;
        Self {
            app_state,
            downstream_key,
            downstream_key_name,
            downstream_api_key_id,
            provider_id: bootstrapped.provider.id,
            provider_key,
            provider_name,
            provider_api_key_id: bootstrapped.created_key.id,
            model_id: bootstrapped.created_model.id,
            model_name: bootstrapped.created_model.model_name,
        }
    }

    pub(super) fn requested_model(&self) -> String {
        format!("{}/{}", self.provider_key, self.model_name)
    }

    async fn replace_proxy_request_config(
        &mut self,
        context: TestDbContext,
        proxy_request: ProxyRequestConfig,
    ) {
        let mut app_state = (*self.app_state).clone();
        app_state.infra = Arc::new(
            AppInfra::new_with_config(
                OutboundHttpConfig::default(),
                proxy_request,
                None,
                Some(context),
            )
            .await,
        );
        self.app_state = Arc::new(app_state);
    }

    fn install_recording_persisted_sink(&self) -> Arc<RecordingPersistedSink> {
        let sink = Arc::new(RecordingPersistedSink::default());
        let persisted_sink: Arc<dyn RequestLogPersistedSink> = sink.clone();
        self.app_state
            .infra
            .log_manager()
            .set_request_log_persisted_sink(persisted_sink);
        sink
    }

    pub(super) async fn send(
        &self,
        fixture: &DirectExecutionFixture,
        stream: bool,
        body: &Value,
    ) -> Response<Body> {
        self.send_with_client_identity(
            fixture,
            stream,
            body,
            SocketAddr::from(([127, 0, 0, 1], 3000)),
            None,
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
        )
        .await
    }

    async fn send_with_client_identity(
        &self,
        fixture: &DirectExecutionFixture,
        stream: bool,
        body: &Value,
        peer_addr: SocketAddr,
        forwarded: Option<&str>,
        resolver: Arc<ClientIdentityResolver>,
    ) -> Response<Body> {
        let path_template = if stream {
            &fixture.downstream_stream_path
        } else {
            &fixture.downstream_path
        };
        let mut uri = path_template.replace("$REQUESTED_MODEL", &self.requested_model());
        let mut builder = Request::builder()
            .method(Method::POST)
            .header(CONTENT_TYPE, "application/json")
            .header(&X_REQUEST_ID, "downstream-forged-request-id")
            .header(&X_CLIENT_REQUEST_ID, DIRECT_CLIENT_REQUEST_ID);
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
        if let Some(forwarded) = forwarded {
            request.headers_mut().insert(
                "forwarded",
                forwarded
                    .parse()
                    .expect("test Forwarded header should construct"),
            );
        }
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(peer_addr));
        create_proxy_router(resolver)
            .with_state(Arc::clone(&self.app_state))
            .oneshot(request)
            .await
            .expect("proxy router should respond")
    }

    async fn send_raw_post(
        &self,
        uri: String,
        body: Value,
        auth: DownstreamAuth,
    ) -> Response<Body> {
        let mut builder = Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json");
        match auth {
            DownstreamAuth::Bearer => {
                builder = builder.header("authorization", format!("Bearer {}", self.downstream_key))
            }
            DownstreamAuth::XApiKey => builder = builder.header("x-api-key", &self.downstream_key),
            DownstreamAuth::GeminiQuery => {
                panic!("Gemini query authentication must be included in the supplied URI")
            }
        }
        let mut request = builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .expect("downstream utility request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3001,
            ))));
        create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
        .with_state(Arc::clone(&self.app_state))
        .oneshot(request)
        .await
        .expect("proxy router should respond")
    }

    async fn send_malformed_generation_body(
        &self,
        fixture: &DirectExecutionFixture,
    ) -> Response<Body> {
        let mut uri = fixture
            .downstream_path
            .replace("$REQUESTED_MODEL", &self.requested_model());
        let mut builder = Request::builder()
            .method(Method::POST)
            .header(CONTENT_TYPE, "application/json")
            .header(&X_CLIENT_REQUEST_ID, DIRECT_CLIENT_REQUEST_ID);
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
            .body(Body::from("{not-json"))
            .expect("malformed downstream request should build");
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(SocketAddr::from((
                [127, 0, 0, 1],
                3002,
            ))));
        create_proxy_router(Arc::new(ClientIdentityResolver::new(
            &ClientIdentityConfig::default(),
        )))
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

    pub(super) async fn request_logs(&self) -> Vec<RequestLogRecord> {
        self.app_state.flush_proxy_logs().await;
        RequestLog::list_full(RequestLogQueryPayload {
            provider_id: Some(self.provider_id),
            model_id: Some(self.model_id),
            page: Some(1),
            page_size: Some(10),
            ..Default::default()
        })
        .expect("request logs should be queryable")
        .list
    }

    async fn wait_for_api_key_lease_release(&self) {
        let deadline = Instant::now() + WAIT_TIMEOUT;
        loop {
            let snapshot = self
                .app_state
                .api_key_governance
                .get_api_key_governance_snapshot(self.downstream_api_key_id)
                .await
                .expect("API key governance snapshot should load");
            if snapshot.current_concurrency == 0 {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "API key request lease should release before deadline"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

async fn assert_single_persisted_terminal_fact(
    sink: &RecordingPersistedSink,
    expected_stage: ExecutionStage,
    expected_visibility: ResponseVisibility,
) {
    let contexts = sink.contexts.lock().await;
    assert_eq!(
        contexts.len(),
        1,
        "exactly one terminal fact should persist"
    );
    assert_eq!(contexts[0].final_error_stage, Some(expected_stage));
    assert_eq!(contexts[0].response_visibility, expected_visibility);
}

pub(super) fn render_value(value: &Value, requested_model: &str) -> Value {
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

fn render_request_id(value: &Value, request_id: &str) -> Value {
    match value {
        Value::String(value) if value == "$REQUEST_ID" => Value::String(request_id.to_string()),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| render_request_id(value, request_id))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), render_request_id(value, request_id)))
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

fn gzip_bytes(body: &[u8]) -> Vec<u8> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(body).expect("gzip fixture should encode");
    encoder.finish().expect("gzip fixture should finish")
}

fn json_body_with_exact_serialized_size(size: usize) -> Value {
    let empty = json!({"padding": ""});
    let overhead = serde_json::to_vec(&empty).unwrap().len();
    assert!(size >= overhead);
    let body = json!({"padding": "x".repeat(size - overhead)});
    assert_eq!(serde_json::to_vec(&body).unwrap().len(), size);
    body
}

fn one_mib_non_stream_proxy_config(disclosure_limit: usize) -> ProxyRequestConfig {
    let mut config = ProxyRequestConfig::default();
    config.upstream_error_body_limit_bytes = disclosure_limit;
    config.non_stream_response.raw_body_limit_bytes = 1_048_576;
    config.non_stream_response.decoded_body_limit_bytes = 1_048_576;
    config
        .validate()
        .expect("test proxy limits should validate");
    config
}

fn sse_proxy_config(
    line_limit_bytes: usize,
    event_limit_bytes: usize,
    buffer_limit_bytes: usize,
    frame_count_limit: u64,
) -> ProxyRequestConfig {
    let mut config = ProxyRequestConfig::default();
    config.sse_response = crate::config::SseResponseConfig {
        line_limit_bytes,
        event_limit_bytes,
        buffer_limit_bytes,
        frame_count_limit,
    };
    config
        .validate()
        .expect("test SSE response limits should validate");
    config
}

fn parse_downstream_events(
    _downstream_protocol: DownstreamProtocol,
    body: &[u8],
) -> Vec<GoldenEvent> {
    let mut parser = SseParser::new(crate::config::SseResponseConfig::default());
    let mut frames = Vec::new();
    let mut next = parser.feed(body).expect("downstream SSE should parse");
    loop {
        match next {
            Some(SseFrame::Event(event)) => frames.push(event),
            Some(SseFrame::NonDispatch) => {}
            None => break,
        }
        next = parser.feed(&[]).expect("downstream SSE should drain");
    }
    while let Some(frame) = parser.finish().expect("downstream SSE EOF should parse") {
        if let SseFrame::Event(event) = frame {
            frames.push(event);
        }
    }
    frames
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

fn stream_text(downstream_protocol: DownstreamProtocol, events: &[GoldenEvent]) -> String {
    events
        .iter()
        .filter_map(|event| match downstream_protocol {
            DownstreamProtocol::Openai => event
                .data
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str),
            DownstreamProtocol::Responses => (event.data.get("type").and_then(Value::as_str)
                == Some("response.output_text.delta"))
            .then(|| event.data.get("delta").and_then(Value::as_str))
            .flatten(),
            DownstreamProtocol::Anthropic => {
                event.data.pointer("/delta/text").and_then(Value::as_str)
            }
            DownstreamProtocol::Gemini => event
                .data
                .pointer("/candidates/0/content/parts/0/text")
                .and_then(Value::as_str),
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
    expected_request_id: &str,
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
    assert_eq!(
        request
            .headers
            .get(&X_REQUEST_ID)
            .and_then(|value| value.to_str().ok()),
        Some(expected_request_id),
        "{name}: upstream canonical request id"
    );
    assert!(
        request.headers.get(&X_CLIENT_REQUEST_ID).is_none(),
        "{name}: client request id must not be sent upstream"
    );
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

fn assert_downstream_request_identity(response: &Response<Body>) -> String {
    let request_id = response
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .expect("proxy response should include canonical request id");
    uuid::Uuid::parse_str(request_id).expect("canonical request id should be a UUID");
    assert_ne!(request_id, "downstream-forged-request-id");
    assert_ne!(request_id, "upstream-forged-request-id");
    assert_eq!(
        response
            .headers()
            .get(&X_CLIENT_REQUEST_ID)
            .and_then(|value| value.to_str().ok()),
        Some(DIRECT_CLIENT_REQUEST_ID),
        "proxy response should echo only the validated downstream client id"
    );
    request_id.to_string()
}

fn assert_log_common(
    router: &RouterFixture,
    fixture: &DirectExecutionFixture,
    log: &RequestLogRecord,
) {
    let upstream_protocol = provider_runtime_profile(&fixture.provider_type).upstream_protocol;
    uuid::Uuid::parse_str(&log.request_id).expect("persisted request id should be a UUID");
    assert_eq!(
        log.client_request_id.as_deref(),
        Some(DIRECT_CLIENT_REQUEST_ID),
        "validated client request id should persist"
    );
    assert_eq!(log.downstream_protocol, fixture.protocol);
    assert_eq!(log.upstream_protocol, Some(upstream_protocol));
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
    assert_eq!(log.client_ip.as_deref(), Some("127.0.0.1"));
}

fn assert_log_timing_order(log: &RequestLogRecord) {
    let sent = log
        .upstream_request_sent_at
        .expect("upstream request timing should be persisted");
    let headers = log
        .upstream_response_headers_at
        .expect("upstream response headers timing should be persisted");
    assert!(headers >= sent);
    if let Some(raw) = log.upstream_first_body_chunk_at {
        assert!(raw >= headers);
    }
    if let Some(first_response_body) = log.first_response_body_at {
        assert!(first_response_body >= sent);
        if let Some(raw) = log.upstream_first_body_chunk_at {
            assert!(first_response_body >= raw);
        }
    }
    if let Some(first_token) = log.first_token_at {
        assert!(log.is_stream);
        let raw = log
            .upstream_first_body_chunk_at
            .expect("TTFT requires an upstream raw body timestamp");
        assert!(first_token >= raw);
    }
    if let Some(max_idle) = log.max_upstream_response_idle_ms {
        assert!(max_idle >= 0);
    }
    let completed = log
        .completed_at
        .expect("completed timing should be persisted");
    assert!(completed >= sent);
    for stage in [
        log.upstream_response_headers_at,
        log.upstream_first_body_chunk_at,
        log.first_response_body_at,
        log.first_token_at,
    ]
    .into_iter()
    .flatten()
    {
        assert!(completed >= stage);
    }
}

fn assert_usage(log: &RequestLogRecord, usage: &UsageGolden) {
    assert_eq!(log.total_input_tokens, Some(usage.input));
    assert_eq!(log.total_output_tokens, Some(usage.output));
    assert_eq!(log.total_tokens, Some(usage.total));
}

fn downstream_error_code(body: &Value, protocol: DownstreamProtocol) -> Option<&str> {
    assert!(
        body.get("code").is_none(),
        "legacy top-level code must be absent"
    );
    assert!(
        body.get("message").is_none(),
        "legacy top-level message must be absent"
    );
    match protocol {
        DownstreamProtocol::Openai
        | DownstreamProtocol::Responses
        | DownstreamProtocol::Anthropic => body["error"]["code"].as_str(),
        DownstreamProtocol::Gemini => {
            body["error"]["details"][0]["metadata"]["cyder_code"].as_str()
        }
    }
}

fn downstream_error_message(body: &Value) -> Option<&str> {
    body["error"]["message"].as_str()
}

#[test]
fn direct_execution_regression_fixtures_define_four_complete_protocols() {
    let fixtures = fixtures();
    assert_eq!(fixtures.len(), 4);
    for (name, fixture) in fixtures {
        validate_fixture(name, &fixture);
    }
}

#[test]
fn malformed_request_is_rejected_before_request_record_or_upstream_call() {
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

        let response = router.send_malformed_generation_body(&fixture).await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("parse error response body should read");
        let body: Value =
            serde_json::from_slice(&body).expect("parse error response should be JSON");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("invalid_request_error")
        );
        assert!(router.request_logs().await.is_empty());
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn model_resolution_preserves_parse_and_capability_error_codes() {
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

        for (requested_model, expected_code) in [
            ("gpt-4o", "invalid_request_error"),
            ("missing-provider/gpt-4o", "unsupported_capability_error"),
        ] {
            let mut request_body = fixture.request.downstream.clone();
            request_body
                .as_object_mut()
                .expect("OpenAI request fixture should be an object")
                .insert("model".to_string(), json!(requested_model));

            let response = router.send(&fixture, false, &request_body).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("model resolution response body should read");
            let body: Value =
                serde_json::from_slice(&body).expect("model resolution response should be JSON");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some(expected_code),
                "{requested_model}"
            );
        }

        assert!(router.request_logs().await.is_empty());
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
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
fn acl_rejection_precedes_invalid_provider_endpoint_preflight() {
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
        Provider::update(
            router.provider_id,
            &UpdateProviderData {
                provider_key: None,
                name: None,
                endpoint: Some("http://user:secret@127.0.0.1:1/v1".to_string()),
                use_proxy: None,
                is_enabled: None,
                provider_type: None,
                provider_api_key_mode: None,
            },
        )
        .expect("legacy invalid endpoint should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
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
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.final_error_code.as_deref(), Some("permission_error"));
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("Access denied"))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn acl_rejection_precedes_missing_proxy_preflight() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let infra_context = context.clone();
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let mut router = RouterFixture::new_with_default_action(
            context,
            &fixture,
            &upstream.base_url,
            Action::Deny,
        )
        .await;
        let mut app_state = (*router.app_state).clone();
        app_state.infra = Arc::new(
            AppInfra::new_with_config(
                OutboundHttpConfig::default(),
                ProxyRequestConfig::default(),
                None,
                Some(infra_context),
            )
            .await,
        );
        router.app_state = Arc::new(app_state);
        Provider::update(
            router.provider_id,
            &UpdateProviderData {
                provider_key: None,
                name: None,
                endpoint: None,
                use_proxy: Some(true),
                is_enabled: None,
                provider_type: None,
                provider_api_key_mode: None,
            },
        )
        .expect("proxy requirement should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
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
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.final_error_code.as_deref(), Some("permission_error"));
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("Access denied"))
        );
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
fn request_patch_query_value_reaches_upstream_but_not_request_log() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        const PATCH_QUERY_SECRET: &str = "patch-query-secret-marker";
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        router
            .app_state
            .admin
            .request_patch
            .create_provider_request_patch(
                router.provider_id,
                CreateRequestPatchPayload {
                    placement: RequestPatchPlacement::Query,
                    target: "diagnostic".to_string(),
                    operation: RequestPatchOperation::Set,
                    value_json: Some(Some(json!(PATCH_QUERY_SECRET))),
                    description: Some("transient query regression".to_string()),
                    is_enabled: Some(true),
                    confirm_dangerous_target: None,
                },
            )
            .await
            .expect("query patch should create");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let _ = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body");
        let captured = upstream.requests().await;
        assert_eq!(captured.len(), 1);
        assert!(
            captured[0]
                .query
                .as_deref()
                .is_some_and(|query| query.contains(PATCH_QUERY_SECRET)),
            "fixture must prove the raw query patch reached the selected upstream"
        );

        let log = router.wait_for_log(RequestStatus::Success).await;
        let persisted = serde_json::to_string(&log).expect("request log should serialize");
        for secret in [
            PATCH_QUERY_SECRET,
            PROVIDER_SECRET,
            router.downstream_key.as_str(),
        ] {
            assert!(
                !persisted.contains(secret),
                "transient URL/query and credentials must not enter Request Log: {secret}"
            );
        }
        assert_eq!(router.request_logs().await.len(), 1);
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
            let request_id = assert_downstream_request_identity(&response);
            assert!(
                response.headers().get("retry-after").is_none(),
                "{name}: provider Retry-After must not pass through"
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
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_log_timing_order(&log);
            assert!(log.first_token_at.is_none());
            assert_eq!(log.upstream_http_status, Some(200));
            assert_usage(&log, &fixture.usage);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn persisted_log_sink_receives_the_canonical_request_id() {
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
        let sink = Arc::new(RecordingPersistedSink::default());
        let persisted_sink: Arc<dyn RequestLogPersistedSink> = sink.clone();
        router
            .app_state
            .infra
            .log_manager()
            .set_request_log_persisted_sink(persisted_sink);

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = assert_downstream_request_identity(&response);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body should be consumed before persistence assertion");
        let log = router.wait_for_log(RequestStatus::Success).await;
        let contexts = sink.contexts.lock().await;

        assert_eq!(
            contexts.as_slice(),
            &[RequestLogPersistedContext {
                request_log_id: log.id,
                request_id,
                final_error_stage: None,
                response_visibility: ResponseVisibility::NotVisible,
            }]
        );
        upstream.shutdown().await;
    });
}

#[test]
fn four_public_downstream_generation_paths_call_upstream_at_most_once() {
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

            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_eq!(
                upstream.requests().await.len(),
                1,
                "{name}: direct execution must issue exactly one upstream request"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_client_identity_http_persists_normalized_forwarded_ip() {
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
        let resolver = Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig {
            trusted_proxy_cidrs: vec!["10.0.0.0/8".parse().expect("test CIDR should parse")],
            max_forwarded_hops: 8,
        }));

        let response = router
            .send_with_client_identity(
                &fixture,
                false,
                &fixture.request.downstream,
                SocketAddr::from(([10, 0, 0, 9], 3000)),
                Some("for=\"[::ffff:198.51.100.42]:8443\""),
                resolver,
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body should be consumed before log assertion");
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert_eq!(log.client_ip.as_deref(), Some("198.51.100.42"));
        upstream.shutdown().await;
    });
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
            let request_id = assert_downstream_request_identity(&response);
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
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Success).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: stream response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_log_timing_order(&log);
            assert!(log.first_token_at.is_some(), "{name}: stream TTFT sample");
            assert!(log.is_stream, "{name}: log should be streaming");
            assert_usage(&log, &fixture.usage);
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_reject_sse_encoding_before_headers() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("text/event-stream; charset=utf-8".to_string()),
                content_encoding: Some("gzip".to_string()),
                body: gzip_bytes(b"data: provider-secret\n\n"),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.stream.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{name}");
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("pre-commit SSE encoding error should be a readable envelope");
            let body: Value = serde_json::from_slice(&body)
                .expect("pre-commit SSE encoding error should use the protocol envelope");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(
                downstream_error_message(&body),
                Some("The gateway could not read a valid response from the upstream provider."),
                "{name}"
            );
            assert!(body.get("upstream_error").is_none(), "{name}");

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{name}: no retry");
            assert_eq!(
                captured[0].headers.get("accept-encoding").unwrap(),
                "identity"
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert_eq!(log.upstream_http_status, Some(200));
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::NotVisible,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            let health = router
                .app_state
                .provider_circuit
                .get_provider_health_snapshot(router.provider_id)
                .await
                .unwrap();
            assert_eq!(health.consecutive_failures, 1, "{name}");
            assert!(!health.half_open_probe_in_flight, "{name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_terminate_body_on_sse_parser_failure() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks: vec![
                    events_to_sse_bytes(&[fixture.cancellation.first_upstream_event.clone()]),
                    b"data: \xff\n\n".to_vec(),
                ],
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            let request_id = assert_downstream_request_identity(&response);
            let mut body = response.into_body().into_data_stream();
            let mut successful_body = Vec::new();
            let mut saw_error = false;
            loop {
                let item = timeout(WAIT_TIMEOUT, body.next())
                    .await
                    .expect("post-commit parser failure should terminate before deadline");
                match item {
                    Some(Ok(chunk)) => successful_body.extend_from_slice(&chunk),
                    Some(Err(_)) => {
                        saw_error = true;
                        break;
                    }
                    None => break,
                }
            }
            assert!(
                saw_error,
                "{name}: parser failure must surface as a Body error"
            );
            assert!(
                !successful_body.is_empty(),
                "{name}: a valid event must precede failure"
            );
            assert!(
                !successful_body
                    .windows("upstream_response_error".len())
                    .any(|window| window == b"upstream_response_error"),
                "{name}: no second protocol envelope may be written after Body start"
            );
            assert!(
                timeout(WAIT_TIMEOUT, body.next()).await.unwrap().is_none(),
                "{name}: Body must end immediately after its single error"
            );
            dropped.wait().await;

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.request_id, request_id, "{name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert_eq!(log.upstream_http_status, Some(200));
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                ResponseVisibility::BodyStarted,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            let health = router
                .app_state
                .provider_circuit
                .get_provider_health_snapshot(router.provider_id)
                .await
                .unwrap();
            assert_eq!(health.consecutive_failures, 1, "{name}");
            assert!(!health.half_open_probe_in_flight, "{name}");
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_sse_resource_limits_finalize_once() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    let first_event = events_to_sse_bytes(&[fixture.cancellation.first_upstream_event.clone()]);
    let exact_data_line = format!("data: {}\n", "x".repeat(1_017)).into_bytes();
    assert_eq!(exact_data_line.len(), 1_024);
    let partial_line = format!("data: {}", "x".repeat(1_019)).into_bytes();
    assert_eq!(partial_line.len(), 1_025);

    let cases = vec![
        (
            "sse-line-plus-one",
            sse_proxy_config(1_024, 2_048, 4_096, 100),
            vec![
                first_event.clone(),
                format!("data: {}\n\n", "x".repeat(1_019)).into_bytes(),
            ],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-event-plus-one",
            sse_proxy_config(1_024, 2_048, 4_096, 100),
            vec![
                first_event.clone(),
                format!(
                    "data: {}\ndata: {}\ndata: {}\n\n",
                    "x".repeat(700),
                    "x".repeat(700),
                    "x".repeat(700)
                )
                .into_bytes(),
            ],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-buffer-plus-one",
            sse_proxy_config(1_024, 2_048, 2_048, 100),
            vec![first_event.clone(), exact_data_line, partial_line],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-frame-plus-one",
            sse_proxy_config(1_024, 2_048, 4_096, 1),
            vec![first_event.clone(), b":\n\n".to_vec()],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-invalid-utf8",
            sse_proxy_config(1_024, 2_048, 4_096, 100),
            vec![first_event, b"data: \xff\n\n".to_vec()],
            ResponseVisibility::BodyStarted,
        ),
        (
            "sse-single-chunk-buffer-plus-one",
            sse_proxy_config(1_024, 2_048, 2_048, 100),
            vec![vec![b'x'; 2_049]],
            ResponseVisibility::HeadersCommitted,
        ),
    ];

    for (case_name, proxy_config, chunks, expected_visibility) in cases {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let dropped = Arc::new(DropSignal::default());
            let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
                content_encoding: None,
                chunks,
                hang_after_chunks: true,
                dropped: Some(Arc::clone(&dropped)),
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, proxy_config)
                .await;
            let persisted_sink = router.install_recording_persisted_sink();

            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let mut body = response.into_body().into_data_stream();
            let mut successful_chunks = 0usize;
            let mut saw_error = false;
            while let Some(item) = timeout(WAIT_TIMEOUT, body.next()).await.unwrap() {
                match item {
                    Ok(_) => successful_chunks += 1,
                    Err(_) => {
                        saw_error = true;
                        break;
                    }
                }
            }
            assert!(saw_error, "{case_name}");
            assert_eq!(
                successful_chunks > 0,
                expected_visibility == ResponseVisibility::BodyStarted,
                "{case_name}"
            );
            dropped.wait().await;
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error")
            );
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::UpstreamResponse,
                expected_visibility,
            )
            .await;
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{case_name}");
            assert_eq!(
                router
                    .app_state
                    .provider_circuit
                    .get_provider_health_snapshot(router.provider_id)
                    .await
                    .unwrap()
                    .consecutive_failures,
                1,
                "{case_name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_sse_eof_residual_is_discarded() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let first_event = events_to_sse_bytes(&[fixture.cancellation.first_upstream_event.clone()]);
        let split = first_event.len() / 2;
        let upstream = TestUpstream::spawn(ScriptedReply::ChunkedSse {
            content_encoding: Some("identity".to_string()),
            chunks: vec![
                first_event[..split].to_vec(),
                first_event[split..].to_vec(),
                b"data: unterminated residual".to_vec(),
            ],
            hang_after_chunks: false,
            dropped: None,
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("EOF residual should be discarded without failing the stream");
        assert_eq!(parse_downstream_events(fixture.protocol, &body).len(), 1);
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_openai_done_closes_upstream_and_finalizes_once() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let dropped = Arc::new(DropSignal::default());
        let upstream = TestUpstream::spawn(ScriptedReply::HangingSse {
            first_event: GoldenEvent {
                event: None,
                data: Value::String("[DONE]".to_string()),
            },
            dropped: Arc::clone(&dropped),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();
        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("[DONE] should terminate the downstream stream cleanly");
        assert_eq!(body, Bytes::from_static(b"data: [DONE]\n\n"));
        dropped.wait().await;
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        router.wait_for_api_key_lease_release().await;
        assert_eq!(router.request_logs().await.len(), 1);
        let contexts = persisted_sink.contexts.lock().await;
        assert_eq!(contexts.len(), 1);
        drop(contexts);
        let health = router
            .app_state
            .provider_circuit
            .get_provider_health_snapshot(router.provider_id)
            .await
            .unwrap();
        assert_eq!(health.consecutive_failures, 0);
        assert!(!health.half_open_probe_in_flight);
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_stream_emits_one_event_per_downstream_body_chunk() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Sse {
            events: fixture.stream.upstream_events.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let response = router
            .send(&fixture, true, &fixture.stream.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut body = response.into_body().into_data_stream();
        let mut body_chunks = 0usize;
        while let Some(chunk) = body.next().await {
            let chunk = chunk.expect("valid stream chunk");
            assert_eq!(
                parse_downstream_events(fixture.protocol, &chunk).len(),
                1,
                "each downstream Body chunk must contain one transformed SSE event"
            );
            body_chunks += 1;
        }
        assert_eq!(body_chunks, fixture.stream.downstream_events.len());
        let log = router.wait_for_log(RequestStatus::Success).await;
        assert!(log.final_error_code.is_none());
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_regression_upstream_429_is_authentic_logged_and_never_retried() {
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
            let request_id = assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                body,
                render_request_id(&fixture.error.downstream_response, &request_id),
                "{name}: complete protocol error envelope and authentic upstream extension"
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
                &request_id,
            );
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: error response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_eq!(log.upstream_http_status, Some(429));
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            assert!(
                log.final_error_message
                    .as_deref()
                    .is_some_and(|message| message.contains("JSON error body with a message field"))
            );
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains("baseline throttled")
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn explicit_upstream_statuses_preserve_json_text_binary_empty_and_truncated_bodies() {
    #[derive(Clone)]
    enum ExpectedBody {
        Json(Value),
        Text(String),
        Base64(Vec<u8>),
    }

    #[derive(Clone)]
    struct ErrorCase {
        name: &'static str,
        upstream_status: StatusCode,
        content_type: Option<&'static str>,
        upstream_body: Vec<u8>,
        downstream_status: StatusCode,
        downstream_code: &'static str,
        downstream_message: &'static str,
        expected_body: ExpectedBody,
        truncated: bool,
    }

    let long_body = "x".repeat(65_537);
    let cases = [
        ErrorCase {
            name: "upstream-401-json",
            upstream_status: StatusCode::UNAUTHORIZED,
            content_type: Some("application/json"),
            upstream_body: br#"{"detail":"invalid provider credential"}"#.to_vec(),
            downstream_status: StatusCode::BAD_GATEWAY,
            downstream_code: "upstream_authentication_error",
            downstream_message: "Upstream provider rejected its credentials.",
            expected_body: ExpectedBody::Json(json!({"detail":"invalid provider credential"})),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-403-text",
            upstream_status: StatusCode::FORBIDDEN,
            content_type: Some("text/plain; charset=utf-8"),
            upstream_body: b"provider credential lacks scope".to_vec(),
            downstream_status: StatusCode::BAD_GATEWAY,
            downstream_code: "upstream_authentication_error",
            downstream_message: "Upstream provider rejected its credentials.",
            expected_body: ExpectedBody::Text("provider credential lacks scope".to_string()),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-413-text",
            upstream_status: StatusCode::PAYLOAD_TOO_LARGE,
            content_type: Some("text/plain"),
            upstream_body: b"provider payload limit".to_vec(),
            downstream_status: StatusCode::PAYLOAD_TOO_LARGE,
            downstream_code: "upstream_payload_too_large_error",
            downstream_message: "Upstream provider rejected the request payload as too large.",
            expected_body: ExpectedBody::Text("provider payload limit".to_string()),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-500-binary",
            upstream_status: StatusCode::INTERNAL_SERVER_ERROR,
            content_type: Some("application/octet-stream"),
            upstream_body: vec![0xff, 0x00, 0x01, 0xfe],
            downstream_status: StatusCode::SERVICE_UNAVAILABLE,
            downstream_code: "upstream_service_error",
            downstream_message: "Upstream provider service failed.",
            expected_body: ExpectedBody::Base64(vec![0xff, 0x00, 0x01, 0xfe]),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-302-empty",
            upstream_status: StatusCode::FOUND,
            content_type: None,
            upstream_body: Vec::new(),
            downstream_status: StatusCode::BAD_GATEWAY,
            downstream_code: "upstream_unexpected_status_error",
            downstream_message: "Upstream provider returned an unexpected HTTP status.",
            expected_body: ExpectedBody::Text(String::new()),
            truncated: false,
        },
        ErrorCase {
            name: "upstream-400-truncated",
            upstream_status: StatusCode::BAD_REQUEST,
            content_type: Some("application/json"),
            upstream_body: long_body.as_bytes().to_vec(),
            downstream_status: StatusCode::BAD_REQUEST,
            downstream_code: "upstream_invalid_request_error",
            downstream_message: "Upstream provider rejected the request.",
            expected_body: ExpectedBody::Text("x".repeat(65_536)),
            truncated: true,
        },
    ];

    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");

    for case in cases {
        let fixture = fixture.clone();
        run_case(case.name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: case.upstream_status,
                content_type: case.content_type.map(str::to_string),
                content_encoding: None,
                body: case.upstream_body.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), case.downstream_status, "{}", case.name);
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("error response should read");
            let body: Value = serde_json::from_slice(&body).expect("error response should be json");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some(case.downstream_code),
                "{}",
                case.name
            );
            assert_eq!(
                downstream_error_message(&body),
                Some(case.downstream_message),
                "{}",
                case.name
            );
            let upstream_error = &body["upstream_error"];
            assert_eq!(
                upstream_error["status"],
                case.upstream_status.as_u16(),
                "{}",
                case.name
            );
            assert_eq!(
                upstream_error["content_type"],
                case.content_type
                    .map_or(Value::Null, |value| Value::String(value.to_string())),
                "{}",
                case.name
            );
            assert_eq!(upstream_error["truncated"], case.truncated, "{}", case.name);
            assert_eq!(
                upstream_error["captured_bytes"],
                case.upstream_body.len().min(65_536),
                "{}",
                case.name
            );
            assert_eq!(upstream_error["limit_bytes"], 65_536, "{}", case.name);

            match &case.expected_body {
                ExpectedBody::Json(expected) => {
                    assert_eq!(&upstream_error["body"], expected, "{}", case.name);
                }
                ExpectedBody::Text(expected) => {
                    assert_eq!(&upstream_error["body_text"], expected, "{}", case.name);
                }
                ExpectedBody::Base64(expected) => {
                    use base64::{Engine as _, engine::general_purpose::STANDARD};
                    let encoded = upstream_error["body_base64"]
                        .as_str()
                        .expect("binary error body should be base64");
                    assert_eq!(
                        STANDARD.decode(encoded).expect("base64 should decode"),
                        *expected,
                        "{}",
                        case.name
                    );
                    assert_eq!(upstream_error["body_encoding"], "base64", "{}", case.name);
                }
            }
            if case.truncated {
                assert_eq!(
                    upstream_error["notice"], "Upstream error body was truncated by the gateway.",
                    "{}",
                    case.name
                );
                assert!(upstream_error.get("body").is_none(), "{}", case.name);
            } else {
                assert!(upstream_error.get("notice").is_none(), "{}", case.name);
            }

            assert_eq!(upstream.requests().await.len(), 1, "{}", case.name);
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some(case.downstream_code),
                "{}",
                case.name
            );
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(case.upstream_status.as_u16())),
                "{}",
                case.name
            );
            if let Ok(upstream_text) = std::str::from_utf8(&case.upstream_body)
                && !upstream_text.is_empty()
            {
                assert!(
                    !log.final_error_message
                        .as_deref()
                        .unwrap_or_default()
                        .contains(upstream_text),
                    "{}: Request Log must not persist the complete upstream body",
                    case.name
                );
            }
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_connect_failure_has_fixed_gateway_payload_without_upstream_extension() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let unused_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("unused local endpoint should bind");
        let unused_addr = unused_listener
            .local_addr()
            .expect("unused local endpoint address");
        drop(unused_listener);
        let endpoint = format!("http://{unused_addr}");
        let router = RouterFixture::new(context, &fixture, &endpoint).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_downstream_request_identity(&response);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("connect error response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("connect error response should be json");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("upstream_connect_error")
        );
        assert_eq!(
            downstream_error_message(&body),
            Some("The gateway could not connect to the upstream provider.")
        );
        assert!(body.get("upstream_error").is_none());

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_connect_error")
        );
        assert!(log.upstream_http_status.is_none());
    });
}

#[test]
fn direct_execution_non_stream_body_interruption_is_an_upstream_response_error() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::InterruptedBody {
            content_type: "application/json".to_string(),
            first_chunk: br#"{"partial":"#.to_vec(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_downstream_request_identity(&response);
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect_err("interrupted successful response body should surface an I/O error");
        assert_eq!(upstream.requests().await.len(), 1);

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_non_stream_identity_and_gzip_enforce_exact_and_plus_one_limits() {
    const LIMIT: usize = 1_048_576;
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");

    for (case_name, use_gzip, body_size, succeeds) in [
        ("identity-exact-limit", false, LIMIT, true),
        ("identity-raw-plus-one", false, LIMIT + 1, false),
        ("gzip-decoded-exact-limit", true, LIMIT, true),
        ("gzip-decoded-plus-one", true, LIMIT + 1, false),
    ] {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let response_value = json_body_with_exact_serialized_size(body_size);
            let decoded = serde_json::to_vec(&response_value).unwrap();
            let (content_encoding, wire_body) = if use_gzip {
                (Some("gzip".to_string()), gzip_bytes(&decoded))
            } else {
                (None, decoded)
            };
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding,
                body: wire_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{case_name}");
            let body_result = axum::body::to_bytes(response.into_body(), usize::MAX).await;
            if succeeds {
                let body = body_result.unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(body, response_value, "{case_name}");
            } else {
                body_result.expect_err("{case_name}: guarded body should surface an I/O error");
            }

            let captured = upstream.requests().await;
            assert_eq!(captured.len(), 1, "{case_name}: no retry");
            assert_eq!(
                captured[0].headers.get("accept-encoding").unwrap(),
                "gzip, identity",
                "{case_name}"
            );
            let expected_status = if succeeds {
                RequestStatus::Success
            } else {
                RequestStatus::Error
            };
            let log = router.wait_for_log(expected_status).await;
            assert_eq!(log.upstream_http_status, Some(200), "{case_name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                (!succeeds).then_some("upstream_response_error"),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            let provider_health = router
                .app_state
                .provider_circuit
                .get_provider_health_snapshot(router.provider_id)
                .await
                .unwrap();
            assert_eq!(
                provider_health.consecutive_failures,
                if succeeds { 0 } else { 1 },
                "{case_name}"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_use_existing_envelopes_for_non_stream_response_limit() {
    const LIMIT: usize = 1_048_576;
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: None,
                body: vec![b'x'; LIMIT + 1],
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_downstream_request_identity(&response);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect_err("{name}: response limit should surface through the guarded body");
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(log.upstream_http_status, Some(200), "{name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            assert_eq!(
                router
                    .app_state
                    .provider_circuit
                    .get_provider_health_snapshot(router.provider_id)
                    .await
                    .unwrap()
                    .consecutive_failures,
                1,
                "{name}"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_use_existing_envelopes_for_decoded_response_limit() {
    const LIMIT: usize = 1_048_576;
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let decoded = serde_json::to_vec(&json_body_with_exact_serialized_size(LIMIT + 1))
                .expect("decoded-limit fixture should serialize");
            let wire_body = gzip_bytes(&decoded);
            assert!(
                wire_body.len() < LIMIT,
                "{name}: fixture must isolate decoded limit"
            );
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::OK,
                content_type: Some("application/json".to_string()),
                content_encoding: Some("gzip".to_string()),
                body: wire_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(context, one_mib_non_stream_proxy_config(65_536))
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::OK, "{name}");
            assert_downstream_request_identity(&response);
            axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect_err(
                    "{name}: decoded response limit should surface through the guarded body",
                );
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_response_error"),
                "{name}"
            );
            assert_eq!(log.upstream_http_status, Some(200), "{name}");
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            assert_eq!(
                router
                    .app_state
                    .provider_circuit
                    .get_provider_health_snapshot(router.provider_id)
                    .await
                    .unwrap()
                    .consecutive_failures,
                1,
                "{name}"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn four_public_protocols_preserve_bounded_provider_error_when_body_reaches_hard_limit() {
    const DISCLOSURE_LIMIT: usize = 1_024;
    const RAW_LIMIT: usize = 1_048_576;
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let marker = format!("provider-error-prefix-{name}");
            let mut upstream_body = marker.as_bytes().to_vec();
            upstream_body.resize(RAW_LIMIT + 1, b'x');
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: StatusCode::TOO_MANY_REQUESTS,
                content_type: Some("text/plain; private=discarded".to_string()),
                content_encoding: None,
                body: upstream_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(
                    context,
                    one_mib_non_stream_proxy_config(DISCLOSURE_LIMIT),
                )
                .await;

            let response = router
                .send(&fixture, false, &fixture.error.downstream_request)
                .await;
            assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS, "{name}");
            assert_downstream_request_identity(&response);
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_rate_limit_error"),
                "{name}"
            );
            assert_eq!(body["upstream_error"]["status"], 429, "{name}");
            assert_eq!(body["upstream_error"]["truncated"], true, "{name}");
            assert_eq!(
                body["upstream_error"]["captured_bytes"], DISCLOSURE_LIMIT,
                "{name}"
            );
            assert_eq!(
                body["upstream_error"]["limit_bytes"], DISCLOSURE_LIMIT,
                "{name}"
            );
            assert!(
                body["upstream_error"]["body_text"]
                    .as_str()
                    .is_some_and(|body| body.starts_with(&marker)),
                "{name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{name}: no retry");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            assert_eq!(log.upstream_http_status, Some(429));
            assert!(
                !log.final_error_message
                    .as_deref()
                    .unwrap_or_default()
                    .contains(&marker),
                "{name}: Provider body must not enter persisted operator diagnostics"
            );
            router.wait_for_api_key_lease_release().await;
            assert_eq!(router.request_logs().await.len(), 1, "{name}");
            assert_eq!(
                router
                    .app_state
                    .provider_circuit
                    .get_provider_health_snapshot(router.provider_id)
                    .await
                    .unwrap()
                    .consecutive_failures,
                1,
                "{name}"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_provider_error_hard_limit_preserves_status_and_disclosure_prefix() {
    const DISCLOSURE_LIMIT: usize = 1_024;
    const RAW_LIMIT: usize = 1_048_576;
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for (case_name, upstream_status, downstream_status, error_code) in [
        (
            "hard-limit-400",
            StatusCode::BAD_REQUEST,
            StatusCode::BAD_REQUEST,
            "upstream_invalid_request_error",
        ),
        (
            "hard-limit-401",
            StatusCode::UNAUTHORIZED,
            StatusCode::BAD_GATEWAY,
            "upstream_authentication_error",
        ),
        (
            "hard-limit-413",
            StatusCode::PAYLOAD_TOO_LARGE,
            StatusCode::PAYLOAD_TOO_LARGE,
            "upstream_payload_too_large_error",
        ),
        (
            "hard-limit-429",
            StatusCode::TOO_MANY_REQUESTS,
            StatusCode::TOO_MANY_REQUESTS,
            "upstream_rate_limit_error",
        ),
        (
            "hard-limit-503",
            StatusCode::SERVICE_UNAVAILABLE,
            StatusCode::SERVICE_UNAVAILABLE,
            "upstream_service_error",
        ),
    ] {
        let fixture = fixture.clone();
        run_case(case_name, move |context| async move {
            let marker = format!("provider-body-secret-marker-{case_name}");
            let mut upstream_body = marker.as_bytes().to_vec();
            upstream_body.resize(RAW_LIMIT + 1, b'x');
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: upstream_status,
                content_type: Some("text/plain; private=discarded".to_string()),
                content_encoding: None,
                body: upstream_body,
            })
            .await;
            let mut router =
                RouterFixture::new(context.clone(), &fixture, &upstream.base_url).await;
            router
                .replace_proxy_request_config(
                    context,
                    one_mib_non_stream_proxy_config(DISCLOSURE_LIMIT),
                )
                .await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), downstream_status, "{case_name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            let body: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some(error_code),
                "{case_name}"
            );
            assert_eq!(
                body["upstream_error"]["status"],
                upstream_status.as_u16(),
                "{case_name}"
            );
            assert_eq!(body["upstream_error"]["truncated"], true, "{case_name}");
            assert_eq!(
                body["upstream_error"]["captured_bytes"], DISCLOSURE_LIMIT,
                "{case_name}"
            );
            assert_eq!(
                body["upstream_error"]["limit_bytes"], DISCLOSURE_LIMIT,
                "{case_name}"
            );
            assert!(
                body["upstream_error"]["body_text"]
                    .as_str()
                    .unwrap()
                    .starts_with(&marker),
                "{case_name}"
            );
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(upstream_status.as_u16())),
                "{case_name}"
            );
            assert_eq!(
                log.final_error_code.as_deref(),
                Some(error_code),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains(&marker),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            assert_eq!(
                router
                    .app_state
                    .provider_circuit
                    .get_provider_health_snapshot(router.provider_id)
                    .await
                    .unwrap()
                    .consecutive_failures,
                1,
                "{case_name}"
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_invalid_gzip_never_falls_back_to_compressed_bytes() {
    let fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    for upstream_status in [StatusCode::OK, StatusCode::TOO_MANY_REQUESTS] {
        let fixture = fixture.clone();
        let case_name = if upstream_status.is_success() {
            "invalid-gzip-success"
        } else {
            "invalid-gzip-provider-error"
        };
        run_case(case_name, move |context| async move {
            let upstream = TestUpstream::spawn(ScriptedReply::Raw {
                status: upstream_status,
                content_type: Some("application/json".to_string()),
                content_encoding: Some("gzip".to_string()),
                body: b"not-a-gzip-provider-body-secret".to_vec(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            if upstream_status.is_success() {
                assert_eq!(response.status(), StatusCode::OK, "{case_name}");
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .expect_err("{case_name}: invalid gzip should fail in the guarded body");
            } else {
                assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{case_name}");
                let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap();
                let body: Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(
                    downstream_error_code(&body, fixture.protocol),
                    Some("upstream_response_error"),
                    "{case_name}"
                );
                assert!(body.get("upstream_error").is_none(), "{case_name}");
            }
            assert_eq!(upstream.requests().await.len(), 1, "{case_name}");
            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(
                log.upstream_http_status,
                Some(i32::from(upstream_status.as_u16())),
                "{case_name}"
            );
            assert!(
                !log.final_error_message
                    .unwrap_or_default()
                    .contains("provider-body-secret"),
                "{case_name}"
            );
            router.wait_for_api_key_lease_release().await;
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_credential_requests_never_follow_redirects() {
    for (name, fixture) in fixtures() {
        run_case(name, move |context| async move {
            let redirect_target = TestUpstream::spawn(ScriptedReply::Json {
                status: StatusCode::OK,
                body: json!({"captured": true}),
            })
            .await;
            let location = format!(
                "{}/credential-capture?evidence=raw",
                redirect_target.base_url
            );
            let upstream = TestUpstream::spawn(ScriptedReply::Redirect {
                status: StatusCode::TEMPORARY_REDIRECT,
                location: location.clone(),
            })
            .await;
            let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;

            let response = router
                .send(&fixture, false, &fixture.request.downstream)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "{name}");
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .expect("redirect error response should read");
            let body: Value =
                serde_json::from_slice(&body).expect("redirect error response should be json");
            assert_eq!(
                downstream_error_code(&body, fixture.protocol),
                Some("upstream_unexpected_status_error")
            );
            assert_eq!(body["upstream_error"]["status"], 307);
            assert_eq!(body["upstream_error"]["body_text"], "redirect");
            assert_eq!(body["upstream_error"]["content_type"], Value::Null);
            assert_eq!(upstream.requests().await.len(), 1, "{name}");
            assert!(
                redirect_target.requests().await.is_empty(),
                "{name}: redirect target must never receive credentials or request body"
            );

            let log = router.wait_for_log(RequestStatus::Error).await;
            assert_eq!(log.upstream_http_status, Some(307), "{name}");
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("upstream_unexpected_status_error"),
                "{name}"
            );

            upstream.shutdown().await;
            redirect_target.shutdown().await;
        });
    }
}

#[test]
fn incompatible_utility_targets_are_rejected_before_any_upstream_call() {
    let openai_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .map(|(_, fixture)| fixture)
        .expect("openai fixture");
    let gemini_fixture = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "gemini")
        .map(|(_, fixture)| fixture)
        .expect("gemini fixture");

    run_case("utility-zero-upstream", move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: json!({"unexpected": true}),
        })
        .await;

        let gemini_target =
            RouterFixture::new(context.clone(), &gemini_fixture, &upstream.base_url).await;
        for (path, body) in [
            (
                "/openai/v1/embeddings",
                json!({"model": gemini_target.requested_model(), "input": "hello"}),
            ),
            (
                "/openai/v1/rerank",
                json!({
                    "model": gemini_target.requested_model(),
                    "query": "hello",
                    "documents": ["world"]
                }),
            ),
        ] {
            let response = gemini_target
                .send_raw_post(path.to_string(), body, DownstreamAuth::Bearer)
                .await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        }

        let openai_target = RouterFixture::new(context, &openai_fixture, &upstream.base_url).await;
        let count_tokens_uri = format!(
            "/gemini/v1/models/{}:countTokens?key={}",
            openai_target.requested_model(),
            openai_target.downstream_key
        );
        let response = openai_target
            .send_raw_post(
                count_tokens_uri,
                json!({"contents": [{"parts": [{"text": "hello"}]}]}),
                DownstreamAuth::Bearer,
            )
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        assert!(
            upstream.requests().await.is_empty(),
            "incompatible utilities must fail before HTTP send"
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_legacy_invalid_endpoint_fails_before_upstream_access() {
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
        Provider::update(
            router.provider_id,
            &UpdateProviderData {
                provider_key: None,
                name: None,
                endpoint: Some("http://user:secret@127.0.0.1:1/v1".to_string()),
                use_proxy: None,
                is_enabled: None,
                provider_type: None,
                provider_api_key_mode: None,
            },
        )
        .expect("legacy invalid endpoint should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("provider configuration response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("provider configuration response should be json");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert_eq!(
            downstream_error_message(&body),
            Some("The gateway provider configuration is invalid.")
        );
        assert!(body.get("upstream_error").is_none());
        assert!(upstream.requests().await.is_empty());
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("provider_configuration_error")
        );
        assert!(
            log.final_error_message
                .as_deref()
                .is_some_and(|message| message.contains("must not contain embedded credentials"))
        );
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_legacy_valid_endpoint_is_normalized_per_request_without_database_write() {
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
        let legacy_endpoint = format!(
            "  {}/v1///  ",
            upstream.base_url.replacen("http://", "HTTP://", 1)
        );
        Provider::update(
            router.provider_id,
            &UpdateProviderData {
                provider_key: None,
                name: None,
                endpoint: Some(legacy_endpoint.clone()),
                use_proxy: None,
                is_enabled: None,
                provider_type: None,
                provider_api_key_mode: None,
            },
        )
        .expect("legacy noncanonical endpoint should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(upstream.requests().await.len(), 1);
        assert_eq!(
            Provider::get_by_id(router.provider_id)
                .expect("provider should remain persisted")
                .endpoint,
            legacy_endpoint
        );
        axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("successful response body should be consumed before log assertion");
        router.wait_for_log(RequestStatus::Success).await;
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_proxy_requirement_without_configuration_fails_closed() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let infra_context = context.clone();
        let upstream = TestUpstream::spawn(ScriptedReply::Json {
            status: StatusCode::OK,
            body: fixture.non_stream.upstream_response.clone(),
        })
        .await;
        let mut router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let mut app_state = (*router.app_state).clone();
        app_state.infra = Arc::new(
            AppInfra::new_with_config(
                OutboundHttpConfig::default(),
                ProxyRequestConfig::default(),
                None,
                Some(infra_context),
            )
            .await,
        );
        router.app_state = Arc::new(app_state);
        Provider::update(
            router.provider_id,
            &UpdateProviderData {
                provider_key: None,
                name: None,
                endpoint: None,
                use_proxy: Some(true),
                is_enabled: None,
                provider_type: None,
                provider_api_key_mode: None,
            },
        )
        .expect("proxy requirement should be seeded directly");
        router
            .app_state
            .catalog
            .invalidate_provider(router.provider_id, Some(&router.provider_key))
            .await
            .expect("provider cache should invalidate");
        router
            .app_state
            .secret_encryption
            .reset_decrypt_call_count();

        let response = router
            .send(&fixture, false, &fixture.request.downstream)
            .await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("proxy configuration response should read");
        let body: Value =
            serde_json::from_slice(&body).expect("proxy configuration response should be json");
        assert_eq!(
            downstream_error_code(&body, fixture.protocol),
            Some("provider_configuration_error")
        );
        assert_eq!(
            downstream_error_message(&body),
            Some("The gateway provider configuration is invalid.")
        );
        assert!(body.get("upstream_error").is_none());
        assert!(upstream.requests().await.is_empty());
        assert_eq!(router.app_state.secret_encryption.decrypt_call_count(), 0);
        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("provider_configuration_error")
        );
        assert_eq!(
            log.final_error_message.as_deref(),
            Some("provider requires the global proxy, but no proxy is configured")
        );
        upstream.shutdown().await;
    });
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
            let persisted_sink = router.install_recording_persisted_sink();
            let response = router
                .send(&fixture, true, &fixture.cancellation.downstream_request)
                .await;
            assert_eq!(
                response.status(),
                StatusCode::OK,
                "{name}: cancellation stream status"
            );
            let request_id = assert_downstream_request_identity(&response);
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
                &request_id,
            );
            let log = router
                .wait_for_log(fixture.cancellation.expected_status.clone())
                .await;
            assert_eq!(
                log.request_id, request_id,
                "{name}: cancelled stream response and persisted canonical request id"
            );
            assert_log_common(&router, &fixture, &log);
            assert_log_timing_order(&log);
            assert_eq!(log.overall_status, RequestStatus::Cancelled);
            assert_eq!(
                log.final_error_code.as_deref(),
                Some("client_cancelled_error")
            );
            assert!(
                log.final_error_message
                    .as_deref()
                    .is_some_and(|message| message.contains("Client disconnected")),
                "{name}: cancelled stream must persist a bounded operator diagnostic"
            );
            router.wait_for_api_key_lease_release().await;
            assert_eq!(
                router.request_logs().await.len(),
                1,
                "{name}: cancelled stream must persist exactly one terminal request log"
            );
            let provider_health = router
                .app_state
                .provider_circuit
                .get_provider_health_snapshot(router.provider_id)
                .await
                .expect("provider circuit snapshot should load");
            assert_eq!(provider_health.consecutive_failures, 0);
            assert!(!provider_health.half_open_probe_in_flight);
            assert_single_persisted_terminal_fact(
                &persisted_sink,
                ExecutionStage::DownstreamSend,
                ResponseVisibility::BodyStarted,
            )
            .await;
            assert_eq!(captured.len(), 1, "{name}: cancellation must not retry");
            upstream.shutdown().await;
        });
    }
}

#[test]
fn direct_execution_regression_interrupted_stream_logs_same_request_id() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::InterruptedSse {
            first_event: fixture.cancellation.first_upstream_event.clone(),
        })
        .await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();
        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = assert_downstream_request_identity(&response);
        let mut body = response.into_body().into_data_stream();
        let first = timeout(WAIT_TIMEOUT, body.next())
            .await
            .expect("first downstream frame deadline")
            .expect("first downstream frame")
            .expect("first downstream frame should be readable");
        assert!(!first.is_empty());
        let interrupted = timeout(WAIT_TIMEOUT, body.next())
            .await
            .expect("stream interruption deadline")
            .expect("stream should yield an interruption");
        assert!(
            interrupted.is_err(),
            "downstream body should surface interruption"
        );

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(
            log.request_id, request_id,
            "interrupted stream response and persisted canonical request id"
        );
        assert_log_common(&router, &fixture, &log);
        assert_eq!(log.overall_status, RequestStatus::Error);
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::BodyStarted,
        )
        .await;
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}

#[test]
fn direct_execution_stream_interruption_before_first_chunk_is_headers_committed() {
    let (name, fixture) = fixtures()
        .into_iter()
        .find(|(name, _)| *name == "openai")
        .expect("openai fixture");
    run_case(name, move |context| async move {
        let upstream = TestUpstream::spawn(ScriptedReply::InterruptedSseBeforeFirstEvent).await;
        let router = RouterFixture::new(context, &fixture, &upstream.base_url).await;
        let persisted_sink = router.install_recording_persisted_sink();

        let response = router
            .send(&fixture, true, &fixture.cancellation.downstream_request)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let request_id = assert_downstream_request_identity(&response);
        let first = timeout(WAIT_TIMEOUT, response.into_body().into_data_stream().next())
            .await
            .expect("pre-first-chunk interruption deadline")
            .expect("stream should yield an interruption");
        assert!(
            first.is_err(),
            "downstream body should fail before its first non-empty chunk"
        );

        let log = router.wait_for_log(RequestStatus::Error).await;
        assert_eq!(log.request_id, request_id);
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_response_error")
        );
        assert_log_timing_order(&log);
        assert!(log.upstream_first_body_chunk_at.is_none());
        assert!(log.first_token_at.is_none());
        assert!(log.first_response_body_at.is_none());
        assert_single_persisted_terminal_fact(
            &persisted_sink,
            ExecutionStage::UpstreamResponse,
            ResponseVisibility::HeadersCommitted,
        )
        .await;
        assert_eq!(upstream.requests().await.len(), 1);
        upstream.shutdown().await;
    });
}
