use config::CONFIG;
use controller::create_router;

use axum::Router;
use cyder_tools::log::{info, LocalLogger};

mod config;
mod controller;
mod database;
mod schema;
mod utils;

#[tokio::main]
async fn main() {
    LocalLogger::init();
    let addr = format!("{}:{}", &CONFIG.host, CONFIG.port);
    info!("server start at {}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        Router::new().nest(&CONFIG.base_path, create_router()),
    )
    .await
    .expect("failed to start server");
}
