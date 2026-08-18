use chrono::Utc;
use diesel::prelude::*;
use serde::Deserialize; // Serialize on Provider is via db_object!, Deserialize for helper structs
use serde::Serialize;

use crate::database::DbResult;
use crate::database::model::{Model, NewModel};
use crate::database::runtime::{DatabaseRuntime, DatabaseWorkload, db_execute as async_db_execute};
use crate::database::upstream_source::{NewUpstreamSource, UpstreamSource};
use crate::db_object;
// db_object! is exported at the crate root by `#[macro_export]` in `database/mod.rs`.
// BaseError is assumed to be accessible, e.g., from `crate::controller::BaseError`.
use crate::controller::BaseError;
use crate::database::request_patch::{RequestPatchVariantAggregate, RequestPatchVariantRepository};
use crate::schema::enum_def::{ModelKind, ProviderApiKeyMode, UpstreamProfileType};
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
#[derive(Insertable, Deserialize, Debug, Clone)]
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

#[derive(Clone)]
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

#[derive(Clone)]
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
type ProviderApiKeySelectionTuple = (
    i64,
    i64,
    Option<Vec<u8>>,
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
);

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
    pub source: NewUpstreamSource,
    pub provider_api_key_mode: ProviderApiKeyMode,
    pub provider_api_key_id: i64,
    pub api_key_description: Option<String>,
    pub key_prefix: String,
    pub key_last4: String,
    pub encrypted_secret: EncryptedSecret,
    pub secret_hmac: ProviderSecretFingerprint,
    pub model_name: String,
    pub real_model_name: Option<String>,
    pub model_kind: ModelKind,
}

#[derive(Debug, Serialize)]
pub struct BootstrapProviderResult {
    pub provider: ProviderAggregate,
    pub created_key: ProviderApiKeySummary,
    pub created_model: Model,
}

