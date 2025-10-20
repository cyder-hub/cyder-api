use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{get_connection, DbResult, ListResult};
// Removed: use crate::controller::custom_field::ListCustomFieldQueryPayload;
use crate::controller::BaseError;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};
use crate::service::app_state::Storable; // Import Storable
use crate::schema::enum_def::{FieldPlacement, FieldType};

// --- Core Database Object Struct (managed by db_object!) ---
db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = custom_field_definition)]
    pub struct CustomFieldDefinition {
        pub id: i64,
        pub definition_name: Option<String>,
        pub definition_description: Option<String>,
        pub field_name: String,
        pub field_placement: FieldPlacement,
        pub field_type: FieldType,
        pub string_value: Option<String>,
        pub integer_value: Option<i64>,
        pub number_value: Option<f32>,
        pub boolean_value: Option<bool>,
        pub is_definition_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

// --- Internal Structs for DB Operations ---
#[derive(Insertable, Debug)]
#[diesel(table_name = custom_field_definition)]
pub struct DbNewCustomFieldDefinition {
    pub id: i64,
    pub definition_name: Option<String>,
    pub definition_description: Option<String>,
    pub field_name: String,
    pub field_placement: FieldPlacement,
    pub field_type: FieldType,
    pub string_value: Option<String>,
    pub integer_value: Option<i64>,
    pub number_value: Option<f32>,
    pub boolean_value: Option<bool>,
    pub is_definition_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(AsChangeset, Debug, Default)]
#[diesel(table_name = custom_field_definition)]
pub struct DbUpdateCustomFieldDefinition {
    pub definition_name: Option<Option<String>>,
    pub definition_description: Option<Option<String>>,
    pub field_name: Option<String>,
    pub field_placement: Option<FieldPlacement>,
    pub field_type: Option<FieldType>,
    pub string_value: Option<Option<String>>,
    pub integer_value: Option<Option<i64>>,
    pub number_value: Option<Option<f32>>,
    pub boolean_value: Option<Option<bool>>,
    pub is_definition_enabled: Option<bool>,
    // updated_at is set manually
    }

    // --- ModelCustomFieldAssignment ---
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = model_custom_field_assignment)]
    #[diesel(primary_key(model_id, custom_field_definition_id))]
    pub struct ModelCustomFieldAssignment {
        pub model_id: i64,
        pub custom_field_definition_id: i64,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }

    // --- ProviderCustomFieldAssignment ---
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = provider_custom_field_assignment)]
    #[diesel(primary_key(provider_id, custom_field_definition_id))]
    pub struct ProviderCustomFieldAssignment {
        pub provider_id: i64,
        pub custom_field_definition_id: i64,
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
    }
    
    #[derive(Insertable, Debug)]
    #[diesel(table_name = model_custom_field_assignment)]
    pub struct DbNewModelCustomFieldAssignment {
    pub model_id: i64,
    pub custom_field_definition_id: i64,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Insertable, Debug)]
#[diesel(table_name = provider_custom_field_assignment)]
pub struct DbNewProviderCustomFieldAssignment {
    pub provider_id: i64,
    pub custom_field_definition_id: i64,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}
}


// --- Payload Structs for API interaction (moved from controller) ---
#[derive(Deserialize, Debug, Default)] // Added Default for convenience if needed elsewhere
pub struct ListCustomFieldQueryPayload {
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub name: Option<String>, // For filtering by definition_name
}

#[derive(Deserialize, Debug)]
pub struct ListByProviderModelQueryPayload {
    pub provider_id: i64,
    pub model_id: i64,
}

