use std::collections::BTreeSet;

use chrono::Utc;
use diesel::prelude::*;
use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    DbResult,
    api_key_acl_rule::{self as api_key_acl_repository, ApiKeyAclRule, ApiKeyAclRuleInput},
    get_connection,
    model_route::NewApiKeyModelOverride,
};
use crate::controller::BaseError;
use crate::schema::enum_def::Action;
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone)]
    #[diesel(table_name = api_key)]
    pub struct ApiKey {
        pub id: i64,
        #[diesel(column_name = api_key_value)]
        pub api_key: String,
        pub api_key_hash: Option<String>,
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
        #[diesel(column_name = api_key_value)]
        pub api_key: String,
        pub api_key_hash: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyReveal {
    pub id: i64,
    pub name: String,
    pub key_prefix: String,
    pub key_last4: String,
    pub api_key: String,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyDetailWithSecret {
    pub detail: ApiKeyDetail,
    pub reveal: ApiKeyReveal,
}

#[derive(Debug, Clone)]
pub struct ApiKeyModelOverrideWriteInput {
    pub source_name: String,
    pub target_route_id: i64,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ApiKeyModelOverrideWriteSummary {
    pub old_source_names: Vec<String>,
    pub new_source_names: Vec<String>,
    pub override_count: usize,
    pub enabled_override_count: usize,
}

impl ApiKeyModelOverrideWriteSummary {
    pub fn invalidation_source_names(&self) -> Vec<String> {
        collect_source_names(self.old_source_names.clone(), self.new_source_names.clone())
    }

    fn from_rows(
        old_source_names: Vec<String>,
        rows: &[NewApiKeyModelOverride],
    ) -> ApiKeyModelOverrideWriteSummary {
        ApiKeyModelOverrideWriteSummary {
            old_source_names,
            new_source_names: rows.iter().map(|row| row.source_name.clone()).collect(),
            override_count: rows.len(),
            enabled_override_count: rows.iter().filter(|row| row.is_enabled).count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CreateApiKeyWithOverridesResult {
    pub created: ApiKeyDetailWithSecret,
    pub override_summary: ApiKeyModelOverrideWriteSummary,
}

#[derive(Debug, Clone)]
pub struct UpdateApiKeyWithOverridesResult {
    pub updated: ApiKeyDetail,
    pub override_summary: ApiKeyModelOverrideWriteSummary,
}

#[derive(Debug, Clone)]
pub struct DeleteApiKeyWithOverridesResult {
    pub deleted: ApiKey,
    pub old_api_key_hash: String,
    pub override_summary: ApiKeyModelOverrideWriteSummary,
}

fn generate_api_key_secret() -> String {
    let random_part: String = rng()
        .sample_iter(&Alphanumeric)
        .take(48)
        .map(char::from)
        .collect();
    format!("cyder-{}", random_part)
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

fn normalize_required_name(field: &str, value: &str) -> DbResult<String> {
    let normalized = value.trim();
    if normalized.is_empty() {
        return Err(BaseError::ParamInvalid(Some(format!(
            "{field} must not be empty"
        ))));
    }
    Ok(normalized.to_string())
}

fn make_model_override_rows(
    api_key_id: i64,
    payloads: &[ApiKeyModelOverrideWriteInput],
    now: i64,
) -> DbResult<Vec<NewApiKeyModelOverride>> {
    let mut rows = Vec::with_capacity(payloads.len());
    for payload in payloads {
        rows.push(NewApiKeyModelOverride {
            id: ID_GENERATOR.generate_id(),
            api_key_id,
            source_name: normalize_required_name("source_name", &payload.source_name)?,
            target_route_id: payload.target_route_id,
            description: payload.description.clone(),
            is_enabled: payload.is_enabled.unwrap_or(true),
            created_at: now,
            updated_at: now,
        });
    }
    Ok(rows)
}

fn collect_source_names(existing: Vec<String>, created: Vec<String>) -> Vec<String> {
    existing
        .into_iter()
        .chain(created)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
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
                use crate::database::model_route::_postgres_model::*;
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
                use crate::database::model_route::_sqlite_model::*;
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

macro_rules! replace_api_key_model_overrides_in_tx {
    ($conn:ident, $api_key_id:expr, $override_rows:expr, $now:expr) => {{
        let override_rows = $override_rows;

        if !override_rows.is_empty() {
            let direct_model_names = provider::table
                .inner_join(model::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                .filter(
                    provider::dsl::deleted_at
                        .is_null()
                        .and(provider::dsl::is_enabled.eq(true))
                        .and(model::dsl::deleted_at.is_null())
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .select((provider::dsl::provider_key, model::dsl::model_name))
                .load::<(String, String)>($conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to load active direct provider/model names: {}",
                        e
                    )))
                })?;

            for override_row in override_rows {
                let source_name = &override_row.source_name;
                if direct_model_names.iter().any(|(provider_key, model_name)| {
                    format!("{provider_key}/{model_name}") == *source_name
                }) {
                    Err(BaseError::ParamInvalid(Some(format!(
                        "name '{}' conflicts with an active direct provider/model address",
                        source_name
                    ))))?;
                }
            }
        }

        let existing_rows = api_key_model_override::table
            .filter(
                api_key_model_override::dsl::api_key_id
                    .eq($api_key_id)
                    .and(api_key_model_override::dsl::deleted_at.is_null()),
            )
            .order(api_key_model_override::dsl::created_at.asc())
            .select(ApiKeyModelOverrideDb::as_select())
            .load::<ApiKeyModelOverrideDb>($conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list api key model overrides for {}: {}",
                    $api_key_id, e
                )))
            })?;
        let old_source_names = existing_rows
            .into_iter()
            .map(ApiKeyModelOverrideDb::from_db)
            .map(|override_row| override_row.source_name)
            .collect::<Vec<_>>();

        diesel::update(
            api_key_model_override::table.filter(
                api_key_model_override::dsl::api_key_id
                    .eq($api_key_id)
                    .and(api_key_model_override::dsl::deleted_at.is_null()),
            ),
        )
        .set((
            api_key_model_override::dsl::deleted_at.eq(Some($now)),
            api_key_model_override::dsl::is_enabled.eq(false),
            api_key_model_override::dsl::updated_at.eq($now),
        ))
        .execute($conn)
        .map_err(|e| {
            BaseError::DatabaseFatal(Some(format!(
                "Failed to replace api key model overrides for {}: {}",
                $api_key_id, e
            )))
        })?;

        for override_row in override_rows {
            model_route::table
                .filter(
                    model_route::dsl::id
                        .eq(override_row.target_route_id)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(model_route::dsl::id)
                .first::<i64>($conn)
                .optional()
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to fetch model route {}: {}",
                        override_row.target_route_id, e
                    )))
                })?
                .ok_or_else(|| {
                    BaseError::NotFound(Some(format!(
                        "Model route {} not found",
                        override_row.target_route_id
                    )))
                })?;

            let db_row = NewApiKeyModelOverrideDb::to_db(override_row);
            diesel::insert_into(api_key_model_override::table)
                .values(&db_row)
                .execute($conn)
                .map_err(|e| map_write_error("Failed to create api key model override", e))?;
        }

        Ok::<ApiKeyModelOverrideWriteSummary, BaseError>(
            ApiKeyModelOverrideWriteSummary::from_rows(old_source_names, override_rows),
        )
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
    }
}

