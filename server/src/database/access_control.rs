use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{get_connection, DbConnection, DbResult};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};
use crate::schema::enum_def::{Action, RuleScope};

// --- Core Database Object Structs (managed by db_object!) ---
db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = access_control_policy)]
    pub struct AccessControlPolicy {
        pub id: i64,
        pub name: String,
        pub description: Option<String>,
        pub default_action: Action, // New field
        pub created_at: i64,
        pub updated_at: i64,
        pub deleted_at: Option<i64>,
    }

    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = access_control_rule)]
    pub struct AccessControlRule {
        pub id: i64,
        pub policy_id: i64,         // Renamed from limit_strategy_id
        pub rule_type: Action,      // New field (replaces item_type logic)
        pub priority: i32,          // New field
        pub scope: RuleScope,          // Renamed from resource_scope
        pub provider_id: Option<i64>, // New field (replaces part of resource_identifier)
        pub model_id: Option<i64>,    // New field (replaces part of resource_identifier)
        pub is_enabled: bool,
        pub description: Option<String>, // New field
        pub created_at: i64,
        pub updated_at: i64,
        pub deleted_at: Option<i64>,       // New field for rules
    }

// --- Internal Structs for DB Operations ---
#[derive(Insertable, Debug)]
#[diesel(table_name = access_control_policy)]
pub struct DbNewAccessControlPolicy {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(AsChangeset, Debug, Default)]
#[diesel(table_name = access_control_policy)]
pub struct DbUpdateAccessControlPolicy {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub default_action: Option<Action>,
    // updated_at is set manually
}

#[derive(Insertable, Debug)]
#[diesel(table_name = access_control_rule)]
pub struct DbNewAccessControlRule {
    pub id: i64,
    pub policy_id: i64,
    pub rule_type: Action,
    pub priority: i32,
    pub scope: RuleScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub is_enabled: bool,
    pub description: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}
}

