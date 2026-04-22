use super::{create_proxy_router, flush_proxy_logs};
use crate::{
    config::CONFIG,
    database::{
        access_control::{AccessControlPolicy, ApiCreateAccessControlPolicyPayload},
        api_key::{ApiKey, CreateApiKeyPayload},
        model::{Model, ModelCapabilityFlags},
        model_route::{CreateModelRoutePayload, ModelRoute, ModelRouteCandidateInput},
        provider::{NewProvider, NewProviderApiKey, Provider, ProviderApiKey},
        request_attempt::RequestAttempt,
        request_log::{RequestLog, RequestLogQueryPayload, RequestLogRecord},
        request_patch::{CreateRequestPatchPayload, RequestPatchMutationOutcome, RequestPatchRule},
    },
    schema::enum_def::{
        Action, LlmApiType, ProviderApiKeyMode, ProviderType, RequestAttemptStatus,
        RequestPatchOperation, RequestPatchPlacement, RequestStatus, SchedulerAction,
    },
    service::{
        app_state::{AppState, create_app_state},
        storage::{get_storage, types::GetObjectOptions},
    },
    utils::{ID_GENERATOR, storage::RequestLogBundleV2},
};
use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, Request},
    http::{
        HeaderMap, HeaderName, HeaderValue, Method, StatusCode,
        header::{AUTHORIZATION, CONTENT_ENCODING, CONTENT_TYPE},
    },
    response::Response,
    routing::any,
    serve,
};
use flate2::{Compression, write::GzEncoder};
use futures::StreamExt;
use serde_json::{Value, json};
use std::{
    io::{ErrorKind, Write},
    net::SocketAddr,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    net::TcpListener,
    sync::{Mutex as AsyncMutex, oneshot},
};
use tower::util::ServiceExt;

/// Shared runtime for all integration tests so the static `LogManager` worker
/// task survives across tests (each `#[tokio::test]` would create and destroy
/// its own runtime, killing the worker spawned during the first test).
static RUNTIME: std::sync::LazyLock<tokio::runtime::Runtime> = std::sync::LazyLock::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("integration test runtime should build")
});

/// Serialize all integration tests to avoid SQLite "database is locked" errors.
static DB_LOCK: std::sync::LazyLock<AsyncMutex<()>> =
    std::sync::LazyLock::new(|| AsyncMutex::new(()));

/// Keep the temp directory alive for the full test process so SQLite can keep
/// using the database file after the initial setup.
static TEST_DB_DIR: std::sync::OnceLock<TempDir> = std::sync::OnceLock::new();

fn ensure_test_database() -> &'static Path {
    TEST_DB_DIR
        .get_or_init(|| {
            let temp_dir =
                tempfile::tempdir().expect("proxy integration temp dir should be created");
            let db_path = temp_dir.path().join("proxy-integration.sqlite");
            let db_url = db_path.to_string_lossy().into_owned();
            unsafe {
                std::env::set_var("DB_URL", &db_url);
            }
            temp_dir
        })
        .path()
}

#[derive(Clone, Debug)]
struct CapturedUpstreamRequest {
    method: Method,
    path: String,
    query: Option<String>,
    headers: HeaderMap,
    body: Bytes,
}

struct UpstreamReply {
    status: StatusCode,
    headers: Vec<(HeaderName, HeaderValue)>,
    body: Body,
}

impl UpstreamReply {
    fn json(status: StatusCode, value: Value) -> Self {
        Self {
            status,
            headers: vec![(CONTENT_TYPE, HeaderValue::from_static("application/json"))],
            body: Body::from(serde_json::to_vec(&value).expect("json body should serialize")),
        }
    }

    fn gzipped_json(status: StatusCode, value: Value) -> Self {
        let raw = serde_json::to_vec(&value).expect("json body should serialize");
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder
            .write_all(&raw)
            .expect("gzip encoder should accept body");
        let gzipped = encoder.finish().expect("gzip encoder should finish");

        Self {
            status,
            headers: vec![
                (CONTENT_TYPE, HeaderValue::from_static("application/json")),
                (CONTENT_ENCODING, HeaderValue::from_static("gzip")),
            ],
            body: Body::from(gzipped),
        }
    }

