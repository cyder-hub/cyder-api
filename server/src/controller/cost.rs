use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    controller::BaseError,
    cost::{
        CostLedger, CostRatingContext, CostRatingResult, CostTemplateSummary, UsageNormalization,
        list_templates, rate_cost,
    },
    database::{
        DbResult,
        cost::{
            CostCatalog, CostCatalogVersion, CostComponent, ImportedCostCatalogTemplate,
            NewCostCatalogPayload, NewCostCatalogVersionPayload, NewCostComponentPayload,
            UpdateCostCatalogData, UpdateCostComponentData,
        },
    },
    service::{
        admin::cost::{DuplicateCostCatalogVersionInput, ImportCostTemplateInput},
        app_state::{AppState, StateRouter, create_state_router},
    },
    utils::HttpResult,
};

#[derive(Debug, Deserialize, Default)]
struct UpdateCostCatalogRequest {
    name: Option<String>,
    description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct CreateCostCatalogVersionRequest {
    version: String,
    currency: String,
    source: Option<String>,
    effective_from: i64,
    effective_until: Option<i64>,
    is_enabled: bool,
}

#[derive(Debug, Deserialize, Default)]
struct UpdateCostComponentRequest {
    meter_key: Option<String>,
    charge_kind: Option<String>,
    unit_price_nanos: Option<Option<i64>>,
    flat_fee_nanos: Option<Option<i64>>,
    tier_config_json: Option<Option<String>>,
    match_attributes_json: Option<Option<String>>,
    priority: Option<i32>,
    description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
struct CostPreviewRequest {
    catalog_version_id: i64,
    normalization: Option<UsageNormalization>,
    ledger: Option<CostLedger>,
    total_input_tokens: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ImportCostTemplateRequest {
    template_key: String,
    catalog_name: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct DuplicateCostCatalogVersionRequest {
    version: Option<String>,
}

#[derive(Debug, Serialize)]
struct CostCatalogListItem {
    catalog: CostCatalog,
    versions: Vec<CostCatalogVersion>,
}

#[derive(Debug, Serialize)]
struct CostCatalogVersionDetail {
    version: CostCatalogVersion,
    components: Vec<CostComponent>,
}

#[derive(Debug, Serialize)]
struct CostPreviewResponse {
    normalization: Option<UsageNormalization>,
    ledger: CostLedger,
    result: CostRatingResult,
}

#[derive(Debug, Serialize)]
struct ImportedCostTemplateResponse {
    template: CostTemplateSummary,
    imported: ImportedCostCatalogTemplate,
}

async fn create_catalog(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<NewCostCatalogPayload>,
) -> DbResult<HttpResult<CostCatalog>> {
    let created = app_state.admin.cost.create_catalog(payload).await?;
    Ok(HttpResult::new(created))
}

async fn update_catalog(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateCostCatalogRequest>,
) -> DbResult<HttpResult<CostCatalog>> {
    let updated = app_state
        .admin
        .cost
        .update_catalog(
            id,
            UpdateCostCatalogData {
                name: payload.name,
                description: payload.description,
            },
        )
        .await?;
    Ok(HttpResult::new(updated))
}

async fn delete_catalog(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    app_state.admin.cost.delete_catalog(id).await?;
    Ok(HttpResult::new(()))
}

async fn list_catalogs() -> DbResult<HttpResult<Vec<CostCatalogListItem>>> {
    let catalogs = CostCatalog::list_all()?;
    let versions = CostCatalogVersion::list_all()?;

    let result = catalogs
        .into_iter()
        .map(|catalog| CostCatalogListItem {
            versions: versions
                .iter()
                .filter(|version| version.catalog_id == catalog.id)
                .cloned()
                .collect(),
            catalog,
        })
        .collect();

    Ok(HttpResult::new(result))
}

async fn create_catalog_version(
    State(app_state): State<Arc<AppState>>,
    Path(catalog_id): Path<i64>,
    Json(payload): Json<CreateCostCatalogVersionRequest>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let created = app_state
        .admin
        .cost
        .create_catalog_version(NewCostCatalogVersionPayload {
            catalog_id,
            version: payload.version,
            currency: payload.currency,
            source: payload.source,
            effective_from: payload.effective_from,
            effective_until: payload.effective_until,
            is_enabled: payload.is_enabled,
        })
        .await?;

    Ok(HttpResult::new(created))
}

async fn get_version(Path(id): Path<i64>) -> DbResult<HttpResult<CostCatalogVersionDetail>> {
    let version = CostCatalogVersion::get_by_id(id)?;
    let components = CostComponent::list_by_catalog_version_id(id)?;
    Ok(HttpResult::new(CostCatalogVersionDetail {
        version,
        components,
    }))
}

async fn delete_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    app_state.admin.cost.delete_version(id).await?;
    Ok(HttpResult::new(()))
}

async fn enable_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let updated = app_state.admin.cost.enable_version(id).await?;
    Ok(HttpResult::new(updated))
}

async fn disable_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let updated = app_state.admin.cost.disable_version(id).await?;
    Ok(HttpResult::new(updated))
}

async fn archive_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let updated = app_state.admin.cost.archive_version(id).await?;
    Ok(HttpResult::new(updated))
}

