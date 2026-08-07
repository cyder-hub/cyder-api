use chrono::Utc;
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use serde::Deserialize; // Serialize on Provider is via db_object!, Deserialize for helper structs
use serde::Serialize;

use crate::database::model::{Model, NewModel};
use crate::database::upstream_source::{NewUpstreamSource, UpstreamSource};
use crate::database::{DbConnection, DbResult, get_connection};
use crate::{db_execute, db_object};
// db_object! is exported at the crate root by `#[macro_export]` in `database/mod.rs`.
// BaseError is assumed to be accessible, e.g., from `crate::controller::BaseError`.
use crate::controller::BaseError;
use crate::database::request_patch::{RequestPatchRule, RequestPatchRuleResponse};
use crate::schema::enum_def::{ProviderApiKeyMode, UpstreamProfileType};
use crate::service::secret_encryption::{
    EncryptedSecret, ProviderSecretFingerprint, SecretEncryptionError,
};
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
        pub is_enabled: bool,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
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
        pub is_enabled: bool,
        pub created_at: i64,
        pub updated_at: i64,
        pub provider_api_key_mode: ProviderApiKeyMode,
    }

// Data structure for updating an existing provider.
#[derive(AsChangeset, Deserialize, Debug, Clone)]
#[diesel(table_name = provider)]
    pub struct UpdateProviderData {
        pub provider_key: Option<String>,
        pub name: Option<String>,
        pub is_enabled: Option<bool>,
        pub provider_api_key_mode: Option<ProviderApiKeyMode>,
    }

}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderAggregate {
    #[serde(flatten)]
    pub provider: Provider,
    pub upstream_sources: Vec<UpstreamSource>,
}

impl std::ops::Deref for ProviderAggregate {
    type Target = Provider;

    fn deref(&self) -> &Self::Target {
        &self.provider
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProviderApiKeySummary {
    pub id: i64,
    pub provider_id: i64,
    pub description: Option<String>,
    pub key_prefix: String,
    pub key_last4: String,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone)]
pub(crate) struct StoredProviderApiKey {
    pub id: i64,
    pub provider_id: i64,
    pub description: Option<String>,
    pub key_prefix: String,
    pub key_last4: String,
    pub secret_ciphertext: Option<Vec<u8>>,
    pub secret_nonce: Option<Vec<u8>>,
    pub secret_format_version: Option<i32>,
    pub secret_key_fingerprint: Option<String>,
    #[cfg(test)]
    pub secret_hmac: Option<String>,
    pub deleted_at: Option<i64>,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Clone)]
pub(crate) struct ProviderApiKeySelection {
    pub id: i64,
    pub provider_id: i64,
    pub secret_ciphertext: Vec<u8>,
    pub secret_nonce: Vec<u8>,
    pub secret_format_version: i32,
    pub secret_key_fingerprint: String,
}

impl std::fmt::Debug for ProviderApiKeySelection {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ProviderApiKeySelection")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("secret_material", &"<redacted>")
            .finish()
    }
}

impl std::fmt::Debug for StoredProviderApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredProviderApiKey")
            .field("id", &self.id)
            .field("provider_id", &self.provider_id)
            .field("description", &self.description)
            .field("key_prefix", &self.key_prefix)
            .field("key_last4", &self.key_last4)
            .field("secret_material", &"<redacted>")
            .field("deleted_at", &self.deleted_at)
            .field("is_enabled", &self.is_enabled)
            .field("created_at", &self.created_at)
            .field("updated_at", &self.updated_at)
            .finish()
    }
}

impl StoredProviderApiKey {
    pub(crate) fn encrypted_secret(&self) -> Result<EncryptedSecret, SecretEncryptionError> {
        EncryptedSecret::from_parts(
            self.secret_ciphertext
                .clone()
                .ok_or(SecretEncryptionError::IncompleteStoredSecret)?,
            self.secret_nonce
                .clone()
                .ok_or(SecretEncryptionError::IncompleteStoredSecret)?,
            self.secret_format_version
                .ok_or(SecretEncryptionError::IncompleteStoredSecret)?,
            self.secret_key_fingerprint
                .clone()
                .ok_or(SecretEncryptionError::IncompleteStoredSecret)?,
        )
    }
}

pub(crate) struct NewProviderApiKey {
    pub id: i64,
    pub provider_id: i64,
    pub description: Option<String>,
    pub key_prefix: String,
    pub key_last4: String,
    pub encrypted_secret: EncryptedSecret,
    pub secret_hmac: ProviderSecretFingerprint,
    pub is_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

pub(crate) struct UpdateProviderApiKeyMetadata {
    pub description: Option<String>,
    pub is_enabled: bool,
}

type StoredProviderApiKeyTuple = (
    i64,
    i64,
    Option<String>,
    String,
    String,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<String>,
    Option<i64>,
    bool,
    i64,
    i64,
);

type ProviderApiKeySummaryTuple = (i64, i64, Option<String>, String, String, bool, i64, i64);

fn provider_api_key_summary_from_tuple(row: ProviderApiKeySummaryTuple) -> ProviderApiKeySummary {
    ProviderApiKeySummary {
        id: row.0,
        provider_id: row.1,
        description: row.2,
        key_prefix: row.3,
        key_last4: row.4,
        is_enabled: row.5,
        created_at: row.6,
        updated_at: row.7,
    }
}

fn stored_provider_api_key_from_tuple(row: StoredProviderApiKeyTuple) -> StoredProviderApiKey {
    StoredProviderApiKey {
        id: row.0,
        provider_id: row.1,
        description: row.2,
        key_prefix: row.3,
        key_last4: row.4,
        secret_ciphertext: row.5,
        secret_nonce: row.6,
        secret_format_version: row.7,
        secret_key_fingerprint: row.8,
        #[cfg(test)]
        secret_hmac: row.9,
        deleted_at: row.10,
        is_enabled: row.11,
        created_at: row.12,
        updated_at: row.13,
    }
}

fn map_provider_api_key_write_error(
    action: &'static str,
    error: diesel::result::Error,
) -> BaseError {
    match error {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(
            "provider API key already exists for this provider".to_string(),
        )),
        other => BaseError::DatabaseFatal(Some(format!(
            "Failed to {action} provider API key: {other}"
        ))),
    }
}