    fn sse(body: impl Into<Bytes>) -> Self {
        Self {
            status: StatusCode::OK,
            headers: vec![(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
            body: Body::from(body.into()),
        }
    }

    fn delayed_sse(chunks: Vec<(u64, &'static [u8])>) -> Self {
        let stream = async_stream::stream! {
            for (delay_ms, chunk) in chunks {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                yield Ok::<Bytes, std::io::Error>(Bytes::from_static(chunk));
            }
        };

        Self {
            status: StatusCode::OK,
            headers: vec![(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
            body: Body::from_stream(stream),
        }
    }

    fn erroring_sse(chunks: Vec<(u64, &'static [u8])>, message: &'static str) -> Self {
        let stream = async_stream::stream! {
            for (delay_ms, chunk) in chunks {
                tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                yield Ok::<Bytes, std::io::Error>(Bytes::from_static(chunk));
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
            yield Err::<Bytes, std::io::Error>(std::io::Error::other(message));
        };

        Self {
            status: StatusCode::OK,
            headers: vec![(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))],
            body: Body::from_stream(stream),
        }
    }

    fn with_header(mut self, name: &'static str, value: &'static str) -> Self {
        self.headers.push((
            HeaderName::from_static(name),
            HeaderValue::from_static(value),
        ));
        self
    }
}

struct TestUpstream {
    base_url: String,
    captured: Arc<AsyncMutex<Vec<CapturedUpstreamRequest>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl TestUpstream {
    async fn spawn<F>(responder: F) -> Result<Self, std::io::Error>
    where
        F: Fn(&CapturedUpstreamRequest) -> UpstreamReply + Send + Sync + 'static,
    {
        let captured = Arc::new(AsyncMutex::new(Vec::new()));
        let responder = Arc::new(responder);
        let router = axum::Router::new().fallback(any({
            let captured = Arc::clone(&captured);
            let responder = Arc::clone(&responder);
            move |request: Request<Body>| {
                let captured = Arc::clone(&captured);
                let responder = Arc::clone(&responder);
                async move {
                    let (parts, body) = request.into_parts();
                    let body = axum::body::to_bytes(body, usize::MAX)
                        .await
                        .expect("mock upstream request body should be readable");
                    let captured_request = CapturedUpstreamRequest {
                        method: parts.method,
                        path: parts.uri.path().to_string(),
                        query: parts.uri.query().map(str::to_string),
                        headers: parts.headers,
                        body,
                    };
                    captured.lock().await.push(captured_request.clone());
                    let reply = responder(&captured_request);

                    let mut response = Response::builder().status(reply.status);
                    for (name, value) in &reply.headers {
                        response = response.header(name, value);
                    }
                    response
                        .body(reply.body)
                        .expect("mock upstream response should build")
                }
            }
        }));

        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener
            .local_addr()
            .expect("mock upstream listener should have local addr");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        tokio::spawn(async move {
            serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await
                .expect("mock upstream server should run");
        });

        Ok(Self {
            base_url: format!("http://{}", addr),
            captured,
            shutdown_tx: Some(shutdown_tx),
        })
    }

    async fn captured_requests(&self) -> Vec<CapturedUpstreamRequest> {
        self.captured.lock().await.clone()
    }
}

async fn spawn_test_upstream_or_skip<F>(responder: F) -> Option<TestUpstream>
where
    F: Fn(&CapturedUpstreamRequest) -> UpstreamReply + Send + Sync + 'static,
{
    match TestUpstream::spawn(responder).await {
        Ok(upstream) => Some(upstream),
        Err(err) if err.kind() == ErrorKind::PermissionDenied => {
            eprintln!(
                "skipping proxy integration scenario: mock upstream listener bind denied: {}",
                err
            );
            None
        }
        Err(err) => panic!("mock upstream listener should bind: {err}"),
    }
}

impl Drop for TestUpstream {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
    }
}

struct TestFixture {
    app_state: Arc<AppState>,
    system_api_key: TestApiKey,
    provider: Provider,
    provider_api_key: ProviderApiKey,
    model: Model,
    access_control_policy_id: Option<i64>,
}

struct TestApiKey {
    id: i64,
    api_key: String,
}

fn create_provider_model(
    provider_type: ProviderType,
    endpoint: String,
    real_model_name: Option<String>,
) -> (Provider, ProviderApiKey, Model) {
    let nonce = ID_GENERATOR.generate_id();
    let now = chrono::Utc::now().timestamp_millis();
    let provider = Provider::create(&NewProvider {
        id: nonce,
        provider_key: format!("proxy-int-provider-{nonce}"),
        name: format!("Proxy Integration Provider {nonce}"),
        endpoint,
        use_proxy: false,
        is_enabled: true,
        created_at: now,
        updated_at: now,
        provider_type,
        provider_api_key_mode: ProviderApiKeyMode::Queue,
    })
    .expect("provider should be created");

    let provider_api_key = ProviderApiKey::insert(&NewProviderApiKey {
        id: ID_GENERATOR.generate_id(),
        provider_id: provider.id,
        api_key: format!("provider-secret-{nonce}"),
        description: Some("integration test provider key".to_string()),
        is_enabled: true,
        created_at: now,
        updated_at: now,
    })
    .expect("provider api key should be created");

    let model = Model::create(
        provider.id,
        &format!("proxy-int-model-{nonce}"),
        real_model_name.as_deref(),
        true,
        ModelCapabilityFlags::default(),
    )
    .expect("model should be created");

    (provider, provider_api_key, model)
}

impl TestFixture {
    async fn new(
        provider_type: ProviderType,
        endpoint: String,
        access_control_policy_id: Option<i64>,
        real_model_name: Option<String>,
    ) -> Self {
        let (provider, provider_api_key, model) =
            create_provider_model(provider_type, endpoint, real_model_name);
        let system_key_nonce = ID_GENERATOR.generate_id();
        let created_api_key = ApiKey::create(&CreateApiKeyPayload {
            name: format!("proxy-int-system-{system_key_nonce}"),
            description: Some("proxy integration test".to_string()),
            default_action: Some(if access_control_policy_id.is_some() {
                Action::Deny
            } else {
                Action::Allow
            }),
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
        .expect("api key should be created");
        let system_api_key = TestApiKey {
            id: created_api_key.detail.id,
            api_key: created_api_key.reveal.api_key,
        };

        let app_state = create_app_state().await;

        Self {
            app_state,
            system_api_key,
            provider,
            provider_api_key,
            model,
            access_control_policy_id,
        }
    }

    fn requested_model(&self) -> String {
        format!("{}/{}", self.provider.provider_key, self.model.model_name)
    }

    async fn send(&self, request: Request<Body>) -> Response<Body> {
        create_proxy_router()
            .with_state(Arc::clone(&self.app_state))
            .oneshot(request)
            .await
            .expect("proxy router should respond")
    }

    async fn list_logs(&self) -> Vec<RequestLogRecord> {
        flush_proxy_logs().await;
        RequestLog::list_full(RequestLogQueryPayload {
            provider_id: Some(self.provider.id),
            model_id: Some(self.model.id),
            page: Some(1),
            page_size: Some(20),
            ..Default::default()
        })
        .expect("request logs should be queryable")
        .list
    }

    async fn latest_log(&self) -> RequestLogRecord {
        const MAX_ATTEMPTS: usize = 20;
        const RETRY_DELAY_MS: u64 = 100;

        for attempt in 1..=MAX_ATTEMPTS {
            let mut logs = self.list_logs().await;
            logs.sort_by_key(|log| log.request_received_at);
            if let Some(log) = logs.pop() {
                return log;
            }

            if attempt < MAX_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
            }
        }

        panic!(
            "expected one request log for provider_id={} model_id={}",
            self.provider.id, self.model.id
        )
    }

    async fn latest_log_for_provider(&self, provider_id: i64) -> RequestLogRecord {
        const MAX_ATTEMPTS: usize = 20;
        const RETRY_DELAY_MS: u64 = 100;

        for attempt in 1..=MAX_ATTEMPTS {
            flush_proxy_logs().await;
            let mut logs = RequestLog::list_full(RequestLogQueryPayload {
                provider_id: Some(provider_id),
                page: Some(1),
                page_size: Some(100),
                ..Default::default()
            })
            .expect("request logs should be queryable")
            .list;
            logs.sort_by_key(|log| log.request_received_at);
            if let Some(log) = logs.pop() {
                return log;
            }

            if attempt < MAX_ATTEMPTS {
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS)).await;
            }
        }

        panic!("expected one request log for provider_id={provider_id}");
    }

    async fn attempts_for_log(&self, log_id: i64) -> Vec<RequestAttempt> {
        flush_proxy_logs().await;
        RequestAttempt::list_by_request_log_id(log_id)
            .expect("request attempts should be queryable")
    }

    async fn create_route(&self, route_name: &str) {
        ModelRoute::create(&CreateModelRoutePayload {
            route_name: route_name.to_string(),
            description: Some("integration test route".to_string()),
            is_enabled: Some(true),
            expose_in_models: Some(true),
            candidates: vec![ModelRouteCandidateInput {
                model_id: self.model.id,
                priority: 0,
                is_enabled: Some(true),
            }],
        })
        .expect("model route should be created");
        self.app_state.reload().await;
    }

    async fn cleanup(&self) {
        flush_proxy_logs().await;
        let _ = Model::delete(self.model.id);
        let _ = ProviderApiKey::delete(self.provider_api_key.id);
        let _ = Provider::delete(self.provider.id);
        let _ = ApiKey::delete(self.system_api_key.id);
        if let Some(policy_id) = self.access_control_policy_id {
            let _ = AccessControlPolicy::delete(policy_id);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn bundle_for_log(log: &RequestLogRecord) -> RequestLogBundleV2 {
    let key = log
        .bundle_storage_key
        .as_deref()
        .expect("request log should have bundle key");
    let storage = get_storage().await;
    let bytes = storage
        .get_object(
            key,
            Some(GetObjectOptions {
                content_encoding: Some("gzip"),
            }),
        )
        .await
        .expect("request log bundle should be readable");

    rmp_serde::from_slice(&bytes).expect("request log bundle should decode")
}

fn bundle_blob_json(bundle: &RequestLogBundleV2, blob_id: i32) -> Value {
    let blob = bundle
        .blob_pool
        .iter()
        .find(|blob| blob.blob_id == blob_id)
        .expect("bundle blob should exist");

    serde_json::from_slice(&blob.body).expect("bundle blob should be json")
}

fn bundle_attempt_request_json(bundle: &RequestLogBundleV2, attempt_index: i32) -> Value {
    let section = bundle
        .attempt_sections
        .iter()
        .find(|section| section.attempt_index == attempt_index)
        .expect("attempt bundle section");
    let mut value = bundle_blob_json(
        bundle,
        section
            .llm_request_blob_id
            .expect("attempt request body blob"),
    );

    if let Some(patch_id) = section.llm_request_patch_id {
        let patch = bundle
            .patch_pool
            .iter()
            .find(|patch| patch.patch_id == patch_id)
            .expect("request body patch should exist");
        let patch: json_patch::Patch =
            serde_json::from_slice(&patch.patch_body).expect("request body patch should decode");
        json_patch::patch(&mut value, &patch).expect("request body patch should apply");
    }

    value
}

fn bundle_attempt_response_json(bundle: &RequestLogBundleV2, attempt_index: i32) -> Value {
    let section = bundle
        .attempt_sections
        .iter()
        .find(|section| section.attempt_index == attempt_index)
        .expect("attempt bundle section");
    bundle_blob_json(
        bundle,
        section
            .llm_response_blob_id
            .expect("attempt response body blob"),
    )
}

fn build_json_request(uri: &str, headers: &[(&str, String)], body: Value) -> Request<Body> {
    let mut builder = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(CONTENT_TYPE, "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    let mut request = builder
        .body(Body::from(
            serde_json::to_vec(&body).expect("request body should serialize"),
        ))
        .expect("request should build");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 3000))));
    request
}

async fn response_body_bytes(response: Response<Body>) -> Bytes {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable")
}

fn deny_all_policy() -> i64 {
    AccessControlPolicy::create(ApiCreateAccessControlPolicyPayload {
        name: format!("deny-all-{}", ID_GENERATOR.generate_id()),
        description: Some("deny all integration test policy".to_string()),
        default_action: Action::Deny,
        rules: None,
    })
    .expect("access control policy should be created")
    .id
}

#[test]
fn openai_generation_handles_gzip_response_and_persists_log() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(request.path, "/v1/chat/completions");
            assert_eq!(request.method, Method::POST);
            assert!(
                request
                    .headers
                    .get(AUTHORIZATION)
                    .expect("provider auth header")
                    .to_str()
                    .expect("auth header should be utf8")
                    .starts_with("Bearer provider-secret-")
            );
            let body: Value = serde_json::from_slice(&request.body)
                .expect("upstream request body should be json");
            assert_eq!(body["model"], "upstream-openai-model");

            UpstreamReply::gzipped_json(
                StatusCode::OK,
                json!({
                    "id": "chatcmpl-openai",
                    "object": "chat.completion",
                    "model": "upstream-openai-model",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "gzip ok"},
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 7,
                        "completion_tokens": 3,
                        "total_tokens": 10
                    }
                }),
            )
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            None,
            Some("upstream-openai-model".to_string()),
        )
        .await;

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": fixture.requested_model(),
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(CONTENT_ENCODING).is_none());
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["choices"][0]["message"]["content"], "gzip ok");
        assert_eq!(upstream.captured_requests().await.len(), 1);

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Success);
        assert_eq!(log.attempt_count, 1);
        assert_eq!(log.bundle_version, Some(2));
        assert!(log.bundle_storage_key.is_some());

        fixture.cleanup().await;
    });
}

