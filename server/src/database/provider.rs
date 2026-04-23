use chrono::Utc;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use serde::Deserialize; // Serialize on Provider is via db_object!, Deserialize for helper structs
use serde::Serialize;

use crate::database::model::{Model, NewModel};
use crate::database::{DbConnection, DbResult, get_connection};
use crate::{db_execute, db_object};
// db_object! is exported at the crate root by `#[macro_export]` in `database/mod.rs`.
// BaseError is assumed to be accessible, e.g., from `crate::controller::BaseError`.
use crate::controller::BaseError;
use crate::database::request_patch::{RequestPatchRule, RequestPatchRuleResponse};
use crate::schema::enum_def::{ProviderApiKeyMode, ProviderType};
use crate::utils::ID_GENERATOR;

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
        pub provider_api_key_mode: ProviderApiKeyMode,
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
        pub provider_api_key_mode: ProviderApiKeyMode,
    }

// Data structure for updating an existing provider.
#[derive(AsChangeset, Deserialize, Debug, Clone)]
#[diesel(table_name = provider)]
    pub struct UpdateProviderData {
        pub provider_key: Option<String>,
        pub name: Option<String>,
        pub endpoint: Option<String>,
        pub use_proxy: Option<bool>,
        pub is_enabled: Option<bool>,
        pub provider_type: Option<ProviderType>,
        pub provider_api_key_mode: Option<ProviderApiKeyMode>,
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
#[derive(Clone, Debug)]
pub struct BootstrapProviderInput {
    pub provider_id: i64,
    pub provider_key: String,
    pub name: String,
    pub endpoint: String,
    pub use_proxy: bool,
    pub provider_type: ProviderType,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub api_key: String,
    pub api_key_description: Option<String>,
    pub model_name: String,
    pub real_model_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BootstrapProviderResult {
    pub provider: Provider,
    pub created_key: ProviderApiKey,
    pub created_model: Model,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummaryItem {
    pub id: i64,
    pub provider_key: String,
    pub name: String,
    pub is_enabled: bool,
}

macro_rules! bootstrap_transaction {
    ($conn:expr, $model_new_db:ident, $model_db:ident, $input:expr) => {{
        let bootstrap_input = $input;
        let current_time = Utc::now().timestamp_millis();

        $conn.batch_execute("BEGIN").map_err(|e| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to start bootstrap transaction: {}",
                e
            )))
        })?;

        let transaction_result: DbResult<BootstrapProviderResult> = (|| {
            let new_provider_data = NewProvider {
                id: bootstrap_input.provider_id,
                provider_key: bootstrap_input.provider_key.clone(),
                name: bootstrap_input.name.clone(),
                endpoint: bootstrap_input.endpoint.clone(),
                use_proxy: bootstrap_input.use_proxy,
                is_enabled: true,
                created_at: current_time,
                updated_at: current_time,
                provider_type: bootstrap_input.provider_type.clone(),
                provider_api_key_mode: bootstrap_input.provider_api_key_mode.clone(),
            };

            let provider_db = diesel::insert_into(provider::table)
                .values(NewProviderDb::to_db(&new_provider_data))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>($conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert bootstrap provider: {}",
                        e
                    )))
                })?;
            let provider = provider_db.from_db();

            let new_provider_api_key_data = NewProviderApiKey {
                id: ID_GENERATOR.generate_id(),
                provider_id: provider.id,
                api_key: bootstrap_input.api_key.clone(),
                description: bootstrap_input.api_key_description.clone(),
                is_enabled: true,
                created_at: current_time,
                updated_at: current_time,
            };

            let created_key_db = diesel::insert_into(provider_api_key::table)
                .values(NewProviderApiKeyDb::to_db(&new_provider_api_key_data))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>($conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert bootstrap provider API key: {}",
                        e
                    )))
                })?;
            let created_key = created_key_db.from_db();

            let new_model_data = NewModel {
                id: ID_GENERATOR.generate_id(),
                provider_id: provider.id,
                model_name: bootstrap_input.model_name.clone(),
                real_model_name: bootstrap_input.real_model_name.clone(),
                supports_streaming: true,
                supports_tools: true,
                supports_reasoning: true,
                supports_image_input: true,
                supports_embeddings: true,
                supports_rerank: true,
                is_enabled: true,
                created_at: current_time,
                updated_at: current_time,
            };

            let created_model_db = diesel::insert_into(model::table)
                .values($model_new_db::to_db(&new_model_data))
                .returning($model_db::as_returning())
                .get_result::<$model_db>($conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert bootstrap model: {}",
                        e
                    )))
                })?;
            let created_model = created_model_db.from_db();

            Ok(BootstrapProviderResult {
                provider,
                created_key,
                created_model,
            })
        })();

        match transaction_result {
            Ok(result) => {
                $conn.batch_execute("COMMIT").map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to commit bootstrap transaction: {}",
                        e
                    )))
                })?;
                Ok(result)
            }
            Err(err) => {
                let _ = $conn.batch_execute("ROLLBACK");
                Err(err)
            }
        }
    }};
}

