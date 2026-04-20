use crate::service::app_state::{AppState, StateRouter, create_state_router}; // Added AppState
use crate::{
    controller::BaseError,
    database::model::{Model, ModelDetail, ModelSummaryItem, UpdateModelData},
    utils::HttpResult, // Import HttpResult
};
use axum::{
    extract::{Json, Path, State}, // Added State
    routing::{delete, get, post, put},
};
use cyder_tools::log::warn;
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

    if let Err(store_err) = app_state.invalidate_models_catalog().await {
        warn!(
            "Failed to invalidate models catalog after model create {}: {:?}",
            created_model.id, store_err
        );
    }

    Ok(HttpResult::new(created_model))
}

async fn delete_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    let num_deleted = Model::delete(id)?;

    if num_deleted > 0 {
        // Invalidate from cache
        if let Err(store_err) = app_state.invalidate_model(id, None).await {
            warn!(
                "Failed to invalidate model from cache after DB delete {}: {:?}",
                id, store_err
            );
        }
    }
    Ok(HttpResult::new(()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateModelRequest {
    // pub provider_id: Option<i64>, // Removed: Provider ID is not updatable this way
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub cost_catalog_id: Option<i64>,
}

async fn update_model(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(request): Json<UpdateModelRequest>,
) -> Result<HttpResult<Model>, BaseError> {
    let update_data = UpdateModelData {
        model_name: Some(request.model_name),
        real_model_name: Some(request.real_model_name),
        is_enabled: Some(request.is_enabled),
        cost_catalog_id: Some(request.cost_catalog_id),
    };
    let updated_model = Model::update(id, &update_data)?;

    // Invalidate from cache
    if let Err(store_err) = app_state.invalidate_model(id, None).await {
        warn!(
            "Failed to invalidate model in cache after DB update {}: {:?}",
            id, store_err
        );
    }

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

pub fn create_model_router() -> StateRouter {
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

#[cfg(test)]
mod tests {
    use crate::database::model::ModelSummaryItem;
    use crate::utils::HttpResult;
    use std::collections::BTreeSet;

    #[test]
    fn model_summary_api_contract_includes_provider_context() {
        let payload = HttpResult::new(vec![ModelSummaryItem {
            id: 7,
            provider_id: 3,
            provider_key: "openai-api-example-com".to_string(),
            provider_name: "OpenAI api.example.com".to_string(),
            model_name: "gpt-4o-mini".to_string(),
            real_model_name: Some("gpt-4o-mini-2024-07-18".to_string()),
            is_enabled: true,
        }]);

        let value = serde_json::to_value(payload).expect("summary payload should serialize");
        let root = value.as_object().expect("payload should be an object");
        assert_eq!(
            root.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from(["code".to_string(), "data".to_string()])
        );
        assert_eq!(root["code"], 0);

        let items = root["data"].as_array().expect("data should be an array");
        let item = items[0].as_object().expect("summary row should be an object");
        assert_eq!(
            item.keys().cloned().collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "id".to_string(),
                "provider_id".to_string(),
                "provider_key".to_string(),
                "provider_name".to_string(),
                "model_name".to_string(),
                "real_model_name".to_string(),
                "is_enabled".to_string(),
            ])
        );
        assert_eq!(item["provider_id"], 3);
        assert_eq!(item["provider_key"], "openai-api-example-com");
        assert_eq!(item["provider_name"], "OpenAI api.example.com");
        assert_eq!(item["model_name"], "gpt-4o-mini");
        assert_eq!(item["real_model_name"], "gpt-4o-mini-2024-07-18");
        assert_eq!(item["is_enabled"], true);
        assert!(item.get("model").is_none());
        assert!(item.get("custom_fields").is_none());
    }
}
