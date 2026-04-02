use crate::service::app_state::StateRouter;
use axum::{Json, response::IntoResponse, routing::get};
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

#[derive(Serialize)]
struct ReadyResponse {
    status: String,
    database: String,
    redis: Option<String>,
}

pub fn create_system_router() -> StateRouter {
    crate::service::app_state::create_state_router()
        .route("/health", get(health_handler))
        .route("/ready", get(ready_handler))
}

async fn health_handler() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

async fn ready_handler() -> impl IntoResponse {
    let mut db_status = "ok";
    if let Err(e) = crate::database::get_connection() {
        cyder_tools::log::error!("Readiness check: Database connection failed: {:?}", e);
        db_status = "error";
    }

    let mut redis_status = None;
    if let Some(pool) = crate::service::redis::get_pool().await {
        redis_status = Some("ok".to_string());
        match pool.get().await {
            Ok(mut conn) => {
                if let Err(e) = bb8_redis::redis::cmd("PING")
                    .query_async::<()>(&mut *conn)
                    .await
                {
                    cyder_tools::log::error!("Readiness check: Redis PING failed: {}", e);
                    redis_status = Some("error".to_string());
                }
            }
            Err(e) => {
                cyder_tools::log::error!("Readiness check: Failed to get Redis connection: {}", e);
                redis_status = Some("error".to_string());
            }
        }
    }

    let overall_status = if db_status == "ok" && redis_status.as_deref() != Some("error") {
        "ok"
    } else {
        "error"
    };

    let response = ReadyResponse {
        status: overall_status.to_string(),
        database: db_status.to_string(),
        redis: redis_status,
    };

    let status_code = if overall_status == "ok" {
        axum::http::StatusCode::OK
    } else {
        axum::http::StatusCode::SERVICE_UNAVAILABLE
    };

    (status_code, Json(response))
}
