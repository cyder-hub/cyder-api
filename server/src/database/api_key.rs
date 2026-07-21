use chrono::Utc;
use diesel::prelude::*;
use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    DbResult,
    api_key_acl_rule::{self as api_key_acl_repository, ApiKeyAclRule, ApiKeyAclRuleInput},
    get_connection,
};
use crate::controller::BaseError;
use crate::schema::enum_def::Action;
use crate::service::secret_encryption::EncryptedSecret;
#[cfg(test)]
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

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

macro_rules! api_key_admin_db_execute {
    ($conn:ident, $block:block) => {
        match $conn {
            crate::database::DbConnection::Postgres($conn) => {
                #[allow(unused_imports)]
                use self::_postgres_model::*;
                use crate::database::_postgres_schema::*;
                #[allow(unused_imports)]
                use crate::database::api_key_acl_rule::_postgres_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
            crate::database::DbConnection::Sqlite($conn) => {
                #[allow(unused_imports)]
                use self::_sqlite_model::*;
                use crate::database::_sqlite_schema::*;
                #[allow(unused_imports)]
                use crate::database::api_key_acl_rule::_sqlite_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;

                $block
            }
        }
    };
}

macro_rules! load_api_key_acl_rules_in_tx {
    ($conn:ident, $api_key_id:expr) => {{
        let rows = api_key_acl_rule::table
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
            .select(ApiKeyAclRuleDb::as_select())
            .load::<ApiKeyAclRuleDb>($conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to load api key ACL rules for {}: {}",
                    $api_key_id, e
                )))
            })?;

        Ok::<Vec<ApiKeyAclRule>, BaseError>(
            rows.into_iter()
                .map(ApiKeyAclRuleDb::from_db)
                .collect::<Vec<_>>(),
        )
    }};
}

macro_rules! insert_api_key_acl_rules_in_tx {
    ($conn:ident, $api_key_id:expr, $acl_rows:expr) => {{
        let acl_rows = $acl_rows;
        if !acl_rows.is_empty() {
            let db_rows: Vec<_> = acl_rows.iter().map(NewApiKeyAclRuleDb::to_db).collect();
            diesel::insert_into(api_key_acl_rule::table)
                .values(&db_rows)
                .execute($conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert ACL rules for api key {}: {}",
                        $api_key_id, e
                    )))
                })?;
        }
        Ok::<(), BaseError>(())
    }};
}

