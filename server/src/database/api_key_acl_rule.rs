use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbConnection, DbResult, get_connection};
use crate::controller::BaseError;
use crate::schema::enum_def::{Action, RuleScope};
use crate::utils::ID_GENERATOR;
use crate::{db_execute, db_object};

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Debug, Clone, Serialize)]
    #[diesel(table_name = api_key_acl_rule)]
    pub struct ApiKeyAclRule {
        pub id: i64,
        pub api_key_id: i64,
        pub effect: Action,
        pub scope: RuleScope,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub priority: i32,
        pub is_enabled: bool,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub deleted_at: Option<i64>,
    }

    #[derive(Insertable, Debug)]
    #[diesel(table_name = api_key_acl_rule)]
    pub struct NewApiKeyAclRule {
        pub id: i64,
        pub api_key_id: i64,
        pub effect: Action,
        pub scope: RuleScope,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub priority: i32,
        pub is_enabled: bool,
        pub description: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
        pub deleted_at: Option<i64>,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyAclRuleInput {
    pub id: Option<i64>,
    pub effect: Action,
    pub scope: RuleScope,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub priority: i32,
    pub is_enabled: Option<bool>,
    pub description: Option<String>,
}

fn validate_rule_input(rule: &ApiKeyAclRuleInput) -> DbResult<()> {
    match rule.scope {
        RuleScope::Provider => {
            if rule.provider_id.is_none() {
                return Err(BaseError::ParamInvalid(Some(
                    "provider-scoped ACL rule requires provider_id".to_string(),
                )));
            }
            if rule.model_id.is_some() {
                return Err(BaseError::ParamInvalid(Some(
                    "provider-scoped ACL rule must not set model_id".to_string(),
                )));
            }
        }
        RuleScope::Model => {
            if rule.model_id.is_none() {
                return Err(BaseError::ParamInvalid(Some(
                    "model-scoped ACL rule requires model_id".to_string(),
                )));
            }
        }
    }

    Ok(())
}

fn map_rule_inputs(
    api_key_id_value: i64,
    rules: &[ApiKeyAclRuleInput],
    now: i64,
) -> DbResult<Vec<NewApiKeyAclRule>> {
    let mut mapped = Vec::with_capacity(rules.len());

    for rule in rules {
        validate_rule_input(rule)?;
        mapped.push(NewApiKeyAclRule {
            id: rule.id.unwrap_or_else(|| ID_GENERATOR.generate_id()),
            api_key_id: api_key_id_value,
            effect: rule.effect.clone(),
            scope: rule.scope.clone(),
            provider_id: rule.provider_id,
            model_id: rule.model_id,
            priority: rule.priority,
            is_enabled: rule.is_enabled.unwrap_or(true),
            description: rule.description.clone(),
            created_at: now,
            updated_at: now,
            deleted_at: None,
        });
    }

    Ok(mapped)
}

fn list_by_api_key_id_with_conn(
    conn: &mut DbConnection,
    api_key_id_value: i64,
) -> DbResult<Vec<ApiKeyAclRule>> {
    db_execute!(conn, {
        let rows = api_key_acl_rule::table
            .filter(
                api_key_acl_rule::dsl::api_key_id
                    .eq(api_key_id_value)
                    .and(api_key_acl_rule::dsl::deleted_at.is_null()),
            )
            .order((
                api_key_acl_rule::dsl::priority.asc(),
                api_key_acl_rule::dsl::created_at.asc(),
                api_key_acl_rule::dsl::id.asc(),
            ))
            .select(ApiKeyAclRuleDb::as_select())
            .load::<ApiKeyAclRuleDb>(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to load api key ACL rules for {}: {}",
                    api_key_id_value, e
                )))
            })?;

        Ok(rows.into_iter().map(ApiKeyAclRuleDb::from_db).collect())
    })
}

pub(crate) fn replace_for_api_key_with_conn(
    conn: &mut DbConnection,
    api_key_id_value: i64,
    rules: &[ApiKeyAclRuleInput],
    now: i64,
) -> DbResult<Vec<ApiKeyAclRule>> {
    let mapped_rules = map_rule_inputs(api_key_id_value, rules, now)?;

    db_execute!(conn, {
        conn.transaction::<(), BaseError, _>(|conn| {
            diesel::delete(
                api_key_acl_rule::table
                    .filter(api_key_acl_rule::dsl::api_key_id.eq(api_key_id_value)),
            )
            .execute(conn)
            .map_err(|e| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to replace ACL rules for api key {}: {}",
                    api_key_id_value, e
                )))
            })?;

            if !mapped_rules.is_empty() {
                let db_rows: Vec<_> = mapped_rules.iter().map(NewApiKeyAclRuleDb::to_db).collect();

                diesel::insert_into(api_key_acl_rule::table)
                    .values(&db_rows)
                    .execute(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to insert ACL rules for api key {}: {}",
                            api_key_id_value, e
                        )))
                    })?;
            }

            Ok(())
        })
    })?;

    list_by_api_key_id_with_conn(conn, api_key_id_value)
}

impl ApiKeyAclRule {
    pub fn list_by_api_key_id(api_key_id_value: i64) -> DbResult<Vec<ApiKeyAclRule>> {
        let conn = &mut get_connection()?;
        list_by_api_key_id_with_conn(conn, api_key_id_value)
    }

    pub fn replace_for_api_key(
        api_key_id_value: i64,
        rules: &[ApiKeyAclRuleInput],
    ) -> DbResult<Vec<ApiKeyAclRule>> {
        let conn = &mut get_connection()?;
        replace_for_api_key_with_conn(conn, api_key_id_value, rules, Utc::now().timestamp_millis())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_rule_without_provider_id_is_invalid() {
        let err = validate_rule_input(&ApiKeyAclRuleInput {
            id: None,
            effect: Action::Allow,
            scope: RuleScope::Provider,
            provider_id: None,
            model_id: None,
            priority: 0,
            is_enabled: None,
            description: None,
        })
        .expect_err("provider rule should require provider_id");

        assert!(matches!(err, BaseError::ParamInvalid(_)));
    }

    #[test]
    fn model_rule_without_model_id_is_invalid() {
        let err = validate_rule_input(&ApiKeyAclRuleInput {
            id: None,
            effect: Action::Deny,
            scope: RuleScope::Model,
            provider_id: Some(1),
            model_id: None,
            priority: 10,
            is_enabled: Some(false),
            description: None,
        })
        .expect_err("model rule should require model_id");

        assert!(matches!(err, BaseError::ParamInvalid(_)));
    }

    #[test]
    fn provider_rule_with_model_id_is_invalid() {
        let err = validate_rule_input(&ApiKeyAclRuleInput {
            id: None,
            effect: Action::Allow,
            scope: RuleScope::Provider,
            provider_id: Some(1),
            model_id: Some(99),
            priority: 1,
            is_enabled: Some(true),
            description: None,
        })
        .expect_err("provider rule must not carry model_id");

        assert!(matches!(err, BaseError::ParamInvalid(_)));
    }
}
