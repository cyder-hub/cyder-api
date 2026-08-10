use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    routing::{get, post, put},
};
use serde::{Deserialize, Serialize};

use crate::{
    database::request_patch::{
        RequestPatchRuleInput, RequestPatchVariantAggregate, RequestPatchVariantInput,
        RequestPatchVariantPreview,
    },
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        request_patch::{
            RequestPatchEvaluation, cache_variant_from_preview_input,
            evaluate_request_patch_variants,
        },
    },
    utils::HttpResult,
};

use super::BaseError;

#[derive(Debug, Serialize)]
struct RequestPatchVariantListResponse {
    source_id: i64,
    model_id: Option<i64>,
    variants: Vec<RequestPatchVariantAggregate>,
    variant_count: usize,
    rule_count: usize,
}

impl RequestPatchVariantListResponse {
    fn new(
        source_id: i64,
        model_id: Option<i64>,
        variants: Vec<RequestPatchVariantAggregate>,
    ) -> Self {
        let rule_count = variants.iter().map(|item| item.rules.len()).sum();
        Self {
            source_id,
            model_id,
            variant_count: variants.len(),
            rule_count,
            variants,
        }
    }
}

#[derive(Debug, Serialize)]
struct ModelRequestPatchOverviewResponse {
    model_id: i64,
    variants: Vec<RequestPatchVariantAggregate>,
    variant_count: usize,
    rule_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestPatchPreviewPayload {
    variant_id: Option<i64>,
    source_id: i64,
    model_id: Option<i64>,
    suffix: Option<String>,
    enabled: bool,
    expose_in_models: bool,
    rules: Vec<RequestPatchRuleInput>,
}

impl From<RequestPatchPreviewPayload> for RequestPatchVariantInput {
    fn from(value: RequestPatchPreviewPayload) -> Self {
        Self {
            source_id: value.source_id,
            model_id: value.model_id,
            suffix: value.suffix,
            enabled: value.enabled,
            expose_in_models: value.expose_in_models,
            rules: value.rules,
        }
    }
}

#[derive(Debug, Serialize)]
struct RequestPatchPreviewResponse {
    historical_snapshot: bool,
    preview: RequestPatchVariantPreview,
    evaluation: Option<RequestPatchEvaluation>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RequestPatchExplainQuery {
    suffix: Option<String>,
}

#[derive(Debug, Serialize)]
struct RequestPatchExplainResponse {
    historical_snapshot: bool,
    source_id: i64,
    model_id: Option<i64>,
    suffix: Option<String>,
    evaluation: RequestPatchEvaluation,
}

fn list_response(
    source_id: i64,
    model_id: Option<i64>,
    variants: Vec<RequestPatchVariantAggregate>,
) -> HttpResult<RequestPatchVariantListResponse> {
    HttpResult::new(RequestPatchVariantListResponse::new(
        source_id, model_id, variants,
    ))
}

async fn list_source_request_patch_variants(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
) -> Result<HttpResult<RequestPatchVariantListResponse>, BaseError> {
    app_state
        .admin
        .request_patch
        .validate_source_route(provider_id, source_id)?;
    let variants = app_state
        .admin
        .request_patch
        .list_source_variants(source_id)?;
    Ok(list_response(source_id, None, variants))
}

async fn create_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
    Json(input): Json<RequestPatchVariantInput>,
) -> Result<HttpResult<RequestPatchVariantAggregate>, BaseError> {
    app_state
        .admin
        .request_patch
        .validate_source_route(provider_id, source_id)?;
    Ok(HttpResult::new(
        app_state
            .admin
            .request_patch
            .create_source_variant(source_id, input)
            .await?,
    ))
}

async fn update_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id, variant_id)): Path<(i64, i64, i64)>,
    Json(input): Json<RequestPatchVariantInput>,
) -> Result<HttpResult<RequestPatchVariantAggregate>, BaseError> {
    app_state
        .admin
        .request_patch
        .validate_source_route(provider_id, source_id)?;
    Ok(HttpResult::new(
        app_state
            .admin
            .request_patch
            .update_source_variant(source_id, variant_id, input)
            .await?,
    ))
}

