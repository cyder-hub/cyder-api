use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{
        OriginalUri, Path, Query, Request, State,
        rejection::{PathRejection, QueryRejection},
    },
    http::{HeaderName, HeaderValue, Method, header::CACHE_CONTROL},
    middleware::{self, Next},
    response::Response,
    routing::{MethodRouter, get, post},
};
use serde::{Deserialize, Deserializer, de::Error as _};
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
use super::{
    ExecutionStage, ProtocolErrorResponseAdapter, ProxyError, ProxyErrorCode, ResponseVisibility,
    RouterRejection,
};

struct QueryParams(HashMap<String, String>);

impl<'de> Deserialize<'de> for QueryParams {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let params = HashMap::<String, String>::deserialize(deserializer)?;
        if params
            .iter()
            .any(|(key, value)| key.contains('\0') || value.contains('\0'))
        {
            return Err(D::Error::custom("query parameters must not contain NUL"));
        }
        Ok(Self(params))
    }
}

fn protocol_adapter(
    request: &Request<Body>,
    downstream_protocol: DownstreamProtocol,
) -> ProtocolErrorResponseAdapter {
    let request_context = request
        .extensions()
        .get::<Arc<ProxyRequestContext>>()
        .expect("protocol surface must run after request identity middleware");
    ProtocolErrorResponseAdapter::new(downstream_protocol, request_context.request_id.clone())
}

fn adapt_handler_result(
    adapter: ProtocolErrorResponseAdapter,
    result: Result<Response<Body>, ProxyError>,
) -> Response<Body> {
    match result {
        Ok(response) => response,
        Err(error) => adapter.proxy_error_response(error),
    }
}

fn extractor_rejection_error(kind: &'static str) -> ProxyError {
    let (public_message, operator_message) = match kind {
        "query" => (
            "The request query parameters are invalid.",
            "query parameter extraction failed",
        ),
        "path" => (
            "The request path parameters are invalid.",
            "path parameter extraction failed",
        ),
        _ => unreachable!("extractor rejection kind must be query or path"),
    };
    ProxyError::gateway(
        ProxyErrorCode::InvalidRequestError,
        ExecutionStage::Parse,
        ResponseVisibility::NotVisible,
        Some(public_message.to_string()),
        operator_message,
    )
}

fn generation_route(downstream_protocol: DownstreamProtocol) -> MethodRouter<Arc<AppState>> {
    post(
        move |State(app_state),
              query: Result<Query<QueryParams>, QueryRejection>,
              request: Request<Body>| async move {
            let adapter = protocol_adapter(&request, downstream_protocol);
            let result = match query {
                Ok(Query(QueryParams(query_params))) => {
                    unified_proxy_handler(app_state, query_params, downstream_protocol, request)
                        .await
                }
                Err(_) => Err(extractor_rejection_error("query")),
            };
            adapt_handler_result(adapter, result)
        },
    )
}

fn openai_utility_route(downstream_path: &'static str) -> MethodRouter<Arc<AppState>> {
    post(
        move |State(app_state),
              query: Result<Query<QueryParams>, QueryRejection>,
              request: Request<Body>| async move {
            let adapter = protocol_adapter(&request, DownstreamProtocol::Openai);
            let result = match query {
                Ok(Query(QueryParams(params))) => {
                    openai_utility_handler(app_state, params, request, downstream_path).await
                }
                Err(_) => Err(extractor_rejection_error("query")),
            };
            adapt_handler_result(adapter, result)
        },
    )
}

