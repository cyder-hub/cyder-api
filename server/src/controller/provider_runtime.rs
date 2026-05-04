use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
};
use chrono::Utc;

use crate::controller::BaseError;
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::service::metrics::provider_runtime::{
    ProviderRuntimeItem, ProviderRuntimeLevel, ProviderRuntimeListParams, ProviderRuntimeSummary,
    ProviderRuntimeSummaryParams, matches_status_filter, runtime_backend_status_for_provider_items,
    search_matches, sort_provider_runtime_items,
};
use crate::utils::HttpResult;

async fn list_provider_runtime(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<ProviderRuntimeListParams>,
) -> Result<HttpResult<Vec<ProviderRuntimeItem>>, BaseError> {
    let window = params
        .window
        .unwrap_or_else(|| app_state.metrics.default_provider_runtime_window());
    let mut items = app_state
        .metrics
        .build_provider_runtime_items(&app_state, window, params.only_enabled)
        .await?;

    if let Some(search) = params.search.as_ref().map(|value| value.trim()) {
        if !search.is_empty() {
            items.retain(|item| search_matches(&item.provider_name, &item.provider_key, search));
        }
    }

    items.retain(|item| matches_status_filter(item.runtime_level, params.status));
    sort_provider_runtime_items(&mut items, params.sort, params.direction);

    Ok(HttpResult::new(items))
}

async fn summary_provider_runtime(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<ProviderRuntimeSummaryParams>,
) -> Result<HttpResult<ProviderRuntimeSummary>, BaseError> {
    let window = params
        .window
        .unwrap_or_else(|| app_state.metrics.default_provider_runtime_window());
    let items = app_state
        .metrics
        .build_provider_runtime_items(&app_state, window, params.only_enabled)
        .await?;
    let runtime_state_backend = runtime_backend_status_for_provider_items(&app_state, &items).await;
    let mut summary = ProviderRuntimeSummary {
        total_provider_count: items.len() as i64,
        healthy_count: 0,
        degraded_count: 0,
        half_open_count: 0,
        open_count: 0,
        no_traffic_count: 0,
        window,
        generated_at: Utc::now().timestamp_millis(),
        runtime_state_backend,
    };

    for item in items {
        match item.runtime_level {
            ProviderRuntimeLevel::Healthy => summary.healthy_count += 1,
            ProviderRuntimeLevel::Degraded => summary.degraded_count += 1,
            ProviderRuntimeLevel::HalfOpen => summary.half_open_count += 1,
            ProviderRuntimeLevel::Open => summary.open_count += 1,
            ProviderRuntimeLevel::NoTraffic => summary.no_traffic_count += 1,
        }
    }

    Ok(HttpResult::new(summary))
}

pub fn create_provider_runtime_router() -> StateRouter {
    create_state_router().nest(
        "/provider/runtime",
        create_state_router()
            .route("/list", get(list_provider_runtime))
            .route("/summary", get(summary_provider_runtime)),
    )
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode},
    };
    use serde_json::Value;
    use tower::ServiceExt;

    use super::create_provider_runtime_router;
    use crate::config::MetricsConfig;
    use crate::database::TestDbContext;
    use crate::service::app_state::{AppState, create_test_app_state};
    use crate::service::metrics::MetricsService;

    fn with_metrics_config(
        app_state: Arc<AppState>,
        metrics_config: MetricsConfig,
    ) -> Arc<AppState> {
        Arc::new(AppState {
            metrics: Arc::new(MetricsService::new(metrics_config)),
            ..(*app_state).clone()
        })
    }

    async fn send(app_state: &Arc<AppState>, uri: &str) -> axum::response::Response {
        create_provider_runtime_router()
            .with_state(Arc::clone(app_state))
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(uri)
                    .body(Body::empty())
                    .expect("request should build"),
            )
            .await
            .expect("provider runtime router should respond")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be JSON")
    }

    #[tokio::test]
    async fn summary_uses_config_default_window_and_query_override() {
        let context = TestDbContext::new_sqlite("provider-runtime-default-window.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let app_state = with_metrics_config(
                    app_state,
                    MetricsConfig {
                        provider_runtime_default_window_seconds: 900,
                        ..MetricsConfig::default()
                    },
                );

                let response = send(&app_state, "/provider/runtime/summary").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/window").and_then(Value::as_str),
                    Some("15m")
                );

                let response = send(&app_state, "/provider/runtime/summary?window=6h").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/window").and_then(Value::as_str),
                    Some("6h")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn summary_falls_back_to_one_hour_for_invalid_config_default_window() {
        let context = TestDbContext::new_sqlite("provider-runtime-invalid-default-window.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let app_state = with_metrics_config(
                    app_state,
                    MetricsConfig {
                        provider_runtime_default_window_seconds: 42,
                        ..MetricsConfig::default()
                    },
                );

                let response = send(&app_state, "/provider/runtime/summary").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/window").and_then(Value::as_str),
                    Some("1h")
                );
            })
            .await;
    }
}
