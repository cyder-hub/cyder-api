use crate::{
    controller::BaseError,
    database::model::{Model, ModelCapabilityFlags, ModelDetail, ModelSummaryItem},
    service::{
        admin::model::{CreateModelInput, UpdateModelInput},
        app_state::{AppState, StateRouter, create_state_router},
    },
    utils::HttpResult, // Import HttpResult
};
use axum::{
    extract::{Json, Path, State}, // Added State
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use std::sync::Arc; // Added Arc

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct InsertModelRequest {
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    #[serde(default = "default_true")]
    pub is_enabled: bool,
    #[serde(default = "default_true")]
    pub supports_streaming: bool,
    #[serde(default = "default_true")]
    pub supports_tools: bool,
    #[serde(default = "default_true")]
    pub supports_reasoning: bool,
    #[serde(default = "default_true")]
    pub supports_image_input: bool,
    #[serde(default = "default_true")]
    pub supports_embeddings: bool,
    #[serde(default = "default_true")]
    pub supports_rerank: bool,
}

async fn insert_model(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<InsertModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let created_model = app_state
        .admin
        .model
        .create_model(CreateModelInput {
            provider_id: request.provider_id,
            model_name: request.model_name,
            real_model_name: request.real_model_name,
            is_enabled: request.is_enabled,
            capabilities: ModelCapabilityFlags {
                supports_streaming: request.supports_streaming,
                supports_tools: request.supports_tools,
                supports_reasoning: request.supports_reasoning,
                supports_image_input: request.supports_image_input,
                supports_embeddings: request.supports_embeddings,
                supports_rerank: request.supports_rerank,
            },
        })
        .await?;

    Ok(HttpResult::new(created_model))
}

async fn delete_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    app_state.admin.model.delete_model(id).await?;
    Ok(HttpResult::new(()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelRequest {
    // pub provider_id: Option<i64>, // Removed: Provider ID is not updatable this way
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub cost_catalog_id: Option<i64>,
    pub supports_streaming: Option<bool>,
    pub supports_tools: Option<bool>,
    pub supports_reasoning: Option<bool>,
    pub supports_image_input: Option<bool>,
    pub supports_embeddings: Option<bool>,
    pub supports_rerank: Option<bool>,
}

async fn update_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let updated_model = app_state
        .admin
        .model
        .update_model(
            id,
            UpdateModelInput {
                model_name: request.model_name,
                real_model_name: request.real_model_name,
                is_enabled: request.is_enabled,
                cost_catalog_id: request.cost_catalog_id,
                supports_streaming: request.supports_streaming,
                supports_tools: request.supports_tools,
                supports_reasoning: request.supports_reasoning,
                supports_image_input: request.supports_image_input,
                supports_embeddings: request.supports_embeddings,
                supports_rerank: request.supports_rerank,
            },
        )
        .await?;

    Ok(HttpResult::new(updated_model))
}

async fn list_models() -> Result<HttpResult<Vec<Model>>, BaseError> {
    let models = Model::list_all()?; // Use list_all
    Ok(HttpResult::new(models))
}

async fn list_model_summaries() -> Result<HttpResult<Vec<ModelSummaryItem>>, BaseError> {
    let models = Model::list_summary()?;
    Ok(HttpResult::new(models))
}

async fn get_model_detail(Path(id): Path<i64>) -> Result<HttpResult<ModelDetail>, BaseError> {
    let detail = Model::get_detail_by_id(id)?;
    Ok(HttpResult::new(detail))
}

// Price related structs and functions (InsertPriceRequest, insert_model_price, list_model_prices)
// are removed as they are not supported by the new server/src/database/model.rs.

pub fn create_model_controller_router() -> StateRouter {
    create_state_router().nest(
        "/model",
        create_state_router()
            .route("/", post(insert_model))
            .route("/summary/list", get(list_model_summaries))
            .route("/list", get(list_models))
            .route("/{id}", delete(delete_model))
            .route("/{id}", put(update_model))
            .route("/{id}/detail", get(get_model_detail)),
        // .route("/{id}/prices", get(list_model_prices)) // Removed price route
        // .route("/{id}/price", post(insert_model_price)), // Removed price route
    )
}
