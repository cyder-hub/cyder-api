use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{OriginalUri, Path, Query, Request, State},
    http::{HeaderName, HeaderValue, Method, header::CACHE_CONTROL},
    middleware::{self, Next},
    response::Response,
    routing::{MethodRouter, get, post},
};
use tower_http::cors::{AllowHeaders, Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    ingress::client_identity::{ClientIdentityResolver, proxy_client_identity_middleware},
    schema::enum_def::DownstreamProtocol,
    service::app_state::{AppState, StateRouter, create_state_router},
};

use super::gemini::handle_gemini_request;
use super::handlers::{list_models_handler, openai_utility_handler};
use super::request_context::{
    ProxyRequestContext, X_CLIENT_REQUEST_ID, X_REQUEST_ID, derive_request_operation_kind,
};
use super::unified::unified_proxy_handler;

type QueryParams = HashMap<String, String>;

fn generation_route(downstream_protocol: DownstreamProtocol) -> MethodRouter<Arc<AppState>> {
    post(
        move |State(app_state), Query(query_params): Query<QueryParams>, request: Request<Body>| async move {
            unified_proxy_handler(app_state, query_params, downstream_protocol, request).await
        },
    )
}

fn openai_utility_route(downstream_path: &'static str) -> MethodRouter<Arc<AppState>> {
    post(
        move |State(app_state), Query(params): Query<QueryParams>, request: Request<Body>| async move {
            openai_utility_handler(app_state, params, request, downstream_path).await
        },
    )
}

fn models_route(downstream_protocol: DownstreamProtocol) -> MethodRouter<Arc<AppState>> {
    get(
        move |State(app_state), Query(params): Query<QueryParams>, request: Request<Body>| async move {
            list_models_handler(app_state, params, request, downstream_protocol).await
        },
    )
}

fn add_generation_routes(
    router: StateRouter,
    downstream_protocol: DownstreamProtocol,
    paths: &[&'static str],
) -> StateRouter {
    paths.iter().fold(router, |router, path| {
        router.route(path, generation_route(downstream_protocol))
    })
}

fn add_openai_utility_routes(
    router: StateRouter,
    paths: &[(&'static str, &'static str)],
) -> StateRouter {
    paths
        .iter()
        .fold(router, |router, (route_path, downstream_path)| {
            router.route(route_path, openai_utility_route(downstream_path))
        })
}

fn nest_router_variants(
    router: StateRouter,
    include_root_routes: bool,
    version_prefixes: &[&'static str],
) -> StateRouter {
    let router_variants = version_prefixes
        .iter()
        .fold(create_state_router(), |router_variants, version_prefix| {
            router_variants.nest(version_prefix, router.clone())
        });

    if include_root_routes {
        router_variants.merge(router)
    } else {
        router_variants
    }
}

fn create_openai_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        DownstreamProtocol::Openai,
        &["/chat/completions"],
    );
    let router = add_openai_utility_routes(
        router,
        &[("/embeddings", "embeddings"), ("/rerank", "rerank")],
    )
    .route("/models", models_route(DownstreamProtocol::Openai));

    nest_router_variants(router, true, &["/v1"])
}

fn create_anthropic_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        DownstreamProtocol::Anthropic,
        &["/messages"],
    )
    .route("/models", models_route(DownstreamProtocol::Anthropic));

    nest_router_variants(router, true, &["/v1"])
}

fn create_responses_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        DownstreamProtocol::Responses,
        &["/responses"],
    )
    .route("/models", models_route(DownstreamProtocol::Responses));

    nest_router_variants(router, true, &["/v1"])
}

fn create_gemini_router() -> StateRouter {
    let router = create_state_router()
        .route("/models", models_route(DownstreamProtocol::Gemini))
        .route(
            "/models/{*model_action_segment}",
            post(
                |Path(path_segment): Path<String>,
                 Query(query_params): Query<QueryParams>,
                 State(app_state),
                 request: Request<Body>| async move {
                    handle_gemini_request(app_state, path_segment, query_params, request).await
                },
            ),
        );

    nest_router_variants(router, true, &["/v1beta", "/v1"])
}