#[derive(Debug, Serialize)]
pub struct ProviderDetail {
    pub provider: ProviderAggregate,
    pub api_keys: Vec<ProviderApiKeySummary>,
    pub request_patch_variants: Vec<RequestPatchVariantAggregate>,
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

macro_rules! provider_async_transaction {
    ($connection:ident as $conn:ident, $block:block) => {{
        match $connection {
            crate::database::runtime::RuntimeConnection::Postgres($conn) => {
                #[allow(unused_imports)]
                use self::_postgres_model::*;
                use crate::database::_postgres_schema::*;
                #[allow(unused_imports)]
                use crate::database::model::_postgres_model::{
                    ModelDb as BootstrapModelDb, NewModelDb as BootstrapNewModelDb,
                };
                #[allow(unused_imports)]
                use crate::database::upstream_source::_postgres_model::{
                    NewUpstreamSourceDb as AggregateNewSourceDb,
                    UpstreamSourceDb as AggregateSourceDb,
                };
                use diesel::prelude::*;
                use diesel_async::AsyncConnection;
                $conn.transaction(async move |$conn| $block).await
            }
            crate::database::runtime::RuntimeConnection::Sqlite($conn) => {
                #[allow(unused_imports)]
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                #[allow(unused_imports)]
                use crate::database::model::_sqlite_model::{
                    ModelDb as BootstrapModelDb, NewModelDb as BootstrapNewModelDb,
                };
                #[allow(unused_imports)]
                use crate::database::upstream_source::_sqlite_model::{
                    NewUpstreamSourceDb as AggregateNewSourceDb,
                    UpstreamSourceDb as AggregateSourceDb,
                };
                use diesel::prelude::*;
                use diesel_async::AsyncConnection;
                $conn.transaction(async move |$conn| $block).await
            }
        }
    }};
}

impl Provider {
    pub async fn get_by_key(
        database: &DatabaseRuntime,
        provider_key_val: &str,
    ) -> DbResult<Option<ProviderAggregate>> {
        let provider_key_val = provider_key_val.to_string();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    let provider = async_db_execute!(connection as conn, {
                        let query = provider::table
                            .filter(
                                provider::dsl::provider_key
                                    .eq(&provider_key_val)
                                    .and(provider::dsl::deleted_at.is_null()),
                            )
                            .select(ProviderDb::as_select());
                        let result: Result<ProviderDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result
                            .optional()
                            .map(|row| row.map(ProviderDb::from_db))
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Error fetching provider by key '{}': {}",
                                    provider_key_val, error
                                )))
                            })
                    })?;
                    match provider {
                        Some(provider) => {
                            let upstream_sources =
                                UpstreamSource::list_active_by_provider_id_with_connection(
                                    connection,
                                    provider.id,
                                )
                                .await?;
                            Ok(Some(ProviderAggregate {
                                provider,
                                upstream_sources,
                            }))
                        }
                        None => Ok(None),
                    }
                })
            })
            .await
    }

    pub async fn get_by_id(
        database: &DatabaseRuntime,
        target_id_value: i64,
    ) -> DbResult<ProviderAggregate> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    let provider = async_db_execute!(connection as conn, {
                        let query = provider::table
                            .filter(
                                provider::dsl::id
                                    .eq(target_id_value)
                                    .and(provider::dsl::deleted_at.is_null()),
                            )
                            .select(ProviderDb::as_select());
                        let result: Result<ProviderDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result.map(ProviderDb::from_db).map_err(|error| {
                            if matches!(error, diesel::result::Error::NotFound) {
                                BaseError::ParamInvalid(Some(format!(
                                    "Provider with id {} not found",
                                    target_id_value
                                )))
                            } else {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Error fetching provider {}: {}",
                                    target_id_value, error
                                )))
                            }
                        })
                    })?;
                    let upstream_sources =
                        UpstreamSource::list_active_by_provider_id_with_connection(
                            connection,
                            provider.id,
                        )
                        .await?;
                    Ok(ProviderAggregate {
                        provider,
                        upstream_sources,
                    })
                })
            })
            .await
    }

    pub async fn list_all(database: &DatabaseRuntime) -> DbResult<Vec<ProviderAggregate>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    let providers = async_db_execute!(connection as conn, {
                        let query = provider::table
                            .filter(provider::dsl::deleted_at.is_null())
                            .order(provider::dsl::created_at.desc())
                            .select(ProviderDb::as_select());
                        let result: Result<Vec<ProviderDb>, diesel::result::Error> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn).await;
                        result
                            .map(|rows| {
                                rows.into_iter()
                                    .map(ProviderDb::from_db)
                                    .collect::<Vec<_>>()
                            })
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to list providers: {}",
                                    error
                                )))
                            })
                    })?;
                    let provider_ids = providers
                        .iter()
                        .map(|provider| provider.id)
                        .collect::<Vec<_>>();
                    let mut sources = UpstreamSource::list_active_for_provider_ids_with_connection(
                        connection,
                        provider_ids,
                    )
                    .await?;
                    Ok(providers
                        .into_iter()
                        .map(|provider| {
                            let upstream_sources = sources.remove(&provider.id).unwrap_or_default();
                            ProviderAggregate {
                                provider,
                                upstream_sources,
                            }
                        })
                        .collect())
                })
            })
            .await
    }

    pub async fn list_summary(database: &DatabaseRuntime) -> DbResult<Vec<ProviderSummaryItem>> {
        let mut providers = Self::list_all(database).await?;
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

    pub async fn get_detail_by_id(
        database: &DatabaseRuntime,
        provider_id_val: i64,
    ) -> DbResult<ProviderDetail> {
        let provider = Self::get_by_id(database, provider_id_val).await?;
        let api_keys =
            ProviderApiKeyRepository::list_summaries_by_provider_id(database, provider_id_val)
                .await?;
        let source_ids = provider
            .upstream_sources
            .iter()
            .map(|source| source.id)
            .collect::<Vec<_>>();
        let request_patch_variants =
            RequestPatchVariantRepository::list_by_source_ids(database, &source_ids).await?;
        Ok(ProviderDetail {
            provider,
            api_keys,
            request_patch_variants,
        })
    }

    pub async fn create_optional(
        database: &DatabaseRuntime,
        new_provider_data: &NewProvider,
        new_source_data: Option<&NewUpstreamSource>,
    ) -> DbResult<ProviderAggregate> {
        let new_provider_data = new_provider_data.clone();
        let new_source_data = new_source_data.cloned();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    provider_async_transaction!(connection as conn, {
                        let query = diesel::insert_into(provider::table)
                            .values(NewProviderDb::to_db(&new_provider_data))
                            .returning(ProviderDb::as_returning());
                        let provider =
                            diesel_async::RunQueryDsl::get_result::<ProviderDb>(query, &mut *conn)
                                .await
                                .map(ProviderDb::from_db)
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to insert provider aggregate: {error}"
                                    )))
                                })?;
                        let upstream_sources = match new_source_data {
                            Some(new_source) => {
                                if new_source.provider_id != provider.id {
                                    return Err(BaseError::ParamInvalid(Some(
                                        "upstream source must belong to its provider".to_string(),
                                    )));
                                }
                                let query = diesel::insert_into(upstream_source::table)
                                    .values(AggregateNewSourceDb::to_db(&new_source))
                                    .returning(AggregateSourceDb::as_returning());
                                let source = diesel_async::RunQueryDsl::get_result::<
                                    AggregateSourceDb,
                                >(query, &mut *conn)
                                .await
                                .map(AggregateSourceDb::from_db)
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to insert provider upstream source: {error}"
                                    )))
                                })?;
                                vec![source]
                            }
                            None => Vec::new(),
                        };
                        Ok(ProviderAggregate {
                            provider,
                            upstream_sources,
                        })
                    })
                })
            })
            .await
    }

    pub async fn update(
        database: &DatabaseRuntime,
        id_value: i64,
        update_data: &UpdateProviderData,
    ) -> DbResult<ProviderAggregate> {
        let current_time = Utc::now().timestamp_millis();
        let mut update_data = update_data.clone();
        update_data.provider_key = None;
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    provider_async_transaction!(connection as conn, {
                        let query = diesel::update(
                            provider::table.filter(
                                provider::dsl::id
                                    .eq(id_value)
                                    .and(provider::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            UpdateProviderDataDb::to_db(&update_data),
                            provider::dsl::updated_at.eq(current_time),
                        ))
                        .returning(ProviderDb::as_returning());
                        let provider =
                            diesel_async::RunQueryDsl::get_result::<ProviderDb>(query, &mut *conn)
                                .await
                                .map(ProviderDb::from_db)
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to update provider {}: {error}",
                                        id_value
                                    )))
                                })?;
                        let query = upstream_source::table
                            .filter(
                                upstream_source::dsl::provider_id
                                    .eq(id_value)
                                    .and(upstream_source::dsl::deleted_at.is_null()),
                            )
                            .order(upstream_source::dsl::id.asc())
                            .select(AggregateSourceDb::as_select());
                        let upstream_sources =
                            diesel_async::RunQueryDsl::load::<AggregateSourceDb>(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to load upstream source for provider {}: {error}",
                                        id_value
                                    )))
                                })?
                                .into_iter()
                                .map(AggregateSourceDb::from_db)
                                .collect();
                        Ok(ProviderAggregate {
                            provider,
                            upstream_sources,
                        })
                    })
                })
            })
            .await
    }

    /// Atomically inserts a logical provider with an initial Source.
    pub async fn create(
        database: &DatabaseRuntime,
        new_provider_data: &NewProvider,
        new_source_data: &NewUpstreamSource,
    ) -> DbResult<ProviderAggregate> {
        Self::create_optional(database, new_provider_data, Some(new_source_data)).await
    }

    pub async fn bootstrap(
        database: &DatabaseRuntime,
        input: &BootstrapProviderInput,
    ) -> DbResult<BootstrapProviderResult> {
        let input = input.clone();
        let current_time = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    provider_async_transaction!(connection as conn, {
                        let new_provider = NewProvider {
                            id: input.provider_id,
                            provider_key: input.provider_key.clone(),
                            name: input.name.clone(),
                            is_enabled: true,
                            created_at: current_time,
                            updated_at: current_time,
                            provider_api_key_mode: input.provider_api_key_mode,
                        };
                        let query = diesel::insert_into(provider::table)
                            .values(NewProviderDb::to_db(&new_provider))
                            .returning(ProviderDb::as_returning());
                        let provider =
                            diesel_async::RunQueryDsl::get_result::<ProviderDb>(query, &mut *conn)
                                .await
                                .map(ProviderDb::from_db)
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to insert bootstrap provider: {}",
                                        error
                                    )))
                                })?;
                        let query = diesel::insert_into(upstream_source::table)
                            .values(AggregateNewSourceDb::to_db(&input.source))
                            .returning(AggregateSourceDb::as_returning());
                        let upstream_source = diesel_async::RunQueryDsl::get_result::<
                            AggregateSourceDb,
                        >(query, &mut *conn)
                        .await
                        .map(AggregateSourceDb::from_db)
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to insert bootstrap upstream source: {}",
                                error
                            )))
                        })?;
                        let new_key = NewProviderApiKey {
                            id: input.provider_api_key_id,
                            provider_id: provider.id,
                            description: input.api_key_description,
                            key_prefix: input.key_prefix,
                            key_last4: input.key_last4,
                            encrypted_secret: input.encrypted_secret,
                            secret_hmac: input.secret_hmac,
                            is_enabled: true,
                            created_at: current_time,
                            updated_at: current_time,
                        };
                        let query = diesel::insert_into(provider_api_key::table)
                            .values((
                                provider_api_key::dsl::id.eq(new_key.id),
                                provider_api_key::dsl::provider_id.eq(new_key.provider_id),
                                provider_api_key::dsl::description.eq(new_key.description),
                                provider_api_key::dsl::key_prefix.eq(new_key.key_prefix),
                                provider_api_key::dsl::key_last4.eq(new_key.key_last4),
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
                                provider_api_key::dsl::is_enabled.eq(true),
                                provider_api_key::dsl::created_at.eq(current_time),
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
                            ));
                        let created_key = diesel_async::RunQueryDsl::get_result::<
                            ProviderApiKeySummaryTuple,
                        >(query, &mut *conn)
                        .await
                        .map(provider_api_key_summary_from_tuple)
                        .map_err(|error| map_provider_api_key_write_error("insert", error))?;
                        let new_model = NewModel {
                            id: ID_GENERATOR.generate_id(),
                            provider_id: provider.id,
                            model_name: input.model_name,
                            real_model_name: input.real_model_name,
                            model_kind: input.model_kind,
                            source_selection_mode: "INHERIT_ALL".to_string(),
                            is_enabled: true,
                            created_at: current_time,
                            updated_at: current_time,
                        };
                        let query = diesel::insert_into(model::table)
                            .values(BootstrapNewModelDb::to_db(&new_model))
                            .returning(BootstrapModelDb::as_returning());
                        let created_model =
                            diesel_async::RunQueryDsl::get_result::<BootstrapModelDb>(
                                query, &mut *conn,
                            )
                            .await
                            .map(BootstrapModelDb::from_db)
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to insert bootstrap model: {}",
                                    error
                                )))
                            })?;
                        Ok(BootstrapProviderResult {
                            provider: ProviderAggregate {
                                provider,
                                upstream_sources: vec![upstream_source],
                            },
                            created_key,
                            created_model,
                        })
                    })
                })
            })
            .await
    }

    pub async fn delete_with_dependents(
        database: &DatabaseRuntime,
        target_id_value: i64,
    ) -> DbResult<usize> {
        let current_time = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    provider_async_transaction!(connection as conn, {
                        let query = diesel::update(provider::table.find(target_id_value)).set((
                            provider::dsl::deleted_at.eq(current_time),
                            provider::dsl::is_enabled.eq(false),
                            provider::dsl::updated_at.eq(current_time),
                        ));
                        let updated = diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to delete provider {}: {}",
                                    target_id_value, error
                                )))
                            })?;
                        let query = diesel::update(
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
                        ));
                        diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to delete upstream source for provider {}: {}",
                                    target_id_value, error
                                )))
                            })?;
                        let query = diesel::update(
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
                            provider_api_key::dsl::secret_key_fingerprint
                                .eq(Option::<String>::None),
                            provider_api_key::dsl::secret_hmac.eq(Option::<String>::None),
                            provider_api_key::dsl::updated_at.eq(current_time),
                        ));
                        diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to delete provider API keys for provider {}: {}",
                                    target_id_value, error
                                )))
                            })?;
                        let query = upstream_source::table
                            .filter(upstream_source::dsl::provider_id.eq(target_id_value))
                            .select(upstream_source::dsl::id);
                        let source_ids =
                            diesel_async::RunQueryDsl::load::<i64>(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to load provider Sources for {}: {}",
                                        target_id_value, error
                                    )))
                                })?;
                        let variant_ids = if source_ids.is_empty() {
                            Vec::new()
                        } else {
                            let query = request_patch_variant::table
                                .filter(
                                    request_patch_variant::dsl::source_id.eq_any(&source_ids),
                                )
                                .filter(request_patch_variant::dsl::deleted_at.is_null())
                                .select(request_patch_variant::dsl::id);
                            diesel_async::RunQueryDsl::load::<i64>(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to load provider request patch Variants for {}: {}",
                                        target_id_value, error
                                    )))
                                })?
                        };
                        if !variant_ids.is_empty() {
                            let query = diesel::update(
                                request_patch_variant::table.filter(
                                    request_patch_variant::dsl::id.eq_any(&variant_ids),
                                ),
                            )
                            .set((
                                request_patch_variant::dsl::deleted_at.eq(current_time),
                                request_patch_variant::dsl::enabled.eq(false),
                                request_patch_variant::dsl::expose_in_models.eq(false),
                                request_patch_variant::dsl::updated_at.eq(current_time),
                            ));
                            diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to delete provider request patch Variants for {}: {}",
                                        target_id_value, error
                                    )))
                                })?;
                            let query = diesel::update(
                                request_patch_rule::table
                                    .filter(
                                        request_patch_rule::dsl::variant_id.eq_any(&variant_ids),
                                    )
                                    .filter(request_patch_rule::dsl::deleted_at.is_null()),
                            )
                            .set(request_patch_rule::dsl::deleted_at.eq(current_time));
                            diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to delete provider request patch Rules for {}: {}",
                                        target_id_value, error
                                    )))
                                })?;
                        }
                        Ok(updated)
                    })
                })
            })
            .await
    }

    /// Soft deletes a provider record by setting `deleted_at` to the current time and `is_enabled` to false.
    pub async fn delete(database: &DatabaseRuntime, target_id_value: i64) -> DbResult<usize> {
        Self::delete_with_dependents(database, target_id_value).await
    }
}