async fn delete_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id, variant_id)): Path<(i64, i64, i64)>,
) -> Result<HttpResult<RequestPatchVariantAggregate>, BaseError> {
    app_state
        .admin
        .request_patch
        .validate_source_route(provider_id, source_id)?;
    Ok(HttpResult::new(
        app_state
            .admin
            .request_patch
            .delete_source_variant(source_id, variant_id)
            .await?,
    ))
}

async fn preview_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
    Json(payload): Json<RequestPatchPreviewPayload>,
) -> Result<HttpResult<RequestPatchPreviewResponse>, BaseError> {
    app_state
        .admin
        .request_patch
        .validate_source_route(provider_id, source_id)?;
    let variant_id = payload.variant_id;
    let input: RequestPatchVariantInput = payload.into();
    let preview = app_state
        .admin
        .request_patch
        .preview_source_variant(source_id, input, variant_id)?;
    Ok(HttpResult::new(RequestPatchPreviewResponse {
        historical_snapshot: false,
        preview,
        evaluation: None,
    }))
}

async fn explain_source_request_patch_variants(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, source_id)): Path<(i64, i64)>,
    Query(query): Query<RequestPatchExplainQuery>,
) -> Result<HttpResult<RequestPatchExplainResponse>, BaseError> {
    app_state
        .admin
        .request_patch
        .validate_source_route(provider_id, source_id)?;
    Ok(HttpResult::new(
        build_explain_response(&app_state, source_id, None, query.suffix).await?,
    ))
}

async fn list_model_request_patch_variants(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ModelRequestPatchOverviewResponse>, BaseError> {
    let variants = app_state
        .admin
        .request_patch
        .list_model_variants(model_id)?;
    let rule_count = variants.iter().map(|item| item.rules.len()).sum();
    Ok(HttpResult::new(ModelRequestPatchOverviewResponse {
        model_id,
        variant_count: variants.len(),
        rule_count,
        variants,
    }))
}

async fn list_model_source_request_patch_variants(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, source_id)): Path<(i64, i64)>,
) -> Result<HttpResult<RequestPatchVariantListResponse>, BaseError> {
    let variants = app_state
        .admin
        .request_patch
        .list_model_source_variants(model_id, source_id)?;
    Ok(list_response(source_id, Some(model_id), variants))
}

async fn create_model_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, source_id)): Path<(i64, i64)>,
    Json(input): Json<RequestPatchVariantInput>,
) -> Result<HttpResult<RequestPatchVariantAggregate>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .request_patch
            .create_model_source_variant(model_id, source_id, input)
            .await?,
    ))
}

async fn update_model_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, source_id, variant_id)): Path<(i64, i64, i64)>,
    Json(input): Json<RequestPatchVariantInput>,
) -> Result<HttpResult<RequestPatchVariantAggregate>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .request_patch
            .update_model_source_variant(model_id, source_id, variant_id, input)
            .await?,
    ))
}

async fn delete_model_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, source_id, variant_id)): Path<(i64, i64, i64)>,
) -> Result<HttpResult<RequestPatchVariantAggregate>, BaseError> {
    Ok(HttpResult::new(
        app_state
            .admin
            .request_patch
            .delete_model_source_variant(model_id, source_id, variant_id)
            .await?,
    ))
}