macro_rules! insert_provider_api_key {
    ($conn:expr, $new_key:expr) => {{
        let new_key = $new_key;
        let row = diesel::insert_into(provider_api_key::table)
            .values((
                provider_api_key::dsl::id.eq(new_key.id),
                provider_api_key::dsl::provider_id.eq(new_key.provider_id),
                provider_api_key::dsl::description.eq(new_key.description.clone()),
                provider_api_key::dsl::key_prefix.eq(new_key.key_prefix.clone()),
                provider_api_key::dsl::key_last4.eq(new_key.key_last4.clone()),
                provider_api_key::dsl::secret_ciphertext
                    .eq(Some(new_key.encrypted_secret.ciphertext().to_vec())),
                provider_api_key::dsl::secret_nonce
                    .eq(Some(new_key.encrypted_secret.nonce().to_vec())),
                provider_api_key::dsl::secret_format_version
                    .eq(Some(new_key.encrypted_secret.format_version())),
                provider_api_key::dsl::secret_key_fingerprint.eq(Some(
                    new_key
                        .encrypted_secret
                        .key_fingerprint()
                        .as_str()
                        .to_string(),
                )),
                provider_api_key::dsl::secret_hmac
                    .eq(Some(new_key.secret_hmac.as_str().to_string())),
                provider_api_key::dsl::is_enabled.eq(new_key.is_enabled),
                provider_api_key::dsl::created_at.eq(new_key.created_at),
                provider_api_key::dsl::updated_at.eq(new_key.updated_at),
            ))
            .returning((
                provider_api_key::dsl::id,
                provider_api_key::dsl::provider_id,
                provider_api_key::dsl::description,
                provider_api_key::dsl::key_prefix,
                provider_api_key::dsl::key_last4,
                provider_api_key::dsl::is_enabled,
                provider_api_key::dsl::created_at,
                provider_api_key::dsl::updated_at,
            ))
            .get_result::<ProviderApiKeySummaryTuple>($conn)
            .map_err(|error| map_provider_api_key_write_error("insert", error))?;
        provider_api_key_summary_from_tuple(row)
    }};
}

macro_rules! stored_provider_key_columns {
    () => {
        (
            provider_api_key::dsl::id,
            provider_api_key::dsl::provider_id,
            provider_api_key::dsl::description,
            provider_api_key::dsl::key_prefix,
            provider_api_key::dsl::key_last4,
            provider_api_key::dsl::secret_ciphertext,
            provider_api_key::dsl::secret_nonce,
            provider_api_key::dsl::secret_format_version,
            provider_api_key::dsl::secret_key_fingerprint,
            provider_api_key::dsl::secret_hmac,
            provider_api_key::dsl::deleted_at,
            provider_api_key::dsl::is_enabled,
            provider_api_key::dsl::created_at,
            provider_api_key::dsl::updated_at,
        )
    };
}

#[derive(Clone, Debug)]
pub struct BootstrapProviderInput {
    pub provider_id: i64,
    pub provider_key: String,
    pub name: String,
    pub source_id: i64,
    pub endpoint: String,
    pub use_proxy: bool,
    pub profile_type: UpstreamProfileType,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub provider_api_key_id: i64,
    pub api_key_description: Option<String>,
    pub key_prefix: String,
    pub key_last4: String,
    pub encrypted_secret: EncryptedSecret,
    pub secret_hmac: ProviderSecretFingerprint,
    pub model_name: String,
    pub real_model_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BootstrapProviderResult {
    pub provider: ProviderAggregate,
    pub created_key: ProviderApiKeySummary,
    pub created_model: Model,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummaryItem {
    pub id: i64,
    pub provider_key: String,
    pub name: String,
    pub is_enabled: bool,
    pub source_count: i64,
    pub enabled_source_count: i64,
    pub default_source_id: Option<i64>,
    pub default_source_profile_type: Option<UpstreamProfileType>,
}

macro_rules! bootstrap_transaction {
    ($conn:expr, $source_new_db:ident, $source_db:ident, $model_new_db:ident, $model_db:ident, $input:expr) => {{
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
                is_enabled: true,
                created_at: current_time,
                updated_at: current_time,
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

            let new_source_data = NewUpstreamSource {
                id: bootstrap_input.source_id,
                provider_id: provider.id,
                profile_type: bootstrap_input.profile_type,
                endpoint: bootstrap_input.endpoint.clone(),
                use_proxy: bootstrap_input.use_proxy,
                is_enabled: true,
                is_default: true,
                created_at: current_time,
                updated_at: current_time,
            };
            let source_db = diesel::insert_into(upstream_source::table)
                .values($source_new_db::to_db(&new_source_data))
                .returning($source_db::as_returning())
                .get_result::<$source_db>($conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert bootstrap upstream source: {}",
                        e
                    )))
                })?;
            let upstream_source = source_db.from_db();

            let new_provider_api_key_data = NewProviderApiKey {
                id: bootstrap_input.provider_api_key_id,
                provider_id: provider.id,
                description: bootstrap_input.api_key_description.clone(),
                key_prefix: bootstrap_input.key_prefix.clone(),
                key_last4: bootstrap_input.key_last4.clone(),
                encrypted_secret: bootstrap_input.encrypted_secret.clone(),
                secret_hmac: bootstrap_input.secret_hmac.clone(),
                is_enabled: true,
                created_at: current_time,
                updated_at: current_time,
            };

            let created_key = insert_provider_api_key!($conn, &new_provider_api_key_data);

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
                provider: ProviderAggregate {
                    provider,
                    upstream_sources: vec![upstream_source],
                },
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
    pub provider: ProviderAggregate,
    pub api_keys: Vec<ProviderApiKeySummary>,
    pub request_patches: Vec<RequestPatchRuleResponse>,
}

