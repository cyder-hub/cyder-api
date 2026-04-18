use axum::{
    Json,
    extract::{Path, State},
    routing::{get, post},
};
use cyder_tools::log::warn;
use std::sync::Arc;

use crate::{
    controller::BaseError,
    database::model_route::{
        CreateModelRoutePayload, ModelRoute, ModelRouteDetail, ModelRouteListItem,
        UpdateModelRoutePayload,
    },
    service::app_state::{AppState, StateRouter, create_state_router},
    utils::HttpResult,
};

async fn create_model_route(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<CreateModelRoutePayload>,
) -> Result<HttpResult<ModelRouteDetail>, BaseError> {
    let detail = ModelRoute::create(&payload)?;
    if let Err(err) = app_state
        .invalidate_model_route(detail.route.id, Some(&detail.route.route_name))
        .await
    {
        warn!(
            "Failed to invalidate model route cache after create {}: {:?}",
            detail.route.id, err
        );
    }
    Ok(HttpResult::new(detail))
}

async fn list_model_routes() -> Result<HttpResult<Vec<ModelRouteListItem>>, BaseError> {
    Ok(HttpResult::new(ModelRoute::list_summary()?))
}

async fn get_model_route(Path(id): Path<i64>) -> Result<HttpResult<ModelRouteDetail>, BaseError> {
    Ok(HttpResult::new(ModelRoute::get_detail(id)?))
}

async fn update_model_route(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateModelRoutePayload>,
) -> Result<HttpResult<ModelRouteDetail>, BaseError> {
    let original_route = ModelRoute::get_by_id(id)?;
    let detail = ModelRoute::update(id, &payload)?;

    if let Err(err) = app_state
        .invalidate_model_route_by_name(&original_route.route_name)
        .await
    {
        warn!(
            "Failed to invalidate stale model route name '{}' after update {}: {:?}",
            original_route.route_name, id, err
        );
    }
    if let Err(err) = app_state
        .invalidate_model_route(detail.route.id, Some(&detail.route.route_name))
        .await
    {
        warn!(
            "Failed to invalidate model route cache after update {}: {:?}",
            detail.route.id, err
        );
    }

    Ok(HttpResult::new(detail))
}

async fn delete_model_route(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    let route = ModelRoute::get_by_id(id)?;
    ModelRoute::delete(id)?;
    if let Err(err) = app_state
        .invalidate_model_route(id, Some(&route.route_name))
        .await
    {
        warn!(
            "Failed to invalidate model route cache after delete {}: {:?}",
            id, err
        );
    }
    Ok(HttpResult::new(()))
}

pub fn create_model_route_router() -> StateRouter {
    create_state_router()
        .route("/model_route", post(create_model_route))
        .nest(
            "/model_route",
        create_state_router()
            .route("/list", get(list_model_routes))
            .route(
                "/{id}",
                get(get_model_route)
                    .put(update_model_route)
                    .delete(delete_model_route),
            ),
        )
}

#[cfg(test)]
mod tests {
    use super::create_model_route_router;

    #[test]
    fn create_model_route_router_registers_routes() {
        let _router = create_model_route_router();
    }
}
