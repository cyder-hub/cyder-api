use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize}; // Add Serialize for the new struct

use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use super::{get_connection, DbResult};

db_object! {
    #[derive(Queryable, Insertable, AsChangeset, Debug)]
    #[diesel(table_name = limit_strategy)]
    pub struct LimitStrategy {
        pub id: i64,
        pub main_strategy: String,
        pub name: String,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Insertable, AsChangeset, Debug, Clone)] // Removed Identifiable, Associations
    #[diesel(table_name = limit_strategy_item)]
    // Removed #[diesel(belongs_to(LimitStrategy))]
    pub struct LimitStrategyItem {
        pub id: i64,
        pub limit_strategy_id: i64,
        pub limit_strategy_type: String,
        pub resource_type: String,
        pub resource_id: Option<i64>,
        pub limit_type: String,
        pub limit_value: Option<i32>,
        pub duration: Option<String>,
    }
}

#[derive(Debug, Serialize)] // Add Serialize if needed for API responses
pub struct LimitStrategyWithItems {
    #[serde(flatten)]
    pub strategy: LimitStrategy,
    pub items: Vec<LimitStrategyItem>,
}

#[derive(Serialize, Debug)]
pub struct ResourceLimitItem {
    id: i64,
    limit_strategy_id: i64,
    pub resource_type: String,
    pub resource_id: Option<i64>,
}

#[derive(Serialize, Debug)]
pub struct QuotaLimitItem {
    id: i64,
    limit_strategy_id: i64,
    resource_type: String,
    resource_id: Option<i64>,
    limit_type: String,
    limit_value: Option<i32>,
    duration: Option<String>,
}

#[derive(Deserialize, Debug, Clone)] // Added Clone
pub struct ResourceLimitItemPayload {
    pub resource_type: String,
    pub resource_id: Option<i64>,
}

#[derive(Deserialize, Debug, Clone)] // Added Clone
pub struct QuotaLimitItemPayload {
    resource_type: String,
    resource_id: Option<i64>,
    limit_type: String,
    limit_value: Option<i32>,
    duration: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LimitStrategyDetail {
    #[serde(flatten)]
    pub strategy: LimitStrategy,
    pub white_list: Vec<ResourceLimitItem>,
    pub black_list: Vec<ResourceLimitItem>,
    pub quota_list: Vec<QuotaLimitItem>,
}

impl LimitStrategyItem {
    pub fn from_black(strategy: &LimitStrategy, item: ResourceLimitItemPayload) -> Self {
        Self {
            id: ID_GENERATOR.generate_id(),
            limit_strategy_type: "black".to_string(),
            limit_strategy_id: strategy.id,
            resource_type: item.resource_type,
            resource_id: item.resource_id,
            limit_type: "none".to_string(),
            limit_value: None,
            duration: None,
        }
    }

    pub fn from_white(strategy: &LimitStrategy, item: ResourceLimitItemPayload) -> Self {
        Self {
            id: ID_GENERATOR.generate_id(),
            limit_strategy_type: "white".to_string(),
            limit_strategy_id: strategy.id,
            resource_type: item.resource_type,
            resource_id: item.resource_id,
            limit_type: "none".to_string(),
            limit_value: None,
            duration: None,
        }
    }

    pub fn from_quota(strategy: &LimitStrategy, item: QuotaLimitItemPayload) -> Self {
        Self {
            id: ID_GENERATOR.generate_id(),
            limit_strategy_type: "quota".to_string(),
            limit_strategy_id: strategy.id,
            resource_type: item.resource_type,
            resource_id: item.resource_id,
            limit_type: item.limit_type,
            limit_value: item.limit_value,
            duration: item.duration,
        }
    }

    fn to_quota(&self) -> QuotaLimitItem {
        QuotaLimitItem {
            id: self.id,
            limit_strategy_id: self.limit_strategy_id,
            resource_type: self.resource_type.clone(),
            resource_id: self.resource_id,
            limit_type: self.limit_type.clone(),
            limit_value: self.limit_value,
            duration: self.duration.clone(),
        }
    }

    fn to_resource(&self) -> ResourceLimitItem {
        ResourceLimitItem {
            id: self.id,
            limit_strategy_id: self.limit_strategy_id,
            resource_type: self.resource_type.clone(),
            resource_id: self.resource_id,
        }
    }
}

impl LimitStrategy {
    pub fn new(
        id: Option<i64>,
        main_strategy: &str,
        name: &str,
        description: Option<&str>,
    ) -> Self {
        let now = Utc::now().timestamp_millis();
        Self {
            id: id.unwrap_or(ID_GENERATOR.generate_id()),
            main_strategy: main_strategy.to_string(),
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            created_at: now,
            updated_at: now,
        }
    }