fn build_reveal(row: &ApiKey) -> ApiKeyReveal {
    ApiKeyReveal {
        id: row.id,
        name: row.name.clone(),
        key_prefix: row.key_prefix.clone(),
        key_last4: row.key_last4.clone(),
        api_key: row.api_key.clone(),
        updated_at: row.updated_at,
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
    }
}

impl ApiKey {
    pub fn create_with_model_overrides(
        payload: &CreateApiKeyPayload,
        model_overrides: &[ApiKeyModelOverrideWriteInput],
    ) -> DbResult<CreateApiKeyWithOverridesResult> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        let secret = generate_api_key_secret();
        let new_key = NewApiKey {
            id: ID_GENERATOR.generate_id(),
            api_key: secret.clone(),
            api_key_hash: Some(hash_api_key(&secret)),
            key_prefix: key_prefix(&secret),
            key_last4: key_last4(&secret),
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
        let override_rows = make_model_override_rows(new_key.id, model_overrides, now)?;

        api_key_admin_db_execute!(conn, {
            conn.transaction::<CreateApiKeyWithOverridesResult, BaseError, _>(|conn| {
                let inserted = diesel::insert_into(api_key::table)
                    .values(NewApiKeyDb::to_db(&new_key))
                    .returning(ApiKeyDb::as_returning())
                    .get_result::<ApiKeyDb>(conn)
                    .map(ApiKeyDb::from_db)
                    .map_err(|e| map_write_error("Failed to create api key", e))?;

                insert_api_key_acl_rules_in_tx!(conn, inserted.id, &acl_rows)?;
                let override_summary =
                    replace_api_key_model_overrides_in_tx!(conn, inserted.id, &override_rows, now)?;
                let acl_rules = load_api_key_acl_rules_in_tx!(conn, inserted.id)?;

                Ok(CreateApiKeyWithOverridesResult {
                    created: ApiKeyDetailWithSecret {
                        detail: build_detail(&inserted, acl_rules),
                        reveal: build_reveal(&inserted),
                    },
                    override_summary,
                })
            })
        })
    }