macro_rules! replace_api_key_acl_rules_in_tx {
    ($conn:ident, $api_key_id:expr, $acl_rows:expr) => {{
        diesel::delete(
            api_key_acl_rule::table.filter(api_key_acl_rule::dsl::api_key_id.eq($api_key_id)),
        )
        .execute($conn)
        .map_err(|e| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to replace ACL rules for api key {}: {}",
                $api_key_id, e
            )))
        })?;

        insert_api_key_acl_rules_in_tx!($conn, $api_key_id, $acl_rows)?;
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
    pub(crate) fn create_issued(
        payload: &CreateApiKeyPayload,
        issuance: &ApiKeyIssuance,
    ) -> DbResult<ApiKeyDetail> {
        let conn = &mut get_connection()?;
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
        api_key_admin_db_execute!(conn, {
            conn.transaction::<ApiKeyDetail, BaseError, _>(|conn| {
                let inserted = diesel::insert_into(api_key::table)
                    .values(NewApiKeyDb::to_db(&new_key))
                    .returning(ApiKeyDb::as_returning())
                    .get_result::<ApiKeyDb>(conn)
                    .map(ApiKeyDb::from_db)
                    .map_err(|e| map_write_error("Failed to create api key", e))?;

                if let Some(encrypted) = issuance.encrypted_secret.as_ref() {
                    diesel::update(api_key::table.filter(api_key::dsl::id.eq(inserted.id)))
                        .set((
                            api_key::dsl::secret_ciphertext
                                .eq(Some(encrypted.ciphertext().to_vec())),
                            api_key::dsl::secret_nonce.eq(Some(encrypted.nonce().to_vec())),
                            api_key::dsl::secret_format_version
                                .eq(Some(encrypted.format_version())),
                            api_key::dsl::secret_key_fingerprint
                                .eq(Some(encrypted.key_fingerprint().as_str().to_string())),
                        ))
                        .execute(conn)
                        .map_err(|e| map_write_error("Failed to store api key secret", e))?;
                }

                insert_api_key_acl_rules_in_tx!(conn, inserted.id, &acl_rows)?;
                let acl_rules = load_api_key_acl_rules_in_tx!(conn, inserted.id)?;

                Ok(build_detail(&inserted, acl_rules))
            })
        })
    }

    #[cfg(test)]
    pub fn create(payload: &CreateApiKeyPayload) -> DbResult<ApiKeyDetailWithSecret> {
        let secret = generate_api_key_secret();
        let issuance = ApiKeyIssuance::new(ID_GENERATOR.generate_id(), &secret, None);
        let detail = Self::create_issued(payload, &issuance)?;
        let row = Self::get_by_id(detail.id)?;
        Ok(ApiKeyDetailWithSecret {
            detail,
            reveal: build_reveal(&row, secret, false),
        })
    }

    pub fn update_metadata(
        id_value: i64,
        payload: &UpdateApiKeyMetadataPayload,
    ) -> DbResult<ApiKeyDetail> {
        let conn = &mut get_connection()?;
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
        api_key_admin_db_execute!(conn, {
            conn.transaction::<ApiKeyDetail, BaseError, _>(|conn| {
                let updated = diesel::update(
                    api_key::table.filter(
                        api_key::dsl::id
                            .eq(id_value)
                            .and(api_key::dsl::deleted_at.is_null()),
                    ),
                )
                .set((
                    UpdateApiKeyDataDb::to_db(&update_data),
                    api_key::dsl::updated_at.eq(now),
                ))
                .execute(conn)
                .map_err(|e| {
                    map_write_error(&format!("Failed to update api key {}", id_value), e)
                })?;

                if updated == 0 {
                    return Err(BaseError::NotFound(Some(format!(
                        "Api key {} not found",
                        id_value
                    ))));
                }

                if let Some(acl_rows) = acl_rows.as_ref() {
                    replace_api_key_acl_rules_in_tx!(conn, id_value, acl_rows)?;
                }

                let row = api_key::table
                    .filter(
                        api_key::dsl::id
                            .eq(id_value)
                            .and(api_key::dsl::deleted_at.is_null()),
                    )
                    .select(ApiKeyDb::as_select())
                    .first::<ApiKeyDb>(conn)
                    .map(ApiKeyDb::from_db)
                    .map_err(|e| match e {
                        diesel::result::Error::NotFound => {
                            BaseError::NotFound(Some(format!("Api key {} not found", id_value)))
                        }
                        other => BaseError::DatabaseFatal(Some(format!(
                            "Failed to fetch api key {}: {}",
                            id_value, other
                        ))),
                    })?;
                let acl_rules = load_api_key_acl_rules_in_tx!(conn, id_value)?;

                Ok(build_detail(&row, acl_rules))
            })
        })
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();

        api_key_admin_db_execute!(conn, {
            conn.transaction::<usize, BaseError, _>(|conn| {
                let updated = diesel::update(
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
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete api key {}: {}",
                        id_value, e
                    )))
                })?;

                if updated == 0 {
                    return Err(BaseError::NotFound(Some(format!(
                        "Api key {} not found",
                        id_value
                    ))));
                }

                diesel::update(
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
                ))
                .execute(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to delete ACL rules for api key {}: {}",
                        id_value, e
                    )))
                })?;

                Ok(1)
            })
        })
    }

    pub(crate) fn rotate_issued(id_value: i64, issuance: &ApiKeyIssuance) -> DbResult<ApiKey> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
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
        let rotated = db_execute!(conn, {
            diesel::update(
                api_key::table.filter(
                    api_key::dsl::id
                        .eq(id_value)
                        .and(api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                api_key::dsl::api_key_hash.eq(&issuance.api_key_hash),
                api_key::dsl::key_prefix.eq(&issuance.key_prefix),
                api_key::dsl::key_last4.eq(&issuance.key_last4),
                api_key::dsl::secret_ciphertext.eq(ciphertext),
                api_key::dsl::secret_nonce.eq(nonce),
                api_key::dsl::secret_format_version.eq(format_version),
                api_key::dsl::secret_key_fingerprint.eq(fingerprint),
                api_key::dsl::updated_at.eq(now),
            ))
            .returning(ApiKeyDb::as_returning())
            .get_result::<ApiKeyDb>(conn)
            .map(ApiKeyDb::from_db)
            .map_err(|e| map_write_error(&format!("Failed to rotate api key {}", id_value), e))
        })?;

        Ok(rotated)
    }

    #[cfg(test)]
    pub fn rotate_key(id_value: i64) -> DbResult<ApiKeyReveal> {
        let secret = generate_api_key_secret();
        let issuance = ApiKeyIssuance::new(id_value, &secret, None);
        let rotated = Self::rotate_issued(id_value, &issuance)?;
        Ok(build_reveal(&rotated, secret, false))
    }

    pub(crate) fn list_reveal_metadata() -> DbResult<Vec<ApiKeyRevealMetadata>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = api_key::table
                .filter(api_key::dsl::deleted_at.is_null())
                .select((
                    api_key::dsl::id,
                    api_key::dsl::secret_ciphertext.is_not_null(),
                    api_key::dsl::secret_nonce.is_not_null(),
                    api_key::dsl::secret_format_version.is_not_null(),
                    api_key::dsl::secret_key_fingerprint,
                ))
                .load::<(i64, bool, bool, bool, Option<String>)>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to load api key reveal metadata: {e}"
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
    }

    pub(crate) fn get_reveal_metadata(id_value: i64) -> DbResult<ApiKeyRevealMetadata> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let (id, has_ciphertext, has_nonce, has_version, fingerprint) = api_key::table
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
                ))
                .first::<(i64, bool, bool, bool, Option<String>)>(conn)
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => {
                        BaseError::NotFound(Some(format!("Api key {id_value} not found")))
                    }
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
    }

    pub(crate) fn get_stored_secret(id_value: i64) -> DbResult<ApiKeyStoredSecret> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let row = api_key::table
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
                ))
                .first::<(
                    i64,
                    String,
                    String,
                    String,
                    i64,
                    Option<Vec<u8>>,
                    Option<Vec<u8>>,
                    Option<i32>,
                    Option<String>,
                )>(conn)
                .map_err(|e| match e {
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
    }

    pub fn load_acl_rules(id_value: i64) -> DbResult<Vec<ApiKeyAclRule>> {
        ApiKeyAclRule::list_by_api_key_id(id_value)
    }

    pub fn get_detail(id_value: i64) -> DbResult<ApiKeyDetail> {
        let row = Self::get_by_id(id_value)?;
        let rules = Self::load_acl_rules(id_value)?;
        Ok(build_detail(&row, rules))
    }

    pub fn list_summary() -> DbResult<Vec<ApiKeySummary>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let rows = api_key::table
                .filter(api_key::dsl::deleted_at.is_null())
                .order(api_key::dsl::created_at.desc())
                .select(ApiKeyDb::as_select())
                .load::<ApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list api keys: {}", e)))
                })?;

            Ok(rows
                .into_iter()
                .map(ApiKeyDb::from_db)
                .map(|row| build_summary(&row))
                .collect())
        })
    }

    pub fn list_all_active() -> DbResult<Vec<ApiKey>> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            let rows = api_key::table
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
                .select(ApiKeyDb::as_select())
                .load::<ApiKeyDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list active api keys: {}", e)))
                })?;

            Ok(rows.into_iter().map(ApiKeyDb::from_db).collect())
        })
    }

    pub fn get_by_id(id_value: i64) -> DbResult<ApiKey> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            api_key::table
                .filter(
                    api_key::dsl::id
                        .eq(id_value)
                        .and(api_key::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(conn)
                .map(ApiKeyDb::from_db)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        BaseError::NotFound(Some(format!("Api key {} not found", id_value)))
                    }
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch api key {}: {}",
                        id_value, other
                    ))),
                })
        })
    }

    pub fn get_by_hash(api_key_hash_value: &str) -> DbResult<ApiKey> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(api_key_hash_value)
                        .and(api_key::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(conn)
                .map(ApiKeyDb::from_db)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => {
                        BaseError::NotFound(Some("Api key hash not found".to_string()))
                    }
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch api key by hash: {}",
                        other
                    ))),
                })
        })
    }

    pub fn get_active_by_hash(api_key_hash_value: &str) -> DbResult<ApiKey> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(api_key_hash_value)
                        .and(api_key::dsl::deleted_at.is_null())
                        .and(api_key::dsl::is_enabled.eq(true))
                        .and(
                            api_key::dsl::expires_at
                                .is_null()
                                .or(api_key::dsl::expires_at.gt(now)),
                        ),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(conn)
                .map(ApiKeyDb::from_db)
                .map_err(|e| match e {
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

    #[test]
    fn missing_hash_error_does_not_echo_authentication_material() {
        let context = crate::database::TestDbContext::new_sqlite("api-key-hash-error.sqlite");
        context.run_sync(|| {
            let hash = "f".repeat(64);
            let error = ApiKey::get_by_hash(&hash).expect_err("hash should not exist");
            let debug = format!("{error:?}");
            assert!(!debug.contains(&hash));
            assert!(!debug.contains("api_key_hash="));
        });
    }
}
