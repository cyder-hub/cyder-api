use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;

use super::{get_connection, DbResult};
use crate::controller::BaseError;
use crate::service::app_state::Storable;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

// --- BillingPlan ---

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone)]
    #[diesel(table_name = billing_plans)]
    pub struct BillingPlan {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub currency: String,
        pub created_at: i64,
        pub updated_at: i64,
        pub deleted_at: Option<i64>,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = billing_plans)]
    pub struct NewBillingPlan {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub currency: String,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = billing_plans)]
    pub struct UpdateBillingPlanData {
        pub name: Option<String>,
        pub description: Option<Option<String>>,
        pub currency: Option<String>,
    }

    #[derive(Queryable, Selectable, Identifiable, Debug, Clone)]
    #[diesel(table_name = price_rules)]
    pub struct PriceRule {
        pub id: i64,
        pub plan_id: i64,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub effective_from: i64,
        pub effective_until: Option<i64>,
        pub period_start_seconds_utc: Option<i32>,
        pub period_end_seconds_utc: Option<i32>,
        pub usage_type: String,
        pub media_type: Option<String>,
        pub condition_had_reasoning: Option<i32>,
        pub tier_from_tokens: Option<i32>,
        pub tier_to_tokens: Option<i32>,
        pub price_in_micro_units: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = price_rules)]
    pub struct NewPriceRule {
        pub id: i64,
        pub plan_id: i64,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub effective_from: i64,
        pub effective_until: Option<i64>,
        pub period_start_seconds_utc: Option<i32>,
        pub period_end_seconds_utc: Option<i32>,
        pub usage_type: String,
        pub media_type: Option<String>,
        pub condition_had_reasoning: Option<i32>,
        pub tier_from_tokens: Option<i32>,
        pub tier_to_tokens: Option<i32>,
        pub price_in_micro_units: i64,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = price_rules)]
    pub struct UpdatePriceRuleData {
        pub plan_id: Option<i64>,
        pub description: Option<Option<String>>,
        pub is_enabled: Option<bool>,
        pub effective_from: Option<i64>,
        pub effective_until: Option<Option<i64>>,
        pub period_start_seconds_utc: Option<Option<i32>>,
        pub period_end_seconds_utc: Option<Option<i32>>,
        pub usage_type: Option<String>,
        pub media_type: Option<Option<String>>,
        pub condition_had_reasoning: Option<Option<i32>>,
        pub tier_from_tokens: Option<Option<i32>>,
        pub tier_to_tokens: Option<Option<i32>>,
        pub price_in_micro_units: Option<i64>,
    }
}

impl Storable for BillingPlan {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> String {
        self.name.clone()
    }
}

#[derive(Deserialize, Debug)]
pub struct NewBillingPlanPayload {
    pub name: String,
    pub description: Option<String>,
    pub currency: String,
}

impl BillingPlan {
    pub fn create(data: &NewBillingPlanPayload) -> DbResult<BillingPlan> {
        let now = Utc::now().timestamp_millis();
        let new_plan_id = ID_GENERATOR.generate_id();

        let new_billing_plan = NewBillingPlan {
            id: new_plan_id,
            name: data.name.clone(),
            description: data.description.clone(),
            currency: data.currency.clone(),
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_db_plan = diesel::insert_into(billing_plans::table)
                .values(NewBillingPlanDb::to_db(&new_billing_plan))
                .returning(BillingPlanDb::as_returning())
                .get_result::<BillingPlanDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to create billing plan: {}", e)))
                })?;
            Ok(inserted_db_plan.from_db())
        })
    }

    pub fn update(id_value: i64, data: &UpdateBillingPlanData) -> DbResult<BillingPlan> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_db_plan = diesel::update(billing_plans::table.find(id_value))
                .set((
                    UpdateBillingPlanDataDb::to_db(data),
                    billing_plans::dsl::updated_at.eq(current_time),
                ))
                .returning(BillingPlanDb::as_returning())
                .get_result::<BillingPlanDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update billing plan {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_plan.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            diesel::update(billing_plans::table.find(id_value))
                .set(billing_plans::dsl::deleted_at.eq(current_time))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete billing plan {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    pub fn list_all() -> DbResult<Vec<BillingPlan>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_plans = billing_plans::table
                .filter(billing_plans::dsl::deleted_at.is_null())
                .order(billing_plans::dsl::created_at.desc())
                .select(BillingPlanDb::as_select())
                .load::<BillingPlanDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list billing plans: {}", e)))
                })?;
            Ok(db_plans
                .into_iter()
                .map(|db_plan| db_plan.from_db())
                .collect())
        })
    }
}

