use std::{collections::BTreeMap, net::SocketAddr, sync::Arc};

use axum::{
    body::{Body, to_bytes},
    extract::{ConnectInfo, Request},
    http::{HeaderValue, Method, StatusCode, header},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tower::ServiceExt;

use super::{
    create_proxy_router,
    direct_execution_regression::{
        DirectExecutionFixture, DownstreamAuth, RouterFixture, TestUpstream, fixtures,
        render_value, run_case,
    },
    request_context::X_REQUEST_ID,
};
use crate::{
    config::ClientIdentityConfig,
    controller::handle_404,
    database::{
        api_key::UpdateApiKeyMetadataPayload,
        request_log::{RequestLog, RequestLogQueryPayload},
    },
    ingress::client_identity::ClientIdentityResolver,
    schema::enum_def::{DownstreamProtocol, RequestStatus},
    service::{
        app_state::{AppState, create_state_router},
        runtime::{
            ApiKeyGovernanceService, FixedApiKeyGovernanceClock,
            api_key_governance::MemoryApiKeyRuntimeStore,
        },
    },
};

const FIXED_RPM_BOUNDARY_MS: i64 = 1_785_760_499_999;
const TEST_ORIGIN: &str = "https://error-contract.example";

const CONTRACT_FIXTURE_SOURCES: [&str; 4] = [
    include_str!("testdata/error_contracts/openai.json"),
    include_str!("testdata/error_contracts/responses.json"),
    include_str!("testdata/error_contracts/anthropic.json"),
    include_str!("testdata/error_contracts/gemini.json"),
];

#[derive(Clone, Debug, Deserialize)]
struct GoldenScenario {
    status: u16,
    body: Value,
    cors: bool,
    #[serde(default)]
    www_authenticate: Option<String>,
    #[serde(default)]
    retry_after: Option<String>,
    #[serde(default)]
    allow: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct ErrorContractFixture {
    protocol: DownstreamProtocol,
    authentication: GoldenScenario,
    rate_limit: GoldenScenario,
    capability: GoldenScenario,
    provider: GoldenScenario,
    internal: GoldenScenario,
    query_rejection: GoldenScenario,
    #[serde(default)]
    path_rejection: Option<GoldenScenario>,
    route_not_found: GoldenScenario,
    method_not_allowed: GoldenScenario,
    #[serde(default)]
    utility_rejections: BTreeMap<String, GoldenScenario>,
}

fn contract_fixtures() -> Vec<ErrorContractFixture> {
    let fixtures = CONTRACT_FIXTURE_SOURCES
        .iter()
        .map(|source| {
            serde_json::from_str::<ErrorContractFixture>(source)
                .expect("error contract fixture should be valid JSON")
        })
        .collect::<Vec<_>>();
    assert_eq!(fixtures.len(), DownstreamProtocol::ALL.len());
    for (fixture, protocol) in fixtures.iter().zip(DownstreamProtocol::ALL) {
        assert_eq!(fixture.protocol, protocol, "contract fixture order drifted");
    }
    fixtures
}

fn client_identity_resolver() -> Arc<ClientIdentityResolver> {
    Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default()))
}

fn render_dynamic(value: &Value, request_id: &str, api_key_name: Option<&str>) -> Value {
    match value {
        Value::String(value) => {
            let value = value.replace("$REQUEST_ID", request_id);
            let value = match api_key_name {
                Some(api_key_name) => value.replace("$API_KEY_NAME", api_key_name),
                None => value,
            };
            Value::String(value)
        }
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(|value| render_dynamic(value, request_id, api_key_name))
                .collect(),
        ),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), render_dynamic(value, request_id, api_key_name)))
                .collect(),
        ),
        _ => value.clone(),
    }
}

