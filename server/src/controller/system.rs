use std::sync::Arc;

use crate::service::app_state::{AppState, StateRouter};
use axum::{Json, extract::State, response::IntoResponse, routing::get};
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

fn readiness_status(
    database_status: &str,
    redis_status: Option<&str>,
) -> (&'static str, axum::http::StatusCode) {
    if database_status == "ok" && redis_status != Some("error") {
        ("ok", axum::http::StatusCode::OK)
    } else {
        ("error", axum::http::StatusCode::SERVICE_UNAVAILABLE)
    }
}

async fn ready_handler(State(app_state): State<Arc<AppState>>) -> impl IntoResponse {
    let mut db_status = "ok";
    if let Err(e) = app_state.database.readiness_probe().await {
        crate::warn_event!(
            "system.readiness_degraded",
            component = "database",
            error = format!("{e:?}"),
        );
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
                    crate::warn_event!(
                        "system.readiness_degraded",
                        component = "redis_ping",
                        error = e,
                    );
                    redis_status = Some("error".to_string());
                }
            }
            Err(e) => {
                crate::warn_event!(
                    "system.readiness_degraded",
                    component = "redis_connection",
                    error = e,
                );
                redis_status = Some("error".to_string());
            }
        }
    }

    let (overall_status, status_code) = readiness_status(db_status, redis_status.as_deref());

    let response = ReadyResponse {
        status: overall_status.to_string(),
        database: db_status.to_string(),
        redis: redis_status,
    };

    (status_code, Json(response))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{body::to_bytes, extract::State, http::StatusCode, response::IntoResponse};
    use serde_json::json;
    use tokio::sync::Semaphore;

    use crate::config::CONFIG;
    use crate::database::TestDatabase;
    use crate::database::runtime::DatabaseWorkload;
    use crate::service::app_state::create_test_app_state;

    use super::{health_handler, readiness_status, ready_handler};

    #[tokio::test]
    async fn system_health_and_ready_responses_preserve_the_public_json_contract() {
        let app_state = create_test_app_state(
            TestDatabase::new_sqlite_default("system-ready-contract.sqlite").await,
        )
        .await;
        let health = health_handler().await.into_response();
        assert_eq!(health.status(), StatusCode::OK);
        let health_body = to_bytes(health.into_body(), usize::MAX)
            .await
            .expect("health response body should read");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&health_body)
                .expect("health response should be JSON"),
            json!({"status": "ok"})
        );

        let ready = ready_handler(State(app_state)).await.into_response();
        assert_eq!(ready.status(), StatusCode::OK);
        let ready_body = to_bytes(ready.into_body(), usize::MAX)
            .await
            .expect("ready response body should read");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&ready_body)
                .expect("ready response should be JSON"),
            json!({"status": "ok", "database": "ok", "redis": null})
        );
    }

    #[test]
    fn readiness_status_combines_database_and_redis_without_changing_contract() {
        assert_eq!(readiness_status("ok", None), ("ok", StatusCode::OK));
        assert_eq!(readiness_status("ok", Some("ok")), ("ok", StatusCode::OK));
        assert_eq!(
            readiness_status("error", None),
            ("error", StatusCode::SERVICE_UNAVAILABLE)
        );
        assert_eq!(
            readiness_status("ok", Some("error")),
            ("error", StatusCode::SERVICE_UNAVAILABLE)
        );
        assert_eq!(
            readiness_status("error", Some("error")),
            ("error", StatusCode::SERVICE_UNAVAILABLE)
        );
    }

    #[tokio::test]
    async fn readiness_rejects_saturation_without_queueing_and_recovers_after_release() {
        let app_state = create_test_app_state(
            TestDatabase::new_sqlite_default("system-ready-saturation.sqlite").await,
        )
        .await;
        let started = Arc::new(Semaphore::new(0));
        let release = Arc::new(Semaphore::new(0));
        let mut holders = Vec::new();
        for _ in 0..CONFIG.db_pool_size {
            let database = Arc::clone(&app_state.database);
            let started = Arc::clone(&started);
            let release = Arc::clone(&release);
            holders.push(tokio::spawn(async move {
                database
                    .run(DatabaseWorkload::Foreground, move |_connection| {
                        Box::pin(async move {
                            started.add_permits(1);
                            release
                                .acquire()
                                .await
                                .expect("release semaphore should remain open")
                                .forget();
                            Ok(())
                        })
                    })
                    .await
            }));
        }
        started
            .acquire_many(CONFIG.db_pool_size)
            .await
            .expect("all database holders should start")
            .forget();
        assert_eq!(app_state.database.snapshot().waiting, 0);

        let unavailable = ready_handler(State(Arc::clone(&app_state)))
            .await
            .into_response();
        assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
        let unavailable_body = to_bytes(unavailable.into_body(), usize::MAX)
            .await
            .expect("unavailable readiness body should read");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&unavailable_body)
                .expect("unavailable readiness should be JSON"),
            json!({"status": "error", "database": "error", "redis": null})
        );
        assert_eq!(app_state.database.snapshot().waiting, 0);
        assert_eq!(
            health_handler().await.into_response().status(),
            StatusCode::OK,
            "liveness must remain independent from database readiness"
        );
        app_state
            .catalog
            .get_models_catalog()
            .await
            .expect("warm catalog reads must not be globally gated by readiness");

        release.add_permits(CONFIG.db_pool_size as usize);
        for holder in holders {
            holder
                .await
                .expect("database holder should join")
                .expect("database holder should complete");
        }
        let recovered = ready_handler(State(app_state)).await.into_response();
        assert_eq!(recovered.status(), StatusCode::OK);
    }
}
