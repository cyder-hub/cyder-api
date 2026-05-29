use std::sync::Arc;

use axum::{
    Json,
    extract::{Query, State},
    routing::{get, post},
};

use crate::{
    controller::BaseError,
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        system_config::{
            ResolvedConfigReport, SystemConfigChangeRequest, SystemConfigHistoryItem,
            SystemConfigHistoryQuery, SystemConfigPreviewResponse, SystemConfigResetRequest,
            SystemConfigServiceError,
        },
    },
    utils::HttpResult,
};

fn map_system_config_error(err: SystemConfigServiceError) -> BaseError {
    match err {
        SystemConfigServiceError::Validation(_)
        | SystemConfigServiceError::MultiInstanceUnsupported => {
            BaseError::ParamInvalid(Some(err.to_string()))
        }
        SystemConfigServiceError::OverrideFile(_)
        | SystemConfigServiceError::History(_)
        | SystemConfigServiceError::ConfigLoad(_)
        | SystemConfigServiceError::RuntimeApply(_) => {
            BaseError::InternalServerError(Some(err.to_string()))
        }
    }
}

async fn get_config(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<ResolvedConfigReport>, BaseError> {
    Ok(HttpResult::new(app_state.system_config.report().await))
}

async fn preview_config_changes(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<SystemConfigChangeRequest>,
) -> Result<HttpResult<SystemConfigPreviewResponse>, BaseError> {
    let preview = app_state
        .system_config
        .preview_changes(&payload)
        .await
        .map_err(map_system_config_error)?;
    Ok(HttpResult::new(preview))
}

async fn apply_config_changes(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<SystemConfigChangeRequest>,
) -> Result<HttpResult<ResolvedConfigReport>, BaseError> {
    let report = app_state
        .system_config
        .apply_changes(payload)
        .await
        .map_err(map_system_config_error)?;
    Ok(HttpResult::new(report))
}

async fn reset_config_paths(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<SystemConfigResetRequest>,
) -> Result<HttpResult<ResolvedConfigReport>, BaseError> {
    let report = app_state
        .system_config
        .reset_paths(payload.paths, payload.reason)
        .await
        .map_err(map_system_config_error)?;
    Ok(HttpResult::new(report))
}

async fn reload_config_override(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<ResolvedConfigReport>, BaseError> {
    let report = app_state
        .system_config
        .reload_override_file()
        .await
        .map_err(map_system_config_error)?;
    Ok(HttpResult::new(report))
}

async fn list_config_history(
    State(app_state): State<Arc<AppState>>,
    Query(query): Query<SystemConfigHistoryQuery>,
) -> Result<HttpResult<Vec<SystemConfigHistoryItem>>, BaseError> {
    let history = app_state
        .system_config
        .history(query.limit, query.offset)
        .map_err(map_system_config_error)?;
    Ok(HttpResult::new(history))
}

pub fn create_system_config_router() -> StateRouter {
    create_state_router()
        .route("/system/config", get(get_config))
        .route("/system/config/preview", post(preview_config_changes))
        .route("/system/config/apply", post(apply_config_changes))
        .route("/system/config/reset", post(reset_config_paths))
        .route("/system/config/reload", post(reload_config_override))
        .route("/system/config/history", get(list_config_history))
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::Arc};

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header::CONTENT_TYPE},
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::create_system_config_router;
    use crate::{
        config::{
            loader::{ConfigLoadOptions, load_effective_config},
            paths::ConfigPaths,
        },
        controller::create_manager_router,
        service::{
            admin::AdminServices,
            alerts::AlertsService,
            app_state::AppState,
            catalog::CatalogService,
            diagnostics::{DiagnosticsPolicy, DiagnosticsPolicyManager, DiagnosticsService},
            infra::AppInfra,
            metrics::MetricsService,
            notification::NotificationService,
            runtime::{ProviderKeySelector, RuntimeStateBackendBundle},
            system_config::SystemConfigService,
        },
    };

    async fn test_app_state() -> (Arc<AppState>, ConfigPaths) {
        test_app_state_with_user_config(None).await
    }

    async fn test_app_state_with_user_config(
        user_config_yaml: Option<&str>,
    ) -> (Arc<AppState>, ConfigPaths) {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let temp_dir = temp_dir.keep();
        let paths = ConfigPaths::for_test(&temp_dir);
        if let Some(user_config_yaml) = user_config_yaml {
            if let Some(parent) = paths.user_config_path.parent() {
                fs::create_dir_all(parent).expect("user config parent should create");
            }
            fs::write(&paths.user_config_path, user_config_yaml).expect("user config should write");
        }
        let load_options = ConfigLoadOptions {
            include_environment: false,
            include_override: true,
        };
        let loaded = load_effective_config(&paths, load_options).expect("config should load");
        let system_config = Arc::new(SystemConfigService::new(loaded.clone(), load_options));
        let snapshot = system_config.runtime_snapshot().await;
        let infra = Arc::new(
            AppInfra::new_with_config(
                snapshot.version,
                snapshot.proxy_request.clone(),
                snapshot.proxy.clone(),
                None,
            )
            .await,
        );
        system_config
            .register_http_client_manager(infra.http_clients())
            .await;
        let diagnostics_policy_manager = Arc::new(DiagnosticsPolicyManager::new(
            DiagnosticsPolicy::from_config(&snapshot.diagnostics),
        ));
        system_config
            .register_diagnostics_policy_manager(Arc::clone(&diagnostics_policy_manager))
            .await;
        let diagnostics = Arc::new(DiagnosticsService::new(diagnostics_policy_manager));
        let metrics = Arc::new(MetricsService::new(loaded.config.metrics.clone()));
        let alerts = Arc::new(AlertsService::new(loaded.config.alerts.clone()));
        let notification = Arc::new(
            NotificationService::new_with_default_channel_cooldown_seconds(
                loaded.config.notification.clone(),
                loaded.config.alerts.default_cooldown_seconds,
            ),
        );
        let runtime_backend = RuntimeStateBackendBundle::from_config(&loaded.config, true)
            .await
            .expect("runtime backend should initialize");
        system_config
            .register_provider_governance_config_manager(
                runtime_backend.provider_circuit.config_manager(),
            )
            .await;
        let catalog = Arc::new(CatalogService::new(true).await);
        let admin = Arc::new(AdminServices::new(Arc::clone(&catalog)));
        let provider_key_selector = ProviderKeySelector::new(
            Arc::clone(&catalog),
            Arc::clone(&runtime_backend.provider_key_cursor_store),
        )
        .await;
        let app_state = Arc::new(AppState {
            infra,
            catalog,
            admin,
            provider_key_selector,
            api_key_governance: Arc::clone(&runtime_backend.api_key_governance),
            provider_circuit: Arc::clone(&runtime_backend.provider_circuit),
            reasoning_continuation_store: Arc::clone(&runtime_backend.reasoning_continuation_store),
            diagnostics,
            metrics,
            alerts,
            notification,
            runtime_backend_status: Arc::new(runtime_backend.status),
            system_config,
        });

        (app_state, paths)
    }

    fn write_test_config(path: &Path, yaml: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("config parent should create");
        }
        fs::write(path, yaml).expect("config file should write");
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_system_config_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("system config router should respond")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    fn request(method: Method, uri: &str, payload: Option<Value>) -> Request<Body> {
        let builder = Request::builder().method(method).uri(uri);
        match payload {
            Some(payload) => builder
                .header(CONTENT_TYPE, "application/json")
                .body(Body::from(
                    serde_json::to_vec(&payload).expect("payload should serialize"),
                ))
                .expect("json request should build"),
            None => builder.body(Body::empty()).expect("request should build"),
        }
    }

    #[test]
    fn create_system_config_router_registers_routes() {
        let _router = create_system_config_router();
    }

    #[tokio::test]
    async fn manager_system_config_requires_authentication() {
        let (app_state, _paths) = test_app_state().await;
        let response = create_manager_router()
            .with_state(app_state)
            .oneshot(request(Method::GET, "/manager/api/system/config", None))
            .await
            .expect("manager router should respond");

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn system_config_http_flow_covers_preview_apply_reset_reload_and_history() {
        let (app_state, paths) = test_app_state().await;

        let response = send(&app_state, request(Method::GET, "/system/config", None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert!(
            body.pointer("/data/fields")
                .and_then(Value::as_array)
                .is_some_and(|fields| !fields.is_empty())
        );

        let preview_payload = json!({
            "changes": {
                "max_body_size": 2097152
            },
            "reason": "raise body limit"
        });
        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/preview",
                Some(preview_payload.clone()),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!paths.override_config_path.exists());

        let response = send(
            &app_state,
            request(Method::POST, "/system/config/apply", Some(preview_payload)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(app_state.system_config.version().await, 2);
        assert!(paths.override_config_path.exists());

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/reset",
                Some(json!({
                    "paths": ["max_body_size"],
                    "reason": "restore"
                })),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(app_state.system_config.version().await, 3);

        write_test_config(&paths.override_config_path, "timezone: Asia/Shanghai\n");
        let response = send(
            &app_state,
            request(Method::POST, "/system/config/reload", Some(json!({}))),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            app_state
                .system_config
                .runtime_snapshot()
                .await
                .timezone
                .as_deref(),
            Some("Asia/Shanghai")
        );

        let response = send(
            &app_state,
            request(Method::GET, "/system/config/history?limit=10", None),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let history = body
            .pointer("/data")
            .and_then(Value::as_array)
            .expect("history should be an array");
        assert!(history.len() >= 3);

        let response = send(
            &app_state,
            request(Method::GET, "/system/config/history?limit=1&offset=1", None),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let history_page = body
            .pointer("/data")
            .and_then(Value::as_array)
            .expect("history page should be an array");
        assert_eq!(history_page.len(), 1);
    }

    #[tokio::test]
    async fn system_config_get_refreshes_manual_override_view_without_reloading_runtime() {
        let (app_state, paths) = test_app_state().await;
        let before = app_state.system_config.runtime_snapshot().await;

        write_test_config(&paths.override_config_path, "log_level: debug\n");

        let response = send(&app_state, request(Method::GET, "/system/config", None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;

        assert_eq!(body.pointer("/data/summary/version"), Some(&json!(1)));
        assert_eq!(
            body.pointer("/data/summary/override_exists"),
            Some(&json!(true))
        );
        assert!(
            body.pointer("/data/override_file/yaml")
                .and_then(Value::as_str)
                .is_some_and(|yaml| yaml.contains("log_level: debug"))
        );
        assert!(
            body.pointer("/data/override_file/last_modified_ms")
                .and_then(Value::as_i64)
                .is_some()
        );
        assert!(
            body.pointer("/data/persistence_health/status")
                .and_then(Value::as_str)
                .is_some()
        );
        assert!(
            body.pointer("/data/persistence_health/items")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().any(|item| {
                    item.pointer("/key") == Some(&json!("override_config"))
                        && item.pointer("/status") == Some(&json!("ok"))
                        && item.pointer("/path")
                            == Some(&json!(paths.override_config_path.display().to_string()))
                        && item.pointer("/readable") == Some(&json!(true))
                        && item.pointer("/writable") == Some(&json!(true))
                }))
        );
        assert_eq!(app_state.system_config.version().await, 1);
        assert_eq!(app_state.system_config.runtime_snapshot().await, before);
    }

    #[tokio::test]
    async fn system_config_get_summarizes_invalid_manual_override_without_leaking_yaml() {
        let (app_state, paths) = test_app_state().await;
        let before = app_state.system_config.runtime_snapshot().await;

        write_test_config(
            &paths.override_config_path,
            "db_url: postgres://secret@example/cyder\nlog_level: debug\n",
        );

        let response = send(&app_state, request(Method::GET, "/system/config", None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let serialized = serde_json::to_string(&body).expect("body should serialize");

        assert_eq!(body.pointer("/data/summary/version"), Some(&json!(1)));
        assert_eq!(
            body.pointer("/data/summary/override_exists"),
            Some(&json!(true))
        );
        assert_eq!(body.pointer("/data/override_file/yaml"), Some(&json!("")));
        assert!(
            body.pointer("/data/override_file/invalid_paths")
                .and_then(Value::as_array)
                .is_some_and(|paths| paths.iter().any(|path| path == "db_url"))
        );
        assert_eq!(
            body.pointer("/data/persistence_health/status"),
            Some(&json!("error"))
        );
        assert!(
            body.pointer("/data/persistence_health/items")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().any(|item| {
                    item.pointer("/key") == Some(&json!("override_config"))
                        && item.pointer("/status") == Some(&json!("error"))
                        && item
                            .pointer("/message")
                            .and_then(Value::as_str)
                            .is_some_and(|message| message.contains("unsupported paths"))
                }))
        );
        assert!(!serialized.contains("postgres://secret"));
        assert!(!serialized.contains("log_level: debug"));
        assert_eq!(app_state.system_config.version().await, 1);
        assert_eq!(app_state.system_config.runtime_snapshot().await, before);
        assert_eq!(app_state.system_config.last_error().await, None);
    }

    #[tokio::test]
    async fn system_config_apply_rejects_multi_instance_mode() {
        let (app_state, _paths) =
            test_app_state_with_user_config(Some("deployment:\n  mode: multi_instance\n")).await;

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/apply",
                Some(json!({
                    "changes": {
                        "max_body_size": 2097152
                    },
                    "reason": "should fail"
                })),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(
            body.pointer("/msg")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("multi_instance_not_supported"))
        );
    }

    #[tokio::test]
    async fn system_config_write_endpoints_require_reason() {
        let (app_state, _paths) = test_app_state().await;

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/apply",
                Some(json!({
                    "changes": {
                        "max_body_size": 2097152
                    }
                })),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(
            body.pointer("/msg")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("reason"))
        );

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/reset",
                Some(json!({
                    "paths": ["max_body_size"]
                })),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(
            body.pointer("/msg")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("reason"))
        );
    }

    #[tokio::test]
    async fn system_config_rejects_noop_apply_and_reset() {
        let (app_state, paths) = test_app_state().await;

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/apply",
                Some(json!({
                    "changes": {
                        "log_level": "info"
                    },
                    "reason": "same value should fail"
                })),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(
            body.pointer("/msg")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("no_effective_change"))
        );

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/reset",
                Some(json!({
                    "paths": ["max_body_size"],
                    "reason": "absent override should fail"
                })),
            ),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert!(
            body.pointer("/msg")
                .and_then(Value::as_str)
                .is_some_and(|message| message.contains("no_effective_change"))
        );
        assert_eq!(app_state.system_config.version().await, 1);
        assert!(!paths.override_history_path.exists());
    }

    #[tokio::test]
    async fn system_config_multi_instance_preview_is_read_only_but_available() {
        let (app_state, _paths) =
            test_app_state_with_user_config(Some("deployment:\n  mode: multi_instance\n")).await;

        let response = send(
            &app_state,
            request(
                Method::POST,
                "/system/config/preview",
                Some(json!({
                    "changes": {
                        "max_body_size": 2097152
                    }
                })),
            ),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(
            body.pointer("/data/write_disabled_reason"),
            Some(&json!("multi_instance_not_supported"))
        );

        let response = send(
            &app_state,
            request(Method::POST, "/system/config/reload", Some(json!({}))),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