async fn assert_golden_response(
    response: axum::response::Response,
    fixture: &ErrorContractFixture,
    scenario: &GoldenScenario,
    api_key_name: Option<&str>,
) -> (String, Value) {
    assert_eq!(response.status().as_u16(), scenario.status);
    assert_eq!(
        response.headers().get(header::CONTENT_TYPE).unwrap(),
        "application/json"
    );
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert_eq!(
        response.headers().get("x-content-type-options").unwrap(),
        "nosniff"
    );
    let request_id = response
        .headers()
        .get(&X_REQUEST_ID)
        .and_then(|value| value.to_str().ok())
        .expect("protocol response should contain X-Request-ID")
        .to_string();
    let parsed = uuid::Uuid::parse_str(&request_id).expect("request id should be a UUID");
    assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
    assert_eq!(parsed.get_variant(), uuid::Variant::RFC4122);
    assert_eq!(
        response
            .headers()
            .get("request-id")
            .and_then(|value| value.to_str().ok()),
        (fixture.protocol == DownstreamProtocol::Anthropic).then_some(request_id.as_str())
    );
    assert_eq!(
        response
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .and_then(|value| value.to_str().ok()),
        scenario.www_authenticate.as_deref()
    );
    assert_eq!(
        response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        scenario.retry_after.as_deref()
    );
    assert_eq!(
        response
            .headers()
            .get(header::ALLOW)
            .and_then(|value| value.to_str().ok()),
        scenario.allow.as_deref()
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        scenario.cors.then_some("*")
    );
    assert_eq!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
            .and_then(|value| value.to_str().ok()),
        scenario.cors.then_some("*")
    );
    assert!(
        response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
            .is_none()
    );

    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("golden response body should read");
    let body: Value = serde_json::from_slice(&body).expect("golden response should be JSON");
    assert_eq!(
        body,
        render_dynamic(&scenario.body, &request_id, api_key_name),
        "{:?} golden body mismatch",
        fixture.protocol
    );
    (request_id, body)
}

fn generation_path(protocol: DownstreamProtocol, requested_model: &str, versioned: bool) -> String {
    match protocol {
        DownstreamProtocol::Openai => format!(
            "/openai/{prefix}chat/completions",
            prefix = if versioned { "v1/" } else { "" }
        ),
        DownstreamProtocol::Responses => format!(
            "/responses/{prefix}responses",
            prefix = if versioned { "v1/" } else { "" }
        ),
        DownstreamProtocol::Anthropic => format!(
            "/anthropic/{prefix}messages",
            prefix = if versioned { "v1/" } else { "" }
        ),
        DownstreamProtocol::Gemini => format!(
            "/gemini/{prefix}models/{requested_model}:generateContent",
            prefix = if versioned { "v1beta/" } else { "" }
        ),
    }
}

fn models_path(protocol: DownstreamProtocol) -> &'static str {
    match protocol {
        DownstreamProtocol::Openai => "/openai/v1/models",
        DownstreamProtocol::Responses => "/responses/v1/models",
        DownstreamProtocol::Anthropic => "/anthropic/v1/models",
        DownstreamProtocol::Gemini => "/gemini/v1/models",
    }
}

fn missing_paths(protocol: DownstreamProtocol) -> [&'static str; 2] {
    match protocol {
        DownstreamProtocol::Openai => ["/openai/missing", "/openai/v1/missing"],
        DownstreamProtocol::Responses => ["/responses/missing", "/responses/v1/missing"],
        DownstreamProtocol::Anthropic => ["/anthropic/missing", "/anthropic/v1/missing"],
        DownstreamProtocol::Gemini => ["/gemini/missing", "/gemini/v1beta/missing"],
    }
}

fn connected_request(method: Method, uri: String, body: Body) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, TEST_ORIGIN)
        .body(body)
        .expect("contract request should build");
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 32_400))));
    request
}

async fn dispatch(app_state: Arc<AppState>, request: Request<Body>) -> axum::response::Response {
    create_proxy_router(client_identity_resolver())
        .with_state(app_state)
        .oneshot(request)
        .await
        .expect("proxy router should respond")
}

