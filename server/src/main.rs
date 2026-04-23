use std::net::SocketAddr;

use cyder_api::config::CONFIG;
use cyder_api::controller::{create_manager_router, create_system_router, handle_404};
use cyder_api::logging::{self, THIRD_PARTY_DEBUG_ENV};
use cyder_api::proxy::{create_proxy_router, flush_proxy_logs};
use cyder_api::service::app_state::{create_app_state, create_state_router};

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
    cyder_api::info_event!(
        "startup.shutdown_signal_received",
        action = "graceful_shutdown"
    );
}

#[tokio::main]
async fn main() {
    logging::init(&CONFIG.log_level);
    let addr = format!("{}:{}", &CONFIG.host, CONFIG.port);
    if matches!(
        CONFIG.log_level.to_ascii_lowercase().as_str(),
        "debug" | "trace"
    ) && std::env::var(THIRD_PARTY_DEBUG_ENV).is_err()
    {
        cyder_api::info_event!(
            "startup.third_party_debug_muted",
            env = THIRD_PARTY_DEBUG_ENV,
            log_level = &CONFIG.log_level,
        );
    }
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind server listener");
    let target_addr = listener
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or(addr.clone());
    let app_state = create_app_state().await;
    cyder_api::info_event!(
        "startup.server_started",
        target_addr = &target_addr,
        base_path = &CONFIG.base_path,
        log_level = &CONFIG.log_level,
    );
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

    cyder_api::info_event!(
        "startup.server_shutdown_waiting",
        background_task = "proxy_logs_flush",
    );
    flush_proxy_logs().await;
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    cyder_api::info_event!(
        "startup.server_shutdown_complete",
        background_task = "proxy_logs_flush",
    );
}