async fn preview_model_source_request_patch_variant(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, source_id)): Path<(i64, i64)>,
    Json(payload): Json<RequestPatchPreviewPayload>,
) -> Result<HttpResult<RequestPatchPreviewResponse>, BaseError> {
    let variant_id = payload.variant_id;
    let input: RequestPatchVariantInput = payload.into();
    let preview = app_state.admin.request_patch.preview_model_source_variant(
        model_id,
        source_id,
        input.clone(),
        variant_id,
    )?;
    let candidate = cache_variant_from_preview_input(&input, variant_id)?;
    let mut variants = app_state
        .catalog
        .get_request_patch_variants()
        .await?
        .as_ref()
        .clone();
    variants.retain(|variant| {
        variant.id != candidate.id
            && !(variant.source_id == candidate.source_id
                && variant.model_id == candidate.model_id
                && variant.suffix == candidate.suffix)
    });
    let suffix = candidate.suffix.clone();
    variants.push(candidate);
    let evaluation = evaluate_request_patch_variants(
        variants.as_slice(),
        source_id,
        Some(model_id),
        suffix.as_deref(),
    );
    Ok(HttpResult::new(RequestPatchPreviewResponse {
        historical_snapshot: false,
        preview,
        evaluation: Some(evaluation),
    }))
}

async fn explain_model_source_request_patch_variants(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, source_id)): Path<(i64, i64)>,
    Query(query): Query<RequestPatchExplainQuery>,
) -> Result<HttpResult<RequestPatchExplainResponse>, BaseError> {
    Ok(HttpResult::new(
        build_explain_response(&app_state, source_id, Some(model_id), query.suffix).await?,
    ))
}

async fn build_explain_response(
    app_state: &AppState,
    source_id: i64,
    model_id: Option<i64>,
    suffix: Option<String>,
) -> Result<RequestPatchExplainResponse, BaseError> {
    let variants = app_state.catalog.get_request_patch_variants().await?;
    let evaluation = evaluate_request_patch_variants(
        variants.as_slice(),
        source_id,
        model_id,
        suffix.as_deref(),
    );
    Ok(RequestPatchExplainResponse {
        historical_snapshot: false,
        source_id,
        model_id,
        suffix,
        evaluation,
    })
}

