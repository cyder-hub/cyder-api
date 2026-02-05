use crate::utils::auth::authorization_access_middleware;
use auth::create_auth_router;
use custom_field::create_custom_field_router; // Add this import
use access_control::create_access_control_policy_router;
use price::create_price_router;
use stat::routes as create_stat_router;
use system_api_key::create_api_key_router;
use crate::service::app_state::{create_state_router, StateRouter};
// Removed duplicate: use stat::routes as create_stat_router; // Add this import
use axum::{
    http::{self, header::CACHE_CONTROL, HeaderValue},
    middleware,
    response::IntoResponse,
};
use model::create_model_router;
use model_alias::create_model_alias_router;
use provider::create_provider_router;
use request_log::create_record_router;

use tower_http::{services::{ServeDir, ServeFile}, set_header::SetResponseHeaderLayer};

mod auth;
mod custom_field; // Add this module declaration
mod error;

mod access_control;
mod model;
mod model_alias;
mod provider;
mod request_log;
mod stat; // Add this module declaration
mod system_api_key;
mod price;

pub use error::BaseError;

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
            .merge(create_api_key_router())
            .merge(create_model_router())
            .merge(create_model_alias_router())
            .merge(create_access_control_policy_router())
            .merge(create_custom_field_router()) // Add this line
            .merge(create_price_router())
            .merge(create_stat_router()) // Add this line
            .layer(middleware::from_fn(authorization_access_middleware))
            .merge(create_auth_router()),
    );

    create_state_router().nest("/manager", create_state_router().merge(api_router).merge(ui_router))
}

pub async fn handle_404() -> impl IntoResponse {
    (http::StatusCode::NOT_FOUND, "not found")
}
