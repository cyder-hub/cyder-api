use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    routing::{get, put},
};
use cyder_tools::log::warn;
use serde::Serialize;

use crate::{
    database::{
        model::Model,
        request_patch::{
            CreateRequestPatchPayload, RequestPatchMutationOutcome, RequestPatchRule,
            RequestPatchRuleResponse, UpdateRequestPatchPayload,
        },
    },
    service::{
        app_state::{AppState, StateRouter, create_state_router},
        cache::types::{
            CacheRequestPatchConflict, CacheRequestPatchExplainEntry, CacheResolvedRequestPatch,
        },
    },
    utils::HttpResult,
};

use super::BaseError;

#[derive(Debug, Serialize)]
struct ModelRequestPatchEffectiveResponse {
    provider_id: i64,
    model_id: i64,
    effective_rules: Vec<CacheResolvedRequestPatch>,
    conflicts: Vec<CacheRequestPatchConflict>,
    has_conflicts: bool,
}

#[derive(Debug, Serialize)]
struct ModelRequestPatchExplainResponse {
    provider_id: i64,
    model_id: i64,
    direct_rules: Vec<crate::service::cache::types::CacheRequestPatchRule>,
    inherited_rules: Vec<crate::service::cache::types::CacheInheritedRequestPatch>,
    effective_rules: Vec<CacheResolvedRequestPatch>,
    explain: Vec<CacheRequestPatchExplainEntry>,
    conflicts: Vec<CacheRequestPatchConflict>,
    has_conflicts: bool,
}

fn log_request_patch_audit(
    action: &'static str,
    scope_kind: &'static str,
    scope_id: i64,
    rule: &RequestPatchRuleResponse,
    is_enabled: bool,
) {
    match (scope_kind, action) {
        ("provider", "create") => crate::info_event!(
            "manager.provider_request_patch_created",
            action = action,
            scope_kind = scope_kind,
            scope_id = scope_id,
            request_patch_rule_id = rule.id,
            placement = format!("{:?}", rule.placement),
            operation = format!("{:?}", rule.operation),
            is_enabled = is_enabled,
        ),
        ("provider", "update") => crate::info_event!(
            "manager.provider_request_patch_updated",
            action = action,
            scope_kind = scope_kind,
            scope_id = scope_id,
            request_patch_rule_id = rule.id,
            placement = format!("{:?}", rule.placement),
            operation = format!("{:?}", rule.operation),
            is_enabled = is_enabled,
        ),
        ("provider", "delete") => crate::info_event!(
            "manager.provider_request_patch_deleted",
            action = action,
            scope_kind = scope_kind,
            scope_id = scope_id,
            request_patch_rule_id = rule.id,
            placement = format!("{:?}", rule.placement),
            operation = format!("{:?}", rule.operation),
            is_enabled = is_enabled,
        ),
        ("model", "create") => crate::info_event!(
            "manager.model_request_patch_created",
            action = action,
            scope_kind = scope_kind,
            scope_id = scope_id,
            request_patch_rule_id = rule.id,
            placement = format!("{:?}", rule.placement),
            operation = format!("{:?}", rule.operation),
            is_enabled = is_enabled,
        ),
        ("model", "update") => crate::info_event!(
            "manager.model_request_patch_updated",
            action = action,
            scope_kind = scope_kind,
            scope_id = scope_id,
            request_patch_rule_id = rule.id,
            placement = format!("{:?}", rule.placement),
            operation = format!("{:?}", rule.operation),
            is_enabled = is_enabled,
        ),
        ("model", "delete") => crate::info_event!(
            "manager.model_request_patch_deleted",
            action = action,
            scope_kind = scope_kind,
            scope_id = scope_id,
            request_patch_rule_id = rule.id,
            placement = format!("{:?}", rule.placement),
            operation = format!("{:?}", rule.operation),
            is_enabled = is_enabled,
        ),
        _ => unreachable!("unsupported request patch audit action: {scope_kind}:{action}"),
    }
}

async fn list_provider_request_patches(
    Path(provider_id): Path<i64>,
) -> Result<HttpResult<Vec<RequestPatchRuleResponse>>, BaseError> {
    Ok(HttpResult::new(RequestPatchRule::list_by_provider_id(
        provider_id,
    )?))
}

async fn create_provider_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path(provider_id): Path<i64>,
    Json(payload): Json<CreateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = RequestPatchRule::create_for_provider(provider_id, &payload)?;
    if let RequestPatchMutationOutcome::Saved { rule } = &outcome {
        if let Err(err) = app_state
            .invalidate_provider_request_patch_rules(provider_id)
            .await
        {
            warn!(
                "Failed to invalidate provider request patch cache after create: {:?}",
                err
            );
        }

        log_request_patch_audit("create", "provider", provider_id, rule, rule.is_enabled);
    }

    Ok(HttpResult::new(outcome))
}

async fn update_provider_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, rule_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = RequestPatchRule::update_for_provider(provider_id, rule_id, &payload)?;
    if let RequestPatchMutationOutcome::Saved { rule } = &outcome {
        if let Err(err) = app_state
            .invalidate_provider_request_patch_rules(provider_id)
            .await
        {
            warn!(
                "Failed to invalidate provider request patch cache after update: {:?}",
                err
            );
        }

        log_request_patch_audit("update", "provider", provider_id, rule, rule.is_enabled);
    }

    Ok(HttpResult::new(outcome))
}