fn provider_api_key_selection_from_row(
    row: ProviderApiKeySelectionTuple,
) -> DbResult<ProviderApiKeySelection> {
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
}

pub struct ProviderApiKeyRepository;

impl ProviderApiKeyRepository {
    pub(crate) async fn insert(
        database: &DatabaseRuntime,
        new_key: &NewProviderApiKey,
    ) -> DbResult<ProviderApiKeySummary> {
        let new_key = new_key.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::insert_into(provider_api_key::table)
                            .values((
                                provider_api_key::dsl::id.eq(new_key.id),
                                provider_api_key::dsl::provider_id.eq(new_key.provider_id),
                                provider_api_key::dsl::description.eq(new_key.description),
                                provider_api_key::dsl::key_prefix.eq(new_key.key_prefix),
                                provider_api_key::dsl::key_last4.eq(new_key.key_last4),
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
                            ));
                        diesel_async::RunQueryDsl::get_result::<ProviderApiKeySummaryTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(provider_api_key_summary_from_tuple)
                        .map_err(|error| map_provider_api_key_write_error("insert", error))
                    })
                })
            })
            .await
    }

    pub(crate) async fn replace_secret(
        database: &DatabaseRuntime,
        provider_id_value: i64,
        key_id: i64,
        key_prefix_value: String,
        key_last4_value: String,
        encrypted_secret: &EncryptedSecret,
        secret_hmac_value: &ProviderSecretFingerprint,
    ) -> DbResult<ProviderApiKeySummary> {
        let current_time = Utc::now().timestamp_millis();
        let ciphertext = encrypted_secret.ciphertext().to_vec();
        let nonce = encrypted_secret.nonce().to_vec();
        let format_version = encrypted_secret.format_version();
        let fingerprint = encrypted_secret.key_fingerprint().as_str().to_string();
        let secret_hmac_value = secret_hmac_value.as_str().to_string();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
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
                            provider_api_key::dsl::secret_ciphertext.eq(Some(ciphertext)),
                            provider_api_key::dsl::secret_nonce.eq(Some(nonce)),
                            provider_api_key::dsl::secret_format_version.eq(Some(format_version)),
                            provider_api_key::dsl::secret_key_fingerprint.eq(Some(fingerprint)),
                            provider_api_key::dsl::secret_hmac.eq(Some(secret_hmac_value)),
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
                        ));
                        diesel_async::RunQueryDsl::get_result::<ProviderApiKeySummaryTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(provider_api_key_summary_from_tuple)
                        .map_err(|error| map_provider_api_key_write_error("replace", error))
                    })
                })
            })
            .await
    }

    pub(crate) async fn update_metadata(
        database: &DatabaseRuntime,
        provider_id_value: i64,
        key_id: i64,
        update: &UpdateProviderApiKeyMetadata,
    ) -> DbResult<ProviderApiKeySummary> {
        let current_time = Utc::now().timestamp_millis();
        let update = update.clone();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
                            provider_api_key::table.filter(
                                provider_api_key::dsl::id
                                    .eq(key_id)
                                    .and(
                                        provider_api_key::dsl::provider_id.eq(provider_id_value),
                                    )
                                    .and(provider_api_key::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            provider_api_key::dsl::description.eq(update.description),
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
                        ));
                        diesel_async::RunQueryDsl::get_result::<ProviderApiKeySummaryTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(provider_api_key_summary_from_tuple)
                        .map_err(|error| match error {
                            diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                                "Provider API key {key_id} not found for provider {provider_id_value}"
                            ))),
                            other => {
                                map_provider_api_key_write_error("update metadata for", other)
                            }
                        })
                    })
                })
            })
            .await
    }

    pub(crate) async fn soft_delete(
        database: &DatabaseRuntime,
        provider_id_value: i64,
        key_id: i64,
    ) -> DbResult<usize> {
        let current_time = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
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
                            provider_api_key::dsl::secret_key_fingerprint
                                .eq(Option::<String>::None),
                            provider_api_key::dsl::secret_hmac.eq(Option::<String>::None),
                            provider_api_key::dsl::updated_at.eq(current_time),
                        ));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map_err(|error| map_provider_api_key_write_error("delete", error))
                    })
                })
            })
            .await
    }

    pub async fn get_summary_by_id(
        database: &DatabaseRuntime,
        provider_id_value: i64,
        key_id: i64,
    ) -> DbResult<ProviderApiKeySummary> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = provider_api_key::table
                            .filter(
                                provider_api_key::dsl::id
                                    .eq(key_id)
                                    .and(
                                        provider_api_key::dsl::provider_id.eq(provider_id_value),
                                    )
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
                            ));
                        diesel_async::RunQueryDsl::first::<ProviderApiKeySummaryTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(provider_api_key_summary_from_tuple)
                        .map_err(|error| match error {
                            diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                                "Provider API key {key_id} not found for provider {provider_id_value}"
                            ))),
                            other => BaseError::DatabaseFatal(Some(format!(
                                "Failed to load provider API key summary: {other}"
                            ))),
                        })
                    })
                })
            })
            .await
    }

    pub async fn list_summaries_by_provider_id(
        database: &DatabaseRuntime,
        provider_id_value: i64,
    ) -> DbResult<Vec<ProviderApiKeySummary>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = provider_api_key::table
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
                            ));
                        diesel_async::RunQueryDsl::load::<ProviderApiKeySummaryTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(|rows| {
                            rows.into_iter()
                                .map(provider_api_key_summary_from_tuple)
                                .collect()
                        })
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to list provider API key summaries: {error}"
                            )))
                        })
                    })
                })
            })
            .await
    }

    pub async fn list_all_summaries(
        database: &DatabaseRuntime,
    ) -> DbResult<Vec<ProviderApiKeySummary>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = provider_api_key::table
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
                            ));
                        let rows = diesel_async::RunQueryDsl::load::<ProviderApiKeySummaryTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
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
                })
            })
            .await
    }

    pub(crate) async fn get_stored_by_id(
        database: &DatabaseRuntime,
        provider_id_value: i64,
        key_id: i64,
    ) -> DbResult<StoredProviderApiKey> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = provider_api_key::table
                            .filter(
                                provider_api_key::dsl::id
                                    .eq(key_id)
                                    .and(
                                        provider_api_key::dsl::provider_id.eq(provider_id_value),
                                    )
                                    .and(provider_api_key::dsl::deleted_at.is_null()),
                            )
                            .select(stored_provider_key_columns!());
                        diesel_async::RunQueryDsl::first::<StoredProviderApiKeyTuple>(
                            query,
                            &mut **conn,
                        )
                        .await
                        .map(stored_provider_api_key_from_tuple)
                        .map_err(|error| match error {
                            diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                                "Provider API key {key_id} not found for provider {provider_id_value}"
                            ))),
                            other => BaseError::DatabaseFatal(Some(format!(
                                "Failed to load provider API key secret: {other}"
                            ))),
                        })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub(crate) async fn set_secret_hmac_for_test(
        database: &DatabaseRuntime,
        provider_id_value: i64,
        key_id: i64,
        secret_hmac: String,
    ) -> DbResult<()> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(provider_api_key::table.filter(
                            provider_api_key::dsl::id
                                .eq(key_id)
                                .and(provider_api_key::dsl::provider_id.eq(provider_id_value)),
                        ))
                        .set(provider_api_key::dsl::secret_hmac.eq(Some(secret_hmac)));
                        let updated = diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to update provider API key test HMAC: {error}"
                                )))
                            })?;
                        if updated == 1 {
                            Ok(())
                        } else {
                            Err(BaseError::NotFound(Some(format!(
                                "Provider API key {key_id} not found for provider {provider_id_value}"
                            ))))
                        }
                    })
                })
            })
            .await
    }
    pub(crate) async fn list_selections_by_provider_id(
        database: &DatabaseRuntime,
        provider_id_value: i64,
    ) -> DbResult<Vec<ProviderApiKeySelection>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = provider_api_key::table
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
                            ));
                        let rows: Vec<ProviderApiKeySelectionTuple> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to list provider API key selections: {error}"
                                    )))
                                })?;

                        rows.into_iter()
                            .map(provider_api_key_selection_from_row)
                            .collect()
                    })
                })
            })
            .await
    }

    pub(crate) async fn list_all_selections(
        database: &DatabaseRuntime,
    ) -> DbResult<Vec<ProviderApiKeySelection>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = provider_api_key::table
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
                            ));
                        let rows: Vec<ProviderApiKeySelectionTuple> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to list provider API key selections: {error}"
                                    )))
                                })?;

                        rows.into_iter()
                            .map(provider_api_key_selection_from_row)
                            .collect()
                    })
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SecretEncryptionConfig;
    use crate::database::upstream_source::UpdateUpstreamSourceData;
    use crate::service::secret_encryption::{
        SecretDomain, SecretEncryptionService, SensitiveSecret,
    };
    use serde_json::Value;

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
            source: NewUpstreamSource {
                id: 103,
                provider_id,
                profile_type: UpstreamProfileType::Openai,
                base_url: "https://api.example.com/v1".to_string(),
                use_proxy: false,
                is_enabled: true,
                is_default: true,
                created_at: 1,
                updated_at: 1,
                ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
            },
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
            model_kind: ModelKind::Chat,
        }
    }

    #[tokio::test]
    async fn bootstrap_provider_creates_provider_key_and_model_atomically() {
        let database =
            crate::database::TestDatabase::new_sqlite_default("provider-bootstrap-atomic.sqlite")
                .await;
        let result = Provider::bootstrap(&database, &sample_input(Some("gpt-4o")))
            .await
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

        assert_eq!(Provider::list_all(&database).await.unwrap().len(), 1);
        assert_eq!(
            ProviderApiKeyRepository::list_summaries_by_provider_id(&database, result.provider.id,)
                .await
                .unwrap()
                .len(),
            1,
        );
        assert_eq!(
            crate::database::model::Model::list_by_provider_id(&database, result.provider.id,)
                .await
                .unwrap()
                .len(),
            1,
        );
    }

    #[tokio::test]
    async fn bootstrap_provider_rolls_back_on_model_validation_failure() {
        let database =
            crate::database::TestDatabase::new_sqlite_default("provider-bootstrap-rollback.sqlite")
                .await;
        let result = Provider::bootstrap(&database, &sample_input(Some("")))
            .await
            .expect_err("bootstrap should fail");

        let message = match result {
            BaseError::DatabaseFatal(msg) => msg.unwrap_or_default(),
            other => format!("{other:?}"),
        };

        assert!(message.contains("Failed to insert bootstrap model"));

        let provider_after = Provider::get_by_id(&database, 101).await;
        assert!(
            matches!(provider_after, Err(BaseError::ParamInvalid(_))),
            "provider bootstrap rollback result: {provider_after:?}"
        );
        assert!(
            ProviderApiKeyRepository::list_summaries_by_provider_id(&database, 101)
                .await
                .unwrap()
                .is_empty()
        );
        assert!(
            crate::database::model::Model::list_by_provider_id(&database, 101)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn provider_summary_list_returns_lightweight_rows() {
        let database =
            crate::database::TestDatabase::new_sqlite_default("provider-summary-list.sqlite").await;
        Provider::bootstrap(&database, &sample_input(Some("gpt-4o")))
            .await
            .expect("bootstrap should succeed");

        let rows = Provider::list_summary(&database)
            .await
            .expect("provider summaries should list");
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
    fn provider_detail_contract_uses_variant_fields() {
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
                    base_url: "https://api.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    deleted_at: None,
                    created_at: 1,
                    updated_at: 1,
                    ..UpstreamSource::test_defaults(UpstreamProfileType::Openai)
                }],
            },
            api_keys: vec![],
            request_patch_variants: vec![],
        };

        let value = serde_json::to_value(detail).expect("provider detail should serialize");
        let object = value
            .as_object()
            .expect("detail should serialize as object");
        assert!(matches!(object.get("api_keys"), Some(Value::Array(_))));
        assert!(matches!(
            object.get("request_patch_variants"),
            Some(Value::Array(_))
        ));
        assert!(object.get("request_patches").is_none());
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
        let database =
            crate::database::TestDatabase::new_sqlite_default("provider-aggregate-crud.sqlite")
                .await;
        (async {
            Provider::create(
                &database,
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
                    base_url: "https://old.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                    ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
                },
            )
            .await
            .expect("aggregate should create");

            let updated = Provider::update(
                &database,
                401,
                &UpdateProviderData {
                    provider_key: Some("must-not-change".to_string()),
                    name: Some("Updated Provider".to_string()),
                    is_enabled: None,
                    provider_api_key_mode: None,
                },
            )
            .await
            .expect("provider should update");
            assert_eq!(updated.name, "Updated Provider");
            assert_eq!(updated.provider_key, "aggregate-provider");

            let updated_source = UpstreamSource::update(
                &database,
                402,
                401,
                &UpdateUpstreamSourceData {
                    base_url: Some("https://new.example.com/v1".to_string()),
                    use_proxy: Some(true),
                    is_enabled: None,
                    is_default: None,
                    updated_at: 2,
                    ..UpdateUpstreamSourceData::test_defaults()
                },
            )
            .await
            .expect("source should update");
            assert_eq!(updated_source.profile_type, UpstreamProfileType::Openai);
            assert_eq!(updated_source.base_url, "https://new.example.com/v1");
            assert!(updated_source.use_proxy);

            assert_eq!(
                Provider::delete_with_dependents(&database, 401)
                    .await
                    .expect("delete"),
                1
            );
            assert!(Provider::get_by_id(&database, 401).await.is_err());

            let deleted_at = UpstreamSource::deleted_at_for_test(&database, 402)
                .await
                .expect("source should remain soft deleted");
            assert!(deleted_at.is_some());
        })
        .await;
    }

    #[tokio::test]
    async fn provider_aggregate_allows_zero_or_more_sources_and_preserves_family_identity() {
        let database =
            crate::database::TestDatabase::new_sqlite_default("provider-source-cardinality.sqlite")
                .await;
        (async {
            let provider = Provider::create_optional(
                &database,
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
            .await
            .expect("provider without a source should be valid");
            assert!(provider.upstream_sources.is_empty());

            let source = UpstreamSource::create(
                &database,
                &NewUpstreamSource {
                    id: 412,
                    provider_id: 411,
                    profile_type: UpstreamProfileType::Openai,
                    base_url: "https://first.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                    ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
                },
            )
            .await
            .expect("first source should create");
            assert!(source.is_default);

            let second = UpstreamSource::create(
                &database,
                &NewUpstreamSource {
                    id: 413,
                    provider_id: 411,
                    profile_type: UpstreamProfileType::Gemini,
                    base_url: "https://second.example.com/v1".to_string(),
                    use_proxy: true,
                    is_enabled: true,
                    is_default: false,
                    created_at: 1,
                    updated_at: 1,
                    ..NewUpstreamSource::test_defaults(UpstreamProfileType::Gemini)
                },
            )
            .await
            .expect("different source family should coexist");
            assert!(!second.is_default);

            let loaded = Provider::get_by_id(&database, 411)
                .await
                .expect("provider should load");
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
        let database =
            crate::database::TestDatabase::new_sqlite_default("provider-key-repository.sqlite")
                .await;
        (async {
                Provider::create(&database, &NewProvider {
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
                    base_url: "https://api.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                    ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
                }).await
                .expect("provider should seed");
                Provider::create(&database, &NewProvider {
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
                    base_url: "https://other.example.com/v1".to_string(),
                    use_proxy: false,
                    is_enabled: true,
                    is_default: true,
                    created_at: 1,
                    updated_at: 1,
                    ..NewUpstreamSource::test_defaults(UpstreamProfileType::Openai)
                }).await
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

                let summary = ProviderApiKeyRepository::insert(&database, &new_key(601)).await
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
                assert!(ProviderApiKeyRepository::insert(&database, &new_key(602)).await.is_err());
                assert!(
                    ProviderApiKeyRepository::update_metadata(&database,
                        502,
                        601,
                        &UpdateProviderApiKeyMetadata {
                            description: None,
                            is_enabled: false,
                        },
                    ).await
                    .is_err()
                );

                ProviderApiKeyRepository::update_metadata(&database,
                    501,
                    601,
                    &UpdateProviderApiKeyMetadata {
                        description: Some("disabled".to_string()),
                        is_enabled: false,
                    },
                ).await
                .expect("metadata update should succeed");
                assert!(ProviderApiKeyRepository::insert(&database, &new_key(602)).await.is_err());

                let stored = ProviderApiKeyRepository::get_stored_by_id(&database, 501, 601).await
                    .expect("stored provider key should load");
                let debug = format!("{stored:?}");
                assert!(debug.contains("<redacted>"));
                assert!(!debug.contains("sk-repository-secret"));
                assert!(!debug.contains(stored.secret_hmac.as_deref().unwrap_or_default()));

                ProviderApiKeyRepository::soft_delete(&database, 501, 601).await
                    .expect("provider key soft delete should succeed");
                assert!(ProviderApiKeyRepository::get_stored_by_id(&database, 501, 601).await.is_err());
                ProviderApiKeyRepository::insert(&database, &new_key(602)).await
                    .expect("soft deletion should release the HMAC uniqueness slot");
            })
            .await;
    }
}
