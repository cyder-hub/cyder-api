use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::utils::auth::authorization_access_middleware;
use api_key::create_api_key_management_router;
use auth::create_auth_router;
use axum::{
    http::{self, HeaderValue, header::CACHE_CONTROL},
    middleware,
    response::IntoResponse,
};
use cost::create_cost_router;
use metrics::create_metrics_router;
use model::create_model_controller_router;
use portable_config::create_portable_config_router;
use provider::create_provider_router;
use provider_runtime::create_provider_runtime_router;
use reasoning_config::create_reasoning_config_router;
use request_log::create_record_router;
use request_patch::create_request_patch_router;
use runtime_feature_config::create_runtime_feature_config_router;
use stat::routes as create_stat_router;
use std::sync::Arc;

use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

mod auth;
mod cost;
mod error;
mod metrics;

mod api_key;
mod model;
mod portable_config;
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

pub fn create_manager_router(app_state: Arc<AppState>) -> StateRouter {
    let serve_dir = ServeDir::new("public").fallback(ServeFile::new("public/index.html"));
    let serve_vendor_dir = ServeDir::new("public/assets");

    let ui_router = create_state_router()
        .nest_service("/ui", serve_dir.clone())
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .nest_service("/ui/assets", serve_vendor_dir);

    let auth_router = create_auth_router(Arc::clone(&app_state));
    let no_store =
        SetResponseHeaderLayer::overriding(CACHE_CONTROL, HeaderValue::from_static("no-store"));
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
            .merge(create_portable_config_router())
            .layer(middleware::from_fn_with_state(
                app_state,
                authorization_access_middleware,
            ))
            .merge(auth_router)
            .layer(no_store),
    );

    create_state_router().nest(
        "/manager",
        create_state_router().merge(api_router).merge(ui_router),
    )
}

pub async fn handle_404() -> impl IntoResponse {
    (http::StatusCode::NOT_FOUND, "not found")
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, sync::Arc};

    use axum::{
        body::Body,
        extract::ConnectInfo,
        http::{Method, Request, StatusCode, header},
    };
    use serde_json::json;
    use tower::ServiceExt;

    use crate::database::TestDbContext;
    use crate::service::app_state::create_test_app_state;

    use super::create_manager_router;

    async fn send(
        app_state: &Arc<crate::service::app_state::AppState>,
        mut request: Request<Body>,
    ) -> axum::response::Response {
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 31_200))));
        create_manager_router(Arc::clone(app_state))
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("manager router should respond")
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
                    Some("no-cache, no-store, must-revalidate")
                );
            })
            .await;
    }
}