async fn delete_provider_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((provider_id, rule_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    let rule = RequestPatchRule::get_provider_rule(provider_id, rule_id)?;
    RequestPatchRule::delete_for_provider(provider_id, rule_id)?;
    if let Err(err) = app_state
        .invalidate_provider_request_patch_rules(provider_id)
        .await
    {
        warn!(
            "Failed to invalidate provider request patch cache after delete: {:?}",
            err
        );
    }

    log_request_patch_audit("delete", "provider", provider_id, &rule, false);

    Ok(HttpResult::new(()))
}

async fn list_model_request_patches(
    Path(model_id): Path<i64>,
) -> Result<HttpResult<Vec<RequestPatchRuleResponse>>, BaseError> {
    Ok(HttpResult::new(RequestPatchRule::list_by_model_id(
        model_id,
    )?))
}

async fn create_model_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
    Json(payload): Json<CreateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = RequestPatchRule::create_for_model(model_id, &payload)?;
    if let RequestPatchMutationOutcome::Saved { rule } = &outcome {
        if let Err(err) = app_state
            .invalidate_model_request_patch_rules(model_id)
            .await
        {
            warn!(
                "Failed to invalidate model request patch cache after create: {:?}",
                err
            );
        }

        log_request_patch_audit("create", "model", model_id, rule, rule.is_enabled);
    }

    Ok(HttpResult::new(outcome))
}

async fn update_model_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, rule_id)): Path<(i64, i64)>,
    Json(payload): Json<UpdateRequestPatchPayload>,
) -> Result<HttpResult<RequestPatchMutationOutcome>, BaseError> {
    let outcome = RequestPatchRule::update_for_model(model_id, rule_id, &payload)?;
    if let RequestPatchMutationOutcome::Saved { rule } = &outcome {
        if let Err(err) = app_state
            .invalidate_model_request_patch_rules(model_id)
            .await
        {
            warn!(
                "Failed to invalidate model request patch cache after update: {:?}",
                err
            );
        }

        log_request_patch_audit("update", "model", model_id, rule, rule.is_enabled);
    }

    Ok(HttpResult::new(outcome))
}

async fn delete_model_request_patch(
    State(app_state): State<Arc<AppState>>,
    Path((model_id, rule_id)): Path<(i64, i64)>,
) -> Result<HttpResult<()>, BaseError> {
    let rule = RequestPatchRule::get_model_rule(model_id, rule_id)?;
    RequestPatchRule::delete_for_model(model_id, rule_id)?;
    if let Err(err) = app_state
        .invalidate_model_request_patch_rules(model_id)
        .await
    {
        warn!(
            "Failed to invalidate model request patch cache after delete: {:?}",
            err
        );
    }

    log_request_patch_audit("delete", "model", model_id, &rule, false);

    Ok(HttpResult::new(()))
}

async fn get_model_request_patch_effective(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ModelRequestPatchEffectiveResponse>, BaseError> {
    let model = Model::get_by_id(model_id)?;
    let Some(resolved) = app_state
        .get_model_effective_request_patches(model_id)
        .await?
    else {
        return Err(BaseError::NotFound(Some(format!(
            "Model request patch effective result for {} not found",
            model_id
        ))));
    };

    Ok(HttpResult::new(ModelRequestPatchEffectiveResponse {
        provider_id: model.provider_id,
        model_id,
        effective_rules: resolved.effective_rules.clone(),
        conflicts: resolved.conflicts.clone(),
        has_conflicts: resolved.has_conflicts,
    }))
}

async fn get_model_request_patch_explain(
    State(app_state): State<Arc<AppState>>,
    Path(model_id): Path<i64>,
) -> Result<HttpResult<ModelRequestPatchExplainResponse>, BaseError> {
    let model = Model::get_by_id(model_id)?;
    let Some(resolved) = app_state
        .get_model_effective_request_patches(model_id)
        .await?
    else {
        return Err(BaseError::NotFound(Some(format!(
            "Model request patch explain result for {} not found",
            model_id
        ))));
    };

    Ok(HttpResult::new(ModelRequestPatchExplainResponse {
        provider_id: model.provider_id,
        model_id,
        direct_rules: resolved.direct_rules.clone(),
        inherited_rules: resolved.inherited_rules.clone(),
        effective_rules: resolved.effective_rules.clone(),
        explain: resolved.explain.clone(),
        conflicts: resolved.conflicts.clone(),
        has_conflicts: resolved.has_conflicts,
    }))
}

pub fn create_request_patch_router() -> StateRouter {
    create_state_router()
        .route(
            "/provider/{id}/request_patch",
            get(list_provider_request_patches).post(create_provider_request_patch),
        )
        .route(
            "/provider/{id}/request_patch/{rule_id}",
            put(update_provider_request_patch).delete(delete_provider_request_patch),
        )
        .route(
            "/model/{id}/request_patch",
            get(list_model_request_patches).post(create_model_request_patch),
        )
        .route(
            "/model/{id}/request_patch/effective",
            get(get_model_request_patch_effective),
        )
        .route(
            "/model/{id}/request_patch/explain",
            get(get_model_request_patch_explain),
        )
        .route(
            "/model/{id}/request_patch/{rule_id}",
            put(update_model_request_patch).delete(delete_model_request_patch),
        )
}
