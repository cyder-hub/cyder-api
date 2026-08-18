use chrono::Utc;
use diesel::prelude::*;
use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    DbResult,
    api_key_acl_rule::{self as api_key_acl_repository, ApiKeyAclRule, ApiKeyAclRuleInput},
    runtime::{DatabaseRuntime, DatabaseWorkload, db_execute as async_db_execute},
};
use crate::controller::BaseError;
use crate::db_object;
use crate::schema::enum_def::Action;
use crate::service::secret_encryption::EncryptedSecret;
#[cfg(test)]
use crate::utils::ID_GENERATOR;

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone)]
    #[diesel(table_name = api_key)]
    pub struct ApiKey {
        pub id: i64,
        pub api_key_hash: String,
        pub key_prefix: String,
        pub key_last4: String,
        pub name: String,
        pub description: Option<String>,
        pub default_action: Action,
        pub is_enabled: bool,
        pub expires_at: Option<i64>,
        pub rate_limit_rpm: Option<i32>,
        pub max_concurrent_requests: Option<i32>,
        pub quota_daily_requests: Option<i64>,
        pub quota_daily_tokens: Option<i64>,
        pub quota_monthly_tokens: Option<i64>,
        pub budget_daily_nanos: Option<i64>,
        pub budget_daily_currency: Option<String>,
        pub budget_monthly_nanos: Option<i64>,
        pub budget_monthly_currency: Option<String>,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = api_key)]
    pub struct NewApiKey {
        pub id: i64,
        pub api_key_hash: String,
        pub key_prefix: String,
        pub key_last4: String,
        pub name: String,
        pub description: Option<String>,
        pub default_action: Action,
        pub is_enabled: bool,
        pub expires_at: Option<i64>,
        pub rate_limit_rpm: Option<i32>,
        pub max_concurrent_requests: Option<i32>,
        pub quota_daily_requests: Option<i64>,
        pub quota_daily_tokens: Option<i64>,
        pub quota_monthly_tokens: Option<i64>,
        pub budget_daily_nanos: Option<i64>,
        pub budget_daily_currency: Option<String>,
        pub budget_monthly_nanos: Option<i64>,
        pub budget_monthly_currency: Option<String>,
        pub deleted_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(AsChangeset, Debug, Default)]
    #[diesel(table_name = api_key)]
    pub struct UpdateApiKeyData {
        pub name: Option<String>,
        pub description: Option<Option<String>>,
        pub default_action: Option<Action>,
        pub is_enabled: Option<bool>,
        pub expires_at: Option<Option<i64>>,
        pub rate_limit_rpm: Option<Option<i32>>,
        pub max_concurrent_requests: Option<Option<i32>>,
        pub quota_daily_requests: Option<Option<i64>>,
        pub quota_daily_tokens: Option<Option<i64>>,
        pub quota_monthly_tokens: Option<Option<i64>>,
        pub budget_daily_nanos: Option<Option<i64>>,
        pub budget_daily_currency: Option<Option<String>>,
        pub budget_monthly_nanos: Option<Option<i64>>,
        pub budget_monthly_currency: Option<Option<String>>,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyPayload {
    pub name: String,
    pub description: Option<String>,
    pub default_action: Option<Action>,
    pub is_enabled: Option<bool>,
    pub expires_at: Option<i64>,
    pub rate_limit_rpm: Option<i32>,
    pub max_concurrent_requests: Option<i32>,
    pub quota_daily_requests: Option<i64>,
    pub quota_daily_tokens: Option<i64>,
    pub quota_monthly_tokens: Option<i64>,
    pub budget_daily_nanos: Option<i64>,
    pub budget_daily_currency: Option<String>,
    pub budget_monthly_nanos: Option<i64>,
    pub budget_monthly_currency: Option<String>,
    pub acl_rules: Option<Vec<ApiKeyAclRuleInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateApiKeyMetadataPayload {
    pub name: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub description: Option<Option<String>>,
    pub default_action: Option<Action>,
    pub is_enabled: Option<bool>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub expires_at: Option<Option<i64>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub rate_limit_rpm: Option<Option<i32>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub max_concurrent_requests: Option<Option<i32>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub quota_daily_requests: Option<Option<i64>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub quota_daily_tokens: Option<Option<i64>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub quota_monthly_tokens: Option<Option<i64>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub budget_daily_nanos: Option<Option<i64>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub budget_daily_currency: Option<Option<String>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub budget_monthly_nanos: Option<Option<i64>>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    pub budget_monthly_currency: Option<Option<String>>,
    pub acl_rules: Option<Vec<ApiKeyAclRuleInput>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeySummary {
    pub id: i64,
    pub key_prefix: String,
    pub key_last4: String,
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action,
    pub is_enabled: bool,
    pub expires_at: Option<i64>,
    pub rate_limit_rpm: Option<i32>,
    pub max_concurrent_requests: Option<i32>,
    pub quota_daily_requests: Option<i64>,
    pub quota_daily_tokens: Option<i64>,
    pub quota_monthly_tokens: Option<i64>,
    pub budget_daily_nanos: Option<i64>,
    pub budget_daily_currency: Option<String>,
    pub budget_monthly_nanos: Option<i64>,
    pub budget_monthly_currency: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub can_reveal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyDetail {
    pub id: i64,
    pub key_prefix: String,
    pub key_last4: String,
    pub name: String,
    pub description: Option<String>,
    pub default_action: Action,
    pub is_enabled: bool,
    pub expires_at: Option<i64>,
    pub rate_limit_rpm: Option<i32>,
    pub max_concurrent_requests: Option<i32>,
    pub quota_daily_requests: Option<i64>,
    pub quota_daily_tokens: Option<i64>,
    pub quota_monthly_tokens: Option<i64>,
    pub budget_daily_nanos: Option<i64>,
    pub budget_daily_currency: Option<String>,
    pub budget_monthly_nanos: Option<i64>,
    pub budget_monthly_currency: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub acl_rules: Vec<ApiKeyAclRule>,
    pub can_reveal: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiKeyReveal {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub api_key: String,
    pub updated_at: i64,
    pub can_reveal: bool,
}

impl std::fmt::Debug for ApiKeyReveal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyReveal")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key_prefix", &self.key_prefix)
            .field("key_last4", &self.key_last4)
            .field("api_key", &"<redacted>")
            .field("updated_at", &self.updated_at)
            .field("can_reveal", &self.can_reveal)
            .finish()
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct ApiKeyDetailWithSecret {
    pub detail: ApiKeyDetail,
    pub reveal: ApiKeyReveal,
}

impl std::fmt::Debug for ApiKeyDetailWithSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyDetailWithSecret")
            .field("detail", &self.detail)
            .field("reveal", &"<redacted>")
            .finish()
    }
}

pub(crate) fn generate_api_key_secret() -> String {
    let random_part: String = rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();
    format!("cyder-{}", random_part)
}

pub(crate) struct ApiKeyIssuance {
    id: i64,
    api_key_hash: String,
    key_prefix: String,
    key_last4: String,
    encrypted_secret: Option<EncryptedSecret>,
}

impl ApiKeyIssuance {
    pub(crate) fn new(id: i64, secret: &str, encrypted_secret: Option<EncryptedSecret>) -> Self {
        Self {
            id,
            api_key_hash: hash_api_key(secret),
            key_prefix: key_prefix(secret),
            key_last4: key_last4(secret),
            encrypted_secret,
        }
    }

    pub(crate) fn has_encrypted_secret(&self) -> bool {
        self.encrypted_secret.is_some()
    }
}

impl std::fmt::Debug for ApiKeyIssuance {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyIssuance")
            .field("id", &self.id)
            .field("api_key_hash", &"<redacted>")
            .field("key_prefix", &self.key_prefix)
            .field("key_last4", &self.key_last4)
            .field(
                "encrypted_secret",
                &self.encrypted_secret.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

#[derive(Clone)]
pub(crate) struct ApiKeyRevealMetadata {
    pub id: i64,
    pub secret_tuple_complete: bool,
    pub secret_key_fingerprint: Option<String>,
}

impl std::fmt::Debug for ApiKeyRevealMetadata {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyRevealMetadata")
            .field("id", &self.id)
            .field("secret_tuple_complete", &self.secret_tuple_complete)
            .field(
                "secret_key_fingerprint",
                &self.secret_key_fingerprint.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

pub(crate) struct ApiKeyStoredSecret {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub updated_at: i64,
    pub ciphertext: Vec<u8>,
    pub nonce: Vec<u8>,
    pub format_version: i32,
    pub key_fingerprint: String,
}

impl std::fmt::Debug for ApiKeyStoredSecret {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ApiKeyStoredSecret")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("key_prefix", &self.key_prefix)
            .field("key_last4", &self.key_last4)
            .field("updated_at", &self.updated_at)
            .field("ciphertext", &"<redacted>")
            .field("nonce", &"<redacted>")
            .field("format_version", &self.format_version)
            .field("key_fingerprint", &"<redacted>")
            .finish()
    }
}

pub(crate) fn hash_api_key(secret: &str) -> String {
    format!("{:x}", Sha256::digest(secret.as_bytes()))
}

pub(crate) fn key_prefix(secret: &str) -> String {
    secret.chars().take(12).collect()
}

pub(crate) fn key_last4(secret: &str) -> String {
    let last4: String = secret.chars().rev().take(4).collect();
    last4.chars().rev().collect()
}

fn map_write_error(context: &str, e: diesel::result::Error) -> BaseError {
    match e {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}

macro_rules! api_key_admin_async_transaction {
    ($connection:ident as $conn:ident, $block:block) => {{
        match $connection {
            crate::database::runtime::RuntimeConnection::Postgres($conn) => {
                #[allow(unused_imports)]
                use self::_postgres_model::*;
                use crate::database::_postgres_schema::*;
                #[allow(unused_imports)]
                use crate::database::api_key_acl_rule::_postgres_model::*;
                use diesel::prelude::*;
                use diesel_async::AsyncConnection;
                $conn.transaction(async move |$conn| $block).await
            }
            crate::database::runtime::RuntimeConnection::Sqlite($conn) => {
                #[allow(unused_imports)]
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                #[allow(unused_imports)]
                use crate::database::api_key_acl_rule::_sqlite_model::*;
                use diesel::prelude::*;
                use diesel_async::AsyncConnection;
                $conn.transaction(async move |$conn| $block).await
            }
        }
    }};
}

macro_rules! load_api_key_acl_rules_async_in_tx {
    ($conn:ident, $api_key_id:expr) => {{
        let query = api_key_acl_rule::table
            .filter(
                api_key_acl_rule::dsl::api_key_id
                    .eq($api_key_id)
                    .and(api_key_acl_rule::dsl::deleted_at.is_null()),
            )
            .order((
                api_key_acl_rule::dsl::priority.asc(),
                api_key_acl_rule::dsl::created_at.asc(),
                api_key_acl_rule::dsl::id.asc(),
            ))
            .select(ApiKeyAclRuleDb::as_select());
        let rows: Vec<ApiKeyAclRuleDb> = diesel_async::RunQueryDsl::load(query, &mut *$conn)
            .await
            .map_err(|error| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to load api key ACL rules for {}: {}",
                    $api_key_id, error
                )))
            })?;
        Ok::<Vec<ApiKeyAclRule>, BaseError>(
            rows.into_iter().map(ApiKeyAclRuleDb::from_db).collect(),
        )
    }};
}

macro_rules! insert_api_key_acl_rules_async_in_tx {
    ($conn:ident, $api_key_id:expr, $acl_rows:expr) => {{
        for acl_row in $acl_rows {
            let db_row = NewApiKeyAclRuleDb::to_db(acl_row);
            let query = diesel::insert_into(api_key_acl_rule::table).values(&db_row);
            diesel_async::RunQueryDsl::execute(query, &mut *$conn)
                .await
                .map_err(|error| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert ACL rules for api key {}: {}",
                        $api_key_id, error
                    )))
                })?;
        }
        Ok::<(), BaseError>(())
    }};
}