fn models_route(downstream_protocol: DownstreamProtocol) -> MethodRouter<Arc<AppState>> {
    get(
        move |State(app_state),
              query: Result<Query<QueryParams>, QueryRejection>,
              request: Request<Body>| async move {
            let adapter = protocol_adapter(&request, downstream_protocol);
            let result = match query {
                Ok(Query(QueryParams(params))) => {
                    list_models_handler(app_state, params, request, downstream_protocol).await
                }
                Err(_) => Err(extractor_rejection_error("query")),
            };
            adapt_handler_result(adapter, result)
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
    downstream_protocol: DownstreamProtocol,
) -> StateRouter {
    let router = configure_protocol_rejections(router, downstream_protocol);
    let router_variants = version_prefixes
        .iter()
        .fold(create_state_router(), |router_variants, version_prefix| {
            router_variants.nest(version_prefix, router.clone())
        });

    let router = if include_root_routes {
        router_variants.merge(router)
    } else {
        router_variants
    };
    configure_protocol_rejections(router, downstream_protocol)
}

fn configure_protocol_rejections(
    router: StateRouter,
    downstream_protocol: DownstreamProtocol,
) -> StateRouter {
    router
        .fallback(move |request: Request<Body>| async move {
            protocol_adapter(&request, downstream_protocol)
                .router_rejection_response(RouterRejection::RouteNotFound)
        })
        .method_not_allowed_fallback(move |request: Request<Body>| async move {
            protocol_adapter(&request, downstream_protocol)
                .router_rejection_response(RouterRejection::MethodNotAllowed)
        })
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

    nest_router_variants(router, true, &["/v1"], DownstreamProtocol::Openai)
}

fn create_anthropic_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        DownstreamProtocol::Anthropic,
        &["/messages"],
    )
    .route("/models", models_route(DownstreamProtocol::Anthropic));

    nest_router_variants(router, true, &["/v1"], DownstreamProtocol::Anthropic)
}

fn create_responses_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        DownstreamProtocol::Responses,
        &["/responses"],
    )
    .route("/models", models_route(DownstreamProtocol::Responses));

    nest_router_variants(router, true, &["/v1"], DownstreamProtocol::Responses)
}

fn create_gemini_router() -> StateRouter {
    let router = create_state_router()
        .route("/models", models_route(DownstreamProtocol::Gemini))
        .route(
            "/models/{*model_action_segment}",
            post(
                |path: Result<Path<String>, PathRejection>,
                 query: Result<Query<QueryParams>, QueryRejection>,
                 State(app_state),
                 request: Request<Body>| async move {
                    let adapter = protocol_adapter(&request, DownstreamProtocol::Gemini);
                    let result = match (path, query) {
                        (Err(_), _) => Err(extractor_rejection_error("path")),
                        (_, Err(_)) => Err(extractor_rejection_error("query")),
                        (Ok(Path(path_segment)), Ok(Query(QueryParams(query_params)))) => {
                            handle_gemini_request(app_state, path_segment, query_params, request)
                                .await
                        }
                    };
                    adapt_handler_result(adapter, result)
                },
            ),
        );

    nest_router_variants(
        router,
        true,
        &["/v1beta", "/v1"],
        DownstreamProtocol::Gemini,
    )
}

#[derive(Clone)]
struct ProtocolSurface {
    downstream_protocol: DownstreamProtocol,
    client_identity_resolver: Arc<ClientIdentityResolver>,
}

impl ProtocolSurface {
    fn new(
        downstream_protocol: DownstreamProtocol,
        client_identity_resolver: Arc<ClientIdentityResolver>,
    ) -> Self {
        Self {
            downstream_protocol,
            client_identity_resolver,
        }
    }

