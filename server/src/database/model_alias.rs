use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize; // For API input deserialization and output serialization

use super::{get_connection, model::Model, provider::Provider, DbResult};
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};
#[derive(Debug, Clone, serde::Serialize)]
pub struct ProviderModelInfo {
    pub model: Model,
    pub provider: Provider,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ModelAliasDetails {
    #[serde(flatten)]
    pub alias: ModelAlias,
    pub model_name: String,
    pub provider_key: String, // Changed to provider_key
    pub real_model_name: Option<String>,
}

db_object! {
    // Main struct for ModelAlias, used for querying and returning data.
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = model_alias)]
    pub struct ModelAlias {
        pub id: i64,
        pub alias_name: String,
        pub target_model_id: i64,
        pub description: Option<String>,
        pub priority: Option<i32>,
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    // Struct for inserting a new ModelAlias.
    // All fields are included to match the table structure for Insertable.
    #[derive(Insertable, Debug, Deserialize)] // Deserialize if created from API payload
    #[diesel(table_name = model_alias)]
    pub struct NewModelAlias {
        pub id: i64,
        pub alias_name: String,
        pub target_model_id: i64,
        pub description: Option<String>,
        pub priority: Option<i32>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    // Struct for updating an existing ModelAlias.
    // Optional fields allow for partial updates.
    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = model_alias)]
    pub struct UpdateModelAliasData {
        pub alias_name: Option<String>,
        pub target_model_id: Option<i64>,
        pub description: Option<Option<String>>, // Option<Option<T>> to allow setting to NULL
        pub priority: Option<Option<i32>>,       // Option<Option<T>> to allow setting to NULL
        pub is_enabled: Option<bool>,
        // updated_at is handled manually.
        // deleted_at is handled by the delete method.
    }
}

impl crate::service::app_state::Storable for ModelAlias {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> String {
        self.alias_name.clone()
    }
}