async fn send_generation(
    router: &RouterFixture,
    fixture: &DirectExecutionFixture,
    body: &Value,
    requested_model: &str,
    include_auth: bool,
    include_connect_info: bool,
) -> axum::response::Response {
    let mut uri = generation_path(fixture.protocol, requested_model, true);
    let mut builder = Request::builder()
        .method(Method::POST)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ORIGIN, TEST_ORIGIN);
    if include_auth {
        match fixture.downstream_auth {
            DownstreamAuth::Bearer => {
                builder = builder.header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", router.downstream_key),
                );
            }
            DownstreamAuth::XApiKey => {
                builder = builder.header("x-api-key", &router.downstream_key);
            }
            DownstreamAuth::GeminiQuery => {
                uri.push_str("?key=");
                uri.push_str(&router.downstream_key);
            }
        }
    }
    let body = render_value(body, requested_model);
    let mut request = builder
        .uri(uri)
        .body(Body::from(
            serde_json::to_vec(&body).expect("downstream fixture should serialize"),
        ))
        .expect("generation request should build");
    if include_connect_info {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 32_401))));
    }
    dispatch(Arc::clone(&router.app_state), request).await
}

#[test]
fn four_downstream_error_contracts_match_golden_fixtures() {
    let contracts = contract_fixtures();
    for ((name, direct_fixture), contract) in fixtures().into_iter().zip(contracts) {
        assert_eq!(direct_fixture.protocol, contract.protocol);
        let case_name = format!("error-contract-provider-{name}");
        run_case(&case_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::TOO_MANY_REQUESTS,
                direct_fixture.error.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &direct_fixture, &upstream.base_url).await;
            let requested_model = router.requested_model();
            let response = send_generation(
                &router,
                &direct_fixture,
                &direct_fixture.error.downstream_request,
                &requested_model,
                true,
                true,
            )
            .await;
            let (request_id, _) =
                assert_golden_response(response, &contract, &contract.provider, None).await;

            assert_eq!(
                upstream.requests().await.len(),
                1,
                "provider error must issue exactly one upstream request"
            );
            let logs = router.request_logs().await;
            assert_eq!(
                logs.len(),
                1,
                "provider error must persist one request record"
            );
            assert_eq!(logs[0].request_id, request_id);
            assert_eq!(logs[0].overall_status, RequestStatus::Error);
            assert_eq!(
                logs[0].final_error_code.as_deref(),
                Some("upstream_rate_limit_error")
            );
            upstream.shutdown().await;
        });
    }
}

