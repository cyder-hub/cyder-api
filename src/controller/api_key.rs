use crate::database::api_key::ApiKey;
use crate::database::DbResult;
use axum::{
    extract::Path,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::utils::HttpResult;

#[derive(Deserialize)]
struct InsertApiKey {
    key: String,
    name: String,
    description: Option<String>,
}

#[derive(Deserialize)]
struct UpdateApiKey {
    api_key: Option<String>,
    name: Option<String>,
    description: Option<String>,
}

async fn insert_one(Json(payload): Json<InsertApiKey>) -> DbResult<HttpResult<ApiKey>> {
    let api_key = ApiKey::new(payload.key, payload.name, payload.description);
    ApiKey::insert_one(&api_key)?;
    Ok(HttpResult::new(api_key))
}

async fn delete_one(Path(id): Path<i64>) -> DbResult<HttpResult<()>> {
    match ApiKey::delete_one(id) {
        Ok(_) => Ok(HttpResult::new(())),
        Err(err) => Err(err),
    }
}

async fn list() -> DbResult<HttpResult<Vec<ApiKey>>> {
    let result = ApiKey::list().unwrap();
    Ok(HttpResult::new(result))
}

async fn update_one(
    Path(id): Path<i64>,
    Json(payload): Json<UpdateApiKey>,
) -> DbResult<HttpResult<ApiKey>> {
    let mut api_key = ApiKey::query_one(id)?;
    if let Some(key) = payload.api_key {
        api_key.api_key = key;
    }
    if let Some(name) = payload.name {
        api_key.name = name;
    }
    api_key.description = payload.description;
    ApiKey::update_one(&api_key)?;
    Ok(HttpResult::new(api_key))
}

pub fn create_api_key_router() -> Router {
    Router::new().nest(
        "/api_key",
        Router::new()
            .route("/", post(insert_one))
            .route("/{id}", delete(delete_one))
            .route("/{id}", put(update_one))
            .route("/list", get(list)),
    )
}