async fn unarchive_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let updated = app_state.admin.cost.unarchive_version(id).await?;
    Ok(HttpResult::new(updated))
}

async fn duplicate_version(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<DuplicateCostCatalogVersionRequest>,
) -> DbResult<HttpResult<CostCatalogVersion>> {
    let duplicated = app_state
        .admin
        .cost
        .duplicate_version(
            id,
            DuplicateCostCatalogVersionInput {
                version: payload.version,
            },
        )
        .await?;
    Ok(HttpResult::new(duplicated))
}

async fn create_component(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<NewCostComponentPayload>,
) -> DbResult<HttpResult<CostComponent>> {
    let created = app_state.admin.cost.create_component(payload).await?;
    Ok(HttpResult::new(created))
}

async fn update_component(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateCostComponentRequest>,
) -> DbResult<HttpResult<CostComponent>> {
    let updated = app_state
        .admin
        .cost
        .update_component(
            id,
            UpdateCostComponentData {
                meter_key: payload.meter_key,
                charge_kind: payload.charge_kind,
                unit_price_nanos: payload.unit_price_nanos,
                flat_fee_nanos: payload.flat_fee_nanos,
                tier_config_json: payload.tier_config_json,
                match_attributes_json: payload.match_attributes_json,
                priority: payload.priority,
                description: payload.description,
            },
        )
        .await?;
    Ok(HttpResult::new(updated))
}

async fn delete_component(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    app_state.admin.cost.delete_component(id).await?;
    Ok(HttpResult::new(()))
}

async fn preview_cost(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CostPreviewRequest>,
) -> DbResult<HttpResult<CostPreviewResponse>> {
    let version = app_state
        .catalog
        .get_cost_catalog_version_by_id(payload.catalog_version_id)
        .await?
        .ok_or_else(|| {
            BaseError::ParamInvalid(Some(format!(
                "Cost catalog version {} not found",
                payload.catalog_version_id
            )))
        })?;

    let normalization = payload.normalization;
    let (ledger, total_input_tokens) = match (&normalization, payload.ledger) {
        (Some(normalization), None) => (
            CostLedger::from(normalization),
            normalization.total_input_tokens,
        ),
        (None, Some(ledger)) => (ledger, payload.total_input_tokens.unwrap_or(0)),
        (Some(_), Some(_)) => {
            return Err(BaseError::ParamInvalid(Some(
                "preview accepts either normalization or ledger, not both".to_string(),
            )));
        }
        (None, None) => {
            return Err(BaseError::ParamInvalid(Some(
                "preview requires normalization or ledger".to_string(),
            )));
        }
    };

    let mut result = rate_cost(&ledger, &CostRatingContext { total_input_tokens }, &version)?;
    if let Some(normalization) = &normalization {
        result
            .warnings
            .extend(normalization.warnings.iter().cloned());
    }

    Ok(HttpResult::new(CostPreviewResponse {
        normalization,
        ledger,
        result,
    }))
}

async fn list_cost_templates() -> DbResult<HttpResult<Vec<CostTemplateSummary>>> {
    Ok(HttpResult::new(list_templates()))
}

async fn import_cost_template(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<ImportCostTemplateRequest>,
) -> DbResult<HttpResult<ImportedCostTemplateResponse>> {
    let imported = app_state
        .admin
        .cost
        .import_template(ImportCostTemplateInput {
            template_key: payload.template_key,
            catalog_name: payload.catalog_name,
        })
        .await?;

    Ok(HttpResult::new(ImportedCostTemplateResponse {
        template: imported.template,
        imported: imported.imported,
    }))
}