#[test]
fn router_and_ingress_rejections_use_protocol_contracts() {
    let contracts = contract_fixtures();
    for ((name, direct_fixture), contract) in fixtures().into_iter().zip(contracts) {
        assert_eq!(direct_fixture.protocol, contract.protocol);
        let case_name = format!("error-contract-router-{name}");
        run_case(&case_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                direct_fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let router = RouterFixture::new(context, &direct_fixture, &upstream.base_url).await;

            let requested_model = router.requested_model();
            let authentication = send_generation(
                &router,
                &direct_fixture,
                &direct_fixture.request.downstream,
                &requested_model,
                false,
                true,
            )
            .await;
            assert_golden_response(authentication, &contract, &contract.authentication, None).await;

            router
                .app_state
                .secret_encryption
                .reset_decrypt_call_count();
            let capability = send_generation(
                &router,
                &direct_fixture,
                &direct_fixture.request.downstream,
                "missing-provider/missing-model",
                true,
                true,
            )
            .await;
            assert_golden_response(capability, &contract, &contract.capability, None).await;

            let internal = send_generation(
                &router,
                &direct_fixture,
                &direct_fixture.request.downstream,
                &requested_model,
                true,
                false,
            )
            .await;
            assert_golden_response(internal, &contract, &contract.internal, None).await;

            let query = connected_request(
                Method::GET,
                format!("{}?private=%00", models_path(contract.protocol)),
                Body::empty(),
            );
            let query = dispatch(Arc::clone(&router.app_state), query).await;
            let (_, query_body) =
                assert_golden_response(query, &contract, &contract.query_rejection, None).await;
            assert!(!query_body.to_string().contains("private"));
            assert!(!query_body.to_string().contains("%00"));

            if let Some(path_rejection) = &contract.path_rejection {
                let path = connected_request(
                    Method::POST,
                    "/gemini/v1/models/%FF".to_string(),
                    Body::from("{}"),
                );
                let path = dispatch(Arc::clone(&router.app_state), path).await;
                let (_, path_body) =
                    assert_golden_response(path, &contract, path_rejection, None).await;
                assert!(!path_body.to_string().contains("%FF"));
            }

            for missing_path in missing_paths(contract.protocol) {
                let request =
                    connected_request(Method::GET, missing_path.to_string(), Body::empty());
                let response = dispatch(Arc::clone(&router.app_state), request).await;
                assert_golden_response(response, &contract, &contract.route_not_found, None).await;
            }

            for versioned in [false, true] {
                let request = connected_request(
                    Method::GET,
                    generation_path(contract.protocol, &requested_model, versioned),
                    Body::empty(),
                );
                let response = dispatch(Arc::clone(&router.app_state), request).await;
                assert_golden_response(response, &contract, &contract.method_not_allowed, None)
                    .await;
            }

            assert_eq!(
                router.app_state.secret_encryption.decrypt_call_count(),
                0,
                "pre-send rejections must not decrypt provider credentials"
            );
            assert!(upstream.requests().await.is_empty());
            assert!(router.request_logs().await.is_empty());
            upstream.shutdown().await;
        });
    }

    let direct_fixtures = fixtures();
    let openai_direct = direct_fixtures
        .iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Openai)
        .map(|(_, fixture)| fixture.clone())
        .expect("OpenAI direct fixture");
    let gemini_direct = direct_fixtures
        .iter()
        .find(|(_, fixture)| fixture.protocol == DownstreamProtocol::Gemini)
        .map(|(_, fixture)| fixture.clone())
        .expect("Gemini direct fixture");
    let contracts = contract_fixtures();
    let openai_contract = contracts
        .iter()
        .find(|fixture| fixture.protocol == DownstreamProtocol::Openai)
        .cloned()
        .expect("OpenAI contract fixture");
    let gemini_contract = contracts
        .iter()
        .find(|fixture| fixture.protocol == DownstreamProtocol::Gemini)
        .cloned()
        .expect("Gemini contract fixture");

    run_case("error-contract-utilities", move |context| async move {
        let upstream = TestUpstream::spawn_json(StatusCode::OK, json!({"unexpected": true})).await;
        let gemini_target =
            RouterFixture::new(context.clone(), &gemini_direct, &upstream.base_url).await;
        for operation in ["embeddings", "rerank"] {
            let body = match operation {
                "embeddings" => json!({
                    "model": gemini_target.requested_model(),
                    "input": "hello"
                }),
                "rerank" => json!({
                    "model": gemini_target.requested_model(),
                    "query": "hello",
                    "documents": ["world"]
                }),
                _ => unreachable!(),
            };
            let mut request = connected_request(
                Method::POST,
                format!("/openai/v1/{operation}"),
                Body::from(serde_json::to_vec(&body).unwrap()),
            );
            request.headers_mut().insert(
                header::AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {}", gemini_target.downstream_key))
                    .expect("test authorization should be valid"),
            );
            let response = dispatch(Arc::clone(&gemini_target.app_state), request).await;
            let scenario = openai_contract
                .utility_rejections
                .get(operation)
                .expect("OpenAI utility golden");
            assert_golden_response(response, &openai_contract, scenario, None).await;
        }

        let openai_target = RouterFixture::new(context, &openai_direct, &upstream.base_url).await;
        let uri = format!(
            "/gemini/v1/models/{}:countTokens?key={}",
            openai_target.requested_model(),
            openai_target.downstream_key
        );
        let request = connected_request(
            Method::POST,
            uri,
            Body::from(
                serde_json::to_vec(&json!({
                    "contents": [{"parts": [{"text": "hello"}]}]
                }))
                .unwrap(),
            ),
        );
        let response = dispatch(Arc::clone(&openai_target.app_state), request).await;
        assert_golden_response(
            response,
            &gemini_contract,
            gemini_contract
                .utility_rejections
                .get("countTokens")
                .expect("Gemini utility golden"),
            None,
        )
        .await;

        let ollama_router = create_state_router()
            .nest(
                "/ai",
                create_state_router()
                    .merge(create_proxy_router(client_identity_resolver()))
                    .fallback(handle_404),
            )
            .with_state(Arc::clone(&openai_target.app_state));
        let ollama = ollama_router
            .oneshot(connected_request(
                Method::POST,
                "/ai/ollama/api/chat".to_string(),
                Body::from("{}"),
            ))
            .await
            .expect("base app should respond");
        assert_eq!(ollama.status(), StatusCode::NOT_FOUND);
        assert!(ollama.headers().get(&X_REQUEST_ID).is_none());
        assert!(ollama.headers().get(header::CACHE_CONTROL).is_none());
        assert!(
            ollama
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .is_none()
        );

        gemini_target.app_state.flush_proxy_logs().await;
        openai_target.app_state.flush_proxy_logs().await;
        let utility_logs = RequestLog::list_full(RequestLogQueryPayload::default())
            .expect("request logs should be queryable")
            .list;
        assert_eq!(
            utility_logs.len(),
            3,
            "the three authenticated utility rejections keep their existing records; Ollama adds none"
        );
        assert!(utility_logs.iter().all(|log| {
            log.overall_status == RequestStatus::Error
                && log.final_error_code.as_deref() == Some("unsupported_capability_error")
        }));
        assert!(upstream.requests().await.is_empty());
        upstream.shutdown().await;
    });
}