pub fn create_request_patch_router() -> StateRouter {
    create_state_router()
        .route(
            "/provider/{provider_id}/sources/{source_id}/request_patch",
            get(list_source_request_patch_variants),
        )
        .route(
            "/provider/{provider_id}/sources/{source_id}/request_patch/variants",
            post(create_source_request_patch_variant),
        )
        .route(
            "/provider/{provider_id}/sources/{source_id}/request_patch/preview",
            post(preview_source_request_patch_variant),
        )
        .route(
            "/provider/{provider_id}/sources/{source_id}/request_patch/explain",
            get(explain_source_request_patch_variants),
        )
        .route(
            "/provider/{provider_id}/sources/{source_id}/request_patch/variants/{variant_id}",
            put(update_source_request_patch_variant).delete(delete_source_request_patch_variant),
        )
        .route(
            "/model/{model_id}/request_patch",
            get(list_model_request_patch_variants),
        )
        .route(
            "/model/{model_id}/sources/{source_id}/request_patch",
            get(list_model_source_request_patch_variants),
        )
        .route(
            "/model/{model_id}/sources/{source_id}/request_patch/variants",
            post(create_model_source_request_patch_variant),
        )
        .route(
            "/model/{model_id}/sources/{source_id}/request_patch/preview",
            post(preview_model_source_request_patch_variant),
        )
        .route(
            "/model/{model_id}/sources/{source_id}/request_patch/explain",
            get(explain_model_source_request_patch_variants),
        )
        .route(
            "/model/{model_id}/sources/{source_id}/request_patch/variants/{variant_id}",
            put(update_model_source_request_patch_variant)
                .delete(delete_model_source_request_patch_variant),
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

    use super::create_request_patch_router;
    use crate::database::TestDbContext;
    use crate::database::model::Model;
    use crate::database::provider::{NewProvider, Provider};
    use crate::database::upstream_source::NewUpstreamSource;
    use crate::schema::enum_def::{
        ProviderApiKeyMode, RequestPatchOperation, RequestPatchPlacement, UpstreamProfileType,
    };
    use crate::service::app_state::{AppState, create_test_app_state};

    #[test]
    fn request_patch_router_registers_only_aggregate_routes() {
        let _router = create_request_patch_router();
    }

    #[test]
    fn request_patch_openapi_matches_aggregate_routes_and_current_detail_shapes() {
        let document: serde_yaml::Value = serde_yaml::from_str(include_str!(
            "../../../docs/openapi/manager-provider.openapi.yaml"
        ))
        .expect("manager Provider OpenAPI should parse");
        assert_eq!(
            document["x-cyder-default-cache-control"].as_str(),
            Some("no-store")
        );

        let expected_operations = [
            (
                "/ai/manager/api/provider/{provider_id}/sources/{source_id}/request_patch",
                "get",
                "RequestPatchVariantList",
            ),
            (
                "/ai/manager/api/provider/{provider_id}/sources/{source_id}/request_patch/variants",
                "post",
                "RequestPatchVariantAggregate",
            ),
            (
                "/ai/manager/api/provider/{provider_id}/sources/{source_id}/request_patch/variants/{variant_id}",
                "put",
                "RequestPatchVariantAggregate",
            ),
            (
                "/ai/manager/api/provider/{provider_id}/sources/{source_id}/request_patch/variants/{variant_id}",
                "delete",
                "RequestPatchVariantAggregate",
            ),
            (
                "/ai/manager/api/provider/{provider_id}/sources/{source_id}/request_patch/preview",
                "post",
                "RequestPatchPreview",
            ),
            (
                "/ai/manager/api/provider/{provider_id}/sources/{source_id}/request_patch/explain",
                "get",
                "RequestPatchExplain",
            ),
            (
                "/ai/manager/api/model/{model_id}/request_patch",
                "get",
                "RequestPatchModelOverview",
            ),
            (
                "/ai/manager/api/model/{model_id}/sources/{source_id}/request_patch",
                "get",
                "RequestPatchVariantList",
            ),
            (
                "/ai/manager/api/model/{model_id}/sources/{source_id}/request_patch/variants",
                "post",
                "RequestPatchVariantAggregate",
            ),
            (
                "/ai/manager/api/model/{model_id}/sources/{source_id}/request_patch/variants/{variant_id}",
                "put",
                "RequestPatchVariantAggregate",
            ),
            (
                "/ai/manager/api/model/{model_id}/sources/{source_id}/request_patch/variants/{variant_id}",
                "delete",
                "RequestPatchVariantAggregate",
            ),
            (
                "/ai/manager/api/model/{model_id}/sources/{source_id}/request_patch/preview",
                "post",
                "RequestPatchPreview",
            ),
            (
                "/ai/manager/api/model/{model_id}/sources/{source_id}/request_patch/explain",
                "get",
                "RequestPatchExplain",
            ),
        ];
        for (path, method, response_name) in expected_operations {
            let operation = &document["paths"][path][method];
            assert!(operation.is_mapping(), "missing {method} {path}");
            let success_ref = operation["responses"]["200"]["$ref"]
                .as_str()
                .expect("success response must reference an explicit DTO");
            assert_eq!(
                success_ref,
                format!("#/components/responses/{response_name}")
            );
            assert_eq!(
                document["components"]["responses"][response_name]["headers"]["Cache-Control"]
                    ["$ref"]
                    .as_str(),
                Some("#/components/headers/NoStore")
            );
        }

        for schema in [
            "RequestPatchRuleInput",
            "RequestPatchVariantInput",
            "RequestPatchPreviewInput",
            "RequestPatchVariant",
            "RequestPatchRule",
            "RequestPatchVariantAggregate",
            "RequestPatchVariantListData",
            "ModelRequestPatchOverviewData",
            "RequestPatchVariantPreview",
            "RequestPatchEvaluation",
        ] {
            assert_eq!(
                document["components"]["schemas"][schema]["additionalProperties"].as_bool(),
                Some(false),
                "{schema} must reject unknown fields"
            );
        }
        assert!(
            document["components"]["schemas"]["RequestPatchVariantInput"]["required"]
                .as_sequence()
                .expect("Variant input required fields should exist")
                .contains(&serde_yaml::Value::from("source_id"))
        );

        let model_detail = &document["components"]["schemas"]["ModelDetail"];
        let model_detail_properties = model_detail["properties"]
            .as_mapping()
            .expect("Model detail properties should exist");
        assert!(
            model_detail_properties
                .contains_key(serde_yaml::Value::from("request_patch_variants",))
        );
        assert!(
            document["components"]["schemas"]["RequestPatchPreviewData"]["required"]
                .as_sequence()
                .expect("Preview response required fields should exist")
                .contains(&serde_yaml::Value::from("evaluation"))
        );
        for forbidden in [
            "request_patches",
            "inherited_request_patches",
            "effective_request_patches",
            "request_patch_explain",
            "request_patch_conflicts",
            "has_request_patch_conflicts",
        ] {
            assert!(
                !model_detail_properties.contains_key(serde_yaml::Value::from(forbidden)),
                "Model detail must not expose retired {forbidden}"
            );
        }
        let provider_detail_properties =
            document["components"]["schemas"]["ProviderDetail"]["properties"]
                .as_mapping()
                .expect("Provider detail properties should exist");
        assert!(
            provider_detail_properties
                .contains_key(serde_yaml::Value::from("request_patch_variants",))
        );
        assert!(
            !provider_detail_properties.contains_key(serde_yaml::Value::from("request_patches"))
        );

        for legacy_path in [
            "/ai/manager/api/provider/{provider_id}/request_patch",
            "/ai/manager/api/model/{model_id}/request_patches",
        ] {
            assert!(
                document["paths"][legacy_path].is_null(),
                "legacy path {legacy_path}"
            );
        }
        for path in document["paths"]
            .as_mapping()
            .expect("OpenAPI paths should be a mapping")
            .keys()
            .filter_map(serde_yaml::Value::as_str)
        {
            assert!(!path.contains("reasoning"), "retired reasoning path {path}");
            assert!(
                !path.contains("runtime_feature"),
                "retired runtime feature path {path}"
            );
        }
    }

    fn seed_provider(provider_id: i64, source_id: i64) -> (Provider, Model) {
        let provider = Provider::create(
            &NewProvider {
                id: provider_id,
                provider_key: format!("provider-{provider_id}"),
                name: format!("Provider {provider_id}"),
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            &NewUpstreamSource {
                id: source_id,
                provider_id,
                profile_type: UpstreamProfileType::Openai,
                endpoint: format!("https://source-{source_id}.example/v1"),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                created_at: 1,
                updated_at: 1,
            },
        )
        .expect("provider seed should succeed")
        .provider;
        let model =
            Model::create(provider.id, "model-a", None, true).expect("model seed should succeed");
        (provider, model)
    }

    async fn send(app_state: &Arc<AppState>, request: Request<Body>) -> axum::response::Response {
        create_request_patch_router()
            .with_state(Arc::clone(app_state))
            .oneshot(request)
            .await
            .expect("request patch router should respond")
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
        serde_json::from_slice(&body).expect("response should be JSON")
    }

    fn source_variant_payload(source_id: i64, suffix: &str, target: &str) -> Value {
        json!({
            "source_id": source_id,
            "model_id": null,
            "suffix": suffix,
            "enabled": true,
            "expose_in_models": true,
            "rules": [{
                "placement": RequestPatchPlacement::Body,
                "target": target,
                "operation": RequestPatchOperation::Set,
                "value_json": 0.2,
                "description": "controller test",
                "confirm_dangerous_target": false
            }]
        })
    }

    #[tokio::test]
    async fn aggregate_source_routes_support_crud_preview_explain_and_negative_legacy_route() {
        let database = TestDbContext::new_sqlite("controller-request-patch-aggregate.sqlite");
        database
            .run_async(async {
                let (provider, model) = seed_provider(8501, 8511);
                let app_state = create_test_app_state(database.clone()).await;
                let source_id = 8511;
                let base = format!(
                    "/provider/{}/sources/{source_id}/request_patch",
                    provider.id
                );
                let variants = format!("{base}/variants");
                assert!(
                    app_state
                        .admin
                        .request_patch
                        .mutation_runner()
                        .drain_audit_events()
                        .is_empty()
                );

                let created = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &variants,
                        source_variant_payload(source_id, "fast", "/options/temperature"),
                    ),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let created_body = response_json(created).await;
                let variant_id = created_body["data"]["variant"]["id"]
                    .as_i64()
                    .expect("created Variant id should exist");
                assert_eq!(
                    app_state
                        .admin
                        .request_patch
                        .mutation_runner()
                        .drain_audit_events()
                        .len(),
                    1
                );

                let listed = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(&base)
                        .body(Body::empty())
                        .expect("list request should build"),
                )
                .await;
                assert_eq!(listed.status(), StatusCode::OK);
                let listed_body = response_json(listed).await;
                assert_eq!(listed_body["data"]["variant_count"], 1);
                assert_eq!(listed_body["data"]["rule_count"], 1);

                let preview = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("{base}/preview"),
                        json!({
                            "source_id": source_id,
                            "model_id": null,
                            "suffix": "secret",
                            "enabled": true,
                            "expose_in_models": true,
                            "rules": [{
                                "placement": "HEADER",
                                "target": "Authorization",
                                "operation": "SET",
                                "value_json": "would-not-be-saved",
                                "description": null,
                                "confirm_dangerous_target": false
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(preview.status(), StatusCode::OK);
                let preview_body = response_json(preview).await;
                assert_eq!(preview_body["data"]["preview"]["valid"], false);
                assert_eq!(
                    preview_body["data"]["preview"]["dangerous_targets"][0]["target"],
                    "authorization"
                );
                assert_eq!(preview_body["data"]["preview"]["valid"], false);
                assert!(preview_body["data"]["evaluation"].is_null());
                assert!(
                    app_state
                        .admin
                        .request_patch
                        .mutation_runner()
                        .drain_audit_events()
                        .is_empty(),
                    "Preview must not emit an audit event"
                );

                let confirmed_preview = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("{base}/preview"),
                        json!({
                            "source_id": source_id,
                            "model_id": null,
                            "suffix": "secret",
                            "enabled": true,
                            "expose_in_models": true,
                            "rules": [{
                                "placement": "HEADER",
                                "target": "Authorization",
                                "operation": "SET",
                                "value_json": "would-not-be-saved",
                                "description": null,
                                "confirm_dangerous_target": true
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(confirmed_preview.status(), StatusCode::OK);
                assert_eq!(
                    response_json(confirmed_preview).await["data"]["preview"]["valid"],
                    true
                );
                assert!(
                    app_state
                        .admin
                        .request_patch
                        .mutation_runner()
                        .drain_audit_events()
                        .is_empty(),
                    "confirmed Preview must not emit an audit event"
                );

                let listed_after_preview = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(&base)
                        .body(Body::empty())
                        .expect("post-preview list request should build"),
                )
                .await;
                assert_eq!(listed_after_preview.status(), StatusCode::OK);
                assert_eq!(
                    response_json(listed_after_preview).await["data"]["variant_count"],
                    1,
                    "Preview must not persist a candidate Variant"
                );

                let explained = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(format!("{base}/explain?suffix=fast"))
                        .body(Body::empty())
                        .expect("explain request should build"),
                )
                .await;
                assert_eq!(explained.status(), StatusCode::OK);
                let explained_body = response_json(explained).await;
                assert_eq!(explained_body["data"]["evaluation"]["executable"], true);
                assert_eq!(
                    explained_body["data"]["evaluation"]["effective_rules"]
                        .as_array()
                        .map(Vec::len),
                    Some(1)
                );

                let updated = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("{variants}/{variant_id}"),
                        source_variant_payload(source_id, "fast", "/options/top_p"),
                    ),
                )
                .await;
                assert_eq!(updated.status(), StatusCode::OK);
                let updated_body = response_json(updated).await;
                assert_eq!(updated_body["data"]["variant"]["id"], variant_id);
                assert_eq!(updated_body["data"]["rules"][0]["target"], "/options/top_p");

                let deleted = send(
                    &app_state,
                    Request::builder()
                        .method(Method::DELETE)
                        .uri(format!("{variants}/{variant_id}"))
                        .body(Body::empty())
                        .expect("delete request should build"),
                )
                .await;
                assert_eq!(deleted.status(), StatusCode::OK);
                assert!(response_json(deleted).await["data"]["variant"]["deleted_at"].is_number());

                let old_route = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(format!("/provider/{}/request_patch", provider.id))
                        .body(Body::empty())
                        .expect("legacy request should build"),
                )
                .await;
                assert_eq!(old_route.status(), StatusCode::NOT_FOUND);

                let _ = model;
            })
            .await;
    }

    #[tokio::test]
    async fn aggregate_model_source_route_rejects_cross_owner_payload() {
        let database = TestDbContext::new_sqlite("controller-request-patch-owner.sqlite");
        database
            .run_async(async {
                let (provider, model) = seed_provider(8601, 8611);
                let app_state = create_test_app_state(database.clone()).await;
                let response = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!(
                            "/model/{}/sources/{}/request_patch/variants",
                            model.id, 8611
                        ),
                        json!({
                            "source_id": 8611,
                            "model_id": provider.id,
                            "suffix": "fast",
                            "enabled": true,
                            "expose_in_models": true,
                            "rules": []
                        }),
                    ),
                )
                .await;
                assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            })
            .await;
    }

    #[tokio::test]
    async fn aggregate_model_source_routes_cover_crud_overview_tombstone_preview_explain_and_rollback()
     {
        let database = TestDbContext::new_sqlite("controller-request-patch-model-source.sqlite");
        database
            .run_async(async {
                let (provider, model) = seed_provider(8701, 8711);
                let app_state = create_test_app_state(database.clone()).await;
                let source_base =
                    format!("/provider/{}/sources/{}/request_patch", provider.id, 8711);
                let source_variant = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("{source_base}/variants"),
                        source_variant_payload(8711, "fast", "/options/temperature"),
                    ),
                )
                .await;
                assert_eq!(source_variant.status(), StatusCode::OK);

                let base = format!("/model/{}/sources/{}/request_patch", model.id, 8711);
                let variants = format!("{base}/variants");
                let tombstone_payload = json!({
                    "source_id": 8711,
                    "model_id": model.id,
                    "suffix": "fast",
                    "enabled": true,
                    "expose_in_models": true,
                    "rules": []
                });
                let created = send(
                    &app_state,
                    json_request(Method::POST, &variants, tombstone_payload.clone()),
                )
                .await;
                assert_eq!(created.status(), StatusCode::OK);
                let created_body = response_json(created).await;
                let variant_id = created_body["data"]["variant"]["id"]
                    .as_i64()
                    .expect("Model+Source Variant id should exist");
                assert_eq!(
                    created_body["data"]["rules"].as_array().map(Vec::len),
                    Some(0)
                );

                let listed = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(&base)
                        .body(Body::empty())
                        .expect("Model+Source list request should build"),
                )
                .await;
                assert_eq!(listed.status(), StatusCode::OK);
                let listed_body = response_json(listed).await;
                assert_eq!(listed_body["data"]["variant_count"], 1);
                assert_eq!(listed_body["data"]["rule_count"], 0);

                let overview = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(format!("/model/{}/request_patch", model.id))
                        .body(Body::empty())
                        .expect("Model overview request should build"),
                )
                .await;
                assert_eq!(overview.status(), StatusCode::OK);
                let overview_body = response_json(overview).await;
                assert_eq!(overview_body["data"]["model_id"], model.id);
                assert_eq!(overview_body["data"]["variant_count"], 1);
                assert_eq!(overview_body["data"]["rule_count"], 0);

                let preview = send(
                    &app_state,
                    json_request(
                        Method::POST,
                        &format!("{base}/preview"),
                        json!({
                            "variant_id": variant_id,
                            "source_id": 8711,
                            "model_id": model.id,
                            "suffix": "fast",
                            "enabled": true,
                            "expose_in_models": true,
                            "rules": []
                        }),
                    ),
                )
                .await;
                assert_eq!(preview.status(), StatusCode::OK);
                let preview_body = response_json(preview).await;
                assert_eq!(preview_body["data"]["preview"]["valid"], true);
                assert_eq!(preview_body["data"]["preview"]["rule_count"], 0);
                assert_eq!(
                    preview_body["data"]["evaluation"]["layers"]
                        .as_array()
                        .map(Vec::len),
                    Some(4)
                );
                assert_eq!(
                    preview_body["data"]["evaluation"]["effective_rules"]
                        .as_array()
                        .map(Vec::len),
                    Some(1)
                );

                let explained = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(format!("{base}/explain?suffix=fast"))
                        .body(Body::empty())
                        .expect("Model+Source explain request should build"),
                )
                .await;
                assert_eq!(explained.status(), StatusCode::OK);
                let explained_body = response_json(explained).await;
                assert_eq!(explained_body["data"]["model_id"], model.id);
                assert_eq!(explained_body["data"]["evaluation"]["executable"], true);
                assert_eq!(
                    explained_body["data"]["evaluation"]["effective_rules"]
                        .as_array()
                        .map(Vec::len),
                    Some(1),
                    "empty Model suffix tombstone should inherit the Source rule"
                );

                let invalid_update = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("{variants}/{variant_id}"),
                        json!({
                            "source_id": 8711,
                            "model_id": model.id,
                            "suffix": "fast",
                            "enabled": true,
                            "expose_in_models": true,
                            "rules": [{
                                "placement": "BODY",
                                "target": "not-a-json-pointer",
                                "operation": "SET",
                                "value_json": true,
                                "description": null
                            }]
                        }),
                    ),
                )
                .await;
                assert_eq!(invalid_update.status(), StatusCode::BAD_REQUEST);

                let updated = send(
                    &app_state,
                    json_request(
                        Method::PUT,
                        &format!("{variants}/{variant_id}"),
                        json!({
                            "source_id": 8711,
                            "model_id": model.id,
                            "suffix": "fast",
                            "enabled": false,
                            "expose_in_models": false,
                            "rules": []
                        }),
                    ),
                )
                .await;
                assert_eq!(updated.status(), StatusCode::OK);
                assert_eq!(
                    response_json(updated).await["data"]["variant"]["enabled"],
                    false
                );

                let deleted = send(
                    &app_state,
                    Request::builder()
                        .method(Method::DELETE)
                        .uri(format!("{variants}/{variant_id}"))
                        .body(Body::empty())
                        .expect("Model+Source delete request should build"),
                )
                .await;
                assert_eq!(deleted.status(), StatusCode::OK);
                assert!(response_json(deleted).await["data"]["variant"]["deleted_at"].is_number());

                let listed_after_delete = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(&base)
                        .body(Body::empty())
                        .expect("post-delete Model+Source list request should build"),
                )
                .await;
                assert_eq!(listed_after_delete.status(), StatusCode::OK);
                assert_eq!(
                    response_json(listed_after_delete).await["data"]["variant_count"],
                    0
                );

                let old_route = send(
                    &app_state,
                    Request::builder()
                        .method(Method::GET)
                        .uri(format!("/model/{}/request_patches", model.id))
                        .body(Body::empty())
                        .expect("legacy Model request should build"),
                )
                .await;
                assert_eq!(old_route.status(), StatusCode::NOT_FOUND);
            })
            .await;
    }
}