fn default_api_key_action() -> Action {
    Action::Allow
}

fn build_summary(row: &ApiKey) -> ApiKeySummary {
    ApiKeySummary {
        id: row.id,
        key_prefix: row.key_prefix.clone(),
        key_last4: row.key_last4.clone(),
        name: row.name.clone(),
        description: row.description.clone(),
        default_action: row.default_action.clone(),
        is_enabled: row.is_enabled,
        expires_at: row.expires_at,
        rate_limit_rpm: row.rate_limit_rpm,
        max_concurrent_requests: row.max_concurrent_requests,
        quota_daily_requests: row.quota_daily_requests,
        quota_daily_tokens: row.quota_daily_tokens,
        quota_monthly_tokens: row.quota_monthly_tokens,
        budget_daily_nanos: row.budget_daily_nanos,
        budget_daily_currency: row.budget_daily_currency.clone(),
        budget_monthly_nanos: row.budget_monthly_nanos,
        budget_monthly_currency: row.budget_monthly_currency.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        can_reveal: false,
    }
}

pub(crate) fn build_reveal(row: &ApiKey, secret: String, can_reveal: bool) -> ApiKeyReveal {
    ApiKeyReveal {
        id: row.id,
        name: row.name.clone(),
        key_prefix: row.key_prefix.clone(),
        key_last4: row.key_last4.clone(),
        api_key: secret,
        updated_at: row.updated_at,
        can_reveal,
    }
}