#[test]
fn retry_after_uses_exact_producer_facts() {
    let contracts = contract_fixtures();
    for ((name, direct_fixture), contract) in fixtures().into_iter().zip(contracts) {
        assert_eq!(direct_fixture.protocol, contract.protocol);
        let case_name = format!("error-contract-retry-{name}");
        run_case(&case_name, move |context| async move {
            let upstream = TestUpstream::spawn_json(
                StatusCode::OK,
                direct_fixture.non_stream.upstream_response.clone(),
            )
            .await;
            let mut router = RouterFixture::new(context, &direct_fixture, &upstream.base_url).await;
            router
                .app_state
                .admin
                .api_key
                .update_api_key(
                    router.downstream_api_key_id,
                    UpdateApiKeyMetadataPayload {
                        rate_limit_rpm: Some(Some(1)),
                        ..Default::default()
                    },
                )
                .await
                .expect("test API key rate limit should update");
            let mut app_state = (*router.app_state).clone();
            app_state.api_key_governance = Arc::new(ApiKeyGovernanceService::new_with_clock(
                Arc::new(MemoryApiKeyRuntimeStore::default()),
                Arc::new(FixedApiKeyGovernanceClock::new(FIXED_RPM_BOUNDARY_MS)),
            ));
            router.app_state = Arc::new(app_state);

            let requested_model = router.requested_model();
            let admitted = send_generation(
                &router,
                &direct_fixture,
                &direct_fixture.request.downstream,
                &requested_model,
                true,
                true,
            )
            .await;
            assert_eq!(admitted.status(), StatusCode::OK);
            to_bytes(admitted.into_body(), usize::MAX)
                .await
                .expect("admitted response should drain");

            let rejected = send_generation(
                &router,
                &direct_fixture,
                &direct_fixture.request.downstream,
                &requested_model,
                true,
                true,
            )
            .await;
            assert_golden_response(
                rejected,
                &contract,
                &contract.rate_limit,
                Some(&router.downstream_key_name),
            )
            .await;

            assert_eq!(
                upstream.requests().await.len(),
                1,
                "rate rejection must not issue a second upstream request"
            );
            let logs = router.request_logs().await;
            assert_eq!(
                logs.len(),
                2,
                "the admitted request and authenticated governance rejection keep their records"
            );
            assert!(
                logs.iter()
                    .any(|log| log.overall_status == RequestStatus::Success)
            );
            assert!(logs.iter().any(|log| {
                log.overall_status == RequestStatus::Error
                    && log.final_error_code.as_deref() == Some("rate_limit_error")
            }));
            upstream.shutdown().await;
        });
    }
}
