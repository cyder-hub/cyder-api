use crate::database::{
    access_control::{
        ApiAccessControlPolicy, ApiCreateAccessControlPolicyPayload,
        ApiUpdateAccessControlPolicyPayload, AccessControlPolicy, // Renamed
    },
    DbResult, // This is Result<T, BaseError>
};
use axum::{
    extract::{Path, State}, // Added State
    routing::{delete, get, post, put},
    Json, 
};
use reqwest::StatusCode;
use std::sync::Arc; // Added Arc for AppState
use crate::service::app_state::{create_state_router, StateRouter, AppState}; // Added AppState
// serde::Deserialize is implicitly used by Json extractor for payloads.

use crate::utils::HttpResult;
use super::BaseError;

// Payload structs (InsertPayload) are now defined in database/limit_strategy.rs
// as ApiCreateAccessControlPolicyPayload and ApiUpdateAccessControlPolicyPayload.

// Handler to list all policies with their rules
async fn list() -> Result<(StatusCode, HttpResult<Vec<ApiAccessControlPolicy>>), BaseError> { // Renamed
    let result = AccessControlPolicy::list_all()?; // Renamed
    Ok((StatusCode::OK, HttpResult::new(result)))
}

// Handler to insert a new policy
async fn insert(
    Json(payload): Json<ApiCreateAccessControlPolicyPayload>, 
) -> DbResult<HttpResult<ApiAccessControlPolicy>> { 
    // The logic for creating policy and rules is now within AccessControlPolicy::create
    let created_policy_from_db = AccessControlPolicy::create(payload)?;
    
    Ok(HttpResult::new(created_policy_from_db))
}

// Handler to get a single policy by ID, including its rules
async fn get_one(Path(id): Path<i64>) -> Result<HttpResult<ApiAccessControlPolicy>, BaseError> { // Renamed
    // AccessControlPolicy::get_by_id returns ApiAccessControlPolicy
    match AccessControlPolicy::get_by_id(id) { // Renamed
        Ok(policy_with_rules) => Ok(HttpResult::new(policy_with_rules)), // Renamed
        Err(err) => Err(err),
    }
}

// Handler to update an existing policy's main fields and rules
async fn update_one(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<ApiUpdateAccessControlPolicyPayload>, 
) -> Result<HttpResult<ApiAccessControlPolicy>, BaseError> { 
    // The logic for updating policy and rules is now within AccessControlPolicy::update
    let updated_policy_from_db = AccessControlPolicy::update(id, payload)?;
    
    // Invalidate from cache
    if let Err(e) = app_state.invalidate_access_control_policy(id).await {
        use cyder_tools::log::warn;
        warn!("Failed to invalidate AccessControlPolicy id {} from cache: {:?}", id, e);
    }

    Ok(HttpResult::new(updated_policy_from_db))
}

// Handler to delete a policy and its associated rules
async fn delete_one(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>
) -> Result<HttpResult<()>, BaseError> {
    // AccessControlPolicy::delete now handles soft-deleting policy and rules
    AccessControlPolicy::delete(id)?; // Ensure DB operation is successful
    
    // Invalidate from cache
    if let Err(e) = app_state.invalidate_access_control_policy(id).await {
        use cyder_tools::log::warn;
        warn!("Failed to invalidate AccessControlPolicy id {} from cache: {:?}", id, e);
    }

    Ok(HttpResult::new(()))
}

// Function to create the router for access control policy endpoints
pub fn create_access_control_policy_router() -> StateRouter { // Renamed function
    create_state_router().nest(
        "/access_control", // Base path for these routes - Renamed
        create_state_router()
            .route("/", post(insert)) // POST /access_control_policy
            .route("/list", get(list)) // GET /access_control_policy/list
            .route("/{id}", get(get_one)) // GET /access_control_policy/{id}
            .route("/{id}", delete(delete_one)) // DELETE /access_control_policy/{id}
            .route("/{id}", put(update_one)), // PUT /access_control_policy/{id}
    )
}