#[test]
fn gemini_generation_routes_to_native_endpoint_and_logs_success() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(
                request.path,
                "/v1beta/models/upstream-gemini-model:generateContent"
            );
            assert_eq!(request.query.as_deref(), Some("foo=bar"));
            assert!(
                request
                    .headers
                    .get("x-goog-api-key")
                    .expect("gemini api key header")
                    .to_str()
                    .expect("gemini api key should be utf8")
                    .starts_with("provider-secret-")
            );
            let body: Value = serde_json::from_slice(&request.body)
                .expect("upstream request body should be json");
            assert_eq!(body["contents"][0]["parts"][0]["text"], "hi gemini");

            UpstreamReply::json(
                StatusCode::OK,
                json!({
                    "candidates": [{
                        "content": {
                            "role": "model",
                            "parts": [{"text": "gemini ok"}]
                        },
                        "finishReason": "STOP",
                        "index": 0
                    }],
                    "usageMetadata": {
                        "promptTokenCount": 4,
                        "candidatesTokenCount": 2,
                        "totalTokenCount": 6
                    }
                }),
            )
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Gemini,
            format!("{}/v1beta/models", upstream.base_url),
            None,
            Some("upstream-gemini-model".to_string()),
        )
        .await;

        let request = build_json_request(
            &format!(
                "/gemini/v1beta/models/{}:generateContent?foo=bar&key={}",
                fixture.requested_model(),
                fixture.system_api_key.api_key
            ),
            &[],
            json!({
                "contents": [{
                    "parts": [{"text": "hi gemini"}]
                }]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(
            body["candidates"][0]["content"]["parts"][0]["text"],
            "gemini ok"
        );

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Success);
        assert_eq!(log.attempt_count, 1);
        assert_eq!(log.total_input_tokens, Some(4));
        assert_eq!(log.total_output_tokens, Some(2));
        assert_eq!(log.total_tokens, Some(6));
        assert_eq!(
            log.final_model_name_snapshot.as_deref(),
            Some(fixture.model.model_name.as_str())
        );
        assert_eq!(
            log.final_real_model_name_snapshot.as_deref(),
            Some("upstream-gemini-model")
        );

        fixture.cleanup().await;
    });
}