    fn bind(self, router: StateRouter) -> StateRouter {
        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods([Method::GET, Method::POST])
            .allow_headers(AllowHeaders::mirror_request())
            .expose_headers(Any)
            .max_age(Duration::from_secs(600));
        let downstream_protocol = self.downstream_protocol;
        let resolver = self.client_identity_resolver;

        router
            .layer(cors)
            .layer(middleware::from_fn(move |request, next| {
                let resolver = Arc::clone(&resolver);
                async move {
                    proxy_client_identity_middleware(resolver, downstream_protocol, request, next)
                        .await
                }
            }))
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
    let openai = ProtocolSurface::new(
        DownstreamProtocol::Openai,
        Arc::clone(&client_identity_resolver),
    )
    .bind(create_openai_router());
    let responses = ProtocolSurface::new(
        DownstreamProtocol::Responses,
        Arc::clone(&client_identity_resolver),
    )
    .bind(create_responses_router());
    let anthropic = ProtocolSurface::new(
        DownstreamProtocol::Anthropic,
        Arc::clone(&client_identity_resolver),
    )
    .bind(create_anthropic_router());
    let gemini = ProtocolSurface::new(DownstreamProtocol::Gemini, client_identity_resolver)
        .bind(create_gemini_router());

    create_state_router()
        .nest("/openai", openai)
        .nest("/anthropic", anthropic)
        .nest("/responses", responses)
        .nest("/gemini", gemini)
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
    use crate::schema::enum_def::{Action, DownstreamProtocol};
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
                assert_eq!(body["error"]["code"], "server_error");
                assert!(body.get("code").is_none());
                assert!(body.get("message").is_none());

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
                assert_eq!(body["error"]["code"], "invalid_request_error");
                assert!(body.get("code").is_none());
                assert!(body.get("message").is_none());

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

    #[tokio::test]
    async fn four_protocol_client_identity_rejections_use_bound_protocol_contracts() {
        let database = TestDbContext::new_sqlite("proxy-client-identity-four-protocols.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let cases = [
                    (
                        "/openai/v1/models",
                        DownstreamProtocol::Openai,
                        "server_error",
                    ),
                    (
                        "/responses/v1/models",
                        DownstreamProtocol::Responses,
                        "server_error",
                    ),
                    (
                        "/anthropic/v1/models",
                        DownstreamProtocol::Anthropic,
                        "api_error",
                    ),
                    ("/gemini/v1/models", DownstreamProtocol::Gemini, "INTERNAL"),
                ];

                for (path, protocol, category) in cases {
                    let mut request = request(path, header::AUTHORIZATION.as_str(), None);
                    request.extensions_mut().remove::<ConnectInfo<SocketAddr>>();
                    request.headers_mut().insert(
                        header::ORIGIN,
                        HeaderValue::from_static("https://client.example"),
                    );
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request)
                        .await
                        .expect("client identity rejection should respond");
                    assert_eq!(
                        response.status(),
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "{path}"
                    );
                    assert_proxy_security(&response);
                    assert!(
                        response
                            .headers()
                            .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                            .is_none(),
                        "{path}: client identity errors must remain outside CORS"
                    );
                    assert_protocol_error_body(response, protocol, "server_error", category).await;
                }
            })
            .await;
    }

    #[tokio::test]
    async fn query_and_path_rejections_use_protocol_contracts_without_raw_input() {
        let database = TestDbContext::new_sqlite("proxy-extractor-rejections.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                app_state.secret_encryption.reset_decrypt_call_count();
                let cases = [
                    (
                        "/openai/v1/models?private=%00",
                        DownstreamProtocol::Openai,
                        "invalid_request_error",
                    ),
                    (
                        "/responses/v1/models?private=%00",
                        DownstreamProtocol::Responses,
                        "invalid_request_error",
                    ),
                    (
                        "/anthropic/v1/models?private=%00",
                        DownstreamProtocol::Anthropic,
                        "invalid_request_error",
                    ),
                    (
                        "/gemini/v1/models?private=%00",
                        DownstreamProtocol::Gemini,
                        "INVALID_ARGUMENT",
                    ),
                ];

                for (path, protocol, category) in cases {
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, header::AUTHORIZATION.as_str(), None))
                        .await
                        .expect("query rejection should respond");
                    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
                    assert_public_cors_response(&response);
                    let body = assert_protocol_error_body(
                        response,
                        protocol,
                        "invalid_request_error",
                        category,
                    )
                    .await;
                    let serialized = body.to_string();
                    assert!(!serialized.contains("private"));
                    assert!(!serialized.contains("%00"));
                }

                let path = "/gemini/v1/models/%FF";
                let response = create_proxy_router(client_identity_resolver())
                    .with_state(Arc::clone(&app_state))
                    .oneshot(method_request(Method::POST, path))
                    .await
                    .expect("path rejection should respond");
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
                let body = assert_protocol_error_body(
                    response,
                    DownstreamProtocol::Gemini,
                    "invalid_request_error",
                    "INVALID_ARGUMENT",
                )
                .await;
                assert!(!body.to_string().contains("%FF"));

                app_state.flush_proxy_logs().await;
                assert!(
                    RequestLog::list_full(RequestLogQueryPayload::default())
                        .expect("request logs should be queryable")
                        .list
                        .is_empty(),
                    "extractor rejections must happen before request persistence"
                );
                assert_eq!(app_state.secret_encryption.decrypt_call_count(), 0);
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

    fn protocol_for_path(path: &str) -> DownstreamProtocol {
        if path.starts_with("/openai/") {
            DownstreamProtocol::Openai
        } else if path.starts_with("/responses/") {
            DownstreamProtocol::Responses
        } else if path.starts_with("/anthropic/") {
            DownstreamProtocol::Anthropic
        } else if path.starts_with("/gemini/") {
            DownstreamProtocol::Gemini
        } else {
            panic!("test path is not a protocol surface: {path}");
        }
    }

    async fn assert_protocol_error_body(
        response: axum::response::Response,
        protocol: DownstreamProtocol,
        expected_code: &str,
        expected_category: &str,
    ) -> serde_json::Value {
        let request_id = response
            .headers()
            .get(&X_REQUEST_ID)
            .and_then(|value| value.to_str().ok())
            .expect("protocol error should include request id")
            .to_string();
        let anthropic_request_id = response
            .headers()
            .get("request-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("protocol error body should read");
        let body: serde_json::Value =
            serde_json::from_slice(&body).expect("protocol error should be JSON");

        assert!(body.get("code").is_none());
        assert!(body.get("message").is_none());
        assert!(body.get("upstream_error").is_none());
        match protocol {
            DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                assert_eq!(body["error"]["code"], expected_code);
                assert_eq!(body["error"]["type"], expected_category);
                assert!(body["error"]["param"].is_null());
                assert!(anthropic_request_id.is_none());
            }
            DownstreamProtocol::Anthropic => {
                assert_eq!(body["type"], "error");
                assert_eq!(body["error"]["code"], expected_code);
                assert_eq!(body["error"]["type"], expected_category);
                assert_eq!(body["request_id"], request_id);
                assert_eq!(anthropic_request_id.as_deref(), Some(request_id.as_str()));
            }
            DownstreamProtocol::Gemini => {
                assert_eq!(body["error"]["status"], expected_category);
                assert_eq!(body["error"]["details"].as_array().map(Vec::len), Some(1));
                assert_eq!(
                    body["error"]["details"][0]["reason"],
                    expected_code.to_ascii_uppercase()
                );
                assert_eq!(
                    body["error"]["details"][0]["metadata"]["request_id"],
                    request_id
                );
                assert_eq!(
                    body["error"]["details"][0]["metadata"]["cyder_code"],
                    expected_code
                );
                assert!(anthropic_request_id.is_none());
            }
        }
        body
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
                    assert_eq!(wrong_method.headers().get(header::ALLOW).unwrap(), "POST");
                    let protocol = protocol_for_path(path);
                    let category = match protocol {
                        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                            "invalid_request_error"
                        }
                        DownstreamProtocol::Anthropic => "invalid_request_error",
                        DownstreamProtocol::Gemini => "UNIMPLEMENTED",
                    };
                    assert_protocol_error_body(
                        wrong_method,
                        protocol,
                        "method_not_allowed_error",
                        category,
                    )
                    .await;

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
                    assert_eq!(wrong_method.headers().get(header::ALLOW).unwrap(), "POST");
                    assert_protocol_error_body(
                        wrong_method,
                        DownstreamProtocol::Openai,
                        "method_not_allowed_error",
                        "invalid_request_error",
                    )
                    .await;
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
                    assert_eq!(
                        wrong_method.headers().get(header::ALLOW).unwrap(),
                        "GET,HEAD"
                    );
                    let protocol = protocol_for_path(path);
                    let category = match protocol {
                        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                            "invalid_request_error"
                        }
                        DownstreamProtocol::Anthropic => "invalid_request_error",
                        DownstreamProtocol::Gemini => "UNIMPLEMENTED",
                    };
                    assert_protocol_error_body(
                        wrong_method,
                        protocol,
                        "method_not_allowed_error",
                        category,
                    )
                    .await;
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
    async fn anthropic_public_surface_exposes_only_messages_and_local_models() {
        let database = TestDbContext::new_sqlite("proxy-anthropic-public-surface.sqlite");
        database
            .run_async(async {
                let created = ApiKey::create(&payload()).expect("proxy key should create");
                let api_key = created.reveal.api_key;
                let api_key_id = created.detail.id;
                let app_state = create_test_app_state(database.clone()).await;

                for path in ["/anthropic/models", "/anthropic/v1/models"] {
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request(path, "x-api-key", Some(&api_key)))
                        .await
                        .expect("Anthropic models route should respond");
                    assert_eq!(response.status(), StatusCode::OK, "{path}");
                    assert_proxy_security(&response);
                }
                for path in ["/anthropic/messages", "/anthropic/v1/messages"] {
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(method_request(Method::POST, path))
                        .await
                        .expect("Anthropic messages route should respond");
                    assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
                    assert_protocol_error_body(
                        response,
                        DownstreamProtocol::Anthropic,
                        "authentication_error",
                        "authentication_error",
                    )
                    .await;
                }

                app_state.secret_encryption.reset_decrypt_call_count();
                let before = app_state
                    .api_key_governance
                    .get_api_key_governance_snapshot(api_key_id)
                    .await
                    .expect("governance snapshot should load");

                for path in [
                    "/anthropic/messages/count_tokens",
                    "/anthropic/v1/messages/count_tokens",
                    "/anthropic/messages/batches",
                    "/anthropic/v1/messages/batches",
                    "/anthropic/message_batches",
                    "/anthropic/v1/message_batches",
                    "/anthropic/files",
                    "/anthropic/v1/files",
                    "/anthropic/v1/files/file_beta",
                    "/anthropic/v1beta/messages",
                    "/anthropic/v1beta/models",
                    "/anthropic/v1beta/files",
                ] {
                    let mut request = method_request(Method::POST, path);
                    request.headers_mut().insert(
                        "x-api-key",
                        HeaderValue::from_str(&api_key).expect("API key header should be valid"),
                    );
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request)
                        .await
                        .expect("unsupported Anthropic product route should respond");
                    assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
                    assert!(response.headers().get(header::ALLOW).is_none(), "{path}");
                    assert_protocol_error_body(
                        response,
                        DownstreamProtocol::Anthropic,
                        "route_not_found_error",
                        "not_found_error",
                    )
                    .await;
                }

                for (method, path, allow) in [
                    (Method::GET, "/anthropic/messages", "POST"),
                    (Method::PUT, "/anthropic/messages", "POST"),
                    (Method::DELETE, "/anthropic/v1/messages", "POST"),
                    (Method::POST, "/anthropic/models", "GET,HEAD"),
                    (Method::PUT, "/anthropic/v1/models", "GET,HEAD"),
                    (Method::DELETE, "/anthropic/v1/models", "GET,HEAD"),
                ] {
                    let mut request = method_request(method.clone(), path);
                    request.headers_mut().insert(
                        "x-api-key",
                        HeaderValue::from_str(&api_key).expect("API key header should be valid"),
                    );
                    let response = create_proxy_router(client_identity_resolver())
                        .with_state(Arc::clone(&app_state))
                        .oneshot(request)
                        .await
                        .expect("wrong Anthropic method should respond");
                    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED, "{path}");
                    assert_eq!(response.headers().get(header::ALLOW).unwrap(), allow);
                    assert_protocol_error_body(
                        response,
                        DownstreamProtocol::Anthropic,
                        "method_not_allowed_error",
                        "invalid_request_error",
                    )
                    .await;
                }

                app_state.flush_proxy_logs().await;
                assert!(
                    RequestLog::list_full(RequestLogQueryPayload::default())
                        .expect("request logs should be queryable")
                        .list
                        .is_empty(),
                    "route/auth/method rejections and local Models must not enter generation logging"
                );
                assert_eq!(
                    app_state.secret_encryption.decrypt_call_count(),
                    0,
                    "unsupported Anthropic surface must not resolve provider credentials"
                );
                let after = app_state
                    .api_key_governance
                    .get_api_key_governance_snapshot(api_key_id)
                    .await
                    .expect("governance snapshot should load");
                assert_eq!(after, before, "unsupported routes must not mutate governance");
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
                    let protocol = protocol_for_path(models_path);
                    let auth_category = match protocol {
                        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                            "authentication_error"
                        }
                        DownstreamProtocol::Anthropic => "authentication_error",
                        DownstreamProtocol::Gemini => "UNAUTHENTICATED",
                    };
                    assert_protocol_error_body(
                        auth_error,
                        protocol,
                        "authentication_error",
                        auth_category,
                    )
                    .await;

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
                    let governance_category = match protocol {
                        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                            "permission_error"
                        }
                        DownstreamProtocol::Anthropic => "permission_error",
                        DownstreamProtocol::Gemini => "PERMISSION_DENIED",
                    };
                    assert_protocol_error_body(
                        governance_error,
                        protocol,
                        "api_key_disabled_error",
                        governance_category,
                    )
                    .await;

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
                    let missing_category = match protocol {
                        DownstreamProtocol::Openai | DownstreamProtocol::Responses => {
                            "invalid_request_error"
                        }
                        DownstreamProtocol::Anthropic => "not_found_error",
                        DownstreamProtocol::Gemini => "NOT_FOUND",
                    };
                    assert_protocol_error_body(
                        missing,
                        protocol,
                        "route_not_found_error",
                        missing_category,
                    )
                    .await;

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
