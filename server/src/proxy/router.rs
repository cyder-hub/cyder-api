use std::{collections::HashMap, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{HeaderName, HeaderValue, Method, header::CACHE_CONTROL},
    middleware,
    routing::{MethodRouter, any, get},
};
use tower_http::cors::{AllowHeaders, Any, CorsLayer};
use tower_http::set_header::SetResponseHeaderLayer;

use crate::{
    ingress::client_identity::{ClientIdentityResolver, proxy_client_identity_middleware},
    schema::enum_def::LlmApiType,
    service::app_state::{AppState, StateRouter, create_state_router},
};

use super::gemini::handle_gemini_request;
use super::handlers::{list_models_handler, openai_utility_handler};
use super::unified::unified_proxy_handler;

type QueryParams = HashMap<String, String>;

fn generation_route(api_type: LlmApiType) -> MethodRouter<Arc<AppState>> {
    any(
        move |State(app_state), Query(query_params): Query<QueryParams>, request: Request<Body>| async move {
            unified_proxy_handler(app_state, query_params, api_type, request).await
        },
    )
}

fn openai_utility_route(downstream_path: &'static str) -> MethodRouter<Arc<AppState>> {
    any(
        move |State(app_state), Query(params): Query<QueryParams>, request: Request<Body>| async move {
            openai_utility_handler(app_state, params, request, downstream_path).await
        },
    )
}

fn models_route(api_type: LlmApiType) -> MethodRouter<Arc<AppState>> {
    get(
        move |State(app_state), Query(params): Query<QueryParams>, request: Request<Body>| async move {
            list_models_handler(app_state, params, request, api_type).await
        },
    )
}

fn add_generation_routes(
    router: StateRouter,
    api_type: LlmApiType,
    paths: &[&'static str],
) -> StateRouter {
    paths.iter().fold(router, |router, path| {
        router.route(path, generation_route(api_type))
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
        LlmApiType::Openai,
        &["/chat/completions"],
    );
    let router = add_openai_utility_routes(
        router,
        &[("/embeddings", "embeddings"), ("/rerank", "rerank")],
    )
    .route("/models", models_route(LlmApiType::Openai));

    nest_router_variants(router, true, &["/v1"])
}

fn create_anthropic_router() -> StateRouter {
    let router =
        add_generation_routes(create_state_router(), LlmApiType::Anthropic, &["/messages"])
            .route("/models", models_route(LlmApiType::Anthropic));

    nest_router_variants(router, true, &["/v1"])
}

fn create_ollama_router() -> StateRouter {
    add_generation_routes(
        create_state_router(),
        LlmApiType::Ollama,
        &["/api/chat", "/api/generate", "/api/embeddings"],
    )
    .route("/api/tags", models_route(LlmApiType::Ollama))
}

fn create_responses_router() -> StateRouter {
    let router = add_generation_routes(
        create_state_router(),
        LlmApiType::Responses,
        &["/responses"],
    )
    .route("/models", models_route(LlmApiType::Responses));

    nest_router_variants(router, true, &["/v1"])
}

fn create_gemini_router() -> StateRouter {
    let router = create_state_router()
        .route("/models", models_route(LlmApiType::Gemini))
        .route(
            "/models/{*model_action_segment}",
            any(
                |Path(path_segment): Path<String>,
                 Query(query_params): Query<QueryParams>,
                 State(app_state),
                 request: Request<Body>| async move {
                    handle_gemini_request(app_state, path_segment, query_params, request).await
                },
            ),
        );

    nest_router_variants(router, false, &["/v1beta", "/v1"])
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
        .nest("/ollama", create_ollama_router())
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
    };
    use tower::ServiceExt;

    use crate::config::ClientIdentityConfig;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::{DbConnection, TestDbContext, get_connection};
    use crate::ingress::client_identity::ClientIdentityResolver;
    use crate::schema::enum_def::Action;
    use crate::service::admin::auth::LoginError;
    use crate::service::app_state::create_test_app_state;
    use diesel::RunQueryDsl;

    use super::create_proxy_router;

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
    async fn all_proxy_protocols_override_cache_control_on_success_and_auth_error() {
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
                    ("/ollama/api/tags", header::AUTHORIZATION.as_str()),
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
                    ("/ollama/api/tags", header::AUTHORIZATION.as_str()),
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
    async fn proxy_public_cors_covers_five_protocols_errors_404_and_preflight() {
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
                    (
                        "/ollama/api/tags",
                        "/ollama/missing",
                        "/ollama/api/chat",
                        header::AUTHORIZATION.as_str(),
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