    pub fn insert_one(strategy: &LimitStrategy) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let data = LimitStrategyDb::to_db(strategy);
            diesel::insert_into(limit_strategy::table)
                .values(&data)
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(())
        })
    }

    pub fn query_one_detail(id: i64) -> DbResult<LimitStrategyDetail> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            // Fetch the strategy
            let strategy_db = limit_strategy::table
                .filter(limit_strategy::dsl::id.eq(id))
                .first::<LimitStrategyDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        BaseError::NotFound(Some(format!("LimitStrategy with id {} not found", id)))
                    }
                    _ => BaseError::DatabaseFatal(Some(e.to_string())),
                })?;
            let strategy = strategy_db.from_db();

            // Fetch associated items
            let items_db = limit_strategy_item::table
                .filter(limit_strategy_item::dsl::limit_strategy_id.eq(id))
                .load::<LimitStrategyItemDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            let items: Vec<LimitStrategyItem> =
                items_db.into_iter().map(|db| db.from_db()).collect();

            Ok(LimitStrategyDetail {
                strategy,
                white_list: items
                    .clone()
                    .into_iter()
                    .filter(|item| item.limit_strategy_type == "white")
                    .map(|item| item.to_resource())
                    .collect(),
                black_list: items
                    .clone()
                    .into_iter()
                    .filter(|item| item.limit_strategy_type == "black")
                    .map(|item| item.to_resource())
                    .collect(),
                quota_list: items
                    .clone()
                    .into_iter()
                    .filter(|item| item.limit_strategy_type == "quota")
                    .map(|item| item.to_quota())
                    .collect(),
            })
        })
    }

    /// Fetches a single LimitStrategy and its associated items by strategy ID.
    pub fn query_one(id: i64) -> DbResult<LimitStrategyWithItems> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            // Fetch the strategy
            let strategy_db = limit_strategy::table
                .filter(limit_strategy::dsl::id.eq(id))
                .first::<LimitStrategyDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        BaseError::NotFound(Some(format!("LimitStrategy with id {} not found", id)))
                    }
                    _ => BaseError::DatabaseFatal(Some(e.to_string())),
                })?;
            let strategy = strategy_db.from_db();

            // Fetch associated items
            let items_db = limit_strategy_item::table
                .filter(limit_strategy_item::dsl::limit_strategy_id.eq(id))
                .load::<LimitStrategyItemDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            let items = items_db.into_iter().map(|db| db.from_db()).collect();

            Ok(LimitStrategyWithItems { strategy, items })
        })
    }

    /// Fetches all LimitStrategies and their associated items.
    pub fn list() -> DbResult<Vec<LimitStrategyDetail>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            // Fetch all strategies
            let strategies_db = limit_strategy::table
                .load::<LimitStrategyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            let strategies: Vec<LimitStrategy> =
                strategies_db.into_iter().map(|db| db.from_db()).collect();

            let mut result: Vec<LimitStrategyDetail> = Vec::new();

            // Manually fetch items for each strategy (N+1 query)
            for strategy in strategies {
                let items_db = limit_strategy_item::table
                    .filter(limit_strategy_item::dsl::limit_strategy_id.eq(strategy.id))
                    .load::<LimitStrategyItemDb>(conn)
                    .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

                let items: Vec<LimitStrategyItem> =
                    items_db.into_iter().map(|db| db.from_db()).collect();
                result.push(LimitStrategyDetail {
                    strategy,
                    white_list: items
                        .clone()
                        .into_iter()
                        .filter(|item| item.limit_strategy_type == "white")
                        .map(|item| item.to_resource())
                        .collect(),
                    black_list: items
                        .clone()
                        .into_iter()
                        .filter(|item| item.limit_strategy_type == "black")
                        .map(|item| item.to_resource())
                        .collect(),
                    quota_list: items
                        .clone()
                        .into_iter()
                        .filter(|item| item.limit_strategy_type == "quota")
                        .map(|item| item.to_quota())
                        .collect(),
                });
            }

            Ok(result)
        })
    }

    // Note: update_one still takes &LimitStrategy. If you need to update items too,
    // this function signature and logic will need to change significantly.
    pub fn update_one(strategy: &LimitStrategy) -> DbResult<LimitStrategy> {
        let conn = &mut get_connection();
        let mut updated_strategy = strategy.clone(); // Assuming LimitStrategy derives Clone
        updated_strategy.updated_at = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let data = LimitStrategyDb::to_db(&updated_strategy);
            diesel::update(limit_strategy::table) // Use schema import
                .filter(limit_strategy::dsl::id.eq(&strategy.id))
                .set(data)
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(updated_strategy) // Return the updated strategy
        })
    }

    /// Updates a LimitStrategy, deletes its existing items, and inserts a new list of items.
    /// Operations are performed sequentially without an explicit transaction.
    /// Assigns the strategy's ID to each new item before insertion.
    /// Generates IDs for new items if they are None (represented by id 0).
    /// WARNING: Operations are not atomic. If item deletion or insertion fails,
    /// the strategy might already be updated, and some old items might be deleted.
    pub fn update_with_items(
        strategy: &LimitStrategy,
        items: &Vec<LimitStrategyItem>,
    ) -> DbResult<LimitStrategy> {
        let conn = &mut get_connection();
        let mut updated_strategy = strategy.clone(); // Clone to update timestamp
        updated_strategy.updated_at = Utc::now().timestamp_millis();

        // 1. Update the strategy
        db_execute!(conn, {
            let strategy_data = LimitStrategyDb::to_db(&updated_strategy);
            diesel::update(limit_strategy::table)
                .filter(limit_strategy::dsl::id.eq(&strategy.id))
                .set(strategy_data)
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update strategy {}: {}",
                        strategy.id, e
                    )))
                })?;
            Ok::<(), diesel::result::Error>(()) // Specify Diesel error type
        })?;

        // 2. Delete existing items for this strategy
        db_execute!(conn, {
            diesel::delete(limit_strategy_item::table)
                .filter(limit_strategy_item::dsl::limit_strategy_id.eq(strategy.id))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete old items for strategy {}: {}",
                        strategy.id, e
                    )))
                })?;
            Ok::<(), diesel::result::Error>(()) // Specify Diesel error type
        })?;

        // 3. Prepare and insert the new items
        if !items.is_empty() {
            db_execute!(conn, {
                let items_to_insert: Vec<LimitStrategyItemDb> = items
                    .iter()
                    .map(|item| LimitStrategyItemDb::to_db(&item))
                    .collect();

                diesel::insert_into(limit_strategy_item::table)
                    .values(&items_to_insert)
                    .execute(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to insert new items for strategy {}: {}",
                            strategy.id, e
                        )))
                    })?;
                Ok::<(), diesel::result::Error>(()) // Specify Diesel error type
            })?;
        }

        // Return the updated strategy object (with the new timestamp)
        Ok(updated_strategy)
    }

    // Note: Deleting a strategy might require handling associated items (e.g., cascade delete or set null)
    // depending on database constraints or application logic. This implementation only deletes the strategy itself.
    // Also, deleting items associated with the strategy should happen first if required.
    pub fn delete_one(id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            // Optionally: Delete associated items first if needed
            diesel::delete(
                limit_strategy_item::table
                    .filter(limit_strategy_item::dsl::limit_strategy_id.eq(id)),
            )
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete items for strategy {}: {}",
                    id, e
                )))
            })?;

            // Delete the strategy itself
            let num_deleted =
                diesel::delete(limit_strategy::table.filter(limit_strategy::dsl::id.eq(id)))
                    .execute(conn)
                    .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            if num_deleted == 0 {
                Err(BaseError::NotFound(Some(format!(
                    "LimitStrategy with id {} not found for deletion",
                    id
                ))))
            } else {
                Ok(())
            }
        })
    }

    /// Inserts a LimitStrategy and its associated items without an explicit transaction.
    /// Assigns the strategy's ID to each item before insertion.
    /// Generates IDs for items if they are None (represented by id 0).
    /// WARNING: If item insertion fails, the strategy will still be inserted.
    pub fn insert_with_items(
        strategy: &LimitStrategy,
        items: &Vec<LimitStrategyItem>,
    ) -> DbResult<()> {
        let conn = &mut get_connection();

        // 1. Insert the strategy
        db_execute!(conn, {
            let strategy_data = LimitStrategyDb::to_db(strategy);
            diesel::insert_into(limit_strategy::table)
                .values(&strategy_data)
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to insert strategy: {}", e)))
                })?;
            Ok::<(), diesel::result::Error>(()) // Specify Diesel error type
        })?; // Propagate error if strategy insertion fails

        // 2. Prepare and insert the items (only if strategy insertion succeeded)
        if !items.is_empty() {
            db_execute!(conn, {
                let items_to_insert: Vec<LimitStrategyItemDb> = items
                    .iter()
                    .map(|item| LimitStrategyItemDb::to_db(item))
                    .collect();

                diesel::insert_into(limit_strategy_item::table)
                    .values(&items_to_insert)
                    .execute(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to insert strategy items: {}",
                            e
                        )))
                    })?;
                Ok::<(), diesel::result::Error>(()) // Specify Diesel error type
            })?; // Propagate error if item insertion fails
        }

        Ok(()) // Return Ok if both steps (or just the first if no items) succeeded
    }
}
