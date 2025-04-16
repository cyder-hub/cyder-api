use crate::database::{
    limit_strategy::{LimitStrategyDetail, QuotaLimitItemPayload, ResourceLimitItemPayload},
    DbResult,
};
use axum::{
    extract::Path,
    routing::{delete, get, post, put},
    Json, Router,
};
use reqwest::StatusCode;
use serde::Deserialize; // Added Deserialize import

use crate::database::limit_strategy::{LimitStrategy, LimitStrategyItem, LimitStrategyWithItems}; // Keep LimitStrategyItem for mapping
use crate::utils::HttpResult;

use super::BaseError;

// Payload for creating a new LimitStrategy
#[derive(Deserialize, Debug)]
struct InsertPayload {
    pub main_strategy: String,
    pub name: String,
    pub description: Option<String>,
    pub white_list: Vec<ResourceLimitItemPayload>,
    pub black_list: Vec<ResourceLimitItemPayload>,
    pub quota_list: Vec<QuotaLimitItemPayload>,
}

// Handler to list all strategies with their items
async fn list() -> Result<(StatusCode, HttpResult<Vec<LimitStrategyDetail>>), BaseError> {
    let result = LimitStrategy::list()?;
    Ok((StatusCode::OK, HttpResult::new(result)))
}

// Handler to insert a new strategy
async fn insert(Json(payload): Json<InsertPayload>) -> DbResult<HttpResult<LimitStrategy>> {
    let strategy = LimitStrategy::new(
        None,
        &payload.main_strategy,
        &payload.name,
        payload.description.as_deref(),
    );

    let white_list: Vec<LimitStrategyItem> = payload
        .white_list
        .into_iter()
        .map(|item_payload| LimitStrategyItem::from_white(&strategy, item_payload))
        .collect();
    let black_list: Vec<LimitStrategyItem> = payload
        .black_list
        .into_iter()
        .map(|item_payload| LimitStrategyItem::from_black(&strategy, item_payload))
        .collect();
    let quota_list: Vec<LimitStrategyItem> = payload
        .quota_list
        .into_iter()
        .map(|item_payload| LimitStrategyItem::from_quota(&strategy, item_payload))
        .collect();

    let mut items = vec![];
    items.extend(white_list);
    items.extend(black_list);
    items.extend(quota_list);

    // Use the function that inserts strategy and items
    LimitStrategy::insert_with_items(&strategy, &items)?;

    // Return the created strategy (items are not included in the return value here)
    Ok(HttpResult::new(strategy))
}

// Handler to get a single strategy by ID, including its items
async fn get_one(Path(id): Path<i64>) -> Result<HttpResult<LimitStrategyDetail>, BaseError> {
    match LimitStrategy::query_one_detail(id) {
        Ok(strategy_with_items) => Ok(HttpResult::new(strategy_with_items)),
        Err(err) => Err(err),
    }
}

// Handler to update an existing strategy's main fields
async fn update_one(
    Path(id): Path<i64>,
    Json(payload): Json<InsertPayload>,
) -> Result<HttpResult<LimitStrategy>, BaseError> {
    // Create a strategy object with the updated fields
    let strategy_to_update = LimitStrategy::new(
        Some(id), // Use the provided ID
        &payload.main_strategy,
        &payload.name,
        payload.description.as_deref(),
    );

    // --- Start: Added item mapping logic from insert ---
    let white_list: Vec<LimitStrategyItem> = payload
        .white_list
        .into_iter()
        .map(|item_payload| LimitStrategyItem::from_white(&strategy_to_update, item_payload)) // Use strategy_to_update
        .collect();
    let black_list: Vec<LimitStrategyItem> = payload
        .black_list
        .into_iter()
        .map(|item_payload| LimitStrategyItem::from_black(&strategy_to_update, item_payload)) // Use strategy_to_update
        .collect();
    let quota_list: Vec<LimitStrategyItem> = payload
        .quota_list
        .into_iter()
        .map(|item_payload| LimitStrategyItem::from_quota(&strategy_to_update, item_payload)) // Use strategy_to_update
        .collect();

    let mut items = vec![];
    items.extend(white_list);
    items.extend(black_list);
    items.extend(quota_list);
    // --- End: Added item mapping logic from insert ---

    // Use the function that updates strategy, deletes old items, and inserts new ones
    let updated_strategy = LimitStrategy::update_with_items(&strategy_to_update, &items)?; // Pass combined items

    Ok(HttpResult::new(updated_strategy))
}

// Handler to delete a strategy and its associated items
async fn delete_one(Path(id): Path<i64>) -> Result<HttpResult<()>, BaseError> {
    match LimitStrategy::delete_one(id) {
        Ok(_) => Ok(HttpResult::new(())),
        Err(err) => Err(err),
    }
}

// Function to create the router for limit strategy endpoints
pub fn create_limit_strategy_router() -> Router {
    Router::new().nest(
        "/limit_strategy", // Base path for these routes
        Router::new()
            .route("/", post(insert)) // POST /limit_strategy
            .route("/list", get(list)) // GET /limit_strategy/list
            .route("/{id}", get(get_one)) // GET /limit_strategy/{id}
            .route("/{id}", delete(delete_one)) // DELETE /limit_strategy/{id}
            .route("/{id}", put(update_one)), // PUT /limit_strategy/{id}
    )
}
