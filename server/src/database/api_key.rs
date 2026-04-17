use chrono::Utc;
use diesel::prelude::*;
use rand::{Rng, distr::Alphanumeric, rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    DbResult,
    api_key_acl_rule::{ApiKeyAclRule, ApiKeyAclRuleInput},
    get_connection,
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

fn map_write_error(context: &str, e: diesel::result::Error) -> BaseError {
    match e {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(context.to_string())),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
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
    pub fn create(payload: &CreateApiKeyPayload) -> DbResult<ApiKeyDetailWithSecret> {
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

        let inserted = db_execute!(conn, {
            diesel::insert_into(api_key::table)
                .values(NewApiKeyDb::to_db(&new_key))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(conn)
                .map(ApiKeyDb::from_db)
                .map_err(|e| map_write_error("Failed to create api key", e))
        })?;

        if let Some(rules) = payload.acl_rules.as_ref() {
            ApiKeyAclRule::replace_for_api_key(inserted.id, rules)?;
        }

        let detail = Self::get_detail(inserted.id)?;
        Ok(ApiKeyDetailWithSecret {
            detail,
            reveal: build_reveal(&inserted),
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
        let conn = &mut get_connection()?;
        let now = Utc::now().timestamp_millis();

        db_execute!(conn, {
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

                Ok(updated)
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