macro_rules! create_provider_transaction {
    ($conn:expr, $source_new_db:ident, $source_db:ident, $new_provider:expr, $new_source:expr) => {{
        $conn.transaction::<ProviderAggregate, BaseError, _>(|conn| {
            let new_source_data = $new_source;
            let provider = diesel::insert_into(provider::table)
                .values(NewProviderDb::to_db($new_provider))
                .returning(ProviderDb::as_returning())
                .get_result::<ProviderDb>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert provider aggregate: {error}"
                    )))
                })?
                .from_db();

            let upstream_sources = match new_source_data {
                Some(new_source) => {
                    if new_source.provider_id != provider.id {
                        return Err(BaseError::ParamInvalid(Some(
                            "upstream source must belong to its provider".to_string(),
                        )));
                    }
                    let upstream_source = diesel::insert_into(upstream_source::table)
                        .values($source_new_db::to_db(new_source))
                        .returning($source_db::as_returning())
                        .get_result::<$source_db>(conn)
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to insert provider upstream source: {error}"
                            )))
                        })?
                        .from_db();
                    vec![upstream_source]
                }
                None => Vec::new(),
            };

            Ok(ProviderAggregate {
                provider,
                upstream_sources,
            })
        })
    }};
}

macro_rules! update_provider_transaction {
    ($conn:expr, $source_db:ident, $id:expr, $provider_update:expr, $now:expr) => {{
        $conn.transaction::<ProviderAggregate, BaseError, _>(|conn| {
            let provider = diesel::update(
                provider::table.filter(
                    provider::dsl::id
                        .eq($id)
                        .and(provider::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                UpdateProviderDataDb::to_db($provider_update),
                provider::dsl::updated_at.eq($now),
            ))
            .returning(ProviderDb::as_returning())
            .get_result::<ProviderDb>(conn)
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to update provider {}: {error}",
                    $id
                )))
            })?
            .from_db();

            let source_rows = upstream_source::table
                .filter(
                    upstream_source::dsl::provider_id
                        .eq($id)
                        .and(upstream_source::dsl::deleted_at.is_null()),
                )
                .order(upstream_source::dsl::id.asc())
                .select($source_db::as_select())
                .load::<$source_db>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to load upstream source for provider {}: {error}",
                        $id
                    )))
                })?;
            let upstream_sources = source_rows
                .into_iter()
                .map(|row| row.from_db())
                .collect::<Vec<_>>();

            Ok(ProviderAggregate {
                provider,
                upstream_sources,
            })
        })
    }};
}

impl Provider {
    /// Atomically inserts a logical provider with an initial Source.
    pub fn create(
        new_provider_data: &NewProvider,
        new_source_data: &NewUpstreamSource,
    ) -> DbResult<ProviderAggregate> {
        Self::create_optional(new_provider_data, Some(new_source_data))
    }

    /// Atomically inserts a logical provider and an optional initial Source.
    pub fn create_optional(
        new_provider_data: &NewProvider,
        new_source_data: Option<&NewUpstreamSource>,
    ) -> DbResult<ProviderAggregate> {
        let conn = &mut get_connection()?;
        match conn {
            DbConnection::Postgres(conn) => {
                use self::_postgres_model::*;
                use crate::database::_postgres_schema::*;
                use crate::database::upstream_source::_postgres_model::{
                    NewUpstreamSourceDb as AggregateNewSourceDb,
                    UpstreamSourceDb as AggregateSourceDb,
                };
                create_provider_transaction!(
                    conn,
                    AggregateNewSourceDb,
                    AggregateSourceDb,
                    new_provider_data,
                    new_source_data
                )
            }
            DbConnection::Sqlite(conn) => {
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                use crate::database::upstream_source::_sqlite_model::{
                    NewUpstreamSourceDb as AggregateNewSourceDb,
                    UpstreamSourceDb as AggregateSourceDb,
                };
                create_provider_transaction!(
                    conn,
                    AggregateNewSourceDb,
                    AggregateSourceDb,
                    new_provider_data,
                    new_source_data
                )
            }
        }
    }

