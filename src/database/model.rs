use chrono::Utc;
use diesel::prelude::Queryable;
use diesel::prelude::*;

use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use super::get_connection;
use super::provider::{CustomField, ProviderApiKey};
use super::{provider::Provider, DbResult};

db_object! {
    #[derive(Queryable, Insertable, Debug, AsChangeset, Clone, serde::Serialize)] // Added Clone, Serialize
    #[diesel(table_name = price)]
    pub struct Price {
        pub id: i64,
        pub model_id: i64,
        pub start_time: i64,
        pub currency: String,
        pub input_price: i32,
        pub output_price: i32,
        pub input_cache_price: i32,
        pub output_cache_price: i32,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Insertable, Debug, AsChangeset, Clone, serde::Serialize)] // Added Clone, Serialize
    #[diesel(table_name = model)]
    pub struct Model {
        pub id: i64,
        pub provider_id: i64,
        pub model_name: String,
        pub real_model_name: Option<String>,
        pub is_deleted: bool,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

impl Model {
    pub fn insert(model: &Self) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::insert_into(model::table)
                .values(&ModelDb::to_db(model))
                .execute(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }
    pub fn list_by_provider_id(provider_id: i64) -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = model::table
                .filter(model::dsl::provider_id.eq(provider_id))
                .load::<ModelDb>(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(result.into_iter().map(|db| db.from_db()).collect())
        })
    }

    pub fn query_provider_model(
        provider_key: &str,
        model_name: &str,
    ) -> DbResult<(Provider, Vec<ProviderApiKey>, Vec<CustomField>, Option<Model>)> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let (provider, provider_api_keys, custom_fields) = Provider::query_key_by_key(provider_key)?;
            let model = model::table
                .filter(
                    model::dsl::model_name
                        .eq(model_name)
                        .and(model::dsl::provider_id.eq(provider.id))
                        .and(model::dsl::is_deleted.eq(false))
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .first::<ModelDb>(conn);
            let model = match model {
                Ok(model) => Some(model.from_db()),
                Err(_) => None,
            };
            Ok((provider, provider_api_keys, custom_fields, model))
        })
    }

    pub fn get(id: i64) -> DbResult<Model> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let model = model::table
                .filter(model::dsl::id.eq(id))
                .first::<ModelDb>(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(model.from_db())
        })
    }

    pub fn update(model: &Model) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::update(model::table.filter(model::dsl::id.eq(model.id)))
                .set(ModelDb::to_db(model))
                .execute(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }

    pub fn delete(id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::update(model::table.filter(model::dsl::id.eq(id)))
                .set(model::dsl::is_deleted.eq(true))
                .execute(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(())
        })
    }

    pub fn upsert_by_provider_and_name(
        provider_id: i64,
        model_name: &str,
        real_model_name: Option<&str>,
    ) -> DbResult<Model> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let existing = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id)
                        .and(model::dsl::model_name.eq(model_name)),
                )
                .first::<ModelDb>(conn);

            let now = Utc::now().timestamp_millis();
            let model = match existing {
                Ok(db) => {
                    let mut model = db.from_db();
                    model.real_model_name = real_model_name.map(|s| s.to_string());
                    model.updated_at = now;
                    model.is_deleted = false;
                    model.is_enabled = true;
                    diesel::update(model::table.filter(model::dsl::id.eq(model.id)))
                        .set(ModelDb::to_db(&model))
                        .execute(conn)
                        .map_err(|_e| BaseError::DatabaseFatal(None))?;
                    model
                }
                Err(_) => {
                    let model = Model {
                        id: ID_GENERATOR.generate_id(),
                        provider_id,
                        model_name: model_name.to_string(),
                        real_model_name: real_model_name.map(|s| s.to_string()),
                        is_deleted: false,
                        is_enabled: true,
                        created_at: now,
                        updated_at: now,
                    };
                    diesel::insert_into(model::table)
                        .values(ModelDb::to_db(&model))
                        .execute(conn)
                        .map_err(|_e| BaseError::DatabaseFatal(None))?;
                    model
                }
            };
            Ok(model)
        })
    }

    pub fn list() -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let result = model::table
                .load::<ModelDb>(conn)
                .map_err(|_e| BaseError::DatabaseFatal(None))?;
            Ok(result.into_iter().map(|db| db.from_db()).collect())
        })
    }

    /// Get the latest price record (highest start_time) for a specific model ID.
    pub fn get_latest_by_model_id(model_id_val: i64) -> DbResult<Price> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            price::table
                .filter(price::dsl::model_id.eq(model_id_val))
                .order_by(price::dsl::start_time.desc())
                .first::<PriceDb>(conn) // Use PriceDb generated by db_object!
                .map(|p| p.from_db())
                .map_err(|e| match e {
                    diesel::NotFound => BaseError::NotFound(Some(format!(
                        "No price found for model_id {}",
                        model_id_val
                    ))),
                    _ => BaseError::DatabaseFatal(Some(e.to_string())),
                })
        })
    }
}

