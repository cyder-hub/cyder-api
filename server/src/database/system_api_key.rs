use chrono::Utc;
use diesel::prelude::*;
use rand::{distr::Alphanumeric, rng, Rng};
use serde::Deserialize; // For potential deserialization into New/Update structs if needed from API

use super::{get_connection, DbResult};
use crate::service::app_state::Storable;
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

// Define the main SystemApiKey struct using db_object!
// This struct is primarily for querying and returning data.
db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug)]
    #[diesel(table_name = system_api_key)] // Refers to table from schema in db_execute!
    pub struct SystemApiKey {
        pub id: i64,
        pub api_key: String,
        pub name: String,
        pub description: Option<String>,
        pub access_control_policy_id: Option<i64>,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
        pub ref_: Option<String>,
    }

    // Struct for inserting a new SystemApiKey.
    // Derives Insertable and points to the specific schema table.
    #[derive(Insertable, Debug)]
    #[diesel(table_name = system_api_key)] // This will resolve correctly inside db_execute!
    pub struct NewSystemApiKey {
        pub id: i64,
        pub api_key: String,
        pub name: String,
        pub description: Option<String>,
        pub access_control_policy_id: Option<i64>,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
        pub ref_: Option<String>,
    }

    // Struct for updating an existing SystemApiKey.
    // Derives AsChangeset and points to the specific schema table.
    // Optional fields allow for partial updates.
    // Option<Option<T>> allows explicitly setting a nullable field to NULL.
    #[derive(AsChangeset, Deserialize, Debug, Default)] // Default can be useful for building update payloads
    #[diesel(table_name = system_api_key)] // This will resolve correctly inside db_execute!
    pub struct UpdateSystemApiKeyData {
        #[diesel(column_name = name)]
        pub name: Option<String>,
        #[diesel(column_name = description)]
        pub description: Option<Option<String>>,
        #[diesel(column_name = access_control_policy_id)]
        pub access_control_policy_id: Option<Option<i64>>,
        #[diesel(column_name = is_enabled)]
        pub is_enabled: Option<bool>,
        // updated_at is handled manually in the update method
    }
}

impl SystemApiKey {
    /// Creates a new system API key.
    pub fn create(
        name: &str,
        description: Option<&str>,
        access_control_policy_id: Option<i64>,
    ) -> DbResult<SystemApiKey> {
        let now = Utc::now().timestamp_millis();
        let new_key_id = ID_GENERATOR.generate_id();

        let random_part: String = rng()
            .sample_iter(&Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();
        let api_key_value = format!("cyder-{}", random_part);

        let new_system_api_key_data = NewSystemApiKey {
            id: new_key_id,
            api_key: api_key_value,
            name: name.to_string(),
            description: description.map(|s| s.to_string()),
            access_control_policy_id,
            is_enabled: true,  // Default for new keys
            deleted_at: None, // Default for new keys
            created_at: now,
            updated_at: now,
            ref_: None,
        };

        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_db_key = diesel::insert_into(system_api_key::table)
                .values(NewSystemApiKeyDb::to_db(&new_system_api_key_data)) // Use NewSystemApiKey directly
                .returning(SystemApiKeyDb::as_returning()) // Use generated SystemApiKeyDb
                .get_result::<SystemApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to create system API key: {}",
                        e
                    )))
                })?;
            Ok(inserted_db_key.from_db()) // Convert SystemApiKeyDb to SystemApiKey
        })
    }

    /// Updates an existing system API key.
    pub fn update(id_value: i64, data: &UpdateSystemApiKeyData) -> DbResult<SystemApiKey> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_db_key = diesel::update(system_api_key::table.find(id_value))
                .set((
                    UpdateSystemApiKeyDataDb::to_db(data), // Use UpdateSystemApiKeyData directly
                    system_api_key::dsl::updated_at.eq(current_time),
                ))
                .returning(SystemApiKeyDb::as_returning())
                .get_result::<SystemApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update system API key {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_key.from_db())
        })
    }

    /// Refreshes the `ref` field of a system API key.
    pub fn refresh_ref(id_value: i64) -> DbResult<SystemApiKey> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        let random_ref: String = rng()
            .sample_iter(&Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();

        db_execute!(conn, {
            let updated_db_key = diesel::update(system_api_key::table.find(id_value))
                .set((
                    system_api_key::dsl::ref_.eq(Some(random_ref)),
                    system_api_key::dsl::updated_at.eq(current_time),
                ))
                .returning(SystemApiKeyDb::as_returning())
                .get_result::<SystemApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to refresh ref for system API key {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_key.from_db())
        })
    }

    /// Soft-deletes a system API key by ID.
    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(system_api_key::table.find(id_value))
                .set((
                    system_api_key::dsl::deleted_at.eq(Some(current_time)),
                    system_api_key::dsl::is_enabled.eq(false), // Typically disable on delete
                    system_api_key::dsl::updated_at.eq(current_time),
                ))
                .execute(conn) // Returns the number of affected rows
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete system API key {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    /// Lists all system API keys that are not marked as deleted.
    pub fn list_all() -> DbResult<Vec<SystemApiKey>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_keys = system_api_key::table
                .filter(system_api_key::dsl::deleted_at.is_null())
                .order(system_api_key::dsl::created_at.desc())
                .select(SystemApiKeyDb::as_select())
                .load::<SystemApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list system API keys: {}", e)))
                })?;
            Ok(db_keys.into_iter().map(|db_key| db_key.from_db()).collect())
        })
    }

    /// Retrieves a system API key by its ID, if not deleted.
    pub fn get_by_id(id_value: i64) -> DbResult<SystemApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_key = system_api_key::table
                .filter(
                    system_api_key::dsl::id
                        .eq(id_value)
                        .and(system_api_key::dsl::deleted_at.is_null()),
                )
                .select(SystemApiKeyDb::as_select())
                .first::<SystemApiKeyDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "System API key with id {} not found or deleted",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching system API key {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(db_key.from_db())
        })
    }

    /// Retrieves an active system API key by its string value.
    /// Active means not deleted and enabled.
    pub fn get_by_key(key_value: &str) -> DbResult<SystemApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_key = system_api_key::table
                .filter(
                    system_api_key::dsl::api_key
                        .eq(key_value)
                        .and(system_api_key::dsl::deleted_at.is_null())
                        .and(system_api_key::dsl::is_enabled.eq(true)),
                )
                .select(SystemApiKeyDb::as_select())
                .first::<SystemApiKeyDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "System API key ending with '...'{} not found, deleted, or disabled",
                        &key_value
                            .chars()
                            .rev()
                            .take(4)
                            .collect::<String>()
                            .chars()
                            .rev()
                            .collect::<String>()
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching system API key by key value: {}",
                        e
                    ))),
                })?;
            Ok(db_key.from_db())
        })
    }

    /// Retrieves an active system API key by its ref string.
    /// Active means not deleted and enabled.
    pub fn get_by_ref(ref_value: &str) -> DbResult<SystemApiKey> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_key = system_api_key::table
                .filter(
                    system_api_key::dsl::ref_
                        .eq(ref_value)
                        .and(system_api_key::dsl::deleted_at.is_null())
                        .and(system_api_key::dsl::is_enabled.eq(true)),
                )
                .select(SystemApiKeyDb::as_select())
                .first::<SystemApiKeyDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "System API key with ref '{}' not found, deleted, or disabled",
                        ref_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching system API key by ref value: {}",
                        e
                    ))),
                })?;
            Ok(db_key.from_db())
        })
    }
}

impl Storable for SystemApiKey {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> String {
        self.api_key.clone()
    }
}