#[test]
fn utility_requests_share_proxy_lifecycle_and_write_logs() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(request.path, "/v1/embeddings");
            assert_eq!(request.query, None);
            assert!(
                request
                    .headers
                    .get(AUTHORIZATION)
                    .expect("provider auth header")
                    .to_str()
                    .expect("auth header should be utf8")
                    .starts_with("Bearer provider-secret-")
            );
            let body: Value = serde_json::from_slice(&request.body)
                .expect("upstream request body should be json");
            assert_eq!(body["model"], "upstream-embedding-model");
            assert_eq!(body["input"], "embed me");

            UpstreamReply::json(
                StatusCode::OK,
                json!({
                    "object": "list",
                    "data": [{
                        "object": "embedding",
                        "index": 0,
                        "embedding": [0.1, 0.2, 0.3]
                    }],
                    "model": "upstream-embedding-model",
                    "usage": {
                        "prompt_tokens": 4,
                        "total_tokens": 4
                    }
                }),
            )
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            None,
            Some("upstream-embedding-model".to_string()),
        )
        .await;

        let request = build_json_request(
            "/openai/v1/embeddings",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": fixture.requested_model(),
                "input": "embed me"
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["data"][0]["embedding"][1], json!(0.2));

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Success);
        assert_eq!(log.attempt_count, 1);
        assert_eq!(log.total_input_tokens, Some(4));
        assert_eq!(log.total_output_tokens, Some(0));
        assert_eq!(log.total_tokens, Some(4));
        assert!(log.bundle_storage_key.is_some());

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 1);
        let attempt = &attempts[0];
        assert_eq!(attempt.attempt_status, RequestAttemptStatus::Success);
        assert_eq!(attempt.scheduler_action, SchedulerAction::ReturnSuccess);
        assert_eq!(attempt.llm_api_type, Some(LlmApiType::Openai));
        assert!(
            attempt
                .request_uri
                .as_deref()
                .expect("attempt request uri")
                .ends_with("/v1/embeddings")
        );
        let logged_headers: Value = serde_json::from_str(
            attempt
                .request_headers_json
                .as_deref()
                .expect("attempt request headers"),
        )
        .expect("request headers json");
        assert!(logged_headers.get("authorization").is_none());
        assert_eq!(logged_headers["content-type"], "application/json");
        assert!(attempt.llm_request_blob_id.is_some());
        assert!(attempt.llm_response_blob_id.is_some());
        assert_eq!(
            attempt.llm_response_capture_state.as_deref(),
            Some("COMPLETE")
        );

        let bundle = bundle_for_log(&log).await;
        let request_json = bundle_attempt_request_json(&bundle, 1);
        let response_json = bundle_attempt_response_json(&bundle, 1);
        assert_eq!(request_json["model"], "upstream-embedding-model");
        assert_eq!(request_json["input"], "embed me");
        assert_eq!(response_json["usage"]["total_tokens"], 4);

        fixture.cleanup().await;
    });
}