    pub fn update_metadata_with_model_overrides(
        id_value: i64,
        payload: &UpdateApiKeyMetadataPayload,
        model_overrides: &[ApiKeyModelOverrideWriteInput],
    ) -> DbResult<UpdateApiKeyWithOverridesResult> {
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
        let override_rows = make_model_override_rows(id_value, model_overrides, now)?;

        api_key_admin_db_execute!(conn, {
            conn.transaction::<UpdateApiKeyWithOverridesResult, BaseError, _>(|conn| {
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
                let override_summary =
                    replace_api_key_model_overrides_in_tx!(conn, id_value, &override_rows, now)?;

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

                Ok(UpdateApiKeyWithOverridesResult {
                    updated: build_detail(&row, acl_rules),
                    override_summary,
                })
            })
        })
    }

    pub fn replace_model_overrides(
        id_value: i64,
        model_overrides: &[ApiKeyModelOverrideWriteInput],
    ) -> DbResult<ApiKeyModelOverrideWriteSummary> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        let override_rows = make_model_override_rows(id_value, model_overrides, now)?;

        api_key_admin_db_execute!(conn, {
            conn.transaction::<ApiKeyModelOverrideWriteSummary, BaseError, _>(|conn| {
                api_key::table
                    .filter(
                        api_key::dsl::id
                            .eq(id_value)
                            .and(api_key::dsl::deleted_at.is_null()),
                    )
                    .select(ApiKeyDb::as_select())
                    .first::<ApiKeyDb>(conn)
                    .map_err(|e| match e {
                        diesel::result::Error::NotFound => {
                            BaseError::NotFound(Some(format!("Api key {} not found", id_value)))
                        }
                        other => BaseError::DatabaseFatal(Some(format!(
                            "Failed to fetch api key {}: {}",
                            id_value, other
                        ))),
                    })?;

                replace_api_key_model_overrides_in_tx!(conn, id_value, &override_rows, now)
            })
        })
    }

    pub fn delete_with_model_overrides(id_value: i64) -> DbResult<DeleteApiKeyWithOverridesResult> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        let empty_override_rows = Vec::<NewApiKeyModelOverride>::new();

        api_key_admin_db_execute!(conn, {
            conn.transaction::<DeleteApiKeyWithOverridesResult, BaseError, _>(|conn| {
                let existing = api_key::table
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
                let old_api_key_hash = existing
                    .api_key_hash
                    .clone()
                    .unwrap_or_else(|| hash_api_key(&existing.api_key));

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

                let override_summary = replace_api_key_model_overrides_in_tx!(
                    conn,
                    id_value,
                    &empty_override_rows,
                    now
                )?;

                Ok(DeleteApiKeyWithOverridesResult {
                    deleted: existing,
                    old_api_key_hash,
                    override_summary,
                })
            })
        })
    }

    pub fn create(payload: &CreateApiKeyPayload) -> DbResult<ApiKeyDetailWithSecret> {
        Ok(Self::create_with_model_overrides(payload, &[])?.created)
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

        db_execute!(conn, {
            diesel::update(
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
            .map_err(|e| map_write_error(&format!("Failed to update api key {}", id_value), e))
        })?;

        if let Some(rules) = payload.acl_rules.as_ref() {
            ApiKeyAclRule::replace_for_api_key(id_value, rules)?;
        }

        Self::get_detail(id_value)
    }

    pub fn rotate_key(id_value: i64) -> DbResult<ApiKeyReveal> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        let secret = generate_api_key_secret();
        let rotated = db_execute!(conn, {
            diesel::update(
                api_key::table.filter(
                    api_key::dsl::id
                        .eq(id_value)
                        .and(api_key::dsl::deleted_at.is_null()),
                ),
            )
            .set((
                api_key::dsl::api_key_value.eq(secret.clone()),
                api_key::dsl::api_key_hash.eq(Some(hash_api_key(&secret))),
                api_key::dsl::key_prefix.eq(key_prefix(&secret)),
                api_key::dsl::key_last4.eq(key_last4(&secret)),
                api_key::dsl::updated_at.eq(now),
            ))
            .returning(ApiKeyDb::as_returning())
            .get_result::<ApiKeyDb>(conn)
            .map(ApiKeyDb::from_db)
            .map_err(|e| map_write_error(&format!("Failed to rotate api key {}", id_value), e))
        })?;

        Ok(build_reveal(&rotated))
    }

    pub fn reveal_key(id_value: i64) -> DbResult<ApiKeyReveal> {
        let api_key = Self::get_by_id(id_value)?;
        Ok(build_reveal(&api_key))
    }

    pub fn delete(id_value: i64) -> DbResult<usize> {
        Self::delete_with_model_overrides(id_value).map(|_| 1)
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
                        .eq(Some(api_key_hash_value.to_string()))
                        .and(api_key::dsl::deleted_at.is_null()),
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

    pub fn get_active_by_hash(api_key_hash_value: &str) -> DbResult<ApiKey> {
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();
        db_execute!(conn, {
            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(Some(api_key_hash_value.to_string()))
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
}
