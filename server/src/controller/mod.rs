use crate::ingress::{
    client_identity::{ClientIdentityResolver, manager_client_identity_middleware},
    web_security::manager_web_security_middleware,
};
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::utils::auth::authorization_access_middleware;
use api_key::create_api_key_management_router;
use auth::create_auth_router;
use axum::{http, middleware, response::IntoResponse};
use cost::create_cost_router;
use metrics::create_metrics_router;
use model::create_model_controller_router;
use provider::create_provider_router;
use provider_runtime::create_provider_runtime_router;
use reasoning_config::create_reasoning_config_router;
use request_log::create_record_router;
use request_patch::create_request_patch_router;
use runtime_feature_config::create_runtime_feature_config_router;
use stat::routes as create_stat_router;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tower_http::services::{ServeDir, ServeFile};

mod auth;
mod cost;
mod error;
mod metrics;

mod api_key;
mod model;
mod provider;
mod provider_runtime;
mod reasoning_config;
mod request_log;
mod request_patch;
mod runtime_feature_config;
mod stat;
mod system;

pub use error::BaseError;
pub use system::create_system_router;

pub fn create_manager_router(
    app_state: Arc<AppState>,
    client_identity_resolver: Arc<ClientIdentityResolver>,
) -> StateRouter {
    create_manager_router_with_public_dir(app_state, client_identity_resolver, "public")
}

fn create_manager_router_with_public_dir(
    app_state: Arc<AppState>,
    client_identity_resolver: Arc<ClientIdentityResolver>,
    public_dir: impl AsRef<Path>,
) -> StateRouter {
    let public_dir = PathBuf::from(public_dir.as_ref());
    let serve_dir =
        ServeDir::new(&public_dir).fallback(ServeFile::new(public_dir.join("index.html")));
    let serve_vendor_dir = ServeDir::new(public_dir.join("assets"));

    let ui_router = create_state_router()
        .nest_service("/ui", serve_dir.clone())
        .nest_service("/ui/assets", serve_vendor_dir);

    let auth_router = create_auth_router(Arc::clone(&app_state));
    let api_router = create_state_router().nest(
        "/api",
        create_state_router()
            .merge(create_record_router())
            .merge(create_provider_router())
            .merge(create_provider_runtime_router())
            .merge(create_api_key_management_router())
            .merge(create_model_controller_router())
            .merge(create_request_patch_router())
            .merge(create_reasoning_config_router())
            .merge(create_runtime_feature_config_router())
            .merge(create_cost_router())
            .merge(create_metrics_router())
            .merge(create_stat_router())
            .layer(middleware::from_fn_with_state(
                app_state,
                authorization_access_middleware,
            ))
            .merge(auth_router),
    );

    create_state_router().nest(
        "/manager",
        create_state_router()
            .merge(api_router)
            .merge(ui_router)
            .fallback(handle_404)
            .layer(middleware::from_fn_with_state(
                client_identity_resolver,
                manager_client_identity_middleware,
            ))
            .layer(middleware::from_fn(manager_web_security_middleware)),
    )
}

pub async fn handle_404() -> impl IntoResponse {
    (http::StatusCode::NOT_FOUND, "not found")
}

#[cfg(test)]
mod tests {
    use std::{fs, net::SocketAddr, path::Path, sync::Arc};

    use axum::{
        body::{Body, to_bytes},
        extract::ConnectInfo,
        http::{HeaderValue, Method, Request, StatusCode, header},
    };
    use serde_json::json;
    use tower::ServiceExt;

    use crate::service::app_state::create_test_app_state;
    use crate::{
        config::ClientIdentityConfig,
        database::TestDbContext,
        ingress::client_identity::ClientIdentityResolver,
        ingress::web_security::{MANAGER_CONTENT_SECURITY_POLICY, MANAGER_PERMISSIONS_POLICY},
    };

    use super::{
        create_manager_router, create_manager_router_with_public_dir, create_system_router,
    };

    async fn send(
        app_state: &Arc<crate::service::app_state::AppState>,
        request: Request<Body>,
    ) -> axum::response::Response {
        send_with_resolver(
            app_state,
            request,
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
            Some(SocketAddr::from(([127, 0, 0, 1], 31_200))),
        )
        .await
    }

    async fn send_with_resolver(
        app_state: &Arc<crate::service::app_state::AppState>,
        mut request: Request<Body>,
        resolver: Arc<ClientIdentityResolver>,
        peer_addr: Option<SocketAddr>,
    ) -> axum::response::Response {
        if let Some(peer_addr) = peer_addr {
            request.extensions_mut().insert(ConnectInfo(peer_addr));
        }
        create_manager_router(Arc::clone(app_state), resolver)
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("manager router should respond")
    }

