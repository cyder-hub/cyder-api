use diesel::prelude::*;
use serde::Deserialize; // Serialize on Provider is via db_object!, Deserialize for helper structs
use serde::Serialize;
use chrono::Utc;

use crate::database::{DbResult, get_connection};
use crate::{db_execute, db_object};
// db_object! is exported at the crate root by `#[macro_export]` in `database/mod.rs`.
// BaseError is assumed to be accessible, e.g., from `crate::controller::BaseError`.
use crate::controller::BaseError;
use crate::database::custom_field::{ApiCustomFieldDefinition, CustomFieldDefinition};
use crate::schema::enum_def::ProviderType;

// Define the main Provider struct and its DB representations using db_object!
// The attributes like `#[derive(Queryable, ...)]` and `#[diesel(table_name = ...)]`
// will be applied to the generated `ProviderDb` structs within the macro expansion,
// where the correct schema (and thus the `provider` table) is in scope.
db_object! {
    #[derive(Queryable, Selectable, Identifiable, AsChangeset)] // Diesel derives for the generated ProviderDb
    #[diesel(table_name = provider)] // Refers to the table from the schema imported by db_object!/db_execute!
    pub struct Provider {
        pub id: i64,
        pub provider_key: String,
        pub name: String,
        pub endpoint: String,
        pub use_proxy: bool,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
        pub provider_type: ProviderType,
    }

// Data structure for inserting a new provider.
// The `#[diesel(table_name = ...)]` here needs to resolve from this file's context.
// `crate::schema::postgres::provider` is the path to the table definition.
#[derive(Insertable, Deserialize, Debug)]
#[diesel(table_name = provider)]
pub struct NewProvider {
    pub id: i64,
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
    pub provider_type: ProviderType,
}

// Data structure for updating an existing provider.
#[derive(AsChangeset, Deserialize, Debug)]
#[diesel(table_name = provider)]
pub struct UpdateProviderData {
    pub provider_key: Option<String>,
    pub name: Option<String>,
    pub endpoint: Option<String>,
    pub use_proxy: Option<bool>,
    pub is_enabled: Option<bool>,
    pub provider_type: Option<ProviderType>,
}

// Define ProviderApiKey struct and its DB representations
    #[derive(Queryable, Selectable, Identifiable, Associations, AsChangeset)]
    #[diesel(belongs_to(Provider))]
    #[diesel(table_name = provider_api_key)]
    pub struct ProviderApiKey {
        pub id: i64,
        pub provider_id: i64,
        pub api_key: String,
        pub description: Option<String>,
        pub deleted_at: Option<i64>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = provider_api_key)]
    pub struct NewProviderApiKey {
        pub id: i64,
        pub provider_id: i64,
        pub api_key: String,
        pub description: Option<String>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Deserialize, Debug)]
    #[diesel(table_name = provider_api_key)]
    pub struct UpdateProviderApiKeyData {
        pub api_key: Option<String>,
        pub description: Option<String>,
        pub is_enabled: Option<bool>,
    }
}
#[derive(Debug, Serialize)]
pub struct ProviderDetail {
    pub provider: Provider,
    pub api_keys: Vec<ProviderApiKey>,
    pub custom_fields: Vec<ApiCustomFieldDefinition>,
}
impl Provider {
    /// Inserts a new provider record into the database.
    pub fn create(new_provider_data: &NewProvider) -> DbResult<Provider> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            // Inside db_execute!, `ProviderDb` refers to the DB-specific generated struct (_postgres_model::ProviderDb or _sqlite_model::ProviderDb).
            // `provider::table` refers to the table from the DB-specific schema.
            // The `new_provider_data` (NewProvider struct) is Insertable into `crate::schema::postgres::provider::table`.
            // This should be compatible as long as the column types match.
            let db_provider = diesel::insert_into(provider::table)
                .values(NewProviderDb::to_db(new_provider_data))
                .returning(ProviderDb::as_returning()) // Use the generated ProviderDb for returning
                .get_result::<ProviderDb>(conn)        // Expect a ProviderDb instance
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to insert provider: {}", e))))?;
            Ok(db_provider.from_db()) // Convert ProviderDb to the main Provider struct
        })
    }

    /// Updates an existing provider record in the database.
    pub fn update(id_value: i64, update_data: &UpdateProviderData) -> DbResult<Provider> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            // The `update_data` (UpdateProviderData struct) is AsChangeset for `crate::schema::postgres::provider::table`.
            let db_provider = diesel::update(provider::table.find(id_value))
                .set((
                    UpdateProviderDataDb::to_db(update_data),
                    provider::dsl::updated_at.eq(current_time)
                ))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to update provider {}: {}", id_value, e))))?;
            Ok(db_provider.from_db())
        })
    }

    /// Soft deletes a provider record by setting `deleted_at` to the current time and `is_enabled` to false.
    pub fn delete(target_id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(provider::table.find(target_id_value))
                .set((
                    provider::dsl::deleted_at.eq(current_time),
                    provider::dsl::is_enabled.eq(false), // Typically, disable when soft-deleting
                    provider::dsl::updated_at.eq(current_time)
                ))
                .execute(conn) // Returns the number of affected rows
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to delete provider {}: {}", target_id_value, e))))
        })
    }

    /// Retrieves a provider by its key, if it's not marked as deleted.
    pub fn get_by_key(provider_key_val: &str) -> DbResult<Option<Provider>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_provider_opt = provider::table
                .filter(provider::dsl::provider_key.eq(provider_key_val).and(provider::dsl::deleted_at.is_null()))
                .select(ProviderDb::as_select())
                .first::<ProviderDb>(conn)
                .optional() // Returns Ok(None) if not found, rather than Err
                .map_err(|e| {
                    // We only expect NotFound to be handled by optional(), other errors are fatal
                    BaseError::DatabaseFatal(Some(format!("Error fetching provider by key '{}': {}", provider_key_val, e)))
                })?;
            
            Ok(db_provider_opt.map(|db_p| db_p.from_db()))
        })
    }

    /// Retrieves a provider by its ID, if it's not marked as deleted.
    pub fn get_by_id(target_id_value: i64) -> DbResult<Provider> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_provider = provider::table
                .filter(provider::dsl::id.eq(target_id_value).and(provider::dsl::deleted_at.is_null()))
                .select(ProviderDb::as_select()) // Select as ProviderDb
                .first::<ProviderDb>(conn)       // Expect a ProviderDb instance
                .map_err(|e| {
                    if matches!(e, diesel::result::Error::NotFound) {
                        BaseError::ParamInvalid(Some(format!("Provider with id {} not found", target_id_value)))
                    } else {
                        BaseError::DatabaseFatal(Some(format!("Error fetching provider {}: {}", target_id_value, e)))
                    }
                })?;
            Ok(db_provider.from_db())
        })
    }

    /// Lists all provider records that are not marked as deleted, ordered by creation date.
    pub fn list_all() -> DbResult<Vec<Provider>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_providers = provider::table
                .filter(provider::dsl::deleted_at.is_null())
                .order(provider::dsl::created_at.desc())
                .select(ProviderDb::as_select()) // Select as Vec<ProviderDb>
                .load::<ProviderDb>(conn)        // Expect Vec<ProviderDb>
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list providers: {}", e))))?;
            
            // Convert Vec<ProviderDb> to Vec<Provider>
            Ok(db_providers.into_iter().map(|db_p| db_p.from_db()).collect())
        })
    }

    /// Lists all active (not deleted and enabled) provider records, ordered by creation date.
    pub fn list_all_active() -> DbResult<Vec<Provider>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_providers = provider::table
                .filter(
                    provider::dsl::deleted_at
                        .is_null()
                        .and(provider::dsl::is_enabled.eq(true)),
                )
                .order(provider::dsl::created_at.desc())
                .select(ProviderDb::as_select())
                .load::<ProviderDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list active providers: {}",
                        e
                    )))
                })?;

            Ok(db_providers.into_iter().map(|db_p| db_p.from_db()).collect())
        })
    }

