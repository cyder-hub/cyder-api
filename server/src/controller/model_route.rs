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
        ApiKeyModelOverride, CreateModelRoutePayload, ModelRoute, ModelRouteDetail,
        ModelRouteListItem, UpdateModelRoutePayload,
    },
    service::app_state::{AppState, StateRouter, create_state_router},
    utils::HttpResult,
};

fn log_model_route_audit(action: &'static str, route: &ModelRoute, candidate_count: Option<usize>) {
    match action {
        "create" => crate::info_event!(
            "manager.model_route_created",
            action = action,
            route_id = route.id,
            route_name = &route.route_name,
            is_enabled = route.is_enabled,
            expose_in_models = route.expose_in_models,
            candidate_count = candidate_count,
        ),
        "update" => crate::info_event!(
            "manager.model_route_updated",
            action = action,
            route_id = route.id,
            route_name = &route.route_name,
            is_enabled = route.is_enabled,
            expose_in_models = route.expose_in_models,
            candidate_count = candidate_count,
        ),
        "delete" => crate::info_event!(
            "manager.model_route_deleted",
            action = action,
            route_id = route.id,
            route_name = &route.route_name,
            is_enabled = route.is_enabled,
            expose_in_models = route.expose_in_models,
            candidate_count = candidate_count,
        ),
        _ => unreachable!("unsupported model route audit action: {action}"),
    }
}

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

    log_model_route_audit("create", &detail.route, Some(detail.candidates.len()));
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

    log_model_route_audit("update", &detail.route, Some(detail.candidates.len()));

    Ok(HttpResult::new(detail))
}

async fn delete_model_route(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<HttpResult<()>, BaseError> {
    let route = ModelRoute::get_by_id(id)?;
    let overrides = ApiKeyModelOverride::list_by_target_route_id(id)?;
    ModelRoute::delete(id)?;

    for override_row in overrides {
        let source_name = override_row.source_name.clone();
        ApiKeyModelOverride::delete(override_row.id)?;
        if let Err(err) = app_state
            .invalidate_api_key_model_override(override_row.api_key_id, &source_name)
            .await
        {
            warn!(
                "Failed to invalidate deleted api key model override {}:{} after route delete {}: {:?}",
                override_row.api_key_id, source_name, id, err
            );
        }
    }

    if let Err(err) = app_state
        .invalidate_model_route(id, Some(&route.route_name))
        .await
    {
        warn!(
            "Failed to invalidate model route cache after delete {}: {:?}",
            id, err
        );
    }

    log_model_route_audit("delete", &route, None);
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