#[derive(Debug, Serialize)]
pub struct ProviderDetail {
    pub provider: Provider,
    pub api_keys: Vec<ProviderApiKey>,
    pub request_patches: Vec<RequestPatchRuleResponse>,
}
impl Provider {
    /// Inserts a new provider record into the database.
    pub fn create(new_provider_data: &NewProvider) -> DbResult<Provider> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            // Inside db_execute!, `ProviderDb` refers to the DB-specific generated struct (_postgres_model::ProviderDb or _sqlite_model::ProviderDb).
            // `provider::table` refers to the table from the DB-specific schema.
            // The `new_provider_data` (NewProvider struct) is Insertable into `crate::schema::postgres::provider::table`.
            // This should be compatible as long as the column types match.
            let db_provider = diesel::insert_into(provider::table)
                .values(NewProviderDb::to_db(new_provider_data))
                .returning(ProviderDb::as_returning()) // Use the generated ProviderDb for returning
                .get_result::<ProviderDb>(conn) // Expect a ProviderDb instance
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to insert provider: {}", e)))
                })?;
            Ok(db_provider.from_db()) // Convert ProviderDb to the main Provider struct
        })
    }

    /// Updates an existing provider record in the database.
    pub fn update(id_value: i64, update_data: &UpdateProviderData) -> DbResult<Provider> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        let mut update_data = update_data.clone();
        update_data.provider_key = None;

        db_execute!(conn, {
            // The `update_data` (UpdateProviderData struct) is AsChangeset for `crate::schema::postgres::provider::table`.
            let db_provider = diesel::update(provider::table.find(id_value))
                .set((
                    UpdateProviderDataDb::to_db(&update_data),
                    provider::dsl::updated_at.eq(current_time),
                ))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update provider {}: {}",
                        id_value, e
                    )))
                })?;
            Ok(db_provider.from_db())
        })
    }

    pub fn bootstrap(input: &BootstrapProviderInput) -> DbResult<BootstrapProviderResult> {
        let conn = &mut get_connection()?;
        match conn {
            DbConnection::Postgres(conn) => {
                use self::_postgres_model::*;
                use crate::database::_postgres_schema::*;
                use crate::database::model::_postgres_model::{
                    ModelDb as BootstrapModelDb, NewModelDb as BootstrapNewModelDb,
                };
                bootstrap_transaction!(conn, BootstrapNewModelDb, BootstrapModelDb, input)
            }
            DbConnection::Sqlite(conn) => {
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                use crate::database::model::_sqlite_model::{
                    ModelDb as BootstrapModelDb, NewModelDb as BootstrapNewModelDb,
                };
                bootstrap_transaction!(conn, BootstrapNewModelDb, BootstrapModelDb, input)
            }
        }
    }

    /// Soft deletes a provider record by setting `deleted_at` to the current time and `is_enabled` to false.
    pub fn delete(target_id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            diesel::update(provider::table.find(target_id_value))
                .set((
                    provider::dsl::deleted_at.eq(current_time),
                    provider::dsl::is_enabled.eq(false), // Typically, disable when soft-deleting
                    provider::dsl::updated_at.eq(current_time),
                ))
                .execute(conn) // Returns the number of affected rows
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete provider {}: {}",
                        target_id_value, e
                    )))
                })
        })
    }

    /// Retrieves a provider by its key, if it's not marked as deleted.
    pub fn get_by_key(provider_key_val: &str) -> DbResult<Option<Provider>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_provider_opt = provider::table
                .filter(
                    provider::dsl::provider_key
                        .eq(provider_key_val)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select(ProviderDb::as_select())
                .first::<ProviderDb>(conn)
                .optional() // Returns Ok(None) if not found, rather than Err
                .map_err(|e| {
                    // We only expect NotFound to be handled by optional(), other errors are fatal
                    BaseError::DatabaseFatal(Some(format!(
                        "Error fetching provider by key '{}': {}",
                        provider_key_val, e
                    )))
                })?;

            Ok(db_provider_opt.map(|db_p| db_p.from_db()))
        })
    }

    /// Retrieves a provider by its ID, if it's not marked as deleted.
    pub fn get_by_id(target_id_value: i64) -> DbResult<Provider> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_provider = provider::table
                .filter(
                    provider::dsl::id
                        .eq(target_id_value)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select(ProviderDb::as_select()) // Select as ProviderDb
                .first::<ProviderDb>(conn) // Expect a ProviderDb instance
                .map_err(|e| {
                    if matches!(e, diesel::result::Error::NotFound) {
                        BaseError::ParamInvalid(Some(format!(
                            "Provider with id {} not found",
                            target_id_value
                        )))
                    } else {
                        BaseError::DatabaseFatal(Some(format!(
                            "Error fetching provider {}: {}",
                            target_id_value, e
                        )))
                    }
                })?;
            Ok(db_provider.from_db())
        })
    }

    /// Lists all provider records that are not marked as deleted, ordered by creation date.
    pub fn list_all() -> DbResult<Vec<Provider>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_providers = provider::table
                .filter(provider::dsl::deleted_at.is_null())
                .order(provider::dsl::created_at.desc())
                .select(ProviderDb::as_select()) // Select as Vec<ProviderDb>
                .load::<ProviderDb>(conn) // Expect Vec<ProviderDb>
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list providers: {}", e)))
                })?;

            // Convert Vec<ProviderDb> to Vec<Provider>
            Ok(db_providers
                .into_iter()
                .map(|db_p| db_p.from_db())
                .collect())
        })
    }

    /// Lists provider summary rows for lightweight dropdowns and maps.
    pub fn list_summary() -> DbResult<Vec<ProviderSummaryItem>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = provider::table
                .filter(provider::dsl::deleted_at.is_null())
                .order(provider::dsl::name.asc())
                .select((
                    provider::dsl::id,
                    provider::dsl::provider_key,
                    provider::dsl::name,
                    provider::dsl::is_enabled,
                ))
                .load::<(i64, String, String, bool)>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list provider summaries: {}",
                        e
                    )))
                })?;

            Ok(rows
                .into_iter()
                .map(|(id, provider_key, name, is_enabled)| ProviderSummaryItem {
                    id,
                    provider_key,
                    name,
                    is_enabled,
                })
                .collect())
        })
    }

    /// Lists all active (not deleted and enabled) provider records, ordered by creation date.
    pub fn list_all_active() -> DbResult<Vec<Provider>> {
        let conn = &mut get_connection()?;
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

            Ok(db_providers
                .into_iter()
                .map(|db_p| db_p.from_db())
                .collect())
        })
    }

    /// Retrieves a provider's details including API keys and direct request patches by its ID.
    pub fn get_detail_by_id(provider_id_val: i64) -> DbResult<ProviderDetail> {
        let provider = Provider::get_by_id(provider_id_val)?;
        let api_keys = ProviderApiKey::list_by_provider_id(provider_id_val)?;
        let request_patches = RequestPatchRule::list_by_provider_id(provider_id_val)?;

        Ok(ProviderDetail {
            provider,
            api_keys,
            request_patches,
        })
    }
}