    /// Atomically updates only logical Provider fields. Source mutations use
    /// `UpstreamSource` repository operations so Profile remains immutable.
    pub fn update(id_value: i64, update_data: &UpdateProviderData) -> DbResult<ProviderAggregate> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        let mut update_data = update_data.clone();
        update_data.provider_key = None;
        match conn {
            DbConnection::Postgres(conn) => {
                use self::_postgres_model::*;
                use crate::database::_postgres_schema::*;
                use crate::database::upstream_source::_postgres_model::UpstreamSourceDb as AggregateSourceDb;
                update_provider_transaction!(
                    conn,
                    AggregateSourceDb,
                    id_value,
                    &update_data,
                    current_time
                )
            }
            DbConnection::Sqlite(conn) => {
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                use crate::database::upstream_source::_sqlite_model::UpstreamSourceDb as AggregateSourceDb;
                update_provider_transaction!(
                    conn,
                    AggregateSourceDb,
                    id_value,
                    &update_data,
                    current_time
                )
            }
        }
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
                use crate::database::upstream_source::_postgres_model::{
                    NewUpstreamSourceDb as BootstrapNewSourceDb,
                    UpstreamSourceDb as BootstrapSourceDb,
                };
                bootstrap_transaction!(
                    conn,
                    BootstrapNewSourceDb,
                    BootstrapSourceDb,
                    BootstrapNewModelDb,
                    BootstrapModelDb,
                    input
                )
            }
            DbConnection::Sqlite(conn) => {
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                use crate::database::model::_sqlite_model::{
                    ModelDb as BootstrapModelDb, NewModelDb as BootstrapNewModelDb,
                };
                use crate::database::upstream_source::_sqlite_model::{
                    NewUpstreamSourceDb as BootstrapNewSourceDb,
                    UpstreamSourceDb as BootstrapSourceDb,
                };
                bootstrap_transaction!(
                    conn,
                    BootstrapNewSourceDb,
                    BootstrapSourceDb,
                    BootstrapNewModelDb,
                    BootstrapModelDb,
                    input
                )
            }
        }
    }

    /// Soft deletes a provider record by setting `deleted_at` to the current time and `is_enabled` to false.
    pub fn delete(target_id_value: i64) -> DbResult<usize> {
        Self::delete_with_dependents(target_id_value)
    }

    /// Soft deletes a provider and all delete-owned dependent rows in one transaction.
    pub fn delete_with_dependents(target_id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            conn.transaction::<usize, BaseError, _>(|conn| {
                let updated = diesel::update(provider::table.find(target_id_value))
                    .set((
                        provider::dsl::deleted_at.eq(current_time),
                        provider::dsl::is_enabled.eq(false),
                        provider::dsl::updated_at.eq(current_time),
                    ))
                    .execute(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to delete provider {}: {}",
                            target_id_value, e
                        )))
                    })?;

                diesel::update(
                    upstream_source::table.filter(
                        upstream_source::dsl::provider_id
                            .eq(target_id_value)
                            .and(upstream_source::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    upstream_source::dsl::deleted_at.eq(current_time),
                    upstream_source::dsl::is_enabled.eq(false),
                    upstream_source::dsl::is_default.eq(false),
                    upstream_source::dsl::updated_at.eq(current_time),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete upstream source for provider {}: {}",
                        target_id_value, e
                    )))
                })?;
                diesel::update(
                    provider_api_key::table.filter(
                        provider_api_key::dsl::provider_id
                            .eq(target_id_value)
                            .and(provider_api_key::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    provider_api_key::dsl::deleted_at.eq(current_time),
                    provider_api_key::dsl::is_enabled.eq(false),
                    provider_api_key::dsl::secret_ciphertext.eq(Option::<Vec<u8>>::None),
                    provider_api_key::dsl::secret_nonce.eq(Option::<Vec<u8>>::None),
                    provider_api_key::dsl::secret_format_version.eq(Option::<i32>::None),
                    provider_api_key::dsl::secret_key_fingerprint.eq(Option::<String>::None),
                    provider_api_key::dsl::secret_hmac.eq(Option::<String>::None),
                    provider_api_key::dsl::updated_at.eq(current_time),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete provider API keys for provider {}: {}",
                        target_id_value, e
                    )))
                })?;

                diesel::update(
                    request_patch_rule::table.filter(
                        request_patch_rule::dsl::provider_id
                            .eq(target_id_value)
                            .and(request_patch_rule::dsl::model_id.is_null())
                            .and(request_patch_rule::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    request_patch_rule::dsl::deleted_at.eq(current_time),
                    request_patch_rule::dsl::is_enabled.eq(false),
                    request_patch_rule::dsl::updated_at.eq(current_time),
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete provider request patch rules for {}: {}",
                        target_id_value, e
                    )))
                })?;

                Ok(updated)
            })
        })
    }

    /// Retrieves a provider by its key, if it's not marked as deleted.
    pub fn get_by_key(provider_key_val: &str) -> DbResult<Option<ProviderAggregate>> {
        let provider = {
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

                Ok::<Option<Provider>, BaseError>(db_provider_opt.map(|db_p| db_p.from_db()))
            })?
        };
        provider.map(attach_sources).transpose()
    }

    /// Retrieves a provider by its ID, if it's not marked as deleted.
    pub fn get_by_id(target_id_value: i64) -> DbResult<ProviderAggregate> {
        let provider = {
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
                Ok::<Provider, BaseError>(db_provider.from_db())
            })?
        };
        attach_sources(provider)
    }

    /// Lists all provider records that are not marked as deleted, ordered by creation date.
    pub fn list_all() -> DbResult<Vec<ProviderAggregate>> {
        let providers = {
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

                Ok::<Vec<Provider>, BaseError>(
                    db_providers
                        .into_iter()
                        .map(|db_p| db_p.from_db())
                        .collect::<Vec<_>>(),
                )
            })?
        };
        attach_sources_batch(providers)
    }

    /// Lists provider summary rows for lightweight dropdowns and maps.
    pub fn list_summary() -> DbResult<Vec<ProviderSummaryItem>> {
        let mut providers = Self::list_all()?;
        providers.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(providers
            .into_iter()
            .map(|aggregate| ProviderSummaryItem {
                id: aggregate.id,
                provider_key: aggregate.provider_key.clone(),
                name: aggregate.name.clone(),
                is_enabled: aggregate.is_enabled,
                source_count: aggregate.upstream_sources.len() as i64,
                enabled_source_count: aggregate
                    .upstream_sources
                    .iter()
                    .filter(|source| source.is_enabled)
                    .count() as i64,
                default_source_id: aggregate
                    .upstream_sources
                    .iter()
                    .find(|source| source.is_default)
                    .map(|source| source.id),
                default_source_profile_type: aggregate
                    .upstream_sources
                    .iter()
                    .find(|source| source.is_default)
                    .map(|source| source.profile_type),
            })
            .collect())
    }

    /// Lists all active (not deleted and enabled) provider records, ordered by creation date.
    pub fn list_all_active() -> DbResult<Vec<ProviderAggregate>> {
        let providers = {
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

                Ok::<Vec<Provider>, BaseError>(
                    db_providers
                        .into_iter()
                        .map(|db_p| db_p.from_db())
                        .collect::<Vec<_>>(),
                )
            })?
        };
        attach_sources_batch(providers)
    }

    /// Retrieves a provider's details including API keys and direct request patches by its ID.
    pub fn get_detail_by_id(provider_id_val: i64) -> DbResult<ProviderDetail> {
        let provider = Provider::get_by_id(provider_id_val)?;
        let api_keys = ProviderApiKeyRepository::list_summaries_by_provider_id(provider_id_val)?;
        let request_patches = RequestPatchRule::list_by_provider_id(provider_id_val)?;

        Ok(ProviderDetail {
            provider,
            api_keys,
            request_patches,
        })
    }
}

fn attach_sources(provider: Provider) -> DbResult<ProviderAggregate> {
    let upstream_sources = UpstreamSource::list_active_by_provider_id(provider.id)?;
    Ok(ProviderAggregate {
        provider,
        upstream_sources,
    })
}

fn attach_sources_batch(providers: Vec<Provider>) -> DbResult<Vec<ProviderAggregate>> {
    let provider_ids = providers
        .iter()
        .map(|provider| provider.id)
        .collect::<Vec<_>>();
    let mut sources = UpstreamSource::list_active_for_provider_ids(&provider_ids)?;
    providers
        .into_iter()
        .map(|provider| {
            let upstream_sources = sources.remove(&provider.id).unwrap_or_default();
            Ok(ProviderAggregate {
                provider,
                upstream_sources,
            })
        })
        .collect()
}