fn build_detail(row: &ApiKey, acl_rules: Vec<ApiKeyAclRule>) -> ApiKeyDetail {
    ApiKeyDetail {
        id: row.id,
        key_prefix: row.key_prefix.clone(),
        key_last4: row.key_last4.clone(),
        name: row.name.clone(),
        description: row.description.clone(),
        default_action: row.default_action.clone(),
        is_enabled: row.is_enabled,
        expires_at: row.expires_at,
        rate_limit_rpm: row.rate_limit_rpm,
        max_concurrent_requests: row.max_concurrent_requests,
        quota_daily_requests: row.quota_daily_requests,
        quota_daily_tokens: row.quota_daily_tokens,
        quota_monthly_tokens: row.quota_monthly_tokens,
        budget_daily_nanos: row.budget_daily_nanos,
        budget_daily_currency: row.budget_daily_currency.clone(),
        budget_monthly_nanos: row.budget_monthly_nanos,
        budget_monthly_currency: row.budget_monthly_currency.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        acl_rules,
        can_reveal: false,
    }
}

impl ApiKey {
    #[cfg(test)]
    pub async fn secret_tuple_for_test(
        database: &DatabaseRuntime,
        id_value: i64,
    ) -> DbResult<(
        Option<Vec<u8>>,
        Option<Vec<u8>>,
        Option<i32>,
        Option<String>,
    )> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table.find(id_value).select((
                            api_key::dsl::secret_ciphertext,
                            api_key::dsl::secret_nonce,
                            api_key::dsl::secret_format_version,
                            api_key::dsl::secret_key_fingerprint,
                        ));
                        diesel_async::RunQueryDsl::first(query, &mut **conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load API key test secret tuple {id_value}: {error}"
                                )))
                            })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn update_secret_fingerprint_for_test(
        database: &DatabaseRuntime,
        id_value: i64,
        fingerprint: String,
    ) -> DbResult<()> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(api_key::table.find(id_value))
                            .set(api_key::dsl::secret_key_fingerprint.eq(Some(fingerprint)));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to update API key test fingerprint {id_value}: {error}"
                                )))
                            })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn corrupt_secret_ciphertext_for_test(
        database: &DatabaseRuntime,
        id_value: i64,
    ) -> DbResult<()> {
        let mut tuple = Self::secret_tuple_for_test(database, id_value).await?;
        let ciphertext = tuple.0.as_mut().ok_or(BaseError::ApiKeySecretUnavailable)?;
        let Some(first) = ciphertext.first_mut() else {
            return Err(BaseError::ApiKeySecretUnavailable);
        };
        *first ^= 1;
        let ciphertext = tuple.0;
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(api_key::table.find(id_value))
                            .set(api_key::dsl::secret_ciphertext.eq(ciphertext));
                        diesel_async::RunQueryDsl::execute(query, &mut **conn)
                            .await
                            .map(|_| ())
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to corrupt API key test ciphertext {id_value}: {error}"
                                )))
                            })
                    })
                })
            })
            .await
    }

    pub async fn load_acl_rules(
        database: &DatabaseRuntime,
        id_value: i64,
    ) -> DbResult<Vec<ApiKeyAclRule>> {
        ApiKeyAclRule::list_by_api_key_id(database, id_value).await
    }

    pub(crate) async fn create_issued(
        database: &DatabaseRuntime,
        payload: &CreateApiKeyPayload,
        issuance: &ApiKeyIssuance,
    ) -> DbResult<ApiKeyDetail> {
        let now = Utc::now().timestamp_millis();
        let new_key = NewApiKey {
            id: issuance.id,
            api_key_hash: issuance.api_key_hash.clone(),
            key_prefix: issuance.key_prefix.clone(),
            key_last4: issuance.key_last4.clone(),
            name: payload.name.clone(),
            description: payload.description.clone(),
            default_action: payload
                .default_action
                .clone()
                .unwrap_or_else(default_api_key_action),
            is_enabled: payload.is_enabled.unwrap_or(true),
            expires_at: payload.expires_at,
            rate_limit_rpm: payload.rate_limit_rpm,
            max_concurrent_requests: payload.max_concurrent_requests,
            quota_daily_requests: payload.quota_daily_requests,
            quota_daily_tokens: payload.quota_daily_tokens,
            quota_monthly_tokens: payload.quota_monthly_tokens,
            budget_daily_nanos: payload.budget_daily_nanos,
            budget_daily_currency: payload.budget_daily_currency.clone(),
            budget_monthly_nanos: payload.budget_monthly_nanos,
            budget_monthly_currency: payload.budget_monthly_currency.clone(),
            deleted_at: None,
            created_at: now,
            updated_at: now,
        };
        let acl_rows = match payload.acl_rules.as_ref() {
            Some(rules) => api_key_acl_repository::map_rule_inputs(new_key.id, rules, now)?,
            None => Vec::new(),
        };
        let encrypted_secret = issuance.encrypted_secret.as_ref().map(|encrypted| {
            (
                encrypted.ciphertext().to_vec(),
                encrypted.nonce().to_vec(),
                encrypted.format_version(),
                encrypted.key_fingerprint().as_str().to_string(),
            )
        });
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    api_key_admin_async_transaction!(connection as conn, {
                        let query = diesel::insert_into(api_key::table)
                            .values(NewApiKeyDb::to_db(&new_key))
                            .returning(ApiKeyDb::as_returning());
                        let inserted =
                            diesel_async::RunQueryDsl::get_result::<ApiKeyDb>(query, &mut *conn)
                                .await
                                .map(ApiKeyDb::from_db)
                                .map_err(|error| {
                                    map_write_error("Failed to create api key", error)
                                })?;

                        if let Some((ciphertext, nonce, format_version, fingerprint)) =
                            encrypted_secret
                        {
                            let query = diesel::update(
                                api_key::table.filter(api_key::dsl::id.eq(inserted.id)),
                            )
                            .set((
                                api_key::dsl::secret_ciphertext.eq(Some(ciphertext)),
                                api_key::dsl::secret_nonce.eq(Some(nonce)),
                                api_key::dsl::secret_format_version.eq(Some(format_version)),
                                api_key::dsl::secret_key_fingerprint.eq(Some(fingerprint)),
                            ));
                            diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    map_write_error("Failed to store api key secret", error)
                                })?;
                        }

                        insert_api_key_acl_rules_async_in_tx!(conn, inserted.id, &acl_rows)?;
                        let acl_rules = load_api_key_acl_rules_async_in_tx!(conn, inserted.id)?;
                        Ok(build_detail(&inserted, acl_rules))
                    })
                })
            })
            .await
    }

    pub async fn update_metadata(
        database: &DatabaseRuntime,
        id_value: i64,
        payload: &UpdateApiKeyMetadataPayload,
    ) -> DbResult<ApiKeyDetail> {
        let now = Utc::now().timestamp_millis();
        let update_data = UpdateApiKeyData {
            name: payload.name.clone(),
            description: payload.description.clone(),
            default_action: payload.default_action.clone(),
            is_enabled: payload.is_enabled,
            expires_at: payload.expires_at,
            rate_limit_rpm: payload.rate_limit_rpm,
            max_concurrent_requests: payload.max_concurrent_requests,
            quota_daily_requests: payload.quota_daily_requests,
            quota_daily_tokens: payload.quota_daily_tokens,
            quota_monthly_tokens: payload.quota_monthly_tokens,
            budget_daily_nanos: payload.budget_daily_nanos,
            budget_daily_currency: payload.budget_daily_currency.clone(),
            budget_monthly_nanos: payload.budget_monthly_nanos,
            budget_monthly_currency: payload.budget_monthly_currency.clone(),
        };
        let acl_rows = match payload.acl_rules.as_ref() {
            Some(rules) => Some(api_key_acl_repository::map_rule_inputs(
                id_value, rules, now,
            )?),
            None => None,
        };
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    api_key_admin_async_transaction!(connection as conn, {
                        let query = diesel::update(
                            api_key::table.filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            UpdateApiKeyDataDb::to_db(&update_data),
                            api_key::dsl::updated_at.eq(now),
                        ));
                        let updated = diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                map_write_error(
                                    &format!("Failed to update api key {}", id_value),
                                    error,
                                )
                            })?;
                        if updated == 0 {
                            return Err(BaseError::NotFound(Some(format!(
                                "Api key {} not found",
                                id_value
                            ))));
                        }
                        if let Some(acl_rows) = acl_rows.as_ref() {
                            let query = diesel::delete(
                                api_key_acl_rule::table
                                    .filter(api_key_acl_rule::dsl::api_key_id.eq(id_value)),
                            );
                            diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to replace ACL rules for api key {}: {}",
                                        id_value, error
                                    )))
                                })?;
                            insert_api_key_acl_rules_async_in_tx!(conn, id_value, acl_rows)?;
                        }
                        let query = api_key::table
                            .filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            )
                            .select(ApiKeyDb::as_select());
                        let row = diesel_async::RunQueryDsl::first::<ApiKeyDb>(query, &mut *conn)
                            .await
                            .map(ApiKeyDb::from_db)
                            .map_err(|error| match error {
                                diesel::result::Error::NotFound => BaseError::NotFound(Some(
                                    format!("Api key {} not found", id_value),
                                )),
                                other => BaseError::DatabaseFatal(Some(format!(
                                    "Failed to fetch api key {}: {}",
                                    id_value, other
                                ))),
                            })?;
                        let acl_rules = load_api_key_acl_rules_async_in_tx!(conn, id_value)?;
                        Ok(build_detail(&row, acl_rules))
                    })
                })
            })
            .await
    }

    pub async fn delete(database: &DatabaseRuntime, id_value: i64) -> DbResult<usize> {
        let now = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    api_key_admin_async_transaction!(connection as conn, {
                        let query = diesel::update(
                            api_key::table.filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            api_key::dsl::deleted_at.eq(Some(now)),
                            api_key::dsl::is_enabled.eq(false),
                            api_key::dsl::secret_ciphertext.eq(None::<Vec<u8>>),
                            api_key::dsl::secret_nonce.eq(None::<Vec<u8>>),
                            api_key::dsl::secret_format_version.eq(None::<i32>),
                            api_key::dsl::secret_key_fingerprint.eq(None::<String>),
                            api_key::dsl::updated_at.eq(now),
                        ));
                        let updated = diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to delete api key {}: {}",
                                    id_value, error
                                )))
                            })?;
                        if updated == 0 {
                            return Err(BaseError::NotFound(Some(format!(
                                "Api key {} not found",
                                id_value
                            ))));
                        }
                        let query = diesel::update(
                            api_key_acl_rule::table.filter(
                                api_key_acl_rule::dsl::api_key_id
                                    .eq(id_value)
                                    .and(api_key_acl_rule::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            api_key_acl_rule::dsl::deleted_at.eq(Some(now)),
                            api_key_acl_rule::dsl::is_enabled.eq(false),
                            api_key_acl_rule::dsl::updated_at.eq(now),
                        ));
                        diesel_async::RunQueryDsl::execute(query, &mut *conn)
                            .await
                            .map_err(|error| {
                                BaseError::DatabaseFatal(Some(format!(
                                    "Failed to delete ACL rules for api key {}: {}",
                                    id_value, error
                                )))
                            })?;
                        Ok(1)
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn create(
        database: &DatabaseRuntime,
        payload: &CreateApiKeyPayload,
    ) -> DbResult<ApiKeyDetailWithSecret> {
        let secret = generate_api_key_secret();
        let issuance = ApiKeyIssuance::new(ID_GENERATOR.generate_id(), &secret, None);
        let detail = Self::create_issued(database, payload, &issuance).await?;
        let row = Self::get_by_id(database, detail.id).await?;
        Ok(ApiKeyDetailWithSecret {
            detail,
            reveal: build_reveal(&row, secret, false),
        })
    }

    pub(crate) async fn rotate_issued(
        database: &DatabaseRuntime,
        id_value: i64,
        issuance: &ApiKeyIssuance,
    ) -> DbResult<ApiKey> {
        let now = Utc::now().timestamp_millis();
        let api_key_hash = issuance.api_key_hash.clone();
        let key_prefix = issuance.key_prefix.clone();
        let key_last4 = issuance.key_last4.clone();
        let (ciphertext, nonce, format_version, fingerprint) = issuance
            .encrypted_secret
            .as_ref()
            .map(|encrypted| {
                (
                    Some(encrypted.ciphertext().to_vec()),
                    Some(encrypted.nonce().to_vec()),
                    Some(encrypted.format_version()),
                    Some(encrypted.key_fingerprint().as_str().to_string()),
                )
            })
            .unwrap_or((None, None, None, None));
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = diesel::update(
                            api_key::table.filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            ),
                        )
                        .set((
                            api_key::dsl::api_key_hash.eq(api_key_hash),
                            api_key::dsl::key_prefix.eq(key_prefix),
                            api_key::dsl::key_last4.eq(key_last4),
                            api_key::dsl::secret_ciphertext.eq(ciphertext),
                            api_key::dsl::secret_nonce.eq(nonce),
                            api_key::dsl::secret_format_version.eq(format_version),
                            api_key::dsl::secret_key_fingerprint.eq(fingerprint),
                            api_key::dsl::updated_at.eq(now),
                        ))
                        .returning(ApiKeyDb::as_returning());
                        diesel_async::RunQueryDsl::get_result::<ApiKeyDb>(query, &mut **conn)
                            .await
                            .map(ApiKeyDb::from_db)
                            .map_err(|error| {
                                map_write_error(
                                    &format!("Failed to rotate api key {}", id_value),
                                    error,
                                )
                            })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn rotate_key(database: &DatabaseRuntime, id_value: i64) -> DbResult<ApiKeyReveal> {
        let secret = generate_api_key_secret();
        let issuance = ApiKeyIssuance::new(id_value, &secret, None);
        let rotated = Self::rotate_issued(database, id_value, &issuance).await?;
        Ok(build_reveal(&rotated, secret, false))
    }

    pub(crate) async fn list_reveal_metadata(
        database: &DatabaseRuntime,
    ) -> DbResult<Vec<ApiKeyRevealMetadata>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(api_key::dsl::deleted_at.is_null())
                            .select((
                                api_key::dsl::id,
                                api_key::dsl::secret_ciphertext.is_not_null(),
                                api_key::dsl::secret_nonce.is_not_null(),
                                api_key::dsl::secret_format_version.is_not_null(),
                                api_key::dsl::secret_key_fingerprint,
                            ));
                        let rows: Vec<(i64, bool, bool, bool, Option<String>)> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to load api key reveal metadata: {error}"
                                    )))
                                })?;
                        Ok(rows
                            .into_iter()
                            .map(
                                |(id, has_ciphertext, has_nonce, has_version, fingerprint)| {
                                    ApiKeyRevealMetadata {
                                        id,
                                        secret_tuple_complete: has_ciphertext
                                            && has_nonce
                                            && has_version
                                            && fingerprint.is_some(),
                                        secret_key_fingerprint: fingerprint,
                                    }
                                },
                            )
                            .collect())
                    })
                })
            })
            .await
    }

    pub(crate) async fn get_reveal_metadata(
        database: &DatabaseRuntime,
        id_value: i64,
    ) -> DbResult<ApiKeyRevealMetadata> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            )
                            .select((
                                api_key::dsl::id,
                                api_key::dsl::secret_ciphertext.is_not_null(),
                                api_key::dsl::secret_nonce.is_not_null(),
                                api_key::dsl::secret_format_version.is_not_null(),
                                api_key::dsl::secret_key_fingerprint,
                            ));
                        let row: Result<(i64, bool, bool, bool, Option<String>), _> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        let (id, has_ciphertext, has_nonce, has_version, fingerprint) = row
                            .map_err(|error| match error {
                                diesel::result::Error::NotFound => BaseError::NotFound(Some(
                                    format!("Api key {id_value} not found"),
                                )),
                                other => BaseError::DatabaseFatal(Some(format!(
                                    "Failed to load api key reveal metadata {id_value}: {other}"
                                ))),
                            })?;
                        Ok(ApiKeyRevealMetadata {
                            id,
                            secret_tuple_complete: has_ciphertext
                                && has_nonce
                                && has_version
                                && fingerprint.is_some(),
                            secret_key_fingerprint: fingerprint,
                        })
                    })
                })
            })
            .await
    }

    pub(crate) async fn get_stored_secret(
        database: &DatabaseRuntime,
        id_value: i64,
    ) -> DbResult<ApiKeyStoredSecret> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            )
                            .select((
                                api_key::dsl::id,
                                api_key::dsl::name,
                                api_key::dsl::key_prefix,
                                api_key::dsl::key_last4,
                                api_key::dsl::updated_at,
                                api_key::dsl::secret_ciphertext,
                                api_key::dsl::secret_nonce,
                                api_key::dsl::secret_format_version,
                                api_key::dsl::secret_key_fingerprint,
                            ));
                        let row = diesel_async::RunQueryDsl::first::<(
                            i64,
                            String,
                            String,
                            String,
                            i64,
                            Option<Vec<u8>>,
                            Option<Vec<u8>>,
                            Option<i32>,
                            Option<String>,
                        )>(query, &mut **conn)
                        .await
                        .map_err(|error| match error {
                            diesel::result::Error::NotFound => {
                                BaseError::NotFound(Some(format!("Api key {id_value} not found")))
                            }
                            other => BaseError::DatabaseFatal(Some(format!(
                                "Failed to load api key secret {id_value}: {other}"
                            ))),
                        })?;
                        let (
                            id,
                            name,
                            key_prefix,
                            key_last4,
                            updated_at,
                            Some(ciphertext),
                            Some(nonce),
                            Some(format_version),
                            Some(key_fingerprint),
                        ) = row
                        else {
                            return Err(BaseError::ApiKeySecretUnavailable);
                        };
                        Ok(ApiKeyStoredSecret {
                            id,
                            name,
                            key_prefix,
                            key_last4,
                            updated_at,
                            ciphertext,
                            nonce,
                            format_version,
                            key_fingerprint,
                        })
                    })
                })
            })
            .await
    }

    pub async fn get_detail(database: &DatabaseRuntime, id_value: i64) -> DbResult<ApiKeyDetail> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    match connection {
                        crate::database::runtime::RuntimeConnection::Postgres(conn) => {
                            use self::_postgres_model::*;
                            use crate::database::_postgres_schema::*;
                            use crate::database::api_key_acl_rule::_postgres_model::*;
                            use diesel::prelude::*;
                            let query = api_key::table
                                .filter(
                                    api_key::dsl::id
                                        .eq(id_value)
                                        .and(api_key::dsl::deleted_at.is_null()),
                                )
                                .select(ApiKeyDb::as_select());
                            let row =
                                diesel_async::RunQueryDsl::first::<ApiKeyDb>(query, &mut **conn)
                                    .await
                                    .map(ApiKeyDb::from_db)
                                    .map_err(|error| match error {
                                        diesel::result::Error::NotFound => BaseError::NotFound(
                                            Some(format!("Api key {} not found", id_value)),
                                        ),
                                        other => BaseError::DatabaseFatal(Some(format!(
                                            "Failed to fetch api key {}: {}",
                                            id_value, other
                                        ))),
                                    })?;
                            let rules = load_api_key_acl_rules_async_in_tx!(conn, id_value)?;
                            Ok(build_detail(&row, rules))
                        }
                        crate::database::runtime::RuntimeConnection::Sqlite(conn) => {
                            use self::_sqlite_model::*;
                            use crate::database::_sqlite_schema::*;
                            use crate::database::api_key_acl_rule::_sqlite_model::*;
                            use diesel::prelude::*;
                            let query = api_key::table
                                .filter(
                                    api_key::dsl::id
                                        .eq(id_value)
                                        .and(api_key::dsl::deleted_at.is_null()),
                                )
                                .select(ApiKeyDb::as_select());
                            let row =
                                diesel_async::RunQueryDsl::first::<ApiKeyDb>(query, &mut **conn)
                                    .await
                                    .map(ApiKeyDb::from_db)
                                    .map_err(|error| match error {
                                        diesel::result::Error::NotFound => BaseError::NotFound(
                                            Some(format!("Api key {} not found", id_value)),
                                        ),
                                        other => BaseError::DatabaseFatal(Some(format!(
                                            "Failed to fetch api key {}: {}",
                                            id_value, other
                                        ))),
                                    })?;
                            let rules = load_api_key_acl_rules_async_in_tx!(conn, id_value)?;
                            Ok(build_detail(&row, rules))
                        }
                    }
                })
            })
            .await
    }

    pub async fn list_summary(database: &DatabaseRuntime) -> DbResult<Vec<ApiKeySummary>> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(api_key::dsl::deleted_at.is_null())
                            .order(api_key::dsl::created_at.desc())
                            .select(ApiKeyDb::as_select());
                        let rows: Vec<ApiKeyDb> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to list api keys: {}",
                                        error
                                    )))
                                })?;
                        Ok(rows
                            .into_iter()
                            .map(ApiKeyDb::from_db)
                            .map(|row| build_summary(&row))
                            .collect())
                    })
                })
            })
            .await
    }

    pub async fn list_all_active(database: &DatabaseRuntime) -> DbResult<Vec<ApiKey>> {
        let now = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(
                                api_key::dsl::deleted_at
                                    .is_null()
                                    .and(api_key::dsl::is_enabled.eq(true))
                                    .and(
                                        api_key::dsl::expires_at
                                            .is_null()
                                            .or(api_key::dsl::expires_at.gt(now)),
                                    ),
                            )
                            .order(api_key::dsl::created_at.desc())
                            .select(ApiKeyDb::as_select());
                        let rows: Vec<ApiKeyDb> =
                            diesel_async::RunQueryDsl::load(query, &mut **conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to list active api keys: {}",
                                        error
                                    )))
                                })?;

                        Ok(rows.into_iter().map(ApiKeyDb::from_db).collect())
                    })
                })
            })
            .await
    }

    pub async fn get_by_id(database: &DatabaseRuntime, id_value: i64) -> DbResult<ApiKey> {
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(
                                api_key::dsl::id
                                    .eq(id_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            )
                            .select(ApiKeyDb::as_select());
                        let result: Result<ApiKeyDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result.map(ApiKeyDb::from_db).map_err(|error| match error {
                            diesel::result::Error::NotFound => {
                                BaseError::NotFound(Some(format!("Api key {} not found", id_value)))
                            }
                            other => BaseError::DatabaseFatal(Some(format!(
                                "Failed to fetch api key {}: {}",
                                id_value, other
                            ))),
                        })
                    })
                })
            })
            .await
    }

    pub async fn get_by_hash(
        database: &DatabaseRuntime,
        api_key_hash_value: &str,
    ) -> DbResult<ApiKey> {
        let api_key_hash_value = api_key_hash_value.to_string();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(
                                api_key::dsl::api_key_hash
                                    .eq(&api_key_hash_value)
                                    .and(api_key::dsl::deleted_at.is_null()),
                            )
                            .select(ApiKeyDb::as_select());
                        let result: Result<ApiKeyDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result.map(ApiKeyDb::from_db).map_err(|error| match error {
                            diesel::result::Error::NotFound => {
                                BaseError::NotFound(Some("Api key hash not found".to_string()))
                            }
                            other => BaseError::DatabaseFatal(Some(format!(
                                "Failed to fetch api key by hash: {}",
                                other
                            ))),
                        })
                    })
                })
            })
            .await
    }

    pub async fn get_active_by_hash(
        database: &DatabaseRuntime,
        api_key_hash_value: &str,
    ) -> DbResult<ApiKey> {
        let api_key_hash_value = api_key_hash_value.to_string();
        let now = Utc::now().timestamp_millis();
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = api_key::table
                            .filter(
                                api_key::dsl::api_key_hash
                                    .eq(&api_key_hash_value)
                                    .and(api_key::dsl::deleted_at.is_null())
                                    .and(api_key::dsl::is_enabled.eq(true))
                                    .and(
                                        api_key::dsl::expires_at
                                            .is_null()
                                            .or(api_key::dsl::expires_at.gt(now)),
                                    ),
                            )
                            .select(ApiKeyDb::as_select());
                        let result: Result<ApiKeyDb, diesel::result::Error> =
                            diesel_async::RunQueryDsl::first(query, &mut **conn).await;
                        result.map(ApiKeyDb::from_db).map_err(|error| match error {
                            diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                                "Api key hash {} not found",
                                api_key_hash_value
                            ))),
                            other => BaseError::DatabaseFatal(Some(format!(
                                "Failed to fetch api key by hash: {}",
                                other
                            ))),
                        })
                    })
                })
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_api_key_projection_does_not_select_secret_columns() {
        use crate::database::api_key::_postgres_model::ApiKeyDb as PostgresApiKeyDb;
        use crate::database::api_key::_sqlite_model::ApiKeyDb as SqliteApiKeyDb;

        let sqlite_query =
            crate::database::_sqlite_schema::api_key::table.select(SqliteApiKeyDb::as_select());
        let sqlite_sql =
            diesel::debug_query::<diesel::sqlite::Sqlite, _>(&sqlite_query).to_string();
        let postgres_query =
            crate::database::_postgres_schema::api_key::table.select(PostgresApiKeyDb::as_select());
        let postgres_sql = diesel::debug_query::<diesel::pg::Pg, _>(&postgres_query).to_string();

        for sql in [sqlite_sql, postgres_sql] {
            for secret_column in [
                "secret_ciphertext",
                "secret_nonce",
                "secret_format_version",
                "secret_key_fingerprint",
            ] {
                assert!(
                    !sql.contains(secret_column),
                    "ordinary api key projection selected {secret_column}: {sql}"
                );
            }
        }
    }

    #[test]
    fn update_payload_distinguishes_explicit_null_from_missing_fields() {
        let payload: UpdateApiKeyMetadataPayload = serde_json::from_value(serde_json::json!({
            "quota_daily_requests": null,
            "budget_daily_currency": null
        }))
        .expect("payload should deserialize");

        assert_eq!(payload.quota_daily_requests, Some(None));
        assert_eq!(payload.budget_daily_currency, Some(None));

        let missing_payload: UpdateApiKeyMetadataPayload =
            serde_json::from_value(serde_json::json!({}))
                .expect("missing payload should deserialize");

        assert_eq!(missing_payload.quota_daily_requests, None);
        assert_eq!(missing_payload.budget_daily_currency, None);
    }

    #[test]
    fn hash_prefix_and_last4_are_stable() {
        let secret = "cyder-abcdefghijklmnopqrstuvwxyz";
        assert_eq!(
            hash_api_key(secret),
            "c7355742a8aca380b74ca3a9daa93a237389768aaa09aa08ad30b05d437addae"
        );
        assert_eq!(key_prefix(secret), "cyder-abcdef");
        assert_eq!(key_last4(secret), "wxyz");
    }

    #[tokio::test]
    async fn missing_hash_error_does_not_echo_authentication_material() {
        let database =
            crate::database::TestDatabase::new_sqlite_default("api-key-hash-error.sqlite").await;
        let hash = "f".repeat(64);
        let error = ApiKey::get_by_hash(database.runtime().as_ref(), &hash)
            .await
            .expect_err("hash should not exist");
        let debug = format!("{error:?}");
        assert!(!debug.contains(&hash));
        assert!(!debug.contains("api_key_hash="));
    }
}
