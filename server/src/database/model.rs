use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize;

use super::{get_connection, DbResult};
use crate::controller::BaseError;
use crate::service::app_state::Storable;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

use crate::database::custom_field::{ApiCustomFieldDefinition, CustomFieldDefinition};
use serde::Serialize;

// Import necessary items from the provider module for query_provider_model

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, serde::Serialize)]
    #[diesel(table_name = model)]
    pub struct Model {
        pub id: i64,
        pub provider_id: i64,
        pub model_name: String,
        pub real_model_name: Option<String>,
        pub billing_plan_id: Option<i64>,
        pub deleted_at: Option<i64>,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

#[derive(Insertable, Deserialize, Debug)]
#[diesel(table_name = model)]
pub struct NewModel {
    pub id: i64,
    pub provider_id: i64,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(AsChangeset, Deserialize, Debug, Default)]
#[diesel(table_name = model)]
pub struct UpdateModelData {
    pub model_name: Option<String>,
    pub real_model_name: Option<Option<String>>, // Allow setting to NULL
    pub is_enabled: Option<bool>,
    pub billing_plan_id: Option<Option<i64>>,
}

}

#[derive(Debug, Serialize)]
pub struct ModelDetail {
    pub model: Model,
    pub custom_fields: Vec<ApiCustomFieldDefinition>,
}