/// Retrieves a provider's details including API keys and custom fields by its ID.
    pub fn get_detail_by_id(provider_id_val: i64) -> DbResult<ProviderDetail> {
        // Get the main provider data
        let provider = Provider::get_by_id(provider_id_val)?;

        // Get associated API keys
        let api_keys = ProviderApiKey::list_by_provider_id(provider_id_val)?;

        // Get associated custom fields
        let custom_fields = CustomFieldDefinition::list_by_provider_id(provider_id_val)?;

        Ok(ProviderDetail {
            provider,
            api_keys,
            custom_fields,
        })
    }
}

impl ProviderApiKey {
    /// Inserts a new provider API key record.
    pub fn insert(new_key_data: &NewProviderApiKey) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_key = diesel::insert_into(provider_api_key::table)
                .values(NewProviderApiKeyDb::to_db(new_key_data))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to insert provider API key: {}", e))))?;
            Ok(db_key.from_db())
        })
    }

    /// Updates an existing provider API key.
    pub fn update(key_id: i64, update_data: &UpdateProviderApiKeyData) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let db_key = diesel::update(provider_api_key::table.find(key_id))
                .set((
                    UpdateProviderApiKeyDataDb::to_db(update_data),
                    provider_api_key::dsl::updated_at.eq(current_time)
                ))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to update provider API key {}: {}", key_id, e))))?;
            Ok(db_key.from_db())
        })
    }

    /// Soft deletes a provider API key.
    pub fn delete(key_id: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            diesel::update(provider_api_key::table.find(key_id))
                .set((
                    provider_api_key::dsl::deleted_at.eq(current_time),
                    provider_api_key::dsl::is_enabled.eq(false),
                    provider_api_key::dsl::updated_at.eq(current_time)
                ))
                .execute(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to delete provider API key {}: {}", key_id, e))))
        })
    }

    /// Retrieves a provider API key by its ID.
    pub fn get_by_id(key_id: i64) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_key = provider_api_key::table
                .filter(provider_api_key::dsl::id.eq(key_id).and(provider_api_key::dsl::deleted_at.is_null()))
                .select(ProviderApiKeyDb::as_select())
                .first::<ProviderApiKeyDb>(conn)
                .map_err(|e| {
                    if matches!(e, diesel::result::Error::NotFound) {
                        BaseError::ParamInvalid(Some(format!("Provider API key with id {} not found", key_id)))
                    } else {
                        BaseError::DatabaseFatal(Some(format!("Error fetching provider API key {}: {}", key_id, e)))
                    }
                })?;
            Ok(db_key.from_db())
        })
    }

    /// Lists all non-deleted API keys for a specific provider.
    pub fn list_by_provider_id(p_id: i64) -> DbResult<Vec<ProviderApiKey>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_keys = provider_api_key::table
                .filter(provider_api_key::dsl::provider_id.eq(p_id).and(provider_api_key::dsl::deleted_at.is_null()))
                .order(provider_api_key::dsl::created_at.desc())
                .select(ProviderApiKeyDb::as_select())
                .load::<ProviderApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list API keys for provider {}: {}", p_id, e))))?;
            Ok(db_keys.into_iter().map(|db_k| db_k.from_db()).collect())
        })
    }

    /// Lists all provider API key records that are not marked as deleted.
    pub fn list_all() -> DbResult<Vec<ProviderApiKey>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_keys = provider_api_key::table
                .filter(provider_api_key::dsl::deleted_at.is_null())
                .order(provider_api_key::dsl::created_at.desc())
                .select(ProviderApiKeyDb::as_select())
                .load::<ProviderApiKeyDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list all provider API keys: {}", e))))?;
            
            Ok(db_keys.into_iter().map(|db_k| db_k.from_db()).collect())
        })
    }
}
