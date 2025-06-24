use crate::database::system_api_key::{SystemApiKey, UpdateSystemApiKeyData}; // Updated import
use crate::database::DbResult;
use axum::{
    extract::{Path, State},
    routing::{delete, get, post, put},
    Json, // Router will be replaced by StateRouter
};
use serde::Deserialize;
use std::sync::Arc;
use crate::service::app_state::{create_state_router, AppState, StateRouter};
use crate::utils::HttpResult;
use cyder_tools::log::warn;

#[derive(Deserialize)]
struct InsertApiKeyRequest { // Renamed for clarity
    api_key_value: String, // Renamed from key
    name: String,
    access_control_policy_id: Option<i64>,
    description: Option<String>,
    // is_enabled is handled by SystemApiKey::create (defaults to true)
}

#[derive(Deserialize)]
struct UpdateApiKeyRequest { // Renamed for clarity
    // api_key field is not updatable via UpdateSystemApiKeyData
    name: Option<String>,
    access_control_policy_id: Option<Option<i64>>, // Allow setting to null
    description: Option<Option<String>>,   // Allow setting to null
    is_enabled: Option<bool>,
}

async fn insert_one(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<InsertApiKeyRequest>
) -> DbResult<HttpResult<SystemApiKey>> {
    let created_api_key = SystemApiKey::create(
        &payload.name,
        &payload.api_key_value,
        payload.description.as_deref(),
        payload.access_control_policy_id,
    )?;

    if let Err(e) = app_state.system_api_key_store.add(created_api_key.clone()) {
        warn!("Failed to add SystemApiKey id {} to store: {:?}", created_api_key.id, e);
    }

    Ok(HttpResult::new(created_api_key))
}

async fn delete_one(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>
) -> DbResult<HttpResult<()>> {
    SystemApiKey::delete(id)?; // delete returns DbResult<usize>

    if let Err(e) = app_state.system_api_key_store.delete(id) {
        warn!("Failed to delete SystemApiKey id {} from store: {:?}", id, e);
    }

    Ok(HttpResult::new(()))
}

async fn list() -> DbResult<HttpResult<Vec<SystemApiKey>>> {
    let result = SystemApiKey::list_all()?;
    Ok(HttpResult::new(result))
}

async fn update_one(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateApiKeyRequest>,
) -> DbResult<HttpResult<SystemApiKey>> {
    let update_data = UpdateSystemApiKeyData {
        name: payload.name,
        description: payload.description,
        access_control_policy_id: payload.access_control_policy_id,
        is_enabled: payload.is_enabled,
    };
    let updated_api_key = SystemApiKey::update(id, &update_data)?;

    if let Err(e) = app_state.system_api_key_store.update(updated_api_key.clone()) {
        warn!("Failed to update SystemApiKey id {} in store: {:?}", updated_api_key.id, e);
    }

    Ok(HttpResult::new(updated_api_key))
}

pub fn create_api_key_router() -> StateRouter {
    create_state_router().nest(
        "/system_api_key",
        create_state_router()
            .route("/", post(insert_one))
            .route("/{id}", delete(delete_one))
            .route("/{id}", put(update_one))
            .route("/list", get(list)),
    )
}