#[test]
fn gemini_utility_requests_capture_attempt_materials_and_usage() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(
                request.path,
                "/v1beta/models/upstream-gemini-model:countTokens"
            );
            assert_eq!(request.query.as_deref(), Some("foo=bar"));
            assert!(
                request
                    .headers
                    .get("x-goog-api-key")
                    .expect("gemini api key header")
                    .to_str()
                    .expect("gemini api key should be utf8")
                    .starts_with("provider-secret-")
            );
            let body: Value = serde_json::from_slice(&request.body)
                .expect("upstream request body should be json");
            assert_eq!(body["contents"][0]["parts"][0]["text"], "count this");

            UpstreamReply::json(
                StatusCode::OK,
                json!({
                    "totalTokens": 9
                }),
            )
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Gemini,
            format!("{}/v1beta/models", upstream.base_url),
            None,
            Some("upstream-gemini-model".to_string()),
        )
        .await;

        let request = build_json_request(
            &format!(
                "/gemini/v1beta/models/{}:countTokens?foo=bar&key={}",
                fixture.requested_model(),
                fixture.system_api_key.api_key
            ),
            &[],
            json!({
                "contents": [{
                    "parts": [{"text": "count this"}]
                }]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["totalTokens"], 9);

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Success);
        assert_eq!(log.attempt_count, 1);
        assert_eq!(log.total_input_tokens, Some(9));
        assert_eq!(log.total_output_tokens, Some(0));
        assert_eq!(log.total_tokens, Some(9));
        assert!(log.bundle_storage_key.is_some());

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 1);
        let attempt = &attempts[0];
        assert_eq!(attempt.attempt_status, RequestAttemptStatus::Success);
        assert_eq!(attempt.scheduler_action, SchedulerAction::ReturnSuccess);
        assert_eq!(attempt.llm_api_type, Some(LlmApiType::Gemini));
        let request_uri = attempt.request_uri.as_deref().expect("attempt request uri");
        assert!(request_uri.ends_with("/v1beta/models/upstream-gemini-model:countTokens?foo=bar"));
        let logged_headers: Value = serde_json::from_str(
            attempt
                .request_headers_json
                .as_deref()
                .expect("attempt request headers"),
        )
        .expect("request headers json");
        assert!(logged_headers.get("x-goog-api-key").is_none());
        assert_eq!(logged_headers["content-type"], "application/json");
        assert!(attempt.llm_request_blob_id.is_some());
        assert!(attempt.llm_response_blob_id.is_some());
        assert_eq!(
            attempt.llm_response_capture_state.as_deref(),
            Some("COMPLETE")
        );

        let bundle = bundle_for_log(&log).await;
        let request_json = bundle_attempt_request_json(&bundle, 1);
        let response_json = bundle_attempt_response_json(&bundle, 1);
        assert_eq!(
            request_json["contents"][0]["parts"][0]["text"],
            "count this"
        );
        assert_eq!(response_json["totalTokens"], 9);

        fixture.cleanup().await;
    });
}

#[test]
fn sse_streaming_requests_are_forwarded_and_logged() {
    RUNTIME.block_on(async {
    let _ = ensure_test_database();
    let _guard = DB_LOCK.lock().await;
    let Some(upstream) = spawn_test_upstream_or_skip(|request| {
        assert_eq!(request.path, "/v1/chat/completions");
        let body: Value =
            serde_json::from_slice(&request.body).expect("upstream request body should be json");
        assert_eq!(body["stream"], true);

        UpstreamReply::sse(Bytes::from_static(
            br#"data: {"id":"chatcmpl-stream","object":"chat.completion.chunk","created":1,"model":"upstream-stream-model","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-stream","object":"chat.completion.chunk","created":2,"model":"upstream-stream-model","choices":[{"index":0,"delta":{"content":"stream ok"},"finish_reason":null}],"usage":null}

data: {"id":"chatcmpl-stream","object":"chat.completion.chunk","created":3,"model":"upstream-stream-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":3,"completion_tokens":2,"total_tokens":5}}

data: [DONE]

"#,
        ))
        .with_header("x-request-id", "stream-req-1")
        .with_header("set-cookie", "session=secret")
    })
    .await
    else {
        return;
    };
    let fixture = TestFixture::new(
        ProviderType::Openai,
        format!("{}/v1", upstream.base_url),
        None,
        Some("upstream-stream-model".to_string()),
    )
    .await;

    let request = build_json_request(
        "/openai/v1/chat/completions",
        &[(
            "authorization",
            format!("Bearer {}", fixture.system_api_key.api_key),
        )],
        json!({
            "model": fixture.requested_model(),
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }),
    );

    let response = fixture.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(CONTENT_TYPE)
            .expect("sse content type"),
        "text/event-stream"
    );
    let body_text = String::from_utf8(response_body_bytes(response).await.to_vec())
        .expect("sse response should be utf8");
    assert!(body_text.contains("stream ok"));
    assert!(body_text.contains("data: [DONE]"));

    let log = fixture.latest_log().await;
    assert_eq!(log.overall_status, RequestStatus::Success);
    assert_eq!(log.attempt_count, 1);
    assert!(log.response_started_to_client_at.is_some());
    assert_eq!(log.total_tokens, Some(5));
    assert!(log.bundle_storage_key.is_some());

    let attempts = fixture.attempts_for_log(log.id).await;
    assert_eq!(attempts.len(), 1);
    let response_headers: Value = serde_json::from_str(
        attempts[0]
            .response_headers_json
            .as_deref()
            .expect("streaming attempt response headers"),
    )
    .expect("response headers json");
    assert_eq!(response_headers["content-type"], "text/event-stream");
    assert_eq!(response_headers["x-request-id"], "stream-req-1");
    assert!(response_headers.get("set-cookie").is_none());
    assert!(response_headers.get("content-length").is_none());
    assert!(response_headers.get("transfer-encoding").is_none());

    fixture.cleanup().await;
    });
}

