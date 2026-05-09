use std::sync::Arc;

use axum::{
    Json,
    extract::{DefaultBodyLimit, State},
    routing::{get, post},
};

use crate::{
    controller::BaseError,
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        portable_config::{
            registry::PortableModuleRegistryResponse,
            schema::{
                PortableApplyRequest, PortableApplyResult, PortableExportRequest,
                PortableExportResponse, PortableImportPreviewRequest, PortablePreviewResponse,
            },
        },
    },
    utils::HttpResult,
};

async fn list_modules(
    State(app_state): State<Arc<AppState>>,
) -> Result<HttpResult<PortableModuleRegistryResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state.admin.portable_config.module_registry(),
    ))
}

async fn export_config(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<PortableExportRequest>,
) -> Result<HttpResult<PortableExportResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .portable_config
            .export_config(payload)
            .await?,
    ))
}

async fn preview_import(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<PortableImportPreviewRequest>,
) -> Result<HttpResult<PortablePreviewResponse>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .portable_config
            .preview_import(payload)
            .await?,
    ))
}

async fn apply_import(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<PortableApplyRequest>,
) -> Result<HttpResult<PortableApplyResult>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .portable_config
            .apply_import(payload)
            .await?,
    ))
}

pub fn create_portable_config_router() -> StateRouter {
    let import_router = create_state_router()
        .route("/system/portable/import/preview", post(preview_import))
        .route("/system/portable/import/apply", post(apply_import))
        .layer(DefaultBodyLimit::disable());

    create_state_router()
        .route("/system/portable/modules", get(list_modules))
        .route("/system/portable/export", post(export_config))
        .merge(import_router)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use axum::{
        body::{Body, to_bytes},
        http::{Method, Request, StatusCode, header::CONTENT_TYPE},
    };
    use serde_json::{Value, json};
    use tower::util::ServiceExt;

    use crate::{
        database::TestDbContext,
        service::app_state::{AppState, create_test_app_state},
    };

    use super::create_portable_config_router;

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_portable_config_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("portable config router should respond")
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

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    #[test]
    fn create_portable_config_router_registers_routes() {
        let _router = create_portable_config_router();
    }

    #[tokio::test]
    async fn portable_import_http_endpoints_accept_large_backup_payloads() {
        let test_db_context = TestDbContext::new_sqlite("controller-portable-large-import.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;
                let large_bundle = serde_json::to_string(&json!({
                    "schema_version": "cyder.portable.v1",
                    "exported_at": 1_778_236_800_000_i64,
                    "cyder_version": "x".repeat(2_200_000),
                    "modules": []
                }))
                .expect("large bundle should serialize");
                assert!(
                    large_bundle.len() > 2 * 1024 * 1024,
                    "test payload must exceed axum's default JSON body limit"
                );

                let preview_response = send(
                    &app_state,
                    request(
                        Method::POST,
                        "/system/portable/import/preview",
                        Some(json!({
                            "content": large_bundle.clone(),
                            "password": null
                        })),
                    ),
                )
                .await;
                assert_eq!(preview_response.status(), StatusCode::OK);
                let preview_body = response_json(preview_response).await;
                assert_eq!(preview_body["code"], 0);
                let bundle_digest = preview_body["data"]["bundle_digest"]
                    .as_str()
                    .expect("preview should return digest")
                    .to_string();

                let apply_response = send(
                    &app_state,
                    request(
                        Method::POST,
                        "/system/portable/import/apply",
                        Some(json!({
                            "content": large_bundle,
                            "password": null,
                            "bundle_digest": bundle_digest,
                            "selected_modules": [],
                            "conflict_strategy": "fail_on_conflict",
                            "reason": "large portable apply smoke test",
                            "dangerous_patch_confirmations": []
                        })),
                    ),
                )
                .await;
                assert_eq!(apply_response.status(), StatusCode::OK);
                let apply_body = response_json(apply_response).await;
                assert_eq!(apply_body["code"], 0);
                assert_eq!(apply_body["data"]["summary"]["total"], 0);
            })
            .await;
    }

    #[tokio::test]
    async fn portable_modules_and_export_http_endpoints_respond() {
        let test_db_context = TestDbContext::new_sqlite("controller-portable-export.sqlite");

        test_db_context
            .run_async(async {
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let modules_response = send(
                    &app_state,
                    request(Method::GET, "/system/portable/modules", None),
                )
                .await;
                assert_eq!(modules_response.status(), StatusCode::OK);
                let modules_body = response_json(modules_response).await;
                assert_eq!(modules_body["code"], 0);
                assert_eq!(
                    modules_body["data"]["default_selected_modules"],
                    json!(["provider_profile", "api_keys"])
                );

                let export_response = send(
                    &app_state,
                    request(
                        Method::POST,
                        "/system/portable/export",
                        Some(json!({
                            "selected_modules": [],
                            "file_protection": "plaintext"
                        })),
                    ),
                )
                .await;
                assert_eq!(export_response.status(), StatusCode::OK);
                let export_body = response_json(export_response).await;
                assert_eq!(export_body["code"], 0);
                assert!(
                    export_body["data"]["bundle_digest"]
                        .as_str()
                        .is_some_and(|digest| digest.starts_with("sha256:"))
                );
                assert!(
                    export_body["data"]["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("\"schema_version\""))
                );

                let exported_content = export_body["data"]["content"]
                    .as_str()
                    .expect("export content should be string");
                let preview_response = send(
                    &app_state,
                    request(
                        Method::POST,
                        "/system/portable/import/preview",
                        Some(json!({
                            "content": exported_content,
                            "password": null
                        })),
                    ),
                )
                .await;
                assert_eq!(preview_response.status(), StatusCode::OK);
                let preview_body = response_json(preview_response).await;
                assert_eq!(preview_body["code"], 0);
                assert_eq!(
                    preview_body["data"]["default_selected_modules"],
                    json!(["provider_profile", "api_keys"])
                );
                assert_eq!(preview_body["data"]["modules"][0]["summary"]["total"], 0);

                let apply_response = send(
                    &app_state,
                    request(
                        Method::POST,
                        "/system/portable/import/apply",
                        Some(json!({
                            "content": exported_content,
                            "password": null,
                            "bundle_digest": export_body["data"]["bundle_digest"],
                            "selected_modules": [],
                            "conflict_strategy": "fail_on_conflict",
                            "reason": "controller portable apply smoke test",
                            "dangerous_patch_confirmations": []
                        })),
                    ),
                )
                .await;
                assert_eq!(apply_response.status(), StatusCode::OK);
                let apply_body = response_json(apply_response).await;
                assert_eq!(apply_body["code"], 0);
                assert_eq!(apply_body["data"]["summary"]["total"], 0);
            })
            .await;
    }
}