    #[tokio::test]
    async fn client_identity_http_manager_fails_closed_and_ignores_untrusted_metadata() {
        let database = TestDbContext::new_sqlite("manager-client-identity-http.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let resolver = Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig {
                    trusted_proxy_cidrs: vec![
                        "10.0.0.0/8".parse().expect("test CIDR should parse"),
                    ],
                    max_forwarded_hops: 8,
                }));

                let missing = send_with_resolver(
                    &app_state,
                    request(
                        Method::GET,
                        "/manager/api/auth/bootstrap/status",
                        None,
                        Body::empty(),
                    ),
                    Arc::clone(&resolver),
                    None,
                )
                .await;
                assert_eq!(missing.status(), StatusCode::INTERNAL_SERVER_ERROR);
                assert_manager_security(&missing);
                assert_no_store(&missing);
                let body = to_bytes(missing.into_body(), usize::MAX)
                    .await
                    .expect("manager error body should read");
                let body: serde_json::Value =
                    serde_json::from_slice(&body).expect("manager error should be JSON");
                assert_eq!(body["code"], 0);

                let mut invalid_request = request(
                    Method::GET,
                    "/manager/api/auth/bootstrap/status",
                    None,
                    Body::empty(),
                );
                invalid_request
                    .headers_mut()
                    .insert("forwarded", HeaderValue::from_static("not-valid"));
                let invalid = send_with_resolver(
                    &app_state,
                    invalid_request,
                    Arc::clone(&resolver),
                    Some(SocketAddr::from(([10, 0, 0, 9], 31_201))),
                )
                .await;
                assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
                assert_manager_security(&invalid);
                assert_no_store(&invalid);
                let body = to_bytes(invalid.into_body(), usize::MAX)
                    .await
                    .expect("manager error body should read");
                let body: serde_json::Value =
                    serde_json::from_slice(&body).expect("manager error should be JSON");
                assert_eq!(body["code"], 1001);

                let mut forged_request = request(
                    Method::GET,
                    "/manager/api/auth/bootstrap/status",
                    None,
                    Body::empty(),
                );
                forged_request
                    .headers_mut()
                    .insert("forwarded", HeaderValue::from_static("not-valid"));
                let ignored = send_with_resolver(
                    &app_state,
                    forged_request,
                    resolver,
                    Some(SocketAddr::from(([192, 0, 2, 9], 31_202))),
                )
                .await;
                assert_eq!(ignored.status(), StatusCode::OK);

                let system = create_system_router()
                    .with_state(Arc::clone(&app_state))
                    .oneshot(
                        Request::builder()
                            .method(Method::GET)
                            .uri("/health")
                            .body(Body::empty())
                            .expect("system request should build"),
                    )
                    .await
                    .expect("system router should respond without ConnectInfo");
                assert_eq!(system.status(), StatusCode::OK);
                assert!(
                    system
                        .headers()
                        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                        .is_none()
                );
            })
            .await;
    }

    fn request(method: Method, uri: &str, token: Option<&str>, body: Body) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        builder.body(body).expect("manager request should build")
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

    fn assert_manager_security(response: &axum::response::Response) {
        let expected = [
            ("x-content-type-options", "nosniff"),
            ("x-frame-options", "DENY"),
            ("referrer-policy", "no-referrer"),
            ("permissions-policy", MANAGER_PERMISSIONS_POLICY),
            ("content-security-policy", MANAGER_CONTENT_SECURITY_POLICY),
        ];
        for (name, value) in expected {
            assert_eq!(
                response
                    .headers()
                    .get(name)
                    .and_then(|header| header.to_str().ok()),
                Some(value),
                "unexpected {name}"
            );
        }
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-credentials")
                .is_none()
        );
    }

    fn assert_cache_control(response: &axum::response::Response, expected: &str) {
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .and_then(|value| value.to_str().ok()),
            Some(expected)
        );
    }

    async fn send_with_public_dir(
        app_state: &Arc<crate::service::app_state::AppState>,
        mut request: Request<Body>,
        public_dir: &Path,
    ) -> axum::response::Response {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_203))));
        create_manager_router_with_public_dir(
            Arc::clone(app_state),
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
            public_dir,
        )
        .with_state(Arc::clone(app_state))
        .oneshot(request)
        .await
        .expect("manager router should respond")
    }

    async fn send_with_public_dir_at_base_path(
        app_state: &Arc<crate::service::app_state::AppState>,
        mut request: Request<Body>,
        public_dir: &Path,
    ) -> axum::response::Response {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_204))));
        let manager_router = create_manager_router_with_public_dir(
            Arc::clone(app_state),
            Arc::new(ClientIdentityResolver::new(&ClientIdentityConfig::default())),
            public_dir,
        );
        crate::service::app_state::create_state_router()
            .nest("/ai", manager_router)
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("base-path manager router should respond")
    }

    #[tokio::test]
    async fn manager_api_overrides_cache_control_for_get_secret_post_and_errors() {
        let database = TestDbContext::new_sqlite("manager-api-no-store.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let tokens = app_state
                    .admin
                    .auth
                    .bootstrap("manager no-store contract password")
                    .await
                    .expect("manager bootstrap should succeed");

                let response = send(
                    &app_state,
                    request(
                        Method::GET,
                        "/manager/api/api_key/list",
                        Some(&tokens.access_token),
                        Body::empty(),
                    ),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_no_store(&response);

                let response = send(
                    &app_state,
                    request(
                        Method::POST,
                        "/manager/api/api_key",
                        Some(&tokens.access_token),
                        Body::from(
                            serde_json::to_vec(&json!({
                                "name": "manager-no-store-secret",
                                "default_action": "ALLOW"
                            }))
                            .expect("create payload should serialize"),
                        ),
                    ),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_no_store(&response);

                let response = send(
                    &app_state,
                    request(
                        Method::GET,
                        "/manager/api/api_key/list",
                        None,
                        Body::empty(),
                    ),
                )
                .await;
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
                assert_no_store(&response);

                let response = send(
                    &app_state,
                    request(Method::GET, "/manager/ui/missing", None, Body::empty()),
                )
                .await;
                assert_eq!(
                    response
                        .headers()
                        .get(header::CACHE_CONTROL)
                        .and_then(|value| value.to_str().ok()),
                    Some("no-store")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn manager_web_security_applies_exact_headers_cache_matrix_and_no_cors() {
        let database = TestDbContext::new_sqlite("manager-web-security.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let public_dir = tempfile::tempdir().expect("manager public fixture should create");
                fs::create_dir(public_dir.path().join("assets"))
                    .expect("asset fixture directory should create");
                fs::write(
                    public_dir.path().join("index.html"),
                    "<!doctype html><html><body><div id=\"app\"></div></body></html>",
                )
                .expect("index fixture should write");
                fs::write(
                    public_dir.path().join("assets/app-ABC123.js"),
                    "console.log('fixture')",
                )
                .expect("fingerprinted asset fixture should write");
                fs::write(public_dir.path().join("robots.txt"), "User-agent: *")
                    .expect("static fixture should write");

                let cases = [
                    (
                        Method::GET,
                        "/manager/ui/",
                        Body::empty(),
                        StatusCode::OK,
                        "no-cache, no-store, must-revalidate",
                    ),
                    (
                        Method::GET,
                        "/manager/ui/spa/route",
                        Body::empty(),
                        StatusCode::OK,
                        "no-cache, no-store, must-revalidate",
                    ),
                    (
                        Method::GET,
                        "/manager/ui/assets/app-ABC123.js",
                        Body::empty(),
                        StatusCode::OK,
                        "public, max-age=31536000, immutable",
                    ),
                    (
                        Method::GET,
                        "/manager/ui/robots.txt",
                        Body::empty(),
                        StatusCode::OK,
                        "no-cache",
                    ),
                    (
                        Method::GET,
                        "/manager/ui/assets/missing.js",
                        Body::empty(),
                        StatusCode::NOT_FOUND,
                        "no-store",
                    ),
                    (
                        Method::GET,
                        "/manager/api/auth/bootstrap/status",
                        Body::empty(),
                        StatusCode::OK,
                        "no-store",
                    ),
                    (
                        Method::GET,
                        "/manager/api/system/overview",
                        Body::empty(),
                        StatusCode::UNAUTHORIZED,
                        "no-store",
                    ),
                    (
                        Method::POST,
                        "/manager/api/auth/login",
                        Body::from("{"),
                        StatusCode::UNPROCESSABLE_ENTITY,
                        "no-store",
                    ),
                    (
                        Method::GET,
                        "/manager/api/missing",
                        Body::empty(),
                        StatusCode::NOT_FOUND,
                        "no-store",
                    ),
                    (
                        Method::GET,
                        "/manager/missing",
                        Body::empty(),
                        StatusCode::NOT_FOUND,
                        "no-store",
                    ),
                ];

                for (method, uri, body, status, cache_control) in cases {
                    let response = send_with_public_dir(
                        &app_state,
                        Request::builder()
                            .method(method)
                            .uri(uri)
                            .header(header::CONTENT_TYPE, "application/json")
                            .header(header::ORIGIN, "http://127.0.0.1:29528")
                            .header("sec-fetch-site", "same-origin")
                            .header("x-cyder-manager-auth", "1")
                            .body(body)
                            .expect("manager matrix request should build"),
                        public_dir.path(),
                    )
                    .await;
                    assert_eq!(response.status(), status, "{uri}");
                    assert_manager_security(&response);
                    assert_cache_control(&response, cache_control);
                }

                let preflight = send_with_public_dir(
                    &app_state,
                    Request::builder()
                        .method(Method::OPTIONS)
                        .uri("/manager/api/auth/login")
                        .header(header::ORIGIN, "https://external.example")
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                        .body(Body::empty())
                        .expect("manager preflight request should build"),
                    public_dir.path(),
                )
                .await;
                assert_eq!(preflight.status(), StatusCode::METHOD_NOT_ALLOWED);
                assert_manager_security(&preflight);
                assert_no_store(&preflight);
            })
            .await;
    }

    #[tokio::test]
    async fn manager_web_security_preserves_cache_policy_under_configured_base_path() {
        let database = TestDbContext::new_sqlite("manager-web-security-base-path.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                app_state
                    .admin
                    .auth
                    .bootstrap("manager base path no-store password")
                    .await
                    .expect("manager bootstrap should succeed");

                let public_dir = tempfile::tempdir().expect("manager public fixture should create");
                fs::create_dir(public_dir.path().join("assets"))
                    .expect("asset fixture directory should create");
                fs::write(
                    public_dir.path().join("assets/app-ABC123.js"),
                    "console.log('fixture')",
                )
                .expect("fingerprinted asset fixture should write");

                let login = send_with_public_dir_at_base_path(
                    &app_state,
                    Request::builder()
                        .method(Method::POST)
                        .uri("/ai/manager/api/auth/login")
                        .header(header::CONTENT_TYPE, "application/json")
                        .header(header::ORIGIN, "http://127.0.0.1:29528")
                        .header("sec-fetch-site", "same-origin")
                        .header("x-cyder-manager-auth", "1")
                        .body(Body::from(
                            r#"{"password":"manager base path no-store password"}"#,
                        ))
                        .expect("manager login request should build"),
                    public_dir.path(),
                )
                .await;
                assert_eq!(login.status(), StatusCode::OK);
                assert_manager_security(&login);
                assert_no_store(&login);
                let login_body = to_bytes(login.into_body(), usize::MAX)
                    .await
                    .expect("manager login body should read");
                let login_body: serde_json::Value =
                    serde_json::from_slice(&login_body).expect("manager login body should be JSON");
                assert!(login_body["data"]["access_token"].as_str().is_some());

                let asset = send_with_public_dir_at_base_path(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri("/ai/manager/ui/assets/app-ABC123.js")
                        .body(Body::empty())
                        .expect("manager asset request should build"),
                    public_dir.path(),
                )
                .await;
                assert_eq!(asset.status(), StatusCode::OK);
                assert_manager_security(&asset);
                assert_cache_control(&asset, "public, max-age=31536000, immutable");
            })
            .await;
    }

    #[tokio::test]
    async fn manager_router_does_not_register_portable_config_routes() {
        let database = TestDbContext::new_sqlite("manager-portable-routes-removed.sqlite");
        database
            .run_async(async {
                let app_state = create_test_app_state(database.clone()).await;
                let tokens = app_state
                    .admin
                    .auth
                    .bootstrap("manager removed portable route password")
                    .await
                    .expect("manager bootstrap should succeed");

                for (method, uri) in [
                    (Method::GET, "/manager/api/system/portable/modules"),
                    (Method::POST, "/manager/api/system/portable/export"),
                    (Method::POST, "/manager/api/system/portable/import/preview"),
                    (Method::POST, "/manager/api/system/portable/import/apply"),
                ] {
                    let response = send(
                        &app_state,
                        request(method, uri, Some(&tokens.access_token), Body::empty()),
                    )
                    .await;
                    assert_eq!(response.status(), StatusCode::NOT_FOUND);
                }
            })
            .await;
    }
}
