use config::CONFIG;
use controller::create_router;

use axum::Router;

mod config;
mod controller;
mod database;
mod schema;
mod utils;

#[tokio::main]
async fn main() {
    let addr = format!("{}:{}", &CONFIG.host, CONFIG.port);
    println!("server start at {}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(
        listener,
        Router::new().nest(&CONFIG.base_path, create_router()),
    )
    .await
    .expect("failed to start server");
}