impl Price {
    /// Get a single price record by its ID.
    pub fn get_by_id(price_id: i64) -> DbResult<Price> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            price::table
                .filter(price::dsl::id.eq(price_id))
                .first::<PriceDb>(conn) // Use PriceDb generated by db_object!
                .map(|p| p.from_db())
                .map_err(|e| match e {
                    diesel::NotFound => BaseError::NotFound(Some(format!("Price with id {} not found", price_id))),
                    _ => BaseError::DatabaseFatal(Some(e.to_string())),
                })
        })
    }

    /// Insert a new price record.
    pub fn insert_one(data: &mut Price) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let now = Utc::now().timestamp_millis();
            data.id = ID_GENERATOR.generate_id(); // Assign a new ID
            data.created_at = now;
            data.updated_at = now;

            let data_db = PriceDb::to_db(data); // Use PriceDb generated by db_object!

            diesel::insert_into(price::table)
                .values(&data_db)
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            // Return the inserted price with the generated ID and timestamps
            Ok(())
        })
    }

    /// Update an existing price record.
    /// Note: This updates all fields based on the provided 'data'.
    /// Consider creating a separate UpdatePrice struct if partial updates are needed.
    pub fn update_one(price_id: i64, data: &Price) -> DbResult<Price> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let mut updated_data = data.clone();
            updated_data.id = price_id; // Ensure the ID is correct for AsChangeset
            updated_data.updated_at = Utc::now().timestamp_millis();

            let data_db = PriceDb::to_db(&updated_data); // Use PriceDb generated by db_object!

            diesel::update(price::table.filter(price::dsl::id.eq(price_id)))
                .set(&data_db) // Use AsChangeset derived struct (PriceDb)
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            Ok(updated_data)
        })
    }

    /// List all prices associated with a specific model ID.
    pub fn list_by_model_id(model_id_val: i64) -> DbResult<Vec<Price>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let results = price::table
                .filter(price::dsl::model_id.eq(model_id_val))
                .load::<PriceDb>(conn) // Use PriceDb generated by db_object!
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            Ok(results.into_iter().map(|db| db.from_db()).collect())
        })
    }

    /// Delete a single price record by its ID.
    pub fn delete_one(price_id: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let num_deleted = diesel::delete(price::table.filter(price::dsl::id.eq(price_id)))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;

            if num_deleted == 0 {
                 Err(BaseError::NotFound(Some(format!("Price with id {} not found for deletion", price_id))))
            } else {
                 Ok(())
            }
        })
    }

    /// Delete all price records associated with a specific model ID.
    pub fn delete_all_by_model_id(model_id_val: i64) -> DbResult<()> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::delete(price::table.filter(price::dsl::model_id.eq(model_id_val)))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(e.to_string())))?;
            Ok(())
        })
    }
}