#[test]
fn streaming_raw_chunk_without_visible_output_keeps_response_started_null_on_error() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(request.path, "/v1/chat/completions");
            UpstreamReply::erroring_sse(
                vec![(0, br#"data: {"id":"partial-chunk"}"#)],
                "upstream stream broke before a visible SSE event",
            )
            .with_header("x-request-id", "stream-error-1")
            .with_header("set-cookie", "session=secret")
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            None,
            Some("upstream-stream-model".to_string()),
        )
        .await;

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": fixture.requested_model(),
                "stream": true,
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(CONTENT_TYPE)
                .expect("sse content type"),
            "text/event-stream"
        );
        let mut stream = response.into_body().into_data_stream();
        let first_item = stream
            .next()
            .await
            .expect("expected streaming body item after upstream failure");
        assert!(first_item.is_err());
        assert!(stream.next().await.is_none());

        tokio::time::sleep(Duration::from_millis(100)).await;
        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Error);
        assert_eq!(log.attempt_count, 1);
        assert_eq!(log.response_started_to_client_at, None);

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 1);
        assert!(!attempts[0].response_started_to_client);
        let response_headers: Value = serde_json::from_str(
            attempts[0]
                .response_headers_json
                .as_deref()
                .expect("streaming error attempt response headers"),
        )
        .expect("response headers json");
        assert_eq!(response_headers["content-type"], "text/event-stream");
        assert_eq!(response_headers["x-request-id"], "stream-error-1");
        assert!(response_headers.get("set-cookie").is_none());
        assert!(response_headers.get("content-length").is_none());
        assert!(response_headers.get("transfer-encoding").is_none());
        assert_eq!(upstream.captured_requests().await.len(), 1);

        fixture.cleanup().await;
    });
}

#[test]
fn streaming_request_without_upstream_response_keeps_attempt_response_headers_null() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let fixture = TestFixture::new(
            ProviderType::Openai,
            "http://127.0.0.1:9/v1".to_string(),
            None,
            Some("upstream-stream-model".to_string()),
        )
        .await;

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": fixture.requested_model(),
                "stream": true,
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Error);
        assert!(log.attempt_count >= 1);

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), log.attempt_count as usize);
        assert!(
            attempts
                .iter()
                .all(|attempt| attempt.response_headers_json.is_none())
        );

        fixture.cleanup().await;
    });
}

#[test]
fn streaming_visible_chunk_before_upstream_error_marks_attempt_visible_without_retry() {
    RUNTIME.block_on(async {
    let _ = ensure_test_database();
    let _guard = DB_LOCK.lock().await;
    let Some(upstream) = spawn_test_upstream_or_skip(|request| {
        assert_eq!(request.path, "/v1/chat/completions");
        UpstreamReply::erroring_sse(
            vec![(
                0,
                br#"data: {"id":"chatcmpl-stream","object":"chat.completion.chunk","created":1,"model":"upstream-stream-model","choices":[{"index":0,"delta":{"content":"hel"},"finish_reason":null}],"usage":null}

"#,
            )],
            "upstream stream broke after a visible SSE event",
        )
    })
    .await
    else {
        return;
    };
    let fixture = TestFixture::new(
        ProviderType::Openai,
        format!("{}/v1", upstream.base_url),
        None,
        Some("upstream-stream-model".to_string()),
    )
    .await;

    let request = build_json_request(
        "/openai/v1/chat/completions",
        &[(
            "authorization",
            format!("Bearer {}", fixture.system_api_key.api_key),
        )],
        json!({
            "model": fixture.requested_model(),
            "stream": true,
            "messages": [{"role": "user", "content": "hi"}]
        }),
    );

    let response = fixture.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let first_chunk = stream
        .next()
        .await
        .expect("expected first streamed chunk")
        .expect("first streamed chunk should succeed");
    assert!(String::from_utf8_lossy(&first_chunk).contains("hel"));
    let second_item = stream
        .next()
        .await
        .expect("expected upstream failure after visible chunk");
    assert!(second_item.is_err());
    assert!(stream.next().await.is_none());

    tokio::time::sleep(Duration::from_millis(100)).await;
    let log = fixture.latest_log().await;
    assert_eq!(log.overall_status, RequestStatus::Error);
    assert_eq!(log.attempt_count, 1);
    assert_eq!(log.retry_count, 0);
    assert_eq!(log.fallback_count, 0);
    assert_eq!(log.final_error_code.as_deref(), Some("upstream_error"));
    assert!(log.response_started_to_client_at.is_some());

    let attempts = fixture.attempts_for_log(log.id).await;
    assert_eq!(attempts.len(), 1);
    assert_eq!(attempts[0].attempt_status, RequestAttemptStatus::Error);
    assert_eq!(attempts[0].scheduler_action, SchedulerAction::FailFast);
    assert_eq!(attempts[0].error_code.as_deref(), Some("upstream_error"));
    assert!(attempts[0].response_started_to_client);
    assert_eq!(upstream.captured_requests().await.len(), 1);

    fixture.cleanup().await;
    });
}