async fn request_identity_middleware(mut request: Request<Body>, next: Next) -> Response {
    let request_context = Arc::new(ProxyRequestContext::from_headers(request.headers()));
    let request_uri = request
        .extensions()
        .get::<OriginalUri>()
        .map(|uri| &uri.0)
        .unwrap_or_else(|| request.uri());
    let request_path = request_uri.path().to_string();
    let operation_kind = derive_request_operation_kind(&request_path);
    let query_param_count = request_uri
        .query()
        .map(|query| {
            query
                .split('&')
                .filter(|parameter| !parameter.is_empty())
                .count()
        })
        .unwrap_or_default();
    let request_id = request_context.request_id.to_string();
    request
        .extensions_mut()
        .insert(Arc::clone(&request_context));

    crate::logging::with_request_id_scope(request_id, async move {
        crate::debug_event!(
            "proxy.request_received",
            request_path = &request_path,
            operation_kind = &operation_kind,
            query_param_count = query_param_count,
            client_request_id = &request_context.client_request_id,
        );

        let mut response = next.run(request).await;
        response.headers_mut().insert(
            &X_REQUEST_ID,
            HeaderValue::from_str(request_context.request_id.as_str())
                .expect("generated request id must be a valid header value"),
        );
        response.headers_mut().remove(&X_CLIENT_REQUEST_ID);
        if let Some(client_request_id) = &request_context.client_request_id {
            response.headers_mut().insert(
                &X_CLIENT_REQUEST_ID,
                HeaderValue::from_str(client_request_id.as_str())
                    .expect("validated client request id must be a valid header value"),
            );
        }

        crate::debug_event!(
            "proxy.response_ready",
            status_code = response.status().as_u16(),
            duration_ms = chrono::Utc::now()
                .timestamp_millis()
                .saturating_sub(request_context.received_at_ms),
        );
        response
    })
    .await
}

pub fn create_proxy_router(client_identity_resolver: Arc<ClientIdentityResolver>) -> StateRouter {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers(AllowHeaders::mirror_request())
        .expose_headers(Any)
        .max_age(Duration::from_secs(600));

    create_state_router()
        .nest("/openai", create_openai_router())
        .nest("/anthropic", create_anthropic_router())
        .nest("/responses", create_responses_router())
        .nest("/gemini", create_gemini_router())
        .layer(cors)
        .layer(middleware::from_fn_with_state(
            client_identity_resolver,
            proxy_client_identity_middleware,
        ))
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-store"),
        ))
        .layer(SetResponseHeaderLayer::overriding(
            HeaderName::from_static("x-content-type-options"),
            HeaderValue::from_static("nosniff"),
        ))
        .layer(middleware::from_fn(request_identity_middleware))
}

