use std::net::SocketAddr;

use crate::proxy::create_proxy_router;
use crate::service::app_state::{create_app_state, create_state_router};
use config::CONFIG;
use controller::{create_manager_router, create_system_router, handle_404}; // Import create_app_state

use cyder_tools::log::{LocalLogger, info};

mod config;
mod controller;
mod database;
mod proxy;
mod schema;
mod service;
mod utils;

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    cyder_tools::log::info!("Received shutdown signal, starting graceful shutdown...");
}

#[tokio::main]
async fn main() {
    LocalLogger::init(&CONFIG.log_level);
    let addr = format!("{}:{}", &CONFIG.host, CONFIG.port);
    info!("server start at {}", &addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    let app_state = create_app_state().await;
    axum::serve(
        listener,
        create_state_router()
            .nest(
                &CONFIG.base_path,
                create_state_router()
                    .merge(create_system_router())
                    .merge(create_manager_router())
                    .merge(create_proxy_router())
                    .fallback(handle_404),
            )
            .with_state(app_state) // Call with_state before into_make_service
            .into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal())
    .await
    .expect("failed to start server");

    cyder_tools::log::info!("Server shut down. Waiting for background tasks to finish...");
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    cyder_tools::log::info!("Shutdown complete.");
}