impl ProviderApiKey {
    /// Inserts a new provider API key record.
    pub fn insert(new_key_data: &NewProviderApiKey) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_key = diesel::insert_into(provider_api_key::table)
                .values(NewProviderApiKeyDb::to_db(new_key_data))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert provider API key: {}",
                        e
                    )))
                })?;
            Ok(db_key.from_db())
        })
    }

    /// Updates an existing provider API key.
    pub fn update(key_id: i64, update_data: &UpdateProviderApiKeyData) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let db_key = diesel::update(provider_api_key::table.find(key_id))
                .set((
                    UpdateProviderApiKeyDataDb::to_db(update_data),
                    provider_api_key::dsl::updated_at.eq(current_time),
                ))
                .returning(ProviderApiKeyDb::as_returning())
                .get_result::<ProviderApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update provider API key {}: {}",
                        key_id, e
                    )))
                })?;
            Ok(db_key.from_db())
        })
    }

    /// Soft deletes a provider API key.
    pub fn delete(key_id: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            diesel::update(provider_api_key::table.find(key_id))
                .set((
                    provider_api_key::dsl::deleted_at.eq(current_time),
                    provider_api_key::dsl::is_enabled.eq(false),
                    provider_api_key::dsl::updated_at.eq(current_time),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete provider API key {}: {}",
                        key_id, e
                    )))
                })
        })
    }

    /// Soft deletes all provider API keys for a provider.
    pub fn soft_delete_by_provider_id(provider_id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            diesel::update(
                provider_api_key::table.filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id_value)
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                provider_api_key::dsl::deleted_at.eq(current_time),
                provider_api_key::dsl::is_enabled.eq(false),
                provider_api_key::dsl::updated_at.eq(current_time),
            ))
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to delete provider API keys for provider {}: {}",
                    provider_id_value, e
                )))
            })
        })
    }

    /// Retrieves a provider API key by its ID.
    pub fn get_by_id(key_id: i64) -> DbResult<ProviderApiKey> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_key = provider_api_key::table
                .filter(
                    provider_api_key::dsl::id
                        .eq(key_id)
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .select(ProviderApiKeyDb::as_select())
                .first::<ProviderApiKeyDb>(conn)
                .map_err(|e| {
                    if matches!(e, diesel::result::Error::NotFound) {
                        BaseError::ParamInvalid(Some(format!(
                            "Provider API key with id {} not found",
                            key_id
                        )))
                    } else {
                        BaseError::DatabaseFatal(Some(format!(
                            "Error fetching provider API key {}: {}",
                            key_id, e
                        )))
                    }
                })?;
            Ok(db_key.from_db())
        })
    }

    /// Lists all non-deleted API keys for a specific provider.
    pub fn list_by_provider_id(p_id: i64) -> DbResult<Vec<ProviderApiKey>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_keys = provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(p_id)
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .order(provider_api_key::dsl::created_at.desc())
                .select(ProviderApiKeyDb::as_select())
                .load::<ProviderApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list API keys for provider {}: {}",
                        p_id, e
                    )))
                })?;
            Ok(db_keys.into_iter().map(|db_k| db_k.from_db()).collect())
        })
    }

    /// Lists all provider API key records that are not marked as deleted.
    pub fn list_all() -> DbResult<Vec<ProviderApiKey>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let db_keys = provider_api_key::table
                .filter(provider_api_key::dsl::deleted_at.is_null())
                .order(provider_api_key::dsl::created_at.desc())
                .select(ProviderApiKeyDb::as_select())
                .load::<ProviderApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list all provider API keys: {}",
                        e
                    )))
                })?;

            Ok(db_keys.into_iter().map(|db_k| db_k.from_db()).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::_sqlite_schema::provider_api_key;
    use diesel::Connection;
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use serde_json::Value;
    use tempfile::tempdir;

    const SQLITE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");

    struct TestSqliteDb {
        _temp_dir: tempfile::TempDir,
        conn: diesel::SqliteConnection,
    }

    fn bootstrap_with_sqlite_connection(
        conn: &mut diesel::SqliteConnection,
        input: &BootstrapProviderInput,
    ) -> DbResult<BootstrapProviderResult> {
        use self::_sqlite_model::*;
        use crate::database::_sqlite_schema::*;
        use crate::database::model::_sqlite_model::{
            ModelDb as BootstrapModelDb, NewModelDb as BootstrapNewModelDb,
        };

        bootstrap_transaction!(conn, BootstrapNewModelDb, BootstrapModelDb, input)
    }

    fn sqlite_connection() -> TestSqliteDb {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("bootstrap.sqlite");
        std::fs::File::create(&db_path).expect("db file should be created");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut conn = diesel::SqliteConnection::establish(&db_url)
            .expect("sqlite connection should be established");
        conn.run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("migrations should run");
        TestSqliteDb {
            _temp_dir: temp_dir,
            conn,
        }
    }

    fn sample_input(real_model_name: Option<&str>) -> BootstrapProviderInput {
        BootstrapProviderInput {
            provider_id: 101,
            provider_key: "openai-api-example-com".to_string(),
            name: "OpenAI api.example.com".to_string(),
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            api_key: "sk-test".to_string(),
            api_key_description: Some("bootstrap key".to_string()),
            model_name: "gpt-4o-mini".to_string(),
            real_model_name: real_model_name.map(ToString::to_string),
        }
    }

    fn provider_count(conn: &mut diesel::SqliteConnection, provider_id: i64) -> i64 {
        use crate::database::_sqlite_schema::*;

        provider::table
            .filter(provider::dsl::id.eq(provider_id))
            .count()
            .get_result(conn)
            .expect("provider count should load")
    }

    fn provider_key_count(conn: &mut diesel::SqliteConnection, provider_id: i64) -> i64 {
        provider_api_key::table
            .filter(provider_api_key::dsl::provider_id.eq(provider_id))
            .count()
            .get_result(conn)
            .expect("provider key count should load")
    }

    fn model_count(conn: &mut diesel::SqliteConnection, provider_id: i64) -> i64 {
        use crate::database::_sqlite_schema::*;

        model::table
            .filter(model::dsl::provider_id.eq(provider_id))
            .count()
            .get_result(conn)
            .expect("model count should load")
    }

    fn provider_summaries(conn: &mut diesel::SqliteConnection) -> Vec<ProviderSummaryItem> {
        use crate::database::_sqlite_schema::*;

        let rows = provider::table
            .filter(provider::dsl::deleted_at.is_null())
            .order(provider::dsl::name.asc())
            .select((
                provider::dsl::id,
                provider::dsl::provider_key,
                provider::dsl::name,
                provider::dsl::is_enabled,
            ))
            .load::<(i64, String, String, bool)>(conn)
            .expect("provider summary rows should load");

        rows.into_iter()
            .map(|(id, provider_key, name, is_enabled)| ProviderSummaryItem {
                id,
                provider_key,
                name,
                is_enabled,
            })
            .collect()
    }

    #[test]
    fn bootstrap_provider_creates_provider_key_and_model_atomically() {
        let mut db = sqlite_connection();
        let result = bootstrap_with_sqlite_connection(&mut db.conn, &sample_input(Some("gpt-4o")))
            .expect("bootstrap should succeed");

        assert_eq!(result.provider.id, 101);
        assert_eq!(result.provider.provider_key, "openai-api-example-com");
        assert_eq!(result.provider.name, "OpenAI api.example.com");
        assert_eq!(result.created_key.provider_id, result.provider.id);
        assert_eq!(result.created_key.api_key, "sk-test");
        assert_eq!(result.created_model.provider_id, result.provider.id);
        assert_eq!(result.created_model.model_name, "gpt-4o-mini");

        assert_eq!(provider_count(&mut db.conn, result.provider.id), 1);
        assert_eq!(provider_key_count(&mut db.conn, result.provider.id), 1);
        assert_eq!(model_count(&mut db.conn, result.provider.id), 1);
    }

    #[test]
    fn bootstrap_provider_rolls_back_on_model_validation_failure() {
        let mut db = sqlite_connection();
        let result = bootstrap_with_sqlite_connection(&mut db.conn, &sample_input(Some("")))
            .expect_err("bootstrap should fail");

        let message = match result {
            BaseError::DatabaseFatal(msg) => msg.unwrap_or_default(),
            other => format!("{other:?}"),
        };

        assert!(message.contains("Failed to insert bootstrap model"));

        assert_eq!(provider_count(&mut db.conn, 101), 0);
        assert_eq!(provider_key_count(&mut db.conn, 101), 0);
        assert_eq!(model_count(&mut db.conn, 101), 0);
    }

    #[test]
    fn provider_summary_list_returns_lightweight_rows() {
        let mut db = sqlite_connection();
        bootstrap_with_sqlite_connection(&mut db.conn, &sample_input(Some("gpt-4o")))
            .expect("bootstrap should succeed");

        let rows = provider_summaries(&mut db.conn);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.id, 101);
        assert_eq!(row.provider_key, "openai-api-example-com");
        assert_eq!(row.name, "OpenAI api.example.com");
        assert!(row.is_enabled);
    }

    #[test]
    fn provider_detail_contract_uses_request_patch_fields() {
        use crate::database::request_patch::RequestPatchScopeKind;
        use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};

        let detail = ProviderDetail {
            provider: Provider {
                id: 1,
                provider_key: "openai".to_string(),
                name: "OpenAI".to_string(),
                endpoint: "https://api.example.com/v1".to_string(),
                use_proxy: false,
                is_enabled: true,
                deleted_at: None,
                created_at: 1,
                updated_at: 1,
                provider_type: ProviderType::Openai,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            },
            api_keys: vec![],
            request_patches: vec![RequestPatchRuleResponse {
                id: 10,
                provider_id: Some(1),
                model_id: None,
                scope: RequestPatchScopeKind::Provider,
                placement: RequestPatchPlacement::Body,
                target: "/generationConfig".to_string(),
                operation: RequestPatchOperation::Set,
                value_json: Some(serde_json::json!({ "temperature": 0.2 })),
                description: Some("provider default".to_string()),
                is_enabled: true,
                created_at: 1,
                updated_at: 1,
            }],
        };

        let value = serde_json::to_value(detail).expect("provider detail should serialize");
        let object = value
            .as_object()
            .expect("detail should serialize as object");
        assert!(matches!(object.get("api_keys"), Some(Value::Array(_))));
        assert!(matches!(
            object.get("request_patches"),
            Some(Value::Array(_))
        ));
        assert_eq!(
            object["request_patches"][0]["value_json"],
            serde_json::json!({ "temperature": 0.2 })
        );
        assert!(object.get("custom_fields").is_none());
    }
}