#[cfg(test)]
mod tests {
    use std::{
        net::{IpAddr, Ipv4Addr, SocketAddr},
        sync::Arc,
    };

    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{HeaderValue, Method, Request, StatusCode, header},
        routing::get,
    };
    use tower::ServiceExt;
    use uuid::Version;

    use crate::config::ClientIdentityConfig;
    use crate::controller::handle_404;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::request_log::{RequestLog, RequestLogQueryPayload};
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::ingress::client_identity::ClientIdentityResolver;
    use crate::schema::enum_def::Action;
    use crate::service::admin::auth::LoginError;
    use crate::service::app_state::{create_state_router, create_test_app_state};
    use diesel::RunQueryDsl;

    use super::{X_CLIENT_REQUEST_ID, X_REQUEST_ID, create_proxy_router};

    fn payload() -> CreateApiKeyPayload {
        CreateApiKeyPayload {
            name: "proxy-no-store".to_string(),
            description: None,
            default_action: Some(Action::Allow),
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
        }
    }

    fn request(path: &str, header_name: &str, api_key: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method(Method::GET).uri(path);
        if let Some(api_key) = api_key {
            let value = if header_name == header::AUTHORIZATION.as_str() {
                format!("Bearer {api_key}")
            } else {
                api_key.to_string()
            };
            builder = builder.header(header_name, value);
        }
        let mut request = builder
            .body(Body::empty())
            .expect("proxy request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_300))));
        request
    }

    fn method_request(method: Method, path: &str) -> Request<Body> {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{}"))
            .expect("proxy request should build");
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_304))));
        request
    }

    fn client_identity_resolver() -> Arc<ClientIdentityResolver> {
        Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default()))
    }

    #[tokio::test]
    async fn client_identity_http_proxy_fails_closed_and_ignores_untrusted_metadata() {
        let database = TestDbContext::new_sqlite("proxy-client-identity-http.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let resolver = Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig {
                    trusted_proxy_cidrs: vec![
                        "10.0.0.0/8".parse().expect("test CIDR should parse"),
                    ],
                    max_forwarded_hops: 8,
                }));

                let mut missing =
                    request("/openai/v1/models", header::AUTHORIZATION.as_str(), None);
                missing.extensions_mut().remove::<ConnectInfo<SocketAddr>>();
                let missing = create_proxy_router(Arc::clone(&resolver))
                    .with_state(Arc::clone(&app_state))
                    .oneshot(missing)
                    .await
                    .expect("proxy router should respond");
                assert_eq!(missing.status(), StatusCode::INTERNAL_SERVER_ERROR);
                assert_proxy_security(&missing);
                assert!(
                    missing
                        .headers()
                        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                        .is_none()
                );
                let body = to_bytes(missing.into_body(), usize::MAX)
                    .await
                    .expect("proxy error body should read");
                let body: serde_json::Value =
                    serde_json::from_slice(&body).expect("proxy error should be JSON");
                assert_eq!(body["code"], "server_error");

                let mut invalid =
                    request("/openai/v1/models", header::AUTHORIZATION.as_str(), None);
                invalid
                    .extensions_mut()
                    .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 9], 31_301))));
                invalid
                    .headers_mut()
                    .insert("forwarded", HeaderValue::from_static("not-valid"));
                let invalid = create_proxy_router(Arc::clone(&resolver))
                    .with_state(Arc::clone(&app_state))
                    .oneshot(invalid)
                    .await
                    .expect("proxy router should respond");
                assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
                assert_proxy_security(&invalid);
                assert!(
                    invalid
                        .headers()
                        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                        .is_none()
                );
                let body = to_bytes(invalid.into_body(), usize::MAX)
                    .await
                    .expect("proxy error body should read");
                let body: serde_json::Value =
                    serde_json::from_slice(&body).expect("proxy error should be JSON");
                assert_eq!(body["code"], "invalid_request_error");

                let mut forged = request("/openai/v1/models", header::AUTHORIZATION.as_str(), None);
                forged
                    .headers_mut()
                    .insert("forwarded", HeaderValue::from_static("not-valid"));
                let ignored = create_proxy_router(resolver)
                    .with_state(app_state)
                    .oneshot(forged)
                    .await
                    .expect("proxy router should respond");
                assert_eq!(ignored.status(), StatusCode::UNAUTHORIZED);
            })
            .await;
    }

    fn assert_no_store(response: &axum::response::Response) {
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some("no-store")
        );
    }

    fn assert_proxy_security(response: &axum::response::Response) {
        assert_no_store(response);
        assert_eq!(
            response
                .headers()
                .get("x-content-type-options")
                .and_then(|value| value.to_str().ok()),
            Some("nosniff")
        );
        assert_request_identity(response);
    }

    fn assert_request_identity(response: &axum::response::Response) -> String {
        let value = response
            .headers()
            .get(&X_REQUEST_ID)
            .and_then(|value| value.to_str().ok())
            .expect("proxy response should include a request id");
        let parsed = uuid::Uuid::parse_str(value).expect("request id should be a valid UUID");
        assert_eq!(value.len(), 36);
        assert_eq!(value, value.to_ascii_lowercase());
        assert_eq!(parsed.get_version(), Some(Version::Random));
        value.to_string()
    }

    fn assert_public_cors_response(response: &axum::response::Response) {
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
        assert_eq!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_EXPOSE_HEADERS)
                .and_then(|value| value.to_str().ok()),
            Some("*")
        );
        assert!(
            response
                .headers()
                .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                .is_none()
        );
        let vary = response
            .headers()
            .get_all(header::VARY)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .collect::<Vec<_>>()
            .join(",")
            .to_ascii_lowercase();
        for expected in [
            "origin",
            "access-control-request-method",
            "access-control-request-headers",
        ] {
            assert!(vary.contains(expected), "Vary missing {expected}: {vary}");
        }
        assert_proxy_security(response);
    }

    #[tokio::test]
    async fn four_protocol_version_aliases_are_direct_and_equivalent() {
        let database = TestDbContext::new_sqlite("proxy-four-protocol-aliases.sqlite");
        database
            .run_async(async {
                let created = ApiKey::create(&payload()).expect("proxy key should create");
                let api_key = created.reveal.api_key;
                let app_state = create_test_app_state(database.clone()).await;
                let cases: [(&str, &[&str], &str); 4] = [
                    (
                        "openai",
                        &["/openai/models", "/openai/v1/models"],
                        header::AUTHORIZATION.as_str(),
                    ),
                    (
                        "responses",
                        &["/responses/models", "/responses/v1/models"],
                        header::AUTHORIZATION.as_str(),
                    ),
                    (
                        "anthropic",
                        &["/anthropic/models", "/anthropic/v1/models"],
                        "x-api-key",
                    ),
                    (
                        "gemini",
                        &[
                            "/gemini/models",
                            "/gemini/v1/models",
                            "/gemini/v1beta/models",
                        ],
                        "x-goog-api-key",
                    ),
                ];

                for (protocol, paths, header_name) in cases {
                    let mut bodies = Vec::new();
                    for path in paths {
                        let response = create_proxy_router(client_identity_resolver())
                            .with_state(Arc::clone(&app_state))
                            .oneshot(request(path, header_name, Some(&api_key)))
                            .await
                            .expect("proxy alias should respond");
                        assert_eq!(response.status(), StatusCode::OK, "{protocol}: {path}");
                        bodies.push(
                            to_bytes(response.into_body(), usize::MAX)
                                .await
                                .expect("models body should read"),
                        );
                    }
                    assert!(
                        bodies.windows(2).all(|pair| pair[0] == pair[1]),
                        "{protocol}: aliases must render the same response without redirect"
                    );
                }
            })
            .await;
    }

    #[tokio::test]
    async fn route_methods_fail_with_405_before_authentication() {
        let database = TestDbContext::new_sqlite("proxy-strict-methods.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let generation_paths = [
                    "/openai/chat/completions",
                    "/openai/v1/chat/completions",
                    "/responses/responses",
                    "/responses/v1/responses",
                    "/anthropic/messages",
                    "/anthropic/v1/messages",
                    "/gemini/models/test:generateContent",
                    "/gemini/v1/models/test:generateContent",
                    "/gemini/v1beta/models/test:generateContent",
                ];
                for path in generation_paths {
                    let wrong_method = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::GET, path))
                        .await
                        .expect("wrong method should respond");
                    assert_eq!(
                        wrong_method.status(),
                        StatusCode::METHOD_NOT_ALLOWED,
                        "{path}"
                    );
                    assert_request_identity(&wrong_method);

                    let routed = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::POST, path))
                        .await
                        .expect("generation route should respond");
                    assert_eq!(routed.status(), StatusCode::UNAUTHORIZED, "{path}");
                }

                for path in [
                    "/openai/embeddings",
                    "/openai/v1/embeddings",
                    "/openai/rerank",
                    "/openai/v1/rerank",
                ] {
                    let wrong_method = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::GET, path))
                        .await
                        .expect("wrong utility method should respond");
                    assert_eq!(
                        wrong_method.status(),
                        StatusCode::METHOD_NOT_ALLOWED,
                        "{path}"
                    );
                    assert_request_identity(&wrong_method);
                }

                for path in [
                    "/openai/models",
                    "/responses/models",
                    "/anthropic/models",
                    "/gemini/models",
                ] {
                    let wrong_method = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::POST, path))
                        .await
                        .expect("wrong models method should respond");
                    assert_eq!(
                        wrong_method.status(),
                        StatusCode::METHOD_NOT_ALLOWED,
                        "{path}"
                    );
                    assert_request_identity(&wrong_method);
                }

                app_state.flush_proxy_logs().await;
                assert!(
                    RequestLog::list_full(RequestLogQueryPayload::default())
                        .expect("request logs should be queryable")
                        .list
                        .is_empty(),
                    "method and authentication rejection must happen before request logging"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn gemini_exposes_only_count_tokens_utility_action() {
        let database = TestDbContext::new_sqlite("proxy-gemini-count-tokens-only.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                for path in [
                    "/gemini/models/test:countMessageTokens",
                    "/gemini/v1/models/test:countTextTokens",
                    "/gemini/v1beta/models/test:countMessageTokens",
                ] {
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::POST, path))
                        .await
                        .expect("unsupported Gemini action should respond");
                    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
                }

                for path in [
                    "/gemini/models/test:countTokens",
                    "/gemini/v1/models/test:countTokens",
                    "/gemini/v1beta/models/test:countTokens",
                ] {
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::POST, path))
                        .await
                        .expect("countTokens route should respond");
                    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
                }
            })
            .await;
    }

    #[tokio::test]
    async fn ollama_paths_are_base_app_404_without_proxy_side_effects() {
        let database = TestDbContext::new_sqlite("proxy-ollama-base-404.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                app_state.secret_encryption.reset_decrypt_call_count();
                let router = create_state_router()
                    .nest(
                        "/ai",
                        create_state_router()
                            .merge(create_proxy_router(client_identity_resolver()))
                            .fallback(handle_404),
                    )
                    .with_state(Arc::clone(&app_state));

                for path in [
                    "/ai/ollama/api/chat",
                    "/ai/ollama/api/generate",
                    "/ai/ollama/api/embeddings",
                    "/ai/ollama/api/tags",
                    "/ai/ollama/arbitrary/nested/path",
                ] {
                    let mut request = method_request(Method::POST, path);
                    request.headers_mut().insert(
                        header::ORIGIN,
                        HeaderValue::from_static("https://client.example"),
                    );
                    let response = router
                        .clone()
                        .oneshot(request)
                        .await
                        .expect("base app should respond");
                    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
                    assert!(
                        response
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                            .is_none(),
                        "{path}: unknown Ollama path must not receive proxy CORS"
                    );
                    assert!(
                        response.headers().get(header::CACHE_CONTROL).is_none(),
                        "{path}: unknown Ollama path must not receive proxy security layers"
                    );
                    assert!(
                        response.headers().get(&X_REQUEST_ID).is_none(),
                        "{path}: unknown Ollama path must not receive proxy request identity"
                    );
                }

                app_state.flush_proxy_logs().await;
                assert!(
                    RequestLog::list_full(RequestLogQueryPayload::default())
                        .expect("request logs should be queryable")
                        .list
                        .is_empty(),
                    "unknown Ollama paths must not enter proxy logging"
                );
                assert_eq!(
                    app_state.secret_encryption.decrypt_call_count(),
                    0,
                    "unknown Ollama paths must not resolve provider credentials"
                );
            })
            .await;
    }

    #[tokio::test]
    async fn proxy_request_identity_is_gateway_owned_and_client_id_is_bounded() {
        let database = TestDbContext::new_sqlite("proxy-request-identity.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;

                let mut valid = request("/openai/v1/models", header::AUTHORIZATION.as_str(), None);
                valid
                    .headers_mut()
                    .insert(&X_REQUEST_ID, HeaderValue::from_static("forged-request-id"));
                valid.headers_mut().insert(
                    &X_CLIENT_REQUEST_ID,
                    HeaderValue::from_static("caller.trace_1:part-2"),
                );
                let valid = create_proxy_router(client_identity_resolver())
                    .with_state(Arc::clone(&app_state))
                    .oneshot(valid)
                    .await
                    .expect("proxy auth error should respond");
                assert_eq!(valid.status(), StatusCode::UNAUTHORIZED);
                let generated = assert_request_identity(&valid);
                assert_ne!(generated, "forged-request-id");
                assert_eq!(
                    valid
                        .headers()
                        .get(&X_CLIENT_REQUEST_ID)
                        .and_then(|value| value.to_str().ok()),
                    Some("caller.trace_1:part-2")
                );

                let mut invalid = request("/anthropic/v1/models", "x-api-key", None);
                invalid.headers_mut().insert(
                    &X_CLIENT_REQUEST_ID,
                    HeaderValue::from_static("contains space"),
                );
                let invalid = create_proxy_router(client_identity_resolver())
                    .with_state(Arc::clone(&app_state))
                    .oneshot(invalid)
                    .await
                    .expect("proxy auth error should respond");
                assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
                assert_request_identity(&invalid);
                assert!(invalid.headers().get(&X_CLIENT_REQUEST_ID).is_none());

                let mut duplicate = request("/gemini/v1/models", "x-goog-api-key", None);
                duplicate
                    .headers_mut()
                    .append(&X_CLIENT_REQUEST_ID, HeaderValue::from_static("caller-one"));
                duplicate
                    .headers_mut()
                    .append(&X_CLIENT_REQUEST_ID, HeaderValue::from_static("caller-two"));
                let duplicate = create_proxy_router(client_identity_resolver())
                    .with_state(app_state)
                    .oneshot(duplicate)
                    .await
                    .expect("proxy auth error should respond");
                assert_eq!(duplicate.status(), StatusCode::UNAUTHORIZED);
                assert_request_identity(&duplicate);
                assert!(duplicate.headers().get(&X_CLIENT_REQUEST_ID).is_none());
            })
            .await;
    }

    #[tokio::test]
    async fn request_identity_layer_does_not_leak_to_non_proxy_routes() {
        let database = TestDbContext::new_sqlite("request-identity-router-boundary.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let router = create_state_router()
                    .route("/system", get(|| async { StatusCode::OK }))
                    .route("/manager", get(|| async { StatusCode::OK }))
                    .merge(create_proxy_router(client_identity_resolver()))
                    .with_state(app_state);

                for path in ["/system", "/manager"] {
                    let response = router
                        .clone()
                        .oneshot(method_request(Method::GET, path))
                        .await
                        .expect("non-proxy route should respond");
                    assert_eq!(response.status(), StatusCode::OK, "{path}");
                    assert!(
                        response.headers().get(&X_REQUEST_ID).is_none(),
                        "{path}: non-proxy route must not receive proxy request identity"
                    );
                }
            })
            .await;
    }

    #[tokio::test]
    async fn all_four_proxy_protocols_override_cache_control_on_success_and_auth_error() {
        let database = TestDbContext::new_sqlite("proxy-api-no-store.sqlite");
        database
            .run_async(async {
                let created = ApiKey::create(&payload()).expect("proxy key should create");
                let api_key = created.reveal.api_key;
                let app_state = create_test_app_state(database.clone()).await;
                let cases = [
                    ("/openai/v1/models", header::AUTHORIZATION.as_str()),
                    ("/responses/v1/models", header::AUTHORIZATION.as_str()),
                    ("/anthropic/v1/models", "x-api-key"),
                    ("/gemini/v1/models", "x-goog-api-key"),
                ];

                for (path, header_name) in cases {
                    let success = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, header_name, Some(&api_key)))
                        .await
                        .expect("proxy success should respond");
                    assert_eq!(success.status(), StatusCode::OK, "{path}");
                    assert_proxy_security(&success);

                    let error = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, header_name, None))
                        .await
                        .expect("proxy auth error should respond");
                    assert_eq!(error.status(), StatusCode::UNAUTHORIZED, "{path}");
                    assert_proxy_security(&error);
                }
            })
            .await;
    }

    #[tokio::test]
    async fn manager_auth_storage_failure_does_not_degrade_proxy_api_key_paths() {
        let database = TestDbContext::new_sqlite("proxy-manager-auth-isolation.sqlite");
        database
            .run_async(async {
                let created = ApiKey::create(&payload()).expect("proxy key should create");
                let api_key = created.reveal.api_key;

                let mut conn = get_connection().expect("connection should load");
                match &mut conn {
                    DbConnection::Postgres(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("manager session table should drop");
                    }
                    DbConnection::Sqlite(conn) => {
                        diesel::sql_query("DROP TABLE manager_auth_instance")
                            .execute(conn)
                            .expect("manager session table should drop");
                    }
                }
                drop(conn);

                let app_state = create_test_app_state(database.clone()).await;
                assert!(matches!(
                    app_state
                        .admin
                        .auth
                        .login(
                            IpAddr::V4(Ipv4Addr::LOCALHOST),
                            "irrelevant unavailable manager password"
                        )
                        .await,
                    Err(LoginError::Unavailable)
                ));

                for (path, header_name) in [
                    ("/openai/v1/models", header::AUTHORIZATION.as_str()),
                    ("/responses/v1/models", header::AUTHORIZATION.as_str()),
                    ("/anthropic/v1/models", "x-api-key"),
                    ("/gemini/v1/models", "x-goog-api-key"),
                ] {
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, header_name, Some(&api_key)))
                        .await
                        .expect("proxy request should respond");
                    assert_eq!(
                        response.status(),
                        StatusCode::OK,
                        "{path} must remain available when manager auth storage is broken"
                    );
                    assert_proxy_security(&response);
                }
            })
            .await;
    }

    #[tokio::test]
    async fn proxy_public_cors_covers_four_protocols_errors_404_and_preflight() {
        let database = TestDbContext::new_sqlite("proxy-public-cors.sqlite");
        database
            .run_async(async {
                let created = ApiKey::create(&payload()).expect("proxy key should create");
                let api_key = created.reveal.api_key;
                let mut disabled_payload = payload();
                disabled_payload.name = "proxy-public-cors-disabled".to_string();
                disabled_payload.is_enabled = Some(false);
                let disabled =
                    ApiKey::create(&disabled_payload).expect("disabled proxy key should create");
                let disabled_api_key = disabled.reveal.api_key;
                let app_state = create_test_app_state(database.clone()).await;
                let cases = [
                    (
                        "/openai/v1/models",
                        "/openai/missing",
                        "/openai/v1/chat/completions",
                        header::AUTHORIZATION.as_str(),
                    ),
                    (
                        "/responses/v1/models",
                        "/responses/missing",
                        "/responses/v1/responses",
                        header::AUTHORIZATION.as_str(),
                    ),
                    (
                        "/anthropic/v1/models",
                        "/anthropic/missing",
                        "/anthropic/v1/messages",
                        "x-api-key",
                    ),
                    (
                        "/gemini/v1/models",
                        "/gemini/missing",
                        "/gemini/v1/models/test:generateContent",
                        "x-goog-api-key",
                    ),
                ];

                for (index, (models_path, missing_path, generation_path, header_name)) in
                    cases.into_iter().enumerate()
                {
                    let origin = format!("https://client-{index}.example");

                    let mut success = request(models_path, header_name, Some(&api_key));
                    success.headers_mut().insert(
                        header::ORIGIN,
                        origin.parse().expect("test Origin should parse"),
                    );
                    let success = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(success)
                        .await
                        .expect("proxy success should respond");
                    assert_eq!(success.status(), StatusCode::OK, "{models_path}");
                    assert_public_cors_response(&success);

                    let mut auth_error = request(models_path, header_name, None);
                    auth_error.headers_mut().insert(
                        header::ORIGIN,
                        origin.parse().expect("test Origin should parse"),
                    );
                    let auth_error = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(auth_error)
                        .await
                        .expect("proxy auth error should respond");
                    assert_eq!(
                        auth_error.status(),
                        StatusCode::UNAUTHORIZED,
                        "{models_path}"
                    );
                    assert_public_cors_response(&auth_error);

                    let mut governance_error =
                        request(models_path, header_name, Some(&disabled_api_key));
                    governance_error.headers_mut().insert(
                        header::ORIGIN,
                        origin.parse().expect("test Origin should parse"),
                    );
                    let governance_error = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(governance_error)
                        .await
                        .expect("proxy governance error should respond");
                    assert_eq!(
                        governance_error.status(),
                        StatusCode::FORBIDDEN,
                        "{models_path}"
                    );
                    assert_public_cors_response(&governance_error);

                    let mut missing = request(missing_path, header_name, None);
                    missing.headers_mut().insert(
                        header::ORIGIN,
                        origin.parse().expect("test Origin should parse"),
                    );
                    let missing = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(missing)
                        .await
                        .expect("proxy 404 should respond");
                    assert_eq!(missing.status(), StatusCode::NOT_FOUND, "{missing_path}");
                    assert_public_cors_response(&missing);

                    let mut preflight = Request::builder()
                        .method(Method::OPTIONS)
                        .uri(generation_path)
                        .header(header::ORIGIN, &origin)
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                        .header(
                            header::ACCESS_CONTROL_REQUEST_HEADERS,
                            "authorization,x-client-header",
                        )
                        .body(Body::empty())
                        .expect("proxy preflight should build");
                    preflight
                        .extensions_mut()
                        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_302))));
                    let preflight = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(preflight)
                        .await
                        .expect("proxy preflight should respond");
                    assert_eq!(preflight.status(), StatusCode::OK, "{generation_path}");
                    assert_eq!(
                        preflight
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                            .and_then(|value| value.to_str().ok()),
                        Some("*")
                    );
                    assert_eq!(
                        preflight
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                            .and_then(|value| value.to_str().ok()),
                        Some("GET,POST")
                    );
                    assert_eq!(
                        preflight
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_HEADERS)
                            .and_then(|value| value.to_str().ok()),
                        Some("authorization,x-client-header")
                    );
                    assert_eq!(
                        preflight
                            .headers()
                            .get(header::ACCESS_CONTROL_MAX_AGE)
                            .and_then(|value| value.to_str().ok()),
                        Some("600")
                    );
                    assert!(
                        preflight
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                            .is_none()
                    );
                    assert_proxy_security(&preflight);

                    let mut invalid_method = Request::builder()
                        .method(Method::OPTIONS)
                        .uri(generation_path)
                        .header(header::ORIGIN, &origin)
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "DELETE")
                        .header(
                            header::ACCESS_CONTROL_REQUEST_HEADERS,
                            "authorization,x-client-header",
                        )
                        .body(Body::empty())
                        .expect("invalid proxy preflight should build");
                    invalid_method
                        .extensions_mut()
                        .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_303))));
                    let invalid_method = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(invalid_method)
                        .await
                        .expect("invalid proxy preflight should respond");
                    let allowed_methods = invalid_method
                        .headers()
                        .get(header::ACCESS_CONTROL_ALLOW_METHODS)
                        .and_then(|value| value.to_str().ok())
                        .expect("configured allowed methods should be present");
                    assert!(!allowed_methods.contains("DELETE"));
                    assert!(
                        invalid_method
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_CREDENTIALS)
                            .is_none()
                    );
                    assert_proxy_security(&invalid_method);
                }
            })
            .await;
    }
}
