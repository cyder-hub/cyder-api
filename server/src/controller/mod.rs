use crate::service::app_state::{StateRouter, create_state_router};
use crate::utils::auth::authorization_access_middleware;
use api_key::create_api_key_management_router;
use auth::create_auth_router;
use axum::{
    http::{self, HeaderValue, header::CACHE_CONTROL},
    middleware,
    response::IntoResponse,
};
use cost::create_cost_router;
use model::create_model_router;
use model_route::create_model_route_router;
use provider::create_provider_router;
use provider_runtime::create_provider_runtime_router;
use request_log::create_record_router;
use request_patch::create_request_patch_router;
use stat::routes as create_stat_router;

use tower_http::{
    services::{ServeDir, ServeFile},
    set_header::SetResponseHeaderLayer,
};

mod auth;
mod cost;
mod error;

mod api_key;
mod model;
mod model_route;
mod provider;
mod provider_runtime;
mod request_log;
mod request_patch;
mod stat;
mod system;

pub use error::BaseError;
pub use system::create_system_router;

pub fn create_manager_router() -> StateRouter {
    let serve_dir = ServeDir::new("public").fallback(ServeFile::new("public/index.html"));
    let serve_vendor_dir = ServeDir::new("public/assets");

    let ui_router = create_state_router()
        .nest_service("/ui", serve_dir.clone())
        .layer(SetResponseHeaderLayer::overriding(
            CACHE_CONTROL,
            HeaderValue::from_static("no-cache, no-store, must-revalidate"),
        ))
        .nest_service("/ui/assets", serve_vendor_dir);

    let api_router = create_state_router().nest(
        "/api",
        create_state_router()
            .merge(create_record_router())
            .merge(create_provider_router())
            .merge(create_provider_runtime_router())
            .merge(create_api_key_management_router())
            .merge(create_model_router())
            .merge(create_model_route_router())
            .merge(create_request_patch_router())
            .merge(create_cost_router())
            .merge(create_stat_router())
            .layer(middleware::from_fn(authorization_access_middleware))
            .merge(create_auth_router()),
    );

    create_state_router().nest(
        "/manager",
        create_state_router().merge(api_router).merge(ui_router),
    )
}

pub async fn handle_404() -> impl IntoResponse {
    (http::StatusCode::NOT_FOUND, "not found")
}