impl Storable for PriceRule {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> String {
        self.id.to_string()
    }

    fn group_id(&self) -> Option<i64> {
        Some(self.plan_id)
    }
}

#[derive(Deserialize, Debug)]
pub struct NewPriceRulePayload {
    pub plan_id: i64,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub effective_from: i64,
    pub effective_until: Option<i64>,
    pub period_start_seconds_utc: Option<i32>,
    pub period_end_seconds_utc: Option<i32>,
    pub usage_type: String,
    pub media_type: Option<String>,
    pub condition_had_reasoning: Option<i32>,
    pub tier_from_tokens: Option<i32>,
    pub tier_to_tokens: Option<i32>,
    pub price_in_micro_units: i64,
}

impl PriceRule {
    pub fn create(data: &NewPriceRulePayload) -> DbResult<PriceRule> {
        let now = Utc::now().timestamp_millis();
        let new_rule_id = ID_GENERATOR.generate_id();

        let new_price_rule = NewPriceRule {
            id: new_rule_id,
            plan_id: data.plan_id,
            description: data.description.clone(),
            is_enabled: data.is_enabled,
            effective_from: data.effective_from,
            effective_until: data.effective_until,
            period_start_seconds_utc: data.period_start_seconds_utc,
            period_end_seconds_utc: data.period_end_seconds_utc,
            usage_type: data.usage_type.clone(),
            media_type: data.media_type.clone(),
            condition_had_reasoning: data.condition_had_reasoning,
            tier_from_tokens: data.tier_from_tokens,
            tier_to_tokens: data.tier_to_tokens,
            price_in_micro_units: data.price_in_micro_units,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_db_rule = diesel::insert_into(price_rules::table)
                .values(NewPriceRuleDb::to_db(&new_price_rule))
                .returning(PriceRuleDb::as_returning())
                .get_result::<PriceRuleDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to create price rule: {}", e)))
                })?;
            Ok(inserted_db_rule.from_db())
        })
    }

    pub fn update(id_value: i64, data: &UpdatePriceRuleData) -> DbResult<PriceRule> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_db_rule = diesel::update(price_rules::table.find(id_value))
                .set((
                    UpdatePriceRuleDataDb::to_db(data),
                    price_rules::dsl::updated_at.eq(current_time),
                ))
                .returning(PriceRuleDb::as_returning())
                .get_result::<PriceRuleDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update price rule {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_rule.from_db())
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::delete(price_rules::table.find(id_value))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete price rule {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    pub fn list_all() -> DbResult<Vec<PriceRule>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_rules = price_rules::table
                .order(price_rules::dsl::created_at.desc())
                .select(PriceRuleDb::as_select())
                .load::<PriceRuleDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list price rules: {}", e)))
                })?;
            Ok(db_rules
                .into_iter()
                .map(|db_rule| db_rule.from_db())
                .collect())
        })
    }

    pub fn list_by_plan_id(plan_id_value: i64) -> DbResult<Vec<PriceRule>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_rules = price_rules::table
                .filter(price_rules::dsl::plan_id.eq(plan_id_value))
                .order(price_rules::dsl::created_at.desc())
                .select(PriceRuleDb::as_select())
                .load::<PriceRuleDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list price rules for plan {}: {}",
                        plan_id_value, e
                    )))
                })?;
            Ok(db_rules
                .into_iter()
                .map(|db_rule| db_rule.from_db())
                .collect())
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<PriceRule> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_rule = price_rules::table
                .find(id_value)
                .select(PriceRuleDb::as_select())
                .first::<PriceRuleDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Price rule with id {} not found",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching price rule {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(db_rule.from_db())
        })
    }
}