// --- API-Facing Structs ---
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiAccessControlRule { // Renamed from ApiLimitStrategyItem
    pub id: Option<i64>, // Optional for creation, present for responses
    pub rule_type: Action,
    pub priority: i32,
    pub scope: RuleScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub description: Option<String>,
    pub is_enabled: Option<bool>, // Defaults to true on creation
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiCreateAccessControlPolicyPayload { // Renamed
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action, // Added
    // pub is_enabled: Option<bool>, // Removed, default_action serves a related purpose
    pub rules: Option<Vec<ApiAccessControlRule>>, // Consolidated from white/black/quota lists
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ApiUpdateAccessControlPolicyPayload { // Renamed
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub default_action: Option<Action>, // Added
    // pub is_enabled: Option<bool>, // Removed
    // To update rules, the full list must be provided.
    // If rules is None, rules are not touched. If Some (even empty Vec), rules are replaced.
    pub rules: Option<Vec<ApiAccessControlRule>>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiAccessControlPolicy { // Renamed
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action, // Added
    // pub is_enabled: bool, // Removed
    pub created_at: i64,
    pub updated_at: i64,
    pub rules: Vec<AccessControlRule>, // Consolidated, using the full DB struct for items in response
}

impl AccessControlPolicy { // Renamed from LimitStrategy
    fn rules_from_payload( // Renamed from items_from_payload
        policy_id_val: i64, // Renamed from strategy_id
        payload_rules: &Option<Vec<ApiAccessControlRule>>, // Renamed
        now: i64,
    ) -> Vec<DbNewAccessControlRule> { // Renamed
        let mut db_rules = Vec::new(); // Renamed
        if let Some(rules) = payload_rules {
            for rule_payload in rules {
                db_rules.push(DbNewAccessControlRule { // Renamed
                    id: ID_GENERATOR.generate_id(),
                    policy_id: policy_id_val,
                    rule_type: rule_payload.rule_type.clone(),
                    priority: rule_payload.priority,
                    scope: rule_payload.scope.clone(),
                    provider_id: rule_payload.provider_id,
                    model_id: rule_payload.model_id,
                    description: rule_payload.description.clone(),
                    is_enabled: rule_payload.is_enabled.unwrap_or(true),
                    created_at: now,
                    updated_at: now,
                });
            }
        }
        db_rules
    }

    fn insert_rules_batch( // Renamed from insert_items_batch
        conn: &mut DbConnection,
        rules_to_insert: Vec<DbNewAccessControlRule>, // Renamed
    ) -> DbResult<()> {
        if rules_to_insert.is_empty() {
            return Ok(());
        }
        db_execute!(conn, {
            let rule_db_records: Vec<_> = rules_to_insert // Renamed
                .into_iter()
                .map(|rule| DbNewAccessControlRuleDb::to_db(&rule)) // Renamed
                .collect();

            diesel::insert_into(access_control_rule::table) // Updated table
                .values(&rule_db_records)
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to insert access control rules: {}", e)))
                })?;
            Ok(())
        })
    }

    fn delete_rules_by_policy_id( // Renamed from delete_items_by_strategy_id
        conn: &mut DbConnection,
        policy_id_val: i64, // Renamed
    ) -> DbResult<usize> {
        db_execute!(conn, {
            diesel::delete(
                access_control_rule::table // Updated table
                    .filter(access_control_rule::dsl::policy_id.eq(policy_id_val)), // Updated field
            )
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete rules for policy {}: {}",
                    policy_id_val, e
                )))
            })
        })
    }

    pub fn create(payload: ApiCreateAccessControlPolicyPayload) -> DbResult<ApiAccessControlPolicy> { // Renamed
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();
        let policy_id = ID_GENERATOR.generate_id(); // Renamed

        let new_policy_db = DbNewAccessControlPolicy { // Renamed
            id: policy_id,
            name: payload.name,
            description: payload.description,
            default_action: payload.default_action,
            // is_enabled: payload.is_enabled.unwrap_or(true), // Removed
            created_at: now,
            updated_at: now,
        };

        db_execute!(conn, {
            diesel::insert_into(access_control_policy::table) // Updated table
                .values(DbNewAccessControlPolicyDb::to_db(&new_policy_db)) // Renamed
                .returning(AccessControlPolicyDb::as_returning()) // Renamed
                .get_result::<AccessControlPolicyDb>(conn) // Renamed
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to create access control policy: {}", e))))
                .map(|p_db| p_db.from_db())? // Renamed
        });

        let all_rules_to_insert = Self::rules_from_payload(policy_id, &payload.rules, now); // Updated

        Self::insert_rules_batch(conn, all_rules_to_insert)?;

        Self::get_by_id(policy_id) // Fetch the full policy with rules
    }

    pub fn update(
        policy_id: i64, // Renamed
        payload: ApiUpdateAccessControlPolicyPayload, // Renamed
    ) -> DbResult<ApiAccessControlPolicy> { // Renamed
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();

        let update_policy_data = DbUpdateAccessControlPolicy { // Renamed
            name: payload.name,
            description: payload.description,
            default_action: payload.default_action,
            // is_enabled: payload.is_enabled, // Removed
        };

        db_execute!(conn, {
            diesel::update(access_control_policy::table.find(policy_id)) // Updated table
                .set((
                    DbUpdateAccessControlPolicyDb::to_db(&update_policy_data), // Renamed
                    access_control_policy::dsl::updated_at.eq(now), // Updated table
                ))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to update policy {}: {}", policy_id, e))))?;
        });

        if payload.rules.is_some() {
            Self::delete_rules_by_policy_id(conn, policy_id)?; // Renamed
            let all_rules_to_insert = Self::rules_from_payload(policy_id, &payload.rules, now); // Updated
            Self::insert_rules_batch(conn, all_rules_to_insert)?; // Renamed
        }

        Self::get_by_id(policy_id)
    }

    pub fn delete(policy_id: i64) -> DbResult<usize> { // Renamed
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();

        // Soft delete the policy
        let num_deleted_policy = db_execute!(conn, { // Renamed
            diesel::update(access_control_policy::table.find(policy_id)) // Updated table
                .set((
                    access_control_policy::dsl::deleted_at.eq(now), // Updated table
                    // access_control_policy::dsl::is_enabled.eq(false), // is_enabled removed
                    access_control_policy::dsl::updated_at.eq(now), // Updated table
                ))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to soft-delete policy {}: {}", policy_id, e))))
        })?;

        if num_deleted_policy > 0 {
            // Also soft-delete associated rules (rules have their own is_deleted flag)
            db_execute!(conn, {
                diesel::update(access_control_rule::table.filter(access_control_rule::dsl::policy_id.eq(policy_id)))
                    .set((
                        access_control_rule::dsl::deleted_at.eq(now),
                        access_control_rule::dsl::is_enabled.eq(false), // Typically disable when policy is deleted
                        access_control_rule::dsl::updated_at.eq(now),
                    ))
                    .execute(conn)
                    .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to soft-delete rules for policy {}: {}", policy_id, e))))
            })?;
        }
        Ok(num_deleted_policy)
    }

    pub fn get_by_id(policy_id: i64) -> DbResult<ApiAccessControlPolicy> { // Renamed
        let conn = &mut get_connection();
        let policy: AccessControlPolicy = db_execute!(conn, { // Renamed
            access_control_policy::table // Updated table
                .filter(access_control_policy::dsl::id.eq(policy_id)) // Updated field
                .filter(access_control_policy::dsl::deleted_at.is_null()) // Updated field
                .select(AccessControlPolicyDb::as_select()) // Renamed
                .first::<AccessControlPolicyDb>(conn) // Renamed
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!("Policy {} not found", policy_id))),
                    _ => BaseError::DatabaseFatal(Some(e.to_string())),
                })
                .map(|p_db| p_db.from_db())? // Renamed
        });

        let all_rules: Vec<AccessControlRule> = db_execute!(conn, { // Renamed
            access_control_rule::table // Updated table
                .filter(access_control_rule::dsl::policy_id.eq(policy_id)) // Updated field
                .filter(access_control_rule::dsl::deleted_at.is_null()) // Filter out deleted rules
                .order(access_control_rule::dsl::priority.asc()) // Order by priority
                .select(AccessControlRuleDb::as_select()) // Renamed
                .load::<AccessControlRuleDb>(conn) // Renamed
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?
                .into_iter()
                .map(|r_db| r_db.from_db()) // Renamed
                .collect()
        });
        
        Ok(ApiAccessControlPolicy { // Renamed
            id: policy.id,
            name: policy.name,
            description: policy.description,
            default_action: policy.default_action,
            // is_enabled: policy.is_enabled, // Removed
            created_at: policy.created_at,
            updated_at: policy.updated_at,
            rules: all_rules, // Consolidated
        })
    }

    pub fn list_all() -> DbResult<Vec<ApiAccessControlPolicy>> { // Renamed
        let conn = &mut get_connection();
        let policies: Vec<AccessControlPolicy> = db_execute!(conn, { // Renamed
            access_control_policy::table // Updated table
                .filter(access_control_policy::dsl::deleted_at.is_null()) // Updated field
                .order(access_control_policy::dsl::created_at.desc()) // Updated field
                .select(AccessControlPolicyDb::as_select()) // Renamed
                .load::<AccessControlPolicyDb>(conn) // Renamed
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?
                .into_iter()
                .map(|p_db| p_db.from_db()) // Renamed
                .collect()
        });

        let mut result = Vec::new();
        for policy in policies { // Renamed
            let all_rules: Vec<AccessControlRule> = db_execute!(conn, { // Renamed
                 access_control_rule::table // Updated table
                    .filter(access_control_rule::dsl::policy_id.eq(policy.id)) // Updated field
                    .filter(access_control_rule::dsl::deleted_at.is_null()) // Filter out deleted rules
                    .order(access_control_rule::dsl::priority.asc()) // Order by priority
                    .select(AccessControlRuleDb::as_select()) // Renamed
                    .load::<AccessControlRuleDb>(conn) // Renamed
                    .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?
                    .into_iter()
                    .map(|r_db| r_db.from_db()) // Renamed
                    .collect()
            });

            result.push(ApiAccessControlPolicy { // Renamed
                id: policy.id,
                name: policy.name.clone(),
                description: policy.description.clone(),
                default_action: policy.default_action.clone(),
                // is_enabled: policy.is_enabled, // Removed
                created_at: policy.created_at,
                updated_at: policy.updated_at,
                rules: all_rules, // Consolidated
            });
        }
        Ok(result)
    }
}
