use std::sync::Arc;

use axum::{
    extract::{Query, State},
    routing::get,
};

use crate::controller::BaseError;
use crate::service::app_state::{AppState, StateRouter, create_state_router};
use crate::service::metrics::provider_runtime::{
    ProviderRuntimeListParams, ProviderRuntimeSnapshot, matches_status_filter, search_matches,
    sort_provider_runtime_items,
};
use crate::utils::HttpResult;

async fn provider_runtime_snapshot(
    State(app_state): State<Arc<AppState>>,
    Query(params): Query<ProviderRuntimeListParams>,
) -> Result<HttpResult<ProviderRuntimeSnapshot>, BaseError> {
    let window = params
        .window
        .unwrap_or_else(|| app_state.metrics.default_provider_runtime_window());
    let mut items = app_state
        .metrics
        .build_provider_runtime_items(&app_state, window, params.only_enabled)
        .await?;
    let summary = app_state
        .metrics
        .provider_runtime_summary_from_items(&app_state, window, &items, params.only_enabled)
        .await?;

    if let Some(search) = params.search.as_ref().map(|value| value.trim()) {
        if !search.is_empty() {
            items.retain(|item| search_matches(item, search));
        }
    }

    items.retain(|item| matches_status_filter(item.runtime_level, params.status));
    sort_provider_runtime_items(&mut items, params.sort, params.direction);

    Ok(HttpResult::new(ProviderRuntimeSnapshot { items, summary }))
}

pub fn create_provider_runtime_router() -> StateRouter {
    create_state_router().nest(
        "/provider/runtime",
        create_state_router().route("/snapshot", get(provider_runtime_snapshot)),
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
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::upstream_source::NewUpstreamSource;
    use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
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

    fn insert_provider(id: i64, is_enabled: bool) {
        Provider::create(
            &NewProvider {
                id,
                provider_key: format!("provider-{id}"),
                name: format!("Provider {id}"),
                is_enabled,
                created_at: 1,
                updated_at: 1,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            &NewUpstreamSource {
                id: id * 10 + 1,
                provider_id: id,
                profile_type: UpstreamProfileType::Openai,
                endpoint: "https://example.com/v1".to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("provider should insert");
    }

    fn insert_provider_without_source(id: i64, is_enabled: bool) {
        Provider::create_optional(
            &NewProvider {
                id,
                provider_key: format!("provider-{id}"),
                name: format!("Provider {id}"),
                is_enabled,
                created_at: 1,
                updated_at: 1,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            None,
        )
        .expect("provider without source should insert");
    }

    #[tokio::test]
    async fn snapshot_uses_config_default_window_and_query_override() {
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

                let response = send(&app_state, "/provider/runtime/snapshot").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/summary/window").and_then(Value::as_str),
                    Some("15m")
                );

                let response = send(&app_state, "/provider/runtime/snapshot?window=6h").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/summary/window").and_then(Value::as_str),
                    Some("6h")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn snapshot_falls_back_to_one_hour_for_invalid_config_default_window() {
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

                let response = send(&app_state, "/provider/runtime/snapshot").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/summary/window").and_then(Value::as_str),
                    Some("1h")
                );
            })
            .await;
    }

    #[tokio::test]
    async fn snapshot_rejects_unknown_sort_values() {
        let context = TestDbContext::new_sqlite("provider-runtime-invalid-sort.sqlite");
        context
            .run_async(async {
                let app_state = create_test_app_state(context.clone()).await;
                let response = send(&app_state, "/provider/runtime/snapshot?sort=unknown").await;
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            })
            .await;
    }

    #[tokio::test]
    async fn snapshot_builds_summary_and_filtered_items_from_one_provider_set() {
        let context = TestDbContext::new_sqlite("provider-runtime-snapshot-filter.sqlite");
        context
            .run_async(async {
                insert_provider(1, true);
                insert_provider(2, false);
                insert_provider_without_source(3, true);
                let app_state = create_test_app_state(context.clone()).await;
                app_state
                    .source_circuit
                    .record_source_failure(11, "source timeout".to_string(), None)
                    .await
                    .expect("source health should update");

                let response = send(&app_state, "/provider/runtime/snapshot").await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/summary/total_provider_count")
                        .and_then(Value::as_i64),
                    Some(2)
                );
                assert_eq!(
                    body.pointer("/data/summary/enabled_provider_count")
                        .and_then(Value::as_i64),
                    Some(2)
                );
                assert_eq!(
                    body.pointer("/data/summary/total_source_count")
                        .and_then(Value::as_i64),
                    Some(1)
                );
                assert_eq!(
                    body.pointer("/data/summary/enabled_source_count")
                        .and_then(Value::as_i64),
                    Some(1)
                );
                assert_eq!(
                    body.pointer("/data/items")
                        .and_then(Value::as_array)
                        .map(Vec::len),
                    Some(1)
                );
                assert_eq!(
                    body.pointer("/data/items/0/source_id")
                        .and_then(Value::as_i64),
                    Some(11)
                );
                assert!(body.pointer("/data/items/0/source_key").is_none());
                assert_eq!(
                    body.pointer("/data/items/0/source_profile_type")
                        .and_then(Value::as_str),
                    Some("OPENAI")
                );
                assert_eq!(
                    body.pointer("/data/items/0/source_endpoint")
                        .and_then(Value::as_str),
                    Some("https://example.com/v1")
                );
                assert_eq!(
                    body.pointer("/data/items/0/consecutive_failures")
                        .and_then(Value::as_u64),
                    Some(1)
                );

                let response = send(
                    &app_state,
                    "/provider/runtime/snapshot?only_enabled=false&search=no-match",
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;
                assert_eq!(
                    body.pointer("/data/summary/total_provider_count")
                        .and_then(Value::as_i64),
                    Some(3)
                );
                assert_eq!(
                    body.pointer("/data/summary/enabled_provider_count")
                        .and_then(Value::as_i64),
                    Some(2)
                );
                assert_eq!(
                    body.pointer("/data/summary/total_source_count")
                        .and_then(Value::as_i64),
                    Some(2)
                );
                assert_eq!(
                    body.pointer("/data/summary/enabled_source_count")
                        .and_then(Value::as_i64),
                    Some(2)
                );
                assert_eq!(
                    body.pointer("/data/items")
                        .and_then(Value::as_array)
                        .map(Vec::len),
                    Some(0)
                );
            })
            .await;
    }
}