#[test]
fn streaming_client_disconnect_marks_log_cancelled() {
    RUNTIME.block_on(async {
    let _ = ensure_test_database();
    let _guard = DB_LOCK.lock().await;
    let Some(upstream) = spawn_test_upstream_or_skip(|request| {
        assert_eq!(request.path, "/v1/chat/completions");
        UpstreamReply::delayed_sse(vec![
            (0, br#"data: {"id":"chunk-1","object":"chat.completion.chunk","created":1,"model":"upstream-stream-model","choices":[{"index":0,"delta":{"content":"hel"},"finish_reason":null}],"usage":null}

"#),
            (150, br#"data: {"id":"chunk-2","object":"chat.completion.chunk","created":2,"model":"upstream-stream-model","choices":[{"index":0,"delta":{"content":"lo"},"finish_reason":null}],"usage":null}

"#),
            (150, b"data: [DONE]\n\n"),
        ])
    })
    .await
    else {
        return;
    };
    let fixture = TestFixture::new(
        ProviderType::Openai,
        format!("{}/v1", upstream.base_url),
        None,
        Some("upstream-stream-model".to_string()),
    )
    .await;

    let request = build_json_request(
        "/openai/v1/chat/completions",
        &[(
            "authorization",
            format!("Bearer {}", fixture.system_api_key.api_key),
        )],
        json!({
            "model": fixture.requested_model(),
            "stream": true,
            "messages": [{"role": "user", "content": "disconnect me"}]
        }),
    );

    let response = fixture.send(request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.into_body().into_data_stream();
    let first_chunk = stream
        .next()
        .await
        .expect("expected first streamed chunk")
        .expect("first streamed chunk should succeed");
    assert!(String::from_utf8_lossy(&first_chunk).contains("hel"));
    drop(stream);

    tokio::time::sleep(Duration::from_millis(300)).await;
    let log = fixture.latest_log().await;
    assert_eq!(log.overall_status, RequestStatus::Cancelled);
    assert!(log.bundle_storage_key.is_some());

    fixture.cleanup().await;
    });
}

#[test]
fn acl_denials_short_circuit_before_upstream_and_persist_route_trace_logs() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|_| {
            panic!("upstream should not be called when ACL denies the request");
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            Some(deny_all_policy()),
            Some("upstream-denied-model".to_string()),
        )
        .await;
        let route_name = format!("proxy-int-route-{}", ID_GENERATOR.generate_id());
        fixture.create_route(&route_name).await;

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": route_name.clone(),
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["code"], "permission_error");
        assert_eq!(upstream.captured_requests().await.len(), 0);
        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Error);
        assert_eq!(
            log.requested_model_name.as_deref(),
            Some(route_name.as_str())
        );
        assert_eq!(log.resolved_name_scope.as_deref(), Some("global_route"));
        assert_eq!(
            log.resolved_route_name.as_deref(),
            Some(route_name.as_str())
        );
        assert_eq!(
            log.final_model_name_snapshot.as_deref(),
            Some(fixture.model.model_name.as_str())
        );
        assert_eq!(log.attempt_count, 1);
        assert_eq!(log.retry_count, 0);
        assert_eq!(log.fallback_count, 0);
        assert_eq!(log.final_error_code.as_deref(), Some("permission_error"));

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].attempt_status, RequestAttemptStatus::Error);
        assert_eq!(attempts[0].scheduler_action, SchedulerAction::FailFast);
        assert_eq!(attempts[0].error_code.as_deref(), Some("permission_error"));
        assert_eq!(attempts[0].http_status, None);
        assert_eq!(attempts[0].request_uri, None);

        fixture.cleanup().await;
    });
}

#[test]
fn provider_governance_open_candidate_is_skipped_and_falls_back_without_upstream_call() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        if !CONFIG.provider_governance.is_enabled() {
            eprintln!("skipping provider governance integration scenario: governance disabled");
            return;
        }

        let Some(skipped_upstream) = spawn_test_upstream_or_skip(|_| {
            UpstreamReply::json(
                StatusCode::INTERNAL_SERVER_ERROR,
                json!({"error": {"message": "skipped provider should not be called"}}),
            )
        })
        .await
        else {
            return;
        };
        let Some(fallback_upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(request.path, "/v1/chat/completions");
            UpstreamReply::json(
                StatusCode::OK,
                json!({
                    "id": "chatcmpl-fallback",
                    "object": "chat.completion",
                    "model": "fallback-openai-model",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "fallback ok"},
                        "finish_reason": "stop"
                    }],
                    "usage": {
                        "prompt_tokens": 3,
                        "completion_tokens": 2,
                        "total_tokens": 5
                    }
                }),
            )
        })
        .await
        else {
            return;
        };

        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", skipped_upstream.base_url),
            None,
            Some("skipped-openai-model".to_string()),
        )
        .await;
        let (fallback_provider, fallback_key, fallback_model) = create_provider_model(
            ProviderType::Openai,
            format!("{}/v1", fallback_upstream.base_url),
            Some("fallback-openai-model".to_string()),
        );
        let route_name = format!("proxy-int-governance-route-{}", ID_GENERATOR.generate_id());
        let route = ModelRoute::create(&CreateModelRoutePayload {
            route_name: route_name.clone(),
            description: Some("provider governance fallback integration test".to_string()),
            is_enabled: Some(true),
            expose_in_models: Some(true),
            candidates: vec![
                ModelRouteCandidateInput {
                    model_id: fixture.model.id,
                    priority: 0,
                    is_enabled: Some(true),
                },
                ModelRouteCandidateInput {
                    model_id: fallback_model.id,
                    priority: 1,
                    is_enabled: Some(true),
                },
            ],
        })
        .expect("model route should be created");
        fixture.app_state.reload().await;

        for _ in 0..CONFIG
            .provider_governance
            .consecutive_failure_threshold
            .max(1)
        {
            fixture
                .app_state
                .record_provider_failure(fixture.provider.id, "forced test failure".to_string())
                .await;
        }

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": route_name.clone(),
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["choices"][0]["message"]["content"], "fallback ok");
        assert_eq!(skipped_upstream.captured_requests().await.len(), 0);
        assert_eq!(fallback_upstream.captured_requests().await.len(), 1);

        let log = fixture.latest_log_for_provider(fallback_provider.id).await;
        assert_eq!(log.overall_status, RequestStatus::Success);
        assert_eq!(log.attempt_count, 2);
        assert_eq!(log.fallback_count, 1);
        assert_eq!(log.final_provider_id, Some(fallback_provider.id));
        assert_eq!(log.final_model_id, Some(fallback_model.id));

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].attempt_status, RequestAttemptStatus::Skipped);
        assert_eq!(
            attempts[0].error_code.as_deref(),
            Some("provider_open_skipped")
        );
        assert_eq!(
            attempts[0].scheduler_action,
            SchedulerAction::FallbackNextCandidate
        );
        assert_eq!(attempts[0].http_status, None);
        assert!(attempts[0].request_uri.is_none());
        assert_eq!(attempts[0].llm_response_blob_id, None);
        assert_eq!(attempts[1].attempt_status, RequestAttemptStatus::Success);
        assert_eq!(attempts[1].scheduler_action, SchedulerAction::ReturnSuccess);

        let _ = ModelRoute::delete(route.route.id);
        let _ = Model::delete(fallback_model.id);
        let _ = ProviderApiKey::delete(fallback_key.id);
        let _ = Provider::delete(fallback_provider.id);
        fixture.cleanup().await;
    });
}

