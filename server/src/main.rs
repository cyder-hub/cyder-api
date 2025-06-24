use std::net::SocketAddr;

use config::CONFIG;
use controller::create_router;
use crate::service::app_state::{create_app_state, create_state_router}; // Import create_app_state

use cyder_tools::log::{info, LocalLogger};

mod config;
mod controller;
mod database;
mod schema;
mod utils;
mod service;

#[tokio::main]
async fn main() {
    LocalLogger::init(&CONFIG.log_level);
    let addr = format!("{}:{}", &CONFIG.host, CONFIG.port);
    info!("server start at {}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    let app_state = create_app_state();
    axum::serve(
        listener,
        create_state_router()
            .nest(&CONFIG.base_path, create_router())
            .with_state(app_state) // Call with_state before into_make_service
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .expect("failed to start server");
}