impl ModelAlias {
    /// Creates a new model alias.
    pub fn create(
        alias_name: &str,
        target_model_id: i64,
        description: Option<&str>,
        priority: Option<i32>,
        is_enabled: bool,
    ) -> DbResult<ModelAlias> {
        let now = Utc::now().timestamp_millis();
        let new_id = ID_GENERATOR.generate_id();

        let new_model_alias_data = NewModelAlias {
            id: new_id,
            alias_name: alias_name.to_string(),
            target_model_id,
            description: description.map(|s| s.to_string()),
            priority,
            is_enabled,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_db_alias = diesel::insert_into(model_alias::table)
                .values(NewModelAliasDb::to_db(&new_model_alias_data))
                .returning(ModelAliasDb::as_returning())
                .get_result::<ModelAliasDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to create model alias: {}", e)))
                })?;
            Ok(inserted_db_alias.from_db())
        })
    }

    /// Updates an existing model alias.
    pub fn update(id_value: i64, data: &UpdateModelAliasData) -> DbResult<ModelAlias> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_db_alias = diesel::update(model_alias::table.find(id_value))
                .set((
                    UpdateModelAliasDataDb::to_db(data),
                    model_alias::dsl::updated_at.eq(current_time),
                ))
                .returning(ModelAliasDb::as_returning())
                .get_result::<ModelAliasDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update model alias {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_alias.from_db())
        })
    }

    /// Soft-deletes a model alias by ID.
    /// Sets `deleted_at` to current time and `is_enabled` to false.
    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(model_alias::table.find(id_value))
                .set((
                    model_alias::dsl::deleted_at.eq(current_time),
                    model_alias::dsl::is_enabled.eq(false),
                    model_alias::dsl::updated_at.eq(current_time),
                ))
                .execute(conn) // Returns the number of affected rows
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete model alias {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    /// Retrieves a model alias by its ID, if not deleted.
    pub fn get_by_id(id_value: i64) -> DbResult<ModelAlias> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_alias = model_alias::table
                .filter(
                    model_alias::dsl::id
                        .eq(id_value)
                        .and(model_alias::dsl::deleted_at.is_null()),
                )
                .select(ModelAliasDb::as_select())
                .first::<ModelAliasDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Model alias with id {} not found or deleted",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching model alias {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(db_alias.from_db())
        })
    }

    /// Lists all model aliases that are not marked as deleted.
    pub fn list_all() -> DbResult<Vec<ModelAlias>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_aliases = model_alias::table
                .filter(model_alias::dsl::deleted_at.is_null())
                .order(model_alias::dsl::created_at.desc())
                .select(ModelAliasDb::as_select())
                .load::<ModelAliasDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list model aliases: {}", e)))
                })?;
            Ok(db_aliases.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Lists all model aliases with their target model details, that are not marked as deleted.
    pub fn list_all_details() -> DbResult<Vec<ModelAliasDetails>> {
            let conn = &mut get_connection();
            db_execute!(conn, {
                let results = model_alias::table
                    .inner_join(model::table.on(model_alias::dsl::target_model_id.eq(model::dsl::id)))
                    .inner_join(provider::table.on(model::dsl::provider_id.eq(provider::dsl::id))) // Added join with provider table
                    .filter(model_alias::dsl::deleted_at.is_null())
                    .order(model_alias::dsl::created_at.desc())
                    .select((
                        ModelAliasDb::as_select(),
                        model::dsl::model_name,
                        provider::dsl::provider_key, // Selected provider_key
                        model::dsl::real_model_name,
                    ))
                    .load::<(ModelAliasDb, String, String, Option<String>)>(conn) // Load type remains the same as provider_key is also String
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to list model alias details: {}",
                            e
                        )))
                    })?;
    
                Ok(results
                    .into_iter()
                    .map(
                        |(alias_db, model_name, provider_key, real_model_name)| ModelAliasDetails { // Updated mapping
                            alias: alias_db.from_db(),
                            model_name,
                            provider_key,
                            real_model_name,
                        },
                    )
                    .collect())
            })
        }
    
        /// Lists all model aliases for a given target_model_id that are not marked as deleted.
    pub fn list_by_target_model_id(target_model_id_val: i64) -> DbResult<Vec<ModelAlias>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_aliases = model_alias::table
                .filter(
                    model_alias::dsl::target_model_id
                        .eq(target_model_id_val)
                        .and(model_alias::dsl::deleted_at.is_null()),
                )
                .order((
                    model_alias::dsl::priority.asc(),
                    model_alias::dsl::alias_name.asc(),
                ))
                .select(ModelAliasDb::as_select())
                .load::<ModelAliasDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list model aliases for target model {}: {}",
                        target_model_id_val, e
                    )))
                })?;
            Ok(db_aliases.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Retrieves an active model alias by its alias name.
    /// Active means not deleted and enabled.
    pub fn get_by_alias_name(name: &str) -> DbResult<Option<ModelAlias>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_alias = model_alias::table
                .filter(
                    model_alias::dsl::alias_name
                        .eq(name)
                        .and(model_alias::dsl::deleted_at.is_null())
                        .and(model_alias::dsl::is_enabled.eq(true)),
                )
                .select(ModelAliasDb::as_select())
                .first::<ModelAliasDb>(conn)
                .optional() // Returns Option<ModelAliasDb>
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Error fetching model alias by name '{}': {}",
                        name, e
                    )))
                })?;
            Ok(db_alias.map(|db_m| db_m.from_db()))
        })
    }
    /// Retrieves active Provider and Model details based on an alias name.
    /// Returns None if the alias, model, or provider is not found, not enabled, or deleted.
    pub fn get_provider_model_info_by_alias_name(
        name: &str,
    ) -> DbResult<Option<ProviderModelInfo>> {
        match Self::get_by_alias_name(name) {
            Ok(Some(model_alias)) => {
                // ModelAlias found and is active (enabled and not deleted)
                let model = match Model::get_by_id(model_alias.target_model_id) {
                    Ok(m) => {
                        if !m.is_enabled {
                            return Ok(None); // Model is not enabled
                        }
                        m
                    }
                    Err(BaseError::ParamInvalid(_)) => return Ok(None), // Model not found or deleted
                    Err(e) => return Err(e),                            // Other database error
                };

                let provider = match Provider::get_by_id(model.provider_id) {
                    Ok(p) => {
                        if !p.is_enabled {
                            return Ok(None); // Provider is not enabled
                        }
                        p
                    }
                    Err(BaseError::ParamInvalid(_)) => return Ok(None), // Provider not found or deleted
                    Err(e) => return Err(e),                            // Other database error
                };

                Ok(Some(ProviderModelInfo { model, provider }))
            }
            Ok(None) => Ok(None), // Alias not found or not active
            Err(e) => Err(e),     // Database error fetching alias
        }
    }
}
