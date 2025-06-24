use axum::{
    extract::{Path, State}, // Added State
    response::Json,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use crate::service::app_state::{create_state_router, StateRouter, AppState}; // Added AppState
use std::sync::Arc; // Added Arc
use crate::{
    controller::BaseError,
    database::model::{Model, ModelDetail, UpdateModelData},
    utils::HttpResult, // Import HttpResult
};

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
}

async fn insert_model(
    State(app_state): State<Arc<AppState>>,
    Json(request): Json<InsertModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let created_model = Model::create(
        request.provider_id,
        &request.model_name,
        request.real_model_name.as_deref(),
        request.is_enabled,
    )?;

    // Add to ModelStore
    if let Err(store_err) = app_state.model_store.add(created_model.clone(), &app_state.provider_store) {
        // Log the error, but the DB operation was successful, so we might not want to fail the request.
        // Depending on desired consistency, this could return an error or just log.
        eprintln!("Failed to add model to store after DB insert: {:?}", store_err);
    }

    Ok(HttpResult::new(created_model))
}

async fn delete_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>
) -> Result<HttpResult<()>, BaseError> {
    let num_deleted = Model::delete(id)?;

    if num_deleted > 0 {
        // Remove from ModelStore
        if let Err(store_err) = app_state.model_store.delete(id, &app_state.provider_store) {
            eprintln!("Failed to delete model from store after DB delete: {:?}", store_err);
            // Potentially return an error or handle inconsistency
        }
    }
    Ok(HttpResult::new(()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelRequest {
    // pub provider_id: Option<i64>, // Removed: Provider ID is not updatable this way
    pub model_name: Option<String>,
    pub real_model_name: Option<Option<String>>, // Allow setting real_model_name to null
    pub is_enabled: Option<bool>,
    pub billing_plan_id: Option<Option<i64>>,
}

async fn update_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let update_data = UpdateModelData {
        model_name: request.model_name,
        real_model_name: request.real_model_name,
        is_enabled: request.is_enabled,
        billing_plan_id: request.billing_plan_id,
    };
    let updated_model = Model::update(id, &update_data)?;

    // Update in ModelStore
    if let Err(store_err) = app_state.model_store.update(updated_model.clone(), &app_state.provider_store) {
        eprintln!("Failed to update model in store after DB update: {:?}", store_err);
        // Potentially return an error or handle inconsistency
    }

    Ok(HttpResult::new(updated_model))
}

async fn list_models() -> Result<HttpResult<Vec<Model>>, BaseError> {
    let models = Model::list_all()?; // Use list_all
    Ok(HttpResult::new(models))
}

async fn get_model_detail(Path(id): Path<i64>) -> Result<HttpResult<ModelDetail>, BaseError> {
    let detail = Model::get_detail_by_id(id)?;
    Ok(HttpResult::new(detail))
}

// Price related structs and functions (InsertPriceRequest, insert_model_price, list_model_prices)
// are removed as they are not supported by the new server/src/database/model.rs.

pub fn create_model_router() -> StateRouter {
    create_state_router().nest(
        "/model",
        create_state_router()
            .route("/", post(insert_model))
            .route("/list", get(list_models))
            .route("/{id}", delete(delete_model))
            .route("/{id}", put(update_model))
            .route("/{id}/detail", get(get_model_detail)),
        // .route("/{id}/prices", get(list_model_prices)) // Removed price route
        // .route("/{id}/price", post(insert_model_price)), // Removed price route
    )
}