pub fn create_cost_router() -> StateRouter {
    create_state_router().nest(
        "/cost",
        create_state_router()
            .route("/template/list", get(list_cost_templates))
            .route("/template/import", post(import_cost_template))
            .route("/catalog", post(create_catalog))
            .route("/catalog/list", get(list_catalogs))
            .route("/catalog/{id}", put(update_catalog).delete(delete_catalog))
            .route("/catalog/{id}/version", post(create_catalog_version))
            .route("/version/{id}", get(get_version).delete(delete_version))
            .route("/version/{id}/enable", post(enable_version))
            .route("/version/{id}/disable", post(disable_version))
            .route("/version/{id}/archive", post(archive_version))
            .route("/version/{id}/unarchive", post(unarchive_version))
            .route("/version/{id}/duplicate", post(duplicate_version))
            .route("/component", post(create_component))
            .route(
                "/component/{id}",
                put(update_component).delete(delete_component),
            )
            .route("/preview", post(preview_cost)),
    )
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

    use crate::database::TestDbContext;
    use crate::database::cost::{
        CostCatalog, CostCatalogVersion, NewCostCatalogPayload, NewCostCatalogVersionPayload,
    };
    use crate::service::app_state::{AppState, create_test_app_state};

    use super::{DuplicateCostCatalogVersionRequest, create_cost_router};

    fn seed_catalog(name: &str) -> CostCatalog {
        CostCatalog::create(&NewCostCatalogPayload {
            name: name.to_string(),
            description: Some("seed".to_string()),
        })
        .expect("catalog seed should succeed")
    }

    fn seed_version(
        catalog_id: i64,
        version: &str,
        effective_from: i64,
        effective_until: Option<i64>,
        is_enabled: bool,
    ) -> CostCatalogVersion {
        CostCatalogVersion::create(&NewCostCatalogVersionPayload {
            catalog_id,
            version: version.to_string(),
            currency: "USD".to_string(),
            source: Some("seed".to_string()),
            effective_from,
            effective_until,
            is_enabled,
        })
        .expect("version seed should succeed")
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_cost_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("cost router should respond")
    }

    fn empty_request(method: Method, uri: &str) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .expect("request should build")
    }

    fn json_request(method: Method, uri: &str, payload: Value) -> Request<Body> {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&payload).expect("payload should serialize"),
            ))
            .expect("request should build")
    }

    async fn response_json(response: axum::response::Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should read");
        serde_json::from_slice(&body).expect("response should be json")
    }

    #[test]
    fn create_cost_router_registers_routes() {
        let _router = create_cost_router();
    }

    #[test]
    fn duplicate_version_request_allows_missing_name_override() {
        let payload = DuplicateCostCatalogVersionRequest::default();
        assert!(payload.version.is_none());
    }

    #[tokio::test]
    async fn enable_version_http_endpoint_updates_response_and_cache() {
        let test_db_context = TestDbContext::new_sqlite("controller-cost-enable-http.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("OpenAI / GPT");
                let existing = seed_version(catalog.id, "2026-04-01", 0, None, true);
                let draft = seed_version(catalog.id, "2026-05-01", 2_000, None, false);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let existing_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should load")
                    .expect("existing version should exist");
                assert_eq!(existing_cached_before.effective_until, None);

                let response = send(
                    &app_state,
                    empty_request(Method::POST, &format!("/cost/version/{}/enable", draft.id)),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;

                assert_eq!(body["code"], 0);
                assert_eq!(body["data"]["id"], draft.id);
                assert_eq!(body["data"]["is_enabled"], true);

                let draft_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(draft.id)
                    .await
                    .expect("draft cache should load")
                    .expect("draft version should exist");
                let existing_cached_after = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing cache should reload")
                    .expect("existing version should exist");

                assert!(draft_cached.is_enabled);
                assert_eq!(existing_cached_after.effective_until, Some(2_000));
            })
            .await;
    }

    #[tokio::test]
    async fn import_cost_template_http_endpoint_updates_response_and_version_cache() {
        let test_db_context =
            TestDbContext::new_sqlite("controller-cost-template-import-http.sqlite");

        test_db_context
            .run_async(async {
                let catalog = seed_catalog("Google / Gemini 2.5 Pro");
                let existing = seed_version(catalog.id, "2026-04-01", 0, None, true);
                let app_state = create_test_app_state(test_db_context.clone()).await;

                let existing_cached_before = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should load")
                    .expect("existing version should exist");
                assert_eq!(existing_cached_before.effective_until, None);

                let response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        "/cost/template/import",
                        json!({
                            "template_key": "google.gemini-2.5-pro.text"
                        }),
                    ),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                let body = response_json(response).await;

                let imported_version_id = body["data"]["imported"]["version"]["id"]
                    .as_i64()
                    .expect("imported version id should exist");
                let imported_effective_from = body["data"]["imported"]["version"]["effective_from"]
                    .as_i64()
                    .expect("imported effective_from should exist");

                assert_eq!(body["code"], 0);
                assert_eq!(
                    body["data"]["template"]["key"],
                    "google.gemini-2.5-pro.text"
                );
                assert!(
                    body["data"]["imported"]["components"]
                        .as_array()
                        .expect("components should be an array")
                        .len()
                        > 0
                );

                let imported_cached = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(imported_version_id)
                    .await
                    .expect("imported version cache should load")
                    .expect("imported version should exist");
                let existing_cached_after = app_state
                    .catalog
                    .get_cost_catalog_version_by_id(existing.id)
                    .await
                    .expect("existing version cache should reload")
                    .expect("existing version should exist");

                assert_eq!(imported_cached.id, imported_version_id);
                assert!(!imported_cached.components.is_empty());
                assert_eq!(
                    existing_cached_after.effective_until,
                    Some(imported_effective_from)
                );
            })
            .await;
    }
}