pub struct ProviderApiKeyRepository;

impl ProviderApiKeyRepository {
    pub(crate) fn insert(new_key: &NewProviderApiKey) -> DbResult<ProviderApiKeySummary> {
        let conn = &mut get_connection()?;
        db_execute!(conn, { Ok(insert_provider_api_key!(conn, new_key)) })
    }

    pub(crate) fn replace_secret(
        provider_id_value: i64,
        key_id: i64,
        key_prefix_value: String,
        key_last4_value: String,
        encrypted_secret: &EncryptedSecret,
        secret_hmac_value: &ProviderSecretFingerprint,
    ) -> DbResult<ProviderApiKeySummary> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let row = diesel::update(
                provider_api_key::table.filter(
                    provider_api_key::dsl::id
                        .eq(key_id)
                        .and(provider_api_key::dsl::provider_id.eq(provider_id_value))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                provider_api_key::dsl::key_prefix.eq(key_prefix_value),
                provider_api_key::dsl::key_last4.eq(key_last4_value),
                provider_api_key::dsl::secret_ciphertext
                    .eq(Some(encrypted_secret.ciphertext().to_vec())),
                provider_api_key::dsl::secret_nonce.eq(Some(encrypted_secret.nonce().to_vec())),
                provider_api_key::dsl::secret_format_version
                    .eq(Some(encrypted_secret.format_version())),
                provider_api_key::dsl::secret_key_fingerprint.eq(Some(
                    encrypted_secret.key_fingerprint().as_str().to_string(),
                )),
                provider_api_key::dsl::secret_hmac.eq(Some(secret_hmac_value.as_str().to_string())),
                provider_api_key::dsl::updated_at.eq(current_time),
            ))
            .returning((
                provider_api_key::dsl::id,
                provider_api_key::dsl::provider_id,
                provider_api_key::dsl::description,
                provider_api_key::dsl::key_prefix,
                provider_api_key::dsl::key_last4,
                provider_api_key::dsl::is_enabled,
                provider_api_key::dsl::created_at,
                provider_api_key::dsl::updated_at,
            ))
            .get_result::<ProviderApiKeySummaryTuple>(conn)
            .map_err(|error| map_provider_api_key_write_error("replace", error))?;
            Ok(provider_api_key_summary_from_tuple(row))
        })
    }

    pub(crate) fn update_metadata(
        provider_id_value: i64,
        key_id: i64,
        update: &UpdateProviderApiKeyMetadata,
    ) -> DbResult<ProviderApiKeySummary> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let row = diesel::update(
                provider_api_key::table.filter(
                    provider_api_key::dsl::id
                        .eq(key_id)
                        .and(provider_api_key::dsl::provider_id.eq(provider_id_value))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                provider_api_key::dsl::description.eq(update.description.clone()),
                provider_api_key::dsl::is_enabled.eq(update.is_enabled),
                provider_api_key::dsl::updated_at.eq(current_time),
            ))
            .returning((
                provider_api_key::dsl::id,
                provider_api_key::dsl::provider_id,
                provider_api_key::dsl::description,
                provider_api_key::dsl::key_prefix,
                provider_api_key::dsl::key_last4,
                provider_api_key::dsl::is_enabled,
                provider_api_key::dsl::created_at,
                provider_api_key::dsl::updated_at,
            ))
            .get_result::<ProviderApiKeySummaryTuple>(conn)
            .map_err(|error| match error {
                diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                    "Provider API key {key_id} not found for provider {provider_id_value}"
                ))),
                other => map_provider_api_key_write_error("update metadata for", other),
            })?;
            Ok(provider_api_key_summary_from_tuple(row))
        })
    }

    pub(crate) fn soft_delete(provider_id_value: i64, key_id: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let current_time = Utc::now().timestamp_millis();
        db_execute!(conn, {
            diesel::update(
                provider_api_key::table.filter(
                    provider_api_key::dsl::id
                        .eq(key_id)
                        .and(provider_api_key::dsl::provider_id.eq(provider_id_value))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                provider_api_key::dsl::deleted_at.eq(Some(current_time)),
                provider_api_key::dsl::is_enabled.eq(false),
                provider_api_key::dsl::secret_ciphertext.eq(Option::<Vec<u8>>::None),
                provider_api_key::dsl::secret_nonce.eq(Option::<Vec<u8>>::None),
                provider_api_key::dsl::secret_format_version.eq(Option::<i32>::None),
                provider_api_key::dsl::secret_key_fingerprint.eq(Option::<String>::None),
                provider_api_key::dsl::secret_hmac.eq(Option::<String>::None),
                provider_api_key::dsl::updated_at.eq(current_time),
            ))
            .execute(conn)
            .map_err(|error| map_provider_api_key_write_error("delete", error))
        })
    }

    pub fn get_summary_by_id(
        provider_id_value: i64,
        key_id: i64,
    ) -> DbResult<ProviderApiKeySummary> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = provider_api_key::table
                .filter(
                    provider_api_key::dsl::id
                        .eq(key_id)
                        .and(provider_api_key::dsl::provider_id.eq(provider_id_value))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::description,
                    provider_api_key::dsl::key_prefix,
                    provider_api_key::dsl::key_last4,
                    provider_api_key::dsl::is_enabled,
                    provider_api_key::dsl::created_at,
                    provider_api_key::dsl::updated_at,
                ))
                .first::<ProviderApiKeySummaryTuple>(conn)
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Provider API key {key_id} not found for provider {provider_id_value}"
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to load provider API key summary: {other}"
                    ))),
                })?;
            Ok(provider_api_key_summary_from_tuple(row))
        })
    }

    pub fn list_summaries_by_provider_id(
        provider_id_value: i64,
    ) -> DbResult<Vec<ProviderApiKeySummary>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id_value)
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .order(provider_api_key::dsl::created_at.desc())
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::description,
                    provider_api_key::dsl::key_prefix,
                    provider_api_key::dsl::key_last4,
                    provider_api_key::dsl::is_enabled,
                    provider_api_key::dsl::created_at,
                    provider_api_key::dsl::updated_at,
                ))
                .load::<ProviderApiKeySummaryTuple>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list provider API key summaries: {error}"
                    )))
                })?;
            Ok(rows
                .into_iter()
                .map(provider_api_key_summary_from_tuple)
                .collect())
        })
    }

    pub fn list_all_summaries() -> DbResult<Vec<ProviderApiKeySummary>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = provider_api_key::table
                .filter(provider_api_key::dsl::deleted_at.is_null())
                .order(provider_api_key::dsl::created_at.desc())
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::description,
                    provider_api_key::dsl::key_prefix,
                    provider_api_key::dsl::key_last4,
                    provider_api_key::dsl::is_enabled,
                    provider_api_key::dsl::created_at,
                    provider_api_key::dsl::updated_at,
                ))
                .load::<ProviderApiKeySummaryTuple>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list provider API key summaries: {error}"
                    )))
                })?;
            Ok(rows
                .into_iter()
                .map(provider_api_key_summary_from_tuple)
                .collect())
        })
    }

    pub(crate) fn list_selections_by_provider_id(
        provider_id_value: i64,
    ) -> DbResult<Vec<ProviderApiKeySelection>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = provider_api_key::table
                .filter(
                    provider_api_key::dsl::provider_id
                        .eq(provider_id_value)
                        .and(provider_api_key::dsl::deleted_at.is_null())
                        .and(provider_api_key::dsl::is_enabled.eq(true)),
                )
                .order((
                    provider_api_key::dsl::created_at.asc(),
                    provider_api_key::dsl::id.asc(),
                ))
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::secret_ciphertext,
                    provider_api_key::dsl::secret_nonce,
                    provider_api_key::dsl::secret_format_version,
                    provider_api_key::dsl::secret_key_fingerprint,
                ))
                .load::<(
                    i64,
                    i64,
                    Option<Vec<u8>>,
                    Option<Vec<u8>>,
                    Option<i32>,
                    Option<String>,
                )>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list provider API key selections: {error}"
                    )))
                })?;

            rows.into_iter()
                .map(|row| {
                    Ok(ProviderApiKeySelection {
                        id: row.0,
                        provider_id: row.1,
                        secret_ciphertext: row.2.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                        secret_nonce: row.3.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                        secret_format_version: row.4.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                        secret_key_fingerprint: row.5.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                    })
                })
                .collect()
        })
    }

    pub(crate) fn list_all_selections() -> DbResult<Vec<ProviderApiKeySelection>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = provider_api_key::table
                .filter(
                    provider_api_key::dsl::deleted_at
                        .is_null()
                        .and(provider_api_key::dsl::is_enabled.eq(true)),
                )
                .order((
                    provider_api_key::dsl::provider_id.asc(),
                    provider_api_key::dsl::created_at.asc(),
                    provider_api_key::dsl::id.asc(),
                ))
                .select((
                    provider_api_key::dsl::id,
                    provider_api_key::dsl::provider_id,
                    provider_api_key::dsl::secret_ciphertext,
                    provider_api_key::dsl::secret_nonce,
                    provider_api_key::dsl::secret_format_version,
                    provider_api_key::dsl::secret_key_fingerprint,
                ))
                .load::<(
                    i64,
                    i64,
                    Option<Vec<u8>>,
                    Option<Vec<u8>>,
                    Option<i32>,
                    Option<String>,
                )>(conn)
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list provider API key selections: {error}"
                    )))
                })?;

            rows.into_iter()
                .map(|row| {
                    Ok(ProviderApiKeySelection {
                        id: row.0,
                        provider_id: row.1,
                        secret_ciphertext: row.2.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                        secret_nonce: row.3.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                        secret_format_version: row.4.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                        secret_key_fingerprint: row.5.ok_or_else(|| {
                            BaseError::DatabaseFatal(Some(
                                "Provider API key secret contract is incomplete".to_string(),
                            ))
                        })?,
                    })
                })
                .collect()
        })
    }

    pub(crate) fn get_stored_by_id(
        provider_id_value: i64,
        key_id: i64,
    ) -> DbResult<StoredProviderApiKey> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = provider_api_key::table
                .filter(
                    provider_api_key::dsl::id
                        .eq(key_id)
                        .and(provider_api_key::dsl::provider_id.eq(provider_id_value))
                        .and(provider_api_key::dsl::deleted_at.is_null()),
                )
                .select(stored_provider_key_columns!())
                .first::<StoredProviderApiKeyTuple>(conn)
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Provider API key {key_id} not found for provider {provider_id_value}"
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to load provider API key secret: {other}"
                    ))),
                })?;
            Ok(stored_provider_api_key_from_tuple(row))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SecretEncryptionConfig;
    use crate::database::_sqlite_schema::provider_api_key;
    use crate::database::upstream_source::UpdateUpstreamSourceData;
    use crate::service::secret_encryption::{
        SecretDomain, SecretEncryptionService, SensitiveSecret,
    };
    use serde_json::Value;

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
        use crate::database::upstream_source::_sqlite_model::{
            NewUpstreamSourceDb as BootstrapNewSourceDb, UpstreamSourceDb as BootstrapSourceDb,
        };

        bootstrap_transaction!(
            conn,
            BootstrapNewSourceDb,
            BootstrapSourceDb,
            BootstrapNewModelDb,
            BootstrapModelDb,
            input
        )
    }

    fn sqlite_connection() -> TestSqliteDb {
        let (temp_dir, conn) =
            crate::database::open_test_sqlite_connection_with_migrations("bootstrap.sqlite");
        TestSqliteDb {
            _temp_dir: temp_dir,
            conn,
        }
    }

    fn sample_input(real_model_name: Option<&str>) -> BootstrapProviderInput {
        let provider_id = 101;
        let provider_api_key_id = 102;
        let api_key = "sk-test";
        let config: SecretEncryptionConfig = serde_yaml::from_str(
            "downstream_mode: one_time\nencryption_key: '000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\n",
        )
        .expect("test secret config should parse");
        let service = SecretEncryptionService::from_config(&config);
        let secret = SensitiveSecret::new(api_key.to_string());
        BootstrapProviderInput {
            provider_id,
            provider_key: "openai-api-example-com".to_string(),
            name: "OpenAI api.example.com".to_string(),
            source_id: 103,
            endpoint: "https://api.example.com/v1".to_string(),
            use_proxy: false,
            profile_type: UpstreamProfileType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            provider_api_key_id,
            api_key_description: Some("bootstrap key".to_string()),
            key_prefix: "sk-t".to_string(),
            key_last4: "test".to_string(),
            encrypted_secret: service
                .encrypt_current(SecretDomain::ProviderApiKey(provider_api_key_id), &secret)
                .expect("test provider secret should encrypt"),
            secret_hmac: service
                .provider_secret_fingerprint(provider_id, &secret)
                .expect("test provider secret should fingerprint"),
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

    fn source_count(conn: &mut diesel::SqliteConnection, provider_id: i64) -> i64 {
        use crate::database::_sqlite_schema::*;

        upstream_source::table
            .filter(upstream_source::dsl::provider_id.eq(provider_id))
            .count()
            .get_result(conn)
            .expect("upstream source count should load")
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
            .map(|(id, provider_key, name, is_enabled)| {
                use crate::database::upstream_source::_sqlite_model::UpstreamSourceDb;

                let sources = upstream_source::table
                    .filter(
                        upstream_source::dsl::provider_id
                            .eq(id)
                            .and(upstream_source::dsl::deleted_at.is_null()),
                    )
                    .select(UpstreamSourceDb::as_select())
                    .load::<UpstreamSourceDb>(conn)
                    .expect("summary sources should load")
                    .into_iter()
                    .map(|source| source.from_db())
                    .collect::<Vec<_>>();
                let default_source = sources.iter().find(|source| source.is_default);

                ProviderSummaryItem {
                    id,
                    provider_key,
                    name,
                    is_enabled,
                    source_count: sources.len() as i64,
                    enabled_source_count: sources.iter().filter(|source| source.is_enabled).count()
                        as i64,
                    default_source_id: default_source.map(|source| source.id),
                    default_source_profile_type: default_source.map(|source| source.profile_type),
                }
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
        assert_eq!(result.provider.upstream_sources.len(), 1);
        assert_eq!(result.provider.upstream_sources[0].id, 103);
        assert!(result.provider.upstream_sources[0].is_enabled);
        assert!(result.provider.upstream_sources[0].is_default);
        assert_eq!(result.created_key.provider_id, result.provider.id);
        assert_eq!(result.created_key.key_prefix, "sk-t");
        assert_eq!(result.created_key.key_last4, "test");
        assert_eq!(result.created_model.provider_id, result.provider.id);
        assert_eq!(result.created_model.model_name, "gpt-4o-mini");

        assert_eq!(provider_count(&mut db.conn, result.provider.id), 1);
        assert_eq!(source_count(&mut db.conn, result.provider.id), 1);
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
        assert_eq!(source_count(&mut db.conn, 101), 0);
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
        assert_eq!(row.source_count, 1);
        assert_eq!(row.enabled_source_count, 1);
        assert_eq!(row.default_source_id, Some(103));
        assert_eq!(
            row.default_source_profile_type,
            Some(UpstreamProfileType::Openai)
        );
    }

    #[test]
    fn provider_detail_contract_uses_request_patch_fields() {
        use crate::database::request_patch::RequestPatchScopeKind;
        use crate::schema::enum_def::{RequestPatchOperation, RequestPatchPlacement};

        let detail = ProviderDetail {
            provider: ProviderAggregate {
                provider: Provider {
                    id: 1,
                    provider_key: "openai".to_string(),
                    name: "OpenAI".to_string(),
                    is_enabled: true,
                    deleted_at: None,
                    created_at: 1,
                    updated_at: 1,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                },
                upstream_sources: vec![UpstreamSource {
                    id: 2,
                    provider_id: 1,
                    profile_type: UpstreamProfileType::Openai,
                    endpoint: "https://api.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    deleted_at: None,
                    created_at: 1,
                    updated_at: 1,
                }],
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
        assert_eq!(
            object["provider"]["upstream_sources"][0]["profile_type"],
            "OPENAI"
        );
        assert!(object["provider"].get("endpoint").is_none());
        assert!(object["provider"].get("provider_type").is_none());
        assert!(object["provider"].get("use_proxy").is_none());
    }

    #[tokio::test]
    async fn provider_aggregate_update_and_delete_keep_source_state_atomic() {
        let database = crate::database::TestDbContext::new_sqlite("provider-aggregate-crud.sqlite");
        database
            .run_async(async {
                Provider::create(
                    &NewProvider {
                        id: 401,
                        provider_key: "aggregate-provider".to_string(),
                        name: "Aggregate Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    &NewUpstreamSource {
                        id: 402,
                        provider_id: 401,
                        profile_type: UpstreamProfileType::Openai,
                        endpoint: "https://old.example.com/v1".to_string(),
                        use_proxy: false,
                        is_enabled: true,
                        is_default: true,
                        created_at: 1,
                        updated_at: 1,
                    },
                )
                .expect("aggregate should create");

                let updated = Provider::update(
                    401,
                    &UpdateProviderData {
                        provider_key: Some("must-not-change".to_string()),
                        name: Some("Updated Provider".to_string()),
                        is_enabled: None,
                        provider_api_key_mode: None,
                    },
                )
                .expect("provider should update");
                assert_eq!(updated.name, "Updated Provider");
                assert_eq!(updated.provider_key, "aggregate-provider");

                let updated_source = UpstreamSource::update(
                    402,
                    401,
                    &UpdateUpstreamSourceData {
                        endpoint: Some("https://new.example.com/v1".to_string()),
                        use_proxy: Some(true),
                        is_enabled: None,
                        is_default: None,
                        updated_at: 2,
                    },
                )
                .expect("source should update");
                assert_eq!(updated_source.profile_type, UpstreamProfileType::Openai);
                assert_eq!(updated_source.endpoint, "https://new.example.com/v1");
                assert!(updated_source.use_proxy);

                assert_eq!(Provider::delete_with_dependents(401).expect("delete"), 1);
                assert!(Provider::get_by_id(401).is_err());

                let conn = &mut get_connection().expect("test connection");
                db_execute!(conn, {
                    let deleted_at = upstream_source::table
                        .find(402)
                        .select(upstream_source::dsl::deleted_at)
                        .first::<Option<i64>>(conn)
                        .expect("source should remain soft deleted");
                    assert!(deleted_at.is_some());
                });
            })
            .await;
    }

    #[tokio::test]
    async fn provider_aggregate_allows_zero_or_more_sources_and_preserves_family_identity() {
        let database =
            crate::database::TestDbContext::new_sqlite("provider-source-cardinality.sqlite");
        database
            .run_async(async {
                let provider = Provider::create_optional(
                    &NewProvider {
                        id: 411,
                        provider_key: "source-free-provider".to_string(),
                        name: "Source Free Provider".to_string(),
                        is_enabled: true,
                        created_at: 1,
                        updated_at: 1,
                        provider_api_key_mode: ProviderApiKeyMode::Queue,
                    },
                    None,
                )
                .expect("provider without a source should be valid");
                assert!(provider.upstream_sources.is_empty());

                let source = UpstreamSource::create(&NewUpstreamSource {
                    id: 412,
                    provider_id: 411,
                    profile_type: UpstreamProfileType::Openai,
                    endpoint: "https://first.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("first source should create");
                assert!(source.is_default);

                let second = UpstreamSource::create(&NewUpstreamSource {
                    id: 413,
                    provider_id: 411,
                    profile_type: UpstreamProfileType::Gemini,
                    endpoint: "https://second.example.com/v1".to_string(),
                    use_proxy: true,
                    is_enabled: true,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("different source family should coexist");
                assert!(!second.is_default);

                let loaded = Provider::get_by_id(411).expect("provider should load");
                assert_eq!(
                    loaded
                        .upstream_sources
                        .iter()
                        .map(|source| source.id)
                        .collect::<Vec<_>>(),
                    vec![412, 413]
                );
            })
            .await;
    }

    #[tokio::test]
    async fn provider_key_repository_enforces_safe_projection_membership_and_hmac_lifecycle() {
        let database = crate::database::TestDbContext::new_sqlite("provider-key-repository.sqlite");
        database
            .run_async(async {
                Provider::create(&NewProvider {
                    id: 501,
                    provider_key: "repository-provider".to_string(),
                    name: "Repository Provider".to_string(),
                    is_enabled: true,
                    created_at: 1,
                    updated_at: 1,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                }, &NewUpstreamSource {
                    id: 503,
                    provider_id: 501,
                    profile_type: UpstreamProfileType::Openai,
                    endpoint: "https://api.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("provider should seed");
                Provider::create(&NewProvider {
                    id: 502,
                    provider_key: "other-provider".to_string(),
                    name: "Other Provider".to_string(),
                    is_enabled: true,
                    created_at: 1,
                    updated_at: 1,
                    provider_api_key_mode: ProviderApiKeyMode::Queue,
                }, &NewUpstreamSource {
                    id: 504,
                    provider_id: 502,
                    profile_type: UpstreamProfileType::Openai,
                    endpoint: "https://other.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                })
                .expect("other provider should seed");

                let config: SecretEncryptionConfig = serde_yaml::from_str(
                    "downstream_mode: one_time\nencryption_key: '000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'\n",
                )
                .expect("test secret config should parse");
                let service = SecretEncryptionService::from_config(&config);
                let secret = SensitiveSecret::new("sk-repository-secret".to_string());
                let new_key = |id| NewProviderApiKey {
                    id,
                    provider_id: 501,
                    description: Some("primary".to_string()),
                    key_prefix: "sk-r".to_string(),
                    key_last4: "cret".to_string(),
                    encrypted_secret: service
                        .encrypt_current(SecretDomain::ProviderApiKey(id), &secret)
                        .expect("secret should encrypt"),
                    secret_hmac: service
                        .provider_secret_fingerprint(501, &secret)
                        .expect("secret should fingerprint"),
                    is_enabled: true,
                    created_at: id,
                    updated_at: id,
                };

                let summary = ProviderApiKeyRepository::insert(&new_key(601))
                    .expect("provider key should insert");
                let json = serde_json::to_value(&summary).expect("summary should serialize");
                let object = json.as_object().expect("summary should be an object");
                for forbidden in [
                    "api_key",
                    "secret_ciphertext",
                    "secret_nonce",
                    "secret_format_version",
                    "secret_key_fingerprint",
                    "secret_hmac",
                ] {
                    assert!(!object.contains_key(forbidden));
                }
                assert!(ProviderApiKeyRepository::insert(&new_key(602)).is_err());
                assert!(
                    ProviderApiKeyRepository::update_metadata(
                        502,
                        601,
                        &UpdateProviderApiKeyMetadata {
                            description: None,
                            is_enabled: false,
                        },
                    )
                    .is_err()
                );

                ProviderApiKeyRepository::update_metadata(
                    501,
                    601,
                    &UpdateProviderApiKeyMetadata {
                        description: Some("disabled".to_string()),
                        is_enabled: false,
                    },
                )
                .expect("metadata update should succeed");
                assert!(ProviderApiKeyRepository::insert(&new_key(602)).is_err());

                let stored = ProviderApiKeyRepository::get_stored_by_id(501, 601)
                    .expect("stored provider key should load");
                let debug = format!("{stored:?}");
                assert!(debug.contains("<redacted>"));
                assert!(!debug.contains("sk-repository-secret"));
                assert!(!debug.contains(stored.secret_hmac.as_deref().unwrap_or_default()));

                ProviderApiKeyRepository::soft_delete(501, 601)
                    .expect("provider key soft delete should succeed");
                assert!(ProviderApiKeyRepository::get_stored_by_id(501, 601).is_err());
                ProviderApiKeyRepository::insert(&new_key(602))
                    .expect("soft deletion should release the HMAC uniqueness slot");
            })
            .await;
    }
}
