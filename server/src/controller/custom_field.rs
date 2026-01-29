use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post, put},
    Json, // Router will be replaced by StateRouter
};
// Removed StatusCode as it's not directly used in the success path of list
use crate::service::app_state::{create_state_router, AppState, StateRouter};
// use reqwest::StatusCode;
// serde::Deserialize is implicitly used by Query extractor

use serde_json::json; // For flexible JSON responses

use crate::{
    database::{
        custom_field::{
            ApiCreateCustomFieldDefinitionPayload, ApiCustomFieldDefinition,
            ApiLinkCustomFieldPayload, // New payload
            ApiUnlinkCustomFieldPayload, // New payload
            ApiUpdateCustomFieldDefinitionPayload, CustomFieldDefinition,
            ListByProviderModelQueryPayload, // New payload for filtered list
            ListCustomFieldQueryPayload,
            // ModelCustomFieldAssignment,    // Removed unused import
            // ProviderCustomFieldAssignment, // Removed unused import
        },
        ListResult,
    },
    utils::HttpResult,
};

use super::BaseError;

// ListCustomFieldQueryPayload struct definition is removed from here

async fn list(
    Query(params): Query<ListCustomFieldQueryPayload>,
) -> Result<HttpResult<ListResult<ApiCustomFieldDefinition>>, BaseError> {
    // page and page_size defaults are handled in the database layer
    let result = CustomFieldDefinition::list(params)?;
    Ok(HttpResult::new(result))
}

async fn insert(
    Json(payload): Json<ApiCreateCustomFieldDefinitionPayload>,
) -> Result<HttpResult<ApiCustomFieldDefinition>, BaseError> {
    let created_cfd = CustomFieldDefinition::create(payload)?;
    Ok(HttpResult::new(created_cfd))
}

async fn get_one(Path(id): Path<i64>) -> Result<HttpResult<ApiCustomFieldDefinition>, BaseError> {
    match CustomFieldDefinition::get_by_id(id) {
        Ok(cfd) => Ok(HttpResult::new(cfd)),
        Err(err) => Err(err),
    }
}

async fn update_one(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<ApiUpdateCustomFieldDefinitionPayload>,
) -> Result<HttpResult<ApiCustomFieldDefinition>, BaseError> {
    let updated_cfd = CustomFieldDefinition::update(id, payload)?;
    
    // Update in cache
    if let Err(e) = app_state.invalidate_custom_field(id).await {
        use cyder_tools::log::warn;
        warn!("Failed to update CustomFieldDefinition id {} in cache: {:?}", updated_cfd.id, e);
    }
    
    Ok(HttpResult::new(updated_cfd))
}

async fn delete_one(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    match CustomFieldDefinition::delete(id) {
        Ok(_) => {
            // Delete from cache
            if let Err(e) = app_state.invalidate_custom_field(id).await {
                use cyder_tools::log::warn;
                warn!("Failed to delete CustomFieldDefinition id {} from cache: {:?}", id, e);
            }
            Ok(HttpResult::new(()))
        }
        Err(err) => Err(err),
    }
}

async fn link_custom_field(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<ApiLinkCustomFieldPayload>,
) -> Result<HttpResult<serde_json::Value>, BaseError> {
    let custom_field_definition_id = payload.custom_field_definition_id;
    let is_enabled = payload.is_enabled.unwrap_or(true);

    match (payload.model_id, payload.provider_id) {
        (Some(model_id), None) => {
            let assignment = CustomFieldDefinition::link_model(
                custom_field_definition_id,
                model_id,
                is_enabled,
            )?;
            let _ = app_state.invalidate_custom_field(custom_field_definition_id).await;
            let _ = app_state.invalidate_model_custom_fields(model_id).await;
            Ok(HttpResult::new(json!(assignment)))
        }
        (None, Some(provider_id)) => {
            let assignment = CustomFieldDefinition::link_provider(
                custom_field_definition_id,
                provider_id,
                is_enabled,
            )?;
            let _ = app_state.invalidate_custom_field(custom_field_definition_id).await;
            let _ = app_state.invalidate_provider_custom_fields(provider_id).await;
            Ok(HttpResult::new(json!(assignment)))
        }
        (Some(_), Some(_)) => Err(BaseError::ParamInvalid(Some(
            "Cannot specify both model_id and provider_id.".to_string(),
        ))),
        (None, None) => Err(BaseError::ParamInvalid(Some(
            "Must specify either model_id or provider_id.".to_string(),
        ))),
    }
}

async fn unlink_custom_field(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<ApiUnlinkCustomFieldPayload>,
) -> Result<HttpResult<usize>, BaseError> {
    let custom_field_definition_id = payload.custom_field_definition_id;

    match (payload.model_id, payload.provider_id) {
        (Some(model_id), None) => {
            let count = CustomFieldDefinition::unlink_model(custom_field_definition_id, model_id)?;
            
            // Remove link from cache
            if let Err(e) = app_state.invalidate_model_custom_fields(model_id).await {
                use cyder_tools::log::warn;
                warn!("Failed to remove custom field link from cache: {:?}", e);
            }
            
            Ok(HttpResult::new(count))
        }
        (None, Some(provider_id)) => {
            let count =
                CustomFieldDefinition::unlink_provider(custom_field_definition_id, provider_id)?;
            
            // Remove link from cache
            if let Err(e) = app_state.invalidate_provider_custom_fields(provider_id).await {
                use cyder_tools::log::warn;
                warn!("Failed to remove custom field link from cache: {:?}", e);
            }
            
            Ok(HttpResult::new(count))
        }
        (Some(_), Some(_)) => Err(BaseError::ParamInvalid(Some(
            "Cannot specify both model_id and provider_id.".to_string(),
        ))),
        (None, None) => Err(BaseError::ParamInvalid(Some(
            "Must specify either model_id or provider_id.".to_string(),
        ))),
    }
}

async fn list_filtered_custom_fields(
    Query(params): Query<ListByProviderModelQueryPayload>,
) -> Result<HttpResult<Vec<ApiCustomFieldDefinition>>, BaseError> {
    let result =
        CustomFieldDefinition::list_by_provider_model(params.provider_id, params.model_id)?;
    Ok(HttpResult::new(result))
}

pub fn create_custom_field_router() -> StateRouter {
    create_state_router().nest(
        "/custom_field_definition", // Base path for these routes
        create_state_router()
            .route("/", post(insert)) // POST /custom_field_definition
            .route("/list", get(list)) // GET /custom_field_definition/list
            .route("/list/filter", get(list_filtered_custom_fields)) // GET /custom_field_definition/list/filter
            .route("/link", post(link_custom_field)) // POST /custom_field_definition/link
            .route("/unlink", post(unlink_custom_field)) // POST /custom_field_definition/unlink
            .route("/{id}", get(get_one)) // GET /custom_field_definition/{id}
            .route("/{id}", delete(delete_one)) // DELETE /custom_field_definition/{id}
            .route("/{id}", put(update_one)), // PUT /custom_field_definition/{id}
    )
}
