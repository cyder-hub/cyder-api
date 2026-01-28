use crate::database::model_alias::{ModelAlias, ModelAliasDetails, UpdateModelAliasData}; // Updated import
use crate::database::DbResult;
use axum::{
    extract::{Path, State},
    routing::{delete, get, post, put},
    Json,
};
use cyder_tools::log::warn;
use serde::Deserialize;
use std::sync::Arc;

use crate::service::app_state::{create_state_router, AppState, StateRouter};
use crate::utils::HttpResult;

#[derive(Deserialize)]
struct CreateAliasRequest {
    alias_name: String,
    target_model_id: i64,
    description: Option<String>,
    priority: Option<i32>,
    is_enabled: bool, // is_enabled is required by ModelAlias::create
}

#[derive(Deserialize)]
struct UpdateAliasRequest {
    alias_name: Option<String>,
    target_model_id: Option<i64>,
    description: Option<Option<String>>, // To allow setting to null
    priority: Option<Option<i32>>,       // To allow setting to null
    is_enabled: Option<bool>,
}

async fn create_alias(
    Json(payload): Json<CreateAliasRequest>,
) -> DbResult<HttpResult<ModelAlias>> {
    let created_alias_from_db = ModelAlias::create(
        &payload.alias_name,
        payload.target_model_id,
        payload.description.as_deref(),
        payload.priority,
        payload.is_enabled,
    )?;
    
    Ok(HttpResult::new(created_alias_from_db))
}

async fn delete_alias(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    let alias_to_delete = ModelAlias::get_by_id(id)?;
    ModelAlias::delete(id)?; // delete returns DbResult<usize>
    
    if let Err(e) = app_state.invalidate_model_alias(&alias_to_delete.alias_name).await {
        warn!("Failed to delete ModelAlias id {} from cache: {:?}", id, e);
    }
    
    Ok(HttpResult::new(()))
}

async fn list_aliases() -> DbResult<HttpResult<Vec<ModelAliasDetails>>> {
    let result = ModelAlias::list_all_details()?;
    Ok(HttpResult::new(result))
}

async fn update_alias(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateAliasRequest>,
) -> DbResult<HttpResult<ModelAlias>> {
    let original_alias = ModelAlias::get_by_id(id)?;
    let update_data = UpdateModelAliasData {
        alias_name: payload.alias_name,
        target_model_id: payload.target_model_id,
        description: payload.description,
        priority: payload.priority,
        is_enabled: payload.is_enabled,
    };
    let updated_alias_from_db = ModelAlias::update(id, &update_data)?;

    // Invalidate cache for the original alias name. This handles name changes correctly.
    if let Err(e) = app_state.invalidate_model_alias(&original_alias.alias_name).await {
        warn!(
            "Failed to invalidate ModelAlias id {} ('{}') in cache: {:?}",
            id, original_alias.alias_name, e
        );
    }

    Ok(HttpResult::new(updated_alias_from_db))
}

async fn get_alias(Path(id): Path<i64>) -> DbResult<HttpResult<ModelAlias>> {
    let model_alias = ModelAlias::get_by_id(id)?;
    Ok(HttpResult::new(model_alias))
}

pub fn create_model_alias_router() -> StateRouter {
    create_state_router().nest(
        "/model_alias", // Changed route prefix
        create_state_router()
            .route("/", post(create_alias))
            .route("/{id}", delete(delete_alias))
            .route("/{id}", put(update_alias))
            .route("/{id}", get(get_alias))
            .route("/list", get(list_aliases)),
    )
}