// --- API-Facing Structs ---
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ApiCustomFieldDefinition {
    pub id: i64,
    pub name: Option<String>,        // Mapped from definition_name
    pub description: Option<String>, // Mapped from definition_description
    pub field_name: String,
    pub field_placement: FieldPlacement,
    pub field_type: FieldType,
    pub string_value: Option<String>,
    pub integer_value: Option<i64>,
    pub number_value: Option<f32>,
    pub boolean_value: Option<bool>,
    pub is_enabled: bool, // Mapped from is_definition_enabled
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiCreateCustomFieldDefinitionPayload {
    pub name: Option<String>,
    pub description: Option<String>,
    pub field_name: String,
    pub field_placement: FieldPlacement,
    pub field_type: FieldType,
    pub string_value: Option<String>,
    pub integer_value: Option<i64>,
    pub number_value: Option<f32>,
    pub boolean_value: Option<bool>,
    pub is_enabled: Option<bool>, // Defaults to true
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ApiUpdateCustomFieldDefinitionPayload {
    pub name: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub field_name: Option<String>,
    pub field_placement: Option<FieldPlacement>,
    pub field_type: Option<FieldType>,
    pub string_value: Option<Option<String>>,
    pub integer_value: Option<Option<i64>>,
    pub number_value: Option<Option<f32>>,
    pub boolean_value: Option<Option<bool>>,
    pub is_enabled: Option<bool>,
}

#[derive(Serialize, Debug)]
pub struct ApiCustomFieldDefinitionList {
    pub items: Vec<ApiCustomFieldDefinition>,
    pub total: i64,
    pub page: i64,
    pub page_size: i64,
}

// --- API Payloads for Linking/Unlinking ---
#[derive(Serialize, Deserialize, Debug)]
pub struct ApiLinkCustomFieldPayload {
    pub custom_field_definition_id: i64,
    pub model_id: Option<i64>,
    pub provider_id: Option<i64>,
    pub is_enabled: Option<bool>, // Defaults to true if not provided
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ApiUnlinkCustomFieldPayload {
    pub custom_field_definition_id: i64,
    pub model_id: Option<i64>,
    pub provider_id: Option<i64>,
}


// Helper to convert DB model (from db_object!) to API response model
fn to_api_response(cfd: CustomFieldDefinition) -> ApiCustomFieldDefinition {
    ApiCustomFieldDefinition {
        id: cfd.id,
        name: cfd.definition_name,
        description: cfd.definition_description,
        field_name: cfd.field_name,
        field_placement: cfd.field_placement,
        field_type: cfd.field_type,
        string_value: cfd.string_value,
        integer_value: cfd.integer_value,
        number_value: cfd.number_value,
        boolean_value: cfd.boolean_value,
        is_enabled: cfd.is_definition_enabled,
        created_at: cfd.created_at,
        updated_at: cfd.updated_at,
    }
}

impl From<ApiCustomFieldDefinition> for CustomFieldDefinition {
    fn from(api_cfd: ApiCustomFieldDefinition) -> Self {
        CustomFieldDefinition {
            id: api_cfd.id,
            definition_name: api_cfd.name,
            definition_description: api_cfd.description,
            field_name: api_cfd.field_name,
            field_placement: api_cfd.field_placement,
            field_type: api_cfd.field_type,
            string_value: api_cfd.string_value,
            integer_value: api_cfd.integer_value,
            number_value: api_cfd.number_value,
            boolean_value: api_cfd.boolean_value,
            is_definition_enabled: api_cfd.is_enabled,
            deleted_at: None,
            created_at: api_cfd.created_at,
            updated_at: api_cfd.updated_at,
        }
    }
}

impl Storable for CustomFieldDefinition {
    fn id(&self) -> i64 {
        self.id
    }

    fn key(&self) -> String {
        // Since the StateStore for CustomFieldDefinition will be created with with_key_map: false,
        // this key might not be strictly used for map lookups within that store.
        // Using id.to_string() as a unique key.
        self.id.to_string()
    }

    // group_id is not applicable for CustomFieldDefinition in this context,
    // so we use the default implementation which returns None.
}

impl CustomFieldDefinition {
    pub fn create(
        payload: ApiCreateCustomFieldDefinitionPayload,
    ) -> DbResult<ApiCustomFieldDefinition> {
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();
        let new_id = ID_GENERATOR.generate_id();

        let new_cfd_db = DbNewCustomFieldDefinition {
            id: new_id,
            definition_name: payload.name,
            definition_description: payload.description,
            field_name: payload.field_name,
            field_placement: payload.field_placement,
            field_type: payload.field_type,
            string_value: payload.string_value,
            integer_value: payload.integer_value,
            number_value: payload.number_value,
            boolean_value: payload.boolean_value,
            is_definition_enabled: payload.is_enabled.unwrap_or(true),
            created_at: now,
            updated_at: now,
        };

        db_execute!(conn, {
            let inserted_cfd_db = diesel::insert_into(custom_field_definition::table)
                .values(DbNewCustomFieldDefinitionDb::to_db(&new_cfd_db))
                .returning(CustomFieldDefinitionDb::as_returning())
                .get_result::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to create custom field definition: {}",
                        e
                    )))
                })?;

            Ok(to_api_response(inserted_cfd_db.from_db()))
        })
    }

    pub fn update(
        id_value: i64,
        payload: ApiUpdateCustomFieldDefinitionPayload,
    ) -> DbResult<ApiCustomFieldDefinition> {
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();

        let update_cfd_data = DbUpdateCustomFieldDefinition {
            definition_name: payload.name,
            definition_description: payload.description,
            field_name: payload.field_name,
            field_placement: payload.field_placement,
            field_type: payload.field_type,
            string_value: payload.string_value,
            integer_value: payload.integer_value,
            number_value: payload.number_value,
            boolean_value: payload.boolean_value,
            is_definition_enabled: payload.is_enabled,
        };

        db_execute!(conn, {
            let updated_cfd_db = diesel::update(custom_field_definition::table.find(id_value))
                .set((
                    DbUpdateCustomFieldDefinitionDb::to_db(&update_cfd_data),
                    custom_field_definition::dsl::updated_at.eq(now),
                ))
                .returning(CustomFieldDefinitionDb::as_returning())
                .get_result::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "CustomFieldDefinition {} not found for update",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Failed to update custom field definition {}: {}",
                        id_value, e
                    ))),
                })?;
            Ok(to_api_response(updated_cfd_db.from_db()))
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(custom_field_definition::table.find(id_value))
                .set((
                    custom_field_definition::dsl::deleted_at.eq(now),
                    custom_field_definition::dsl::is_definition_enabled.eq(false),
                    custom_field_definition::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to soft-delete custom field definition {}: {}",
                        id_value, e
                    )))
                })
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<ApiCustomFieldDefinition> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let cfd_db = custom_field_definition::table
                .filter(custom_field_definition::dsl::id.eq(id_value))
                .filter(custom_field_definition::dsl::deleted_at.is_null())
                .select(CustomFieldDefinitionDb::as_select())
                .first::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "CustomFieldDefinition {} not found",
                        id_value
                    ))),
                    _ => BaseError::DatabaseFatal(Some(e.to_string())),
                })?;
            Ok(to_api_response(cfd_db.from_db()))
        })
    }

    pub fn list(
        payload: ListCustomFieldQueryPayload,
    ) -> DbResult<ListResult<ApiCustomFieldDefinition>> {
        let conn = &mut get_connection();

        let page = payload.page.unwrap_or(1);
        let page_size = payload.page_size.unwrap_or(10); // Default page size
        let offset = (page - 1) * page_size;

        db_execute!(conn, {
            // `conn` inside this block is the actual PgConnection or SqliteConnection
            let query = custom_field_definition::table // Removed mut
                .into_boxed()
                .filter(custom_field_definition::dsl::deleted_at.is_null());
            
            let count_query = custom_field_definition::table // Removed mut
                .into_boxed()
                .filter(custom_field_definition::dsl::deleted_at.is_null());

            //if let Some(name_val) = payload.name.as_ref() {
            //    let pattern = format!("%{}%", name_val);
            //    query = query.filter(custom_field_definition::dsl::definition_name.ilike(pattern.clone()));
            //    count_query = count_query.filter(custom_field_definition::dsl::definition_name.ilike(pattern));
            //}

            let total = count_query
                .select(diesel::dsl::count_star())
                .first::<i64>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to count custom field definitions: {}", e))))?;

            let items_db = query
                .order(custom_field_definition::dsl::created_at.desc())
                .limit(page_size)
                .offset(offset)
                .select(CustomFieldDefinitionDb::as_select())
                .load::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list custom field definitions: {}", e))))?;

            let api_list: Vec<ApiCustomFieldDefinition> = items_db
                .into_iter()
                .map(|cfd_db| to_api_response(cfd_db.from_db()))
                .collect();

            Ok(ListResult {
                list: api_list, // Assuming ListResult uses 'list' field name
                total,
                page,
                page_size,
            })
        })
    }

    pub fn list_all_active() -> DbResult<Vec<CustomFieldDefinition>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            custom_field_definition::table
                .filter(custom_field_definition::dsl::deleted_at.is_null())
                .filter(custom_field_definition::dsl::is_definition_enabled.eq(true))
                .select(CustomFieldDefinitionDb::as_select())
                .load::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list all active custom field definitions: {}", e))))
                .map(|vec_db| vec_db.into_iter().map(|db_item| db_item.from_db()).collect())
        })
    }

    // --- Model Custom Field Assignment ---
    pub fn link_model(
        custom_field_definition_id_val: i64,
        model_id_val: i64,
        is_enabled_val: bool,
    ) -> DbResult<ModelCustomFieldAssignment> {
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();

        let new_assignment_db = DbNewModelCustomFieldAssignment {
            model_id: model_id_val,
            custom_field_definition_id: custom_field_definition_id_val,
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        db_execute!(conn, {
            diesel::insert_into(model_custom_field_assignment::table)
                .values(DbNewModelCustomFieldAssignmentDb::to_db(&new_assignment_db))
                .returning(ModelCustomFieldAssignmentDb::as_returning())
                .get_result::<ModelCustomFieldAssignmentDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        info,
                    ) => BaseError::DatabaseFatal(Some(format!( // Changed to Conflict
                        "Custom field already assigned to this model: {}",
                        info.message()
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Failed to link custom field to model: {}", // Changed message
                        e
                    ))),
                })
                .map(|db_obj| db_obj.from_db())
        })
    }

    pub fn unlink_model(
        custom_field_definition_id_val: i64,
        model_id_val: i64,
    ) -> DbResult<usize> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::delete(
                model_custom_field_assignment::table
                    .filter(model_custom_field_assignment::dsl::custom_field_definition_id.eq(custom_field_definition_id_val))
                    .filter(model_custom_field_assignment::dsl::model_id.eq(model_id_val)),
            )
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to unlink custom field from model: {}", // Changed message
                    e
                )))
            })
        })
    }

    pub fn list_all_enabled_model_assignments() -> DbResult<Vec<ModelCustomFieldAssignment>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            model_custom_field_assignment::table
                .filter(model_custom_field_assignment::dsl::is_enabled.eq(true))
                .select(ModelCustomFieldAssignmentDb::as_select())
                .load::<ModelCustomFieldAssignmentDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list all enabled model custom field assignments: {}", e))))
                .map(|vec_db| vec_db.into_iter().map(|db_item| db_item.from_db()).collect())
        })
    }

    // --- Provider Custom Field Assignment ---
    pub fn link_provider(
        custom_field_definition_id_val: i64,
        provider_id_val: i64,
        is_enabled_val: bool,
    ) -> DbResult<ProviderCustomFieldAssignment> {
        let conn = &mut get_connection();
        let now = Utc::now().timestamp_millis();

        let new_assignment_db = DbNewProviderCustomFieldAssignment {
            provider_id: provider_id_val,
            custom_field_definition_id: custom_field_definition_id_val,
            is_enabled: is_enabled_val,
            created_at: now,
            updated_at: now,
        };

        db_execute!(conn, {
            diesel::insert_into(provider_custom_field_assignment::table)
                .values(DbNewProviderCustomFieldAssignmentDb::to_db(&new_assignment_db))
                .returning(ProviderCustomFieldAssignmentDb::as_returning())
                .get_result::<ProviderCustomFieldAssignmentDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::DatabaseError(
                        diesel::result::DatabaseErrorKind::UniqueViolation,
                        info,
                    ) => BaseError::DatabaseFatal(Some(format!( // Changed to Conflict
                        "Custom field already assigned to this provider: {}",
                        info.message()
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Failed to link custom field to provider: {}", // Changed message
                        e
                    ))),
                })
                .map(|db_obj| db_obj.from_db())
        })
    }

    pub fn unlink_provider(
        custom_field_definition_id_val: i64,
        provider_id_val: i64,
    ) -> DbResult<usize> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            diesel::delete(
                provider_custom_field_assignment::table
                    .filter(provider_custom_field_assignment::dsl::custom_field_definition_id.eq(custom_field_definition_id_val))
                    .filter(provider_custom_field_assignment::dsl::provider_id.eq(provider_id_val)),
            )
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to unlink custom field from provider: {}", // Changed message
                    e
                )))
            })
        })
    }

    pub fn list_all_enabled_provider_assignments() -> DbResult<Vec<ProviderCustomFieldAssignment>> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            provider_custom_field_assignment::table
                .filter(provider_custom_field_assignment::dsl::is_enabled.eq(true))
                .select(ProviderCustomFieldAssignmentDb::as_select())
                .load::<ProviderCustomFieldAssignmentDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list all enabled provider custom field assignments: {}", e))))
                .map(|vec_db| vec_db.into_iter().map(|db_item| db_item.from_db()).collect())
        })
    }

    pub fn list_by_provider_id(
        provider_id_val: i64,
    ) -> DbResult<Vec<ApiCustomFieldDefinition>> {
        let conn = &mut get_connection();

        db_execute!(conn, {
            let query = custom_field_definition::table
                .inner_join(provider_custom_field_assignment::table.on(
                    custom_field_definition::dsl::id.eq(provider_custom_field_assignment::dsl::custom_field_definition_id)
                ))
                .filter(custom_field_definition::dsl::deleted_at.is_null())
                .filter(custom_field_definition::dsl::is_definition_enabled.eq(true))
                .filter(
                    provider_custom_field_assignment::dsl::provider_id.eq(provider_id_val)
                    .and(provider_custom_field_assignment::dsl::is_enabled.eq(true))
                )
                .select(CustomFieldDefinitionDb::as_select())
                .distinct();

            let items_db = query
                .load::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list custom fields by provider: {}", e))))?;

            let api_list: Vec<ApiCustomFieldDefinition> = items_db
                .into_iter()
                .map(|cfd_db| to_api_response(cfd_db.from_db()))
                .collect();

            Ok(api_list)
        })
    }

    pub fn list_by_model_id(
        model_id_val: i64,
    ) -> DbResult<Vec<ApiCustomFieldDefinition>> {
        let conn = &mut get_connection();

        db_execute!(conn, {
            let query = custom_field_definition::table
                .inner_join(model_custom_field_assignment::table.on(
                    custom_field_definition::dsl::id.eq(model_custom_field_assignment::dsl::custom_field_definition_id)
                ))
                .filter(custom_field_definition::dsl::deleted_at.is_null())
                .filter(custom_field_definition::dsl::is_definition_enabled.eq(true))
                .filter(
                    model_custom_field_assignment::dsl::model_id.eq(model_id_val)
                    .and(model_custom_field_assignment::dsl::is_enabled.eq(true))
                )
                .select(CustomFieldDefinitionDb::as_select())
                .distinct();

            let items_db = query
                .load::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list custom fields by model: {}", e))))?;

            let api_list: Vec<ApiCustomFieldDefinition> = items_db
                .into_iter()
                .map(|cfd_db| to_api_response(cfd_db.from_db()))
                .collect();

            Ok(api_list)
        })
    }

    pub fn list_by_provider_model(
        provider_id_val: i64,
        model_id_val: i64,
    ) -> DbResult<Vec<ApiCustomFieldDefinition>> {
        let conn = &mut get_connection();

        db_execute!(conn, {
            // Ensure DSLs are in scope. db_object! should handle this.
            // use crate::schema::{custom_field_definition, model_custom_field_assignment, provider_custom_field_assignment};

            let query = custom_field_definition::table
                .left_join(model_custom_field_assignment::table.on(
                    custom_field_definition::dsl::id.eq(model_custom_field_assignment::dsl::custom_field_definition_id)
                ))
                .left_join(provider_custom_field_assignment::table.on(
                    custom_field_definition::dsl::id.eq(provider_custom_field_assignment::dsl::custom_field_definition_id)
                ))
                .filter(custom_field_definition::dsl::deleted_at.is_null())
                .filter(custom_field_definition::dsl::is_definition_enabled.eq(true))
                .filter(
                    model_custom_field_assignment::dsl::model_id.eq(model_id_val)
                        .and(model_custom_field_assignment::dsl::is_enabled.eq(true))
                    .or(
                        provider_custom_field_assignment::dsl::provider_id.eq(provider_id_val)
                        .and(provider_custom_field_assignment::dsl::is_enabled.eq(true))
                    )
                )
                .select(CustomFieldDefinitionDb::as_select())
                .distinct();

            let items_db = query
                .load::<CustomFieldDefinitionDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list custom fields by provider/model: {}", e))))?;

            let api_list: Vec<ApiCustomFieldDefinition> = items_db
                .into_iter()
                .map(|cfd_db| to_api_response(cfd_db.from_db()))
                .collect();

            Ok(api_list)
        })
    }
}
