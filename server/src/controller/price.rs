use crate::database::price::{
    BillingPlan, NewBillingPlanPayload, NewPriceRulePayload, PriceRule, UpdateBillingPlanData,
    UpdatePriceRuleData,
};
use crate::database::DbResult;
use crate::service::app_state::{create_state_router, AppState, StateRouter};
use crate::utils::HttpResult;
use axum::{
    extract::{Path, Query, State},
    routing::{delete, get, post},
    Json,
};
use cyder_tools::log::warn;
use serde::Deserialize;
use std::sync::Arc;

// --- Price Rule Handlers ---

#[derive(Deserialize)]
struct ListByPlanIdParams {
    plan_id: i64,
}

#[derive(Deserialize, Debug, Default)]
struct UpdatePriceRuleRequest {
    plan_id: Option<i64>,
    description: Option<Option<String>>,
    is_enabled: Option<bool>,
    effective_from: Option<i64>,
    effective_until: Option<Option<i64>>,
    period_start_seconds_utc: Option<Option<i32>>,
    period_end_seconds_utc: Option<Option<i32>>,
    usage_type: Option<String>,
    media_type: Option<Option<String>>,
    condition_had_reasoning: Option<Option<i32>>,
    tier_from_tokens: Option<Option<i32>>,
    tier_to_tokens: Option<Option<i32>>,
    price_in_micro_units: Option<i64>,
}

async fn insert_rule(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<NewPriceRulePayload>,
) -> DbResult<HttpResult<PriceRule>> {
    let created_rule = PriceRule::create(&payload)?;

    if let Err(e) = app_state.price_rule_store.add(created_rule.clone()) {
        warn!(
            "Failed to add PriceRule id {} to store: {:?}",
            created_rule.id, e
        );
    }

    Ok(HttpResult::new(created_rule))
}

async fn delete_rule(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    PriceRule::delete(id)?;

    if let Err(e) = app_state.price_rule_store.delete(id) {
        warn!("Failed to delete PriceRule id {} from store: {:?}", id, e);
    }

    Ok(HttpResult::new(()))
}

async fn list_rules() -> DbResult<HttpResult<Vec<PriceRule>>> {
    let result = PriceRule::list_all()?;
    Ok(HttpResult::new(result))
}

async fn list_rules_by_plan_id(
    Query(params): Query<ListByPlanIdParams>,
) -> DbResult<HttpResult<Vec<PriceRule>>> {
    let result = PriceRule::list_by_plan_id(params.plan_id)?;
    Ok(HttpResult::new(result))
}

async fn update_rule(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdatePriceRuleRequest>,
) -> DbResult<HttpResult<PriceRule>> {
    let update_data = UpdatePriceRuleData {
        plan_id: payload.plan_id,
        description: payload.description,
        is_enabled: payload.is_enabled,
        effective_from: payload.effective_from,
        effective_until: payload.effective_until,
        period_start_seconds_utc: payload.period_start_seconds_utc,
        period_end_seconds_utc: payload.period_end_seconds_utc,
        usage_type: payload.usage_type,
        media_type: payload.media_type,
        condition_had_reasoning: payload.condition_had_reasoning,
        tier_from_tokens: payload.tier_from_tokens,
        tier_to_tokens: payload.tier_to_tokens,
        price_in_micro_units: payload.price_in_micro_units,
    };
    let updated_rule = PriceRule::update(id, &update_data)?;

    if let Err(e) = app_state.price_rule_store.update(updated_rule.clone()) {
        warn!(
            "Failed to update PriceRule id {} in store: {:?}",
            updated_rule.id, e
        );
    }

    Ok(HttpResult::new(updated_rule))
}

// --- Billing Plan Handlers ---

#[derive(Deserialize, Debug, Default)]
struct UpdateBillingPlanRequest {
    name: Option<String>,
    description: Option<Option<String>>,
    currency: Option<String>,
}

async fn insert_plan(
    State(app_state): State<Arc<AppState>>,
    Json(payload): Json<NewBillingPlanPayload>,
) -> DbResult<HttpResult<BillingPlan>> {
    let created_plan = BillingPlan::create(&payload)?;

    if let Err(e) = app_state.billing_plan_store.add(created_plan.clone()) {
        warn!(
            "Failed to add BillingPlan id {} to store: {:?}",
            created_plan.id, e
        );
    }

    Ok(HttpResult::new(created_plan))
}

async fn delete_plan(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> DbResult<HttpResult<()>> {
    BillingPlan::delete(id)?;

    if let Err(e) = app_state.billing_plan_store.delete(id) {
        warn!("Failed to delete BillingPlan id {} from store: {:?}", id, e);
    }

    Ok(HttpResult::new(()))
}

async fn list_plans() -> DbResult<HttpResult<Vec<BillingPlan>>> {
    let result = BillingPlan::list_all()?;
    Ok(HttpResult::new(result))
}

async fn update_plan(
    State(app_state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateBillingPlanRequest>,
) -> DbResult<HttpResult<BillingPlan>> {
    let update_data = UpdateBillingPlanData {
        name: payload.name,
        description: payload.description,
        currency: payload.currency,
    };
    let updated_plan = BillingPlan::update(id, &update_data)?;

    if let Err(e) = app_state.billing_plan_store.update(updated_plan.clone()) {
        warn!(
            "Failed to update BillingPlan id {} in store: {:?}",
            updated_plan.id, e
        );
    }

    Ok(HttpResult::new(updated_plan))
}

pub fn create_price_router() -> StateRouter {
    create_state_router().nest(
        "/price",
        create_state_router()
            .route("/rule", post(insert_rule))
            .route("/rule/{id}", delete(delete_rule).put(update_rule))
            .route("/rule/list", get(list_rules))
            .route("/rule/list_by_plan", get(list_rules_by_plan_id))
            .route("/plan", post(insert_plan))
            .route("/plan/{id}", delete(delete_plan).put(update_plan))
            .route("/plan/list", get(list_plans)),
    )
}