#[test]
fn request_patch_conflict_uses_stable_error_code_in_attempt_and_request_log() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|_| {
            UpstreamReply::json(
                StatusCode::OK,
                json!({
                    "id": "chatcmpl-conflict",
                    "object": "chat.completion",
                    "choices": [{
                        "index": 0,
                        "message": {"role": "assistant", "content": "should not call upstream"},
                        "finish_reason": "stop"
                    }]
                }),
            )
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            None,
            Some("upstream-conflict-model".to_string()),
        )
        .await;

        let provider_rule = RequestPatchRule::create_for_provider(
            fixture.provider.id,
            &CreateRequestPatchPayload {
                placement: RequestPatchPlacement::Body,
                target: "/generation_config".to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some(Some(json!({ "temperature": 0.8 }))),
                description: Some("integration provider body patch".to_string()),
                is_enabled: Some(true),
                confirm_dangerous_target: None,
            },
        )
        .expect("provider request patch should be created");
        assert!(matches!(
            provider_rule,
            RequestPatchMutationOutcome::Saved { .. }
        ));
        let model_rule = RequestPatchRule::create_for_model(
            fixture.model.id,
            &CreateRequestPatchPayload {
                placement: RequestPatchPlacement::Body,
                target: "/generation_config/temperature".to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some(Some(json!(0.2))),
                description: Some("integration model body patch".to_string()),
                is_enabled: Some(true),
                confirm_dangerous_target: None,
            },
        )
        .expect("model request patch should be created");
        assert!(matches!(
            model_rule,
            RequestPatchMutationOutcome::Saved { .. }
        ));
        fixture.app_state.reload().await;

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": fixture.requested_model(),
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["code"], "request_patch_conflict_error");
        assert!(
            body["message"]
                .as_str()
                .expect("error message")
                .contains("Request patch conflicts prevent model")
        );
        assert_eq!(upstream.captured_requests().await.len(), 0);

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Error);
        assert_eq!(log.attempt_count, 1);
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("request_patch_conflict_error")
        );

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].attempt_status, RequestAttemptStatus::Error);
        assert_eq!(
            attempts[0].error_code.as_deref(),
            Some("request_patch_conflict_error")
        );
        assert_eq!(attempts[0].scheduler_action, SchedulerAction::FailFast);
        assert_eq!(attempts[0].http_status, None);

        fixture.cleanup().await;
    });
}

#[test]
fn upstream_errors_are_mapped_and_persisted_in_request_logs() {
    RUNTIME.block_on(async {
        let _ = ensure_test_database();
        let _guard = DB_LOCK.lock().await;
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            assert_eq!(request.path, "/v1/chat/completions");
            UpstreamReply::json(
                StatusCode::TOO_MANY_REQUESTS,
                json!({
                    "error": {
                        "message": "quota exceeded"
                    }
                }),
            )
        })
        .await
        else {
            return;
        };
        let fixture = TestFixture::new(
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            None,
            Some("upstream-rate-limit-model".to_string()),
        )
        .await;

        let request = build_json_request(
            "/openai/v1/chat/completions",
            &[(
                "authorization",
                format!("Bearer {}", fixture.system_api_key.api_key),
            )],
            json!({
                "model": fixture.requested_model(),
                "messages": [{"role": "user", "content": "hi"}]
            }),
        );

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        let body: Value =
            serde_json::from_slice(&response_body_bytes(response).await).expect("proxy body json");
        assert_eq!(body["code"], "upstream_rate_limit_error");
        assert!(
            body["message"]
                .as_str()
                .expect("error message")
                .contains("quota exceeded")
        );

        let log = fixture.latest_log().await;
        assert_eq!(log.overall_status, RequestStatus::Error);
        assert_eq!(log.attempt_count, 2);
        assert_eq!(log.retry_count, 1);
        assert_eq!(log.fallback_count, 0);
        assert_eq!(
            log.final_error_code.as_deref(),
            Some("upstream_rate_limit_error")
        );
        assert!(log.bundle_storage_key.is_some());

        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].attempt_status, RequestAttemptStatus::Error);
        assert_eq!(
            attempts[0].scheduler_action,
            SchedulerAction::RetrySameCandidate
        );
        assert_eq!(
            attempts[0].error_code.as_deref(),
            Some("upstream_rate_limit_error")
        );
        assert_eq!(attempts[0].http_status, Some(429));
        assert!(attempts[0].backoff_ms.is_some());
        assert_eq!(attempts[1].attempt_status, RequestAttemptStatus::Error);
        assert_eq!(attempts[1].scheduler_action, SchedulerAction::FailFast);
        assert_eq!(
            attempts[1].error_code.as_deref(),
            Some("upstream_rate_limit_error")
        );
        assert_eq!(attempts[1].http_status, Some(429));
        assert_eq!(upstream.captured_requests().await.len(), 2);

        fixture.cleanup().await;
    });
}