impl Model {
    /// Creates a new model record.
    pub fn create(
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
        is_enabled_val: bool,
    ) -> DbResult<Model> {
        let now = Utc::now().timestamp_millis();
        let new_id = ID_GENERATOR.generate_id();

        let new_model_data = NewModel {
            id: new_id,
            provider_id: provider_id_val,
            model_name: model_name_val.to_string(),
            real_model_name: real_model_name_val.map(|s| s.to_string()),
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_db_model = diesel::insert_into(model::table)
                .values(NewModelDb::to_db(&new_model_data))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to create model: {}", e)))
                })?;
            Ok(inserted_db_model.from_db())
        })
    }

    /// Updates an existing model record.
    pub fn update(id_value: i64, data: &UpdateModelData) -> DbResult<Model> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_db_model = diesel::update(model::table.find(id_value))
                .set((
                    UpdateModelDataDb::to_db(data),
                    model::dsl::updated_at.eq(current_time),
                ))
                .returning(ModelDb::as_returning())
                .get_result::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update model {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(updated_db_model.from_db())
        })
    }

    /// Soft-deletes a model by ID (sets deleted_at to current time and is_enabled to false).
    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(model::table.find(id_value))
                .set((
                    model::dsl::deleted_at.eq(current_time),
                    model::dsl::is_enabled.eq(false), // Typically disable on delete
                    model::dsl::updated_at.eq(current_time),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete model {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    /// Retrieves a model by its name and provider ID, if not deleted.
    pub fn get_by_name_and_provider_id(
        model_name_val: &str,
        provider_id_val: i64,
    ) -> DbResult<Option<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_model_opt = model::table
                .filter(
                    model::dsl::model_name
                        .eq(model_name_val)
                        .and(model::dsl::provider_id.eq(provider_id_val))
                        .and(model::dsl::deleted_at.is_null()),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(conn)
                .optional() // This makes it return Ok(None) if not found, instead of Err
                .map_err(|e| {
                    // We only expect NotFound to be handled by optional(), other errors are fatal
                    BaseError::DatabaseFatal(Some(format!(
                        "Error fetching model '{}' for provider {}: {}",
                        model_name_val, provider_id_val, e
                    )))
                })?;

            Ok(db_model_opt.map(|db_m| db_m.from_db()))
        })
    }

    /// Retrieves a model by its ID, if not deleted.
    pub fn get_by_id(id_value: i64) -> DbResult<Model> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_model = model::table
                .filter(
                    model::dsl::id
                        .eq(id_value)
                        .and(model::dsl::deleted_at.is_null()),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::ParamInvalid(Some(format!(
                        "Model with id {} not found or deleted",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching model {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(db_model.from_db())
        })
    }

    pub fn get_detail_by_id(model_id_val: i64) -> DbResult<ModelDetail> {
        let model = Model::get_by_id(model_id_val)?;
        let custom_fields = CustomFieldDefinition::list_by_model_id(model_id_val)?;
        Ok(ModelDetail {
            model,
            custom_fields,
        })
    }

    /// Lists all models for a given provider_id that are not marked as deleted.
    pub fn list_by_provider_id(provider_id_val: i64) -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_models = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id_val)
                        .and(model::dsl::deleted_at.is_null()),
                )
                .order(model::dsl::model_name.asc()) // Or by created_at, etc.
                .select(ModelDb::as_select())
                .load::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list models for provider {}: {}",
                        provider_id_val, e
                    )))
                })?;
            Ok(db_models.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Lists all models that are not marked as deleted.
    pub fn list_all() -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_models = model::table
                .left_join(provider::table.on(provider::dsl::id.eq(model::dsl::provider_id)))
                .filter(provider::dsl::deleted_at.is_null())
                .filter(model::dsl::deleted_at.is_null())
                .order(model::dsl::created_at.desc())
                .select(ModelDb::as_select())
                .load::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list all models: {}", e)))
                })?;
            Ok(db_models.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Lists all active (not deleted and enabled) models for a given provider_id.
    pub fn list_active_by_provider_id(provider_id_val: i64) -> DbResult<Vec<Model>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let db_models = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id_val)
                        .and(model::dsl::deleted_at.is_null())
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .order(model::dsl::model_name.asc())
                .select(ModelDb::as_select())
                .load::<ModelDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list active models for provider {}: {}",
                        provider_id_val, e
                    )))
                })?;
            Ok(db_models.into_iter().map(|db_m| db_m.from_db()).collect())
        })
    }

    /// Upserts a model based on provider_id and model_name.
    /// If it exists, it updates real_model_name, updated_at, and ensures is_deleted is false and is_enabled is true.
    /// If it doesn't exist, it creates a new model.
    pub fn upsert_by_provider_and_name(
        provider_id_val: i64,
        model_name_val: &str,
        real_model_name_val: Option<&str>,
    ) -> DbResult<Model> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let existing_model_db = model::table
                .filter(
                    model::dsl::provider_id
                        .eq(provider_id_val)
                        .and(model::dsl::model_name.eq(model_name_val)),
                )
                .select(ModelDb::as_select())
                .first::<ModelDb>(conn)
                .optional() // Makes it return Option<ModelDb>
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Error checking for existing model: {}",
                        e
                    )))
                })?;

            let now = Utc::now().timestamp_millis();

            match existing_model_db {
                Some(db_model) => {
                    let model_item = ModelDb::from_db(db_model);

                    // Update existing model
                    let update_data = UpdateModelData {
                        real_model_name: Some(real_model_name_val.map(|s| s.to_string())),
                        is_enabled: Some(true), // Ensure it's enabled
                        model_name: None,       // Not changing model_name itself here
                        billing_plan_id: None,  // Do not update billing_plan_id during upsert
                    };

                    // Also ensure it's not deleted
                    let updated_db_model = diesel::update(model::table.find(model_item.id))
                        .set((
                            UpdateModelDataDb::to_db(&update_data),
                            model::dsl::deleted_at.eq(None::<i64>),
                            model::dsl::updated_at.eq(now),
                        ))
                        .returning(ModelDb::as_returning())
                        .get_result::<ModelDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to update existing model during upsert: {}",
                                e
                            )))
                        })?;
                    Ok(updated_db_model.from_db())
                }
                None => {
                    // Create new model
                    let new_id = ID_GENERATOR.generate_id();
                    let new_model_data = NewModel {
                        id: new_id,
                        provider_id: provider_id_val,
                        model_name: model_name_val.to_string(),
                        real_model_name: real_model_name_val.map(|s| s.to_string()),
                        is_enabled: true,
                        created_at: now,
                        updated_at: now,
                    };
                    let inserted_db_model = diesel::insert_into(model::table)
                        .values(NewModelDb::to_db(&new_model_data))
                        .returning(ModelDb::as_returning())
                        .get_result::<ModelDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to insert new model during upsert: {}",
                                e
                            )))
                        })?;
                    Ok(inserted_db_model.from_db())
                }
            }
        })
    }
}

impl Storable for Model {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> String {
        self.id.to_string()
    }
}
