use crate::database::model_transform::ModelTransform;
use crate::database::DbResult;
use axum::{
    extract::Path,
    routing::{delete, get, post, put},
    Json, Router,
};
use serde::Deserialize;

use crate::utils::HttpResult;

#[derive(Deserialize)]
struct InsertModelTransform {
    model_name: String,
    map_model_name: String,
}

#[derive(Deserialize)]
struct UpdateModelTransform {
    model_name: Option<String>,
    map_model_name: Option<String>,
    is_enabled: Option<bool>,
}

async fn insert_one(Json(payload): Json<InsertModelTransform>) -> DbResult<HttpResult<ModelTransform>> {
    let model_transform = ModelTransform::new(payload.model_name, payload.map_model_name);
    ModelTransform::insert_one(&model_transform)?;
    Ok(HttpResult::new(model_transform))
}

async fn delete_one(Path(id): Path<i64>) -> DbResult<HttpResult<()>> {
    match ModelTransform::delete_one(id) {
        Ok(_) => Ok(HttpResult::new(())),
        Err(err) => Err(err),
    }
}

async fn list() -> DbResult<HttpResult<Vec<ModelTransform>>> {
    let result = ModelTransform::list()?;
    Ok(HttpResult::new(result))
}

async fn update_one(
    Path(id): Path<i64>,
    Json(payload): Json<UpdateModelTransform>,
) -> DbResult<HttpResult<ModelTransform>> {
    let mut model_transform = ModelTransform::query_one(id)?;
    if let Some(model_name) = payload.model_name {
        model_transform.model_name = model_name;
    }
    if let Some(map_model_name) = payload.map_model_name {
        model_transform.map_model_name = map_model_name;
    }
    if let Some(is_enabled) = payload.is_enabled {
        model_transform.is_enabled = is_enabled;
    }
    ModelTransform::update_one(&model_transform)?;
    Ok(HttpResult::new(model_transform))
}

async fn query_one(Path(id): Path<i64>) -> DbResult<HttpResult<ModelTransform>> {
    let model_transform = ModelTransform::query_one(id)?;
    Ok(HttpResult::new(model_transform))
}

pub fn create_model_transform_router() -> Router {
    Router::new().nest(
        "/model_transform",
        Router::new()
            .route("/", post(insert_one))
            .route("/{id}", delete(delete_one))
            .route("/{id}", put(update_one))
            .route("/{id}", get(query_one))
            .route("/list", get(list)),
    )
}
