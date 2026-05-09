use diesel::prelude::*;

use crate::{
    controller::BaseError,
    database::{
        DbResult,
        api_key::{ApiKey, NewApiKey, UpdateApiKeyData, hash_api_key, key_last4, key_prefix},
        api_key_acl_rule::{ApiKeyAclRule, NewApiKeyAclRule},
        model_route::{ApiKeyModelOverride, NewApiKeyModelOverride},
    },
    schema::enum_def::Action,
    service::portable_config::schema::PortableModelRef,
    utils::ID_GENERATOR,
};

use super::{PortableRepositoryConnection, map_write_error};

#[derive(Debug, Clone)]
pub(crate) struct RawApiKeyImportInput {
    pub raw_api_key: String,
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
    pub now: i64,
}

impl RawApiKeyImportInput {
    #[cfg(test)]
    fn test(raw_api_key: &str, name: &str, now: i64) -> Self {
        Self {
            raw_api_key: raw_api_key.to_string(),
            name: name.to_string(),
            description: None,
            default_action: Action::Allow,
            is_enabled: true,
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: None,
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            now,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ExportApiKeyAclRule {
    pub rule: ApiKeyAclRule,
    pub provider_ref: Option<String>,
    pub model_ref: Option<PortableModelRef>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExportApiKeyModelOverride {
    pub row: ApiKeyModelOverride,
    pub target_route_ref: String,
}

pub(crate) fn list_api_keys_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
) -> DbResult<Vec<ApiKey>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::ApiKeyDb;

            api_key::table
                .filter(api_key::dsl::deleted_at.is_null())
                .order((api_key::dsl::created_at.asc(), api_key::dsl::id.asc()))
                .select(ApiKeyDb::as_select())
                .load::<ApiKeyDb>(*conn)
                .map(|rows| rows.into_iter().map(ApiKeyDb::from_db).collect())
                .map_err(|err| map_write_error("Failed to list api keys for portable export", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::ApiKeyDb;

            api_key::table
                .filter(api_key::dsl::deleted_at.is_null())
                .order((api_key::dsl::created_at.asc(), api_key::dsl::id.asc()))
                .select(ApiKeyDb::as_select())
                .load::<ApiKeyDb>(*conn)
                .map(|rows| rows.into_iter().map(ApiKeyDb::from_db).collect())
                .map_err(|err| map_write_error("Failed to list api keys for portable export", err))
        }
    }
}

pub(crate) fn find_active_api_key_by_raw_key(
    conn: &mut PortableRepositoryConnection<'_>,
    raw_api_key: &str,
) -> DbResult<Option<ApiKey>> {
    let api_key_hash = hash_api_key(raw_api_key);
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::ApiKeyDb;

            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(Some(api_key_hash))
                        .and(api_key::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(*conn)
                .optional()
                .map(|row| row.map(ApiKeyDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup api key by hash", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::ApiKeyDb;

            api_key::table
                .filter(
                    api_key::dsl::api_key_hash
                        .eq(Some(api_key_hash))
                        .and(api_key::dsl::deleted_at.is_null()),
                )
                .select(ApiKeyDb::as_select())
                .first::<ApiKeyDb>(*conn)
                .optional()
                .map(|row| row.map(ApiKeyDb::from_db))
                .map_err(|err| map_write_error("Failed to lookup api key by hash", err))
        }
    }
}

pub(crate) fn list_api_key_acl_rules_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    api_key_id: i64,
) -> DbResult<Vec<ExportApiKeyAclRule>> {
    let rules: Vec<ApiKeyAclRule> = match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key_acl_rule;
            use crate::database::api_key_acl_rule::_postgres_model::ApiKeyAclRuleDb;

            api_key_acl_rule::table
                .filter(
                    api_key_acl_rule::dsl::api_key_id
                        .eq(api_key_id)
                        .and(api_key_acl_rule::dsl::deleted_at.is_null()),
                )
                .order((
                    api_key_acl_rule::dsl::priority.asc(),
                    api_key_acl_rule::dsl::created_at.asc(),
                    api_key_acl_rule::dsl::id.asc(),
                ))
                .select(ApiKeyAclRuleDb::as_select())
                .load::<ApiKeyAclRuleDb>(*conn)
                .map(|rows| rows.into_iter().map(ApiKeyAclRuleDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list api key ACL rules for portable export", err)
                })?
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key_acl_rule;
            use crate::database::api_key_acl_rule::_sqlite_model::ApiKeyAclRuleDb;

            api_key_acl_rule::table
                .filter(
                    api_key_acl_rule::dsl::api_key_id
                        .eq(api_key_id)
                        .and(api_key_acl_rule::dsl::deleted_at.is_null()),
                )
                .order((
                    api_key_acl_rule::dsl::priority.asc(),
                    api_key_acl_rule::dsl::created_at.asc(),
                    api_key_acl_rule::dsl::id.asc(),
                ))
                .select(ApiKeyAclRuleDb::as_select())
                .load::<ApiKeyAclRuleDb>(*conn)
                .map(|rows| rows.into_iter().map(ApiKeyAclRuleDb::from_db).collect())
                .map_err(|err| {
                    map_write_error("Failed to list api key ACL rules for portable export", err)
                })?
        }
    };

    let mut exported = Vec::with_capacity(rules.len());
    for rule in rules {
        let provider_ref = match rule.provider_id {
            Some(provider_id) => find_provider_key_by_id(conn, provider_id)?,
            None => None,
        };
        let model_ref = match rule.model_id {
            Some(model_id) => find_model_ref_by_id(conn, model_id)?,
            None => None,
        };
        exported.push(ExportApiKeyAclRule {
            rule,
            provider_ref,
            model_ref,
        });
    }
    Ok(exported)
}

pub(crate) fn list_api_key_model_overrides_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    api_key_id: i64,
) -> DbResult<Vec<ExportApiKeyModelOverride>> {
    let rows: Vec<ApiKeyModelOverride> = match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key_model_override;
            use crate::database::model_route::_postgres_model::ApiKeyModelOverrideDb;

            api_key_model_override::table
                .filter(
                    api_key_model_override::dsl::api_key_id
                        .eq(api_key_id)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                )
                .order((
                    api_key_model_override::dsl::created_at.asc(),
                    api_key_model_override::dsl::id.asc(),
                ))
                .select(ApiKeyModelOverrideDb::as_select())
                .load::<ApiKeyModelOverrideDb>(*conn)
                .map(|rows| {
                    rows.into_iter()
                        .map(ApiKeyModelOverrideDb::from_db)
                        .collect()
                })
                .map_err(|err| {
                    map_write_error(
                        "Failed to list api key model overrides for portable export",
                        err,
                    )
                })?
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key_model_override;
            use crate::database::model_route::_sqlite_model::ApiKeyModelOverrideDb;

            api_key_model_override::table
                .filter(
                    api_key_model_override::dsl::api_key_id
                        .eq(api_key_id)
                        .and(api_key_model_override::dsl::deleted_at.is_null()),
                )
                .order((
                    api_key_model_override::dsl::created_at.asc(),
                    api_key_model_override::dsl::id.asc(),
                ))
                .select(ApiKeyModelOverrideDb::as_select())
                .load::<ApiKeyModelOverrideDb>(*conn)
                .map(|rows| {
                    rows.into_iter()
                        .map(ApiKeyModelOverrideDb::from_db)
                        .collect()
                })
                .map_err(|err| {
                    map_write_error(
                        "Failed to list api key model overrides for portable export",
                        err,
                    )
                })?
        }
    };

    let mut exported = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some(target_route_ref) = find_route_name_by_id(conn, row.target_route_id)? {
            exported.push(ExportApiKeyModelOverride {
                row,
                target_route_ref,
            });
        }
    }
    Ok(exported)
}

pub(crate) fn insert_raw_api_key(
    conn: &mut PortableRepositoryConnection<'_>,
    input: &RawApiKeyImportInput,
) -> DbResult<ApiKey> {
    validate_raw_api_key_input(input)?;
    let new_key = NewApiKey {
        id: ID_GENERATOR.generate_id(),
        api_key: input.raw_api_key.clone(),
        api_key_hash: Some(hash_api_key(&input.raw_api_key)),
        key_prefix: key_prefix(&input.raw_api_key),
        key_last4: key_last4(&input.raw_api_key),
        name: input.name.trim().to_string(),
        description: input.description.clone(),
        default_action: input.default_action.clone(),
        is_enabled: input.is_enabled,
        expires_at: input.expires_at,
        rate_limit_rpm: input.rate_limit_rpm,
        max_concurrent_requests: input.max_concurrent_requests,
        quota_daily_requests: input.quota_daily_requests,
        quota_daily_tokens: input.quota_daily_tokens,
        quota_monthly_tokens: input.quota_monthly_tokens,
        budget_daily_nanos: input.budget_daily_nanos,
        budget_daily_currency: input.budget_daily_currency.clone(),
        budget_monthly_nanos: input.budget_monthly_nanos,
        budget_monthly_currency: input.budget_monthly_currency.clone(),
        deleted_at: None,
        created_at: input.now,
        updated_at: input.now,
    };

    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::{ApiKeyDb, NewApiKeyDb};

            diesel::insert_into(api_key::table)
                .values(NewApiKeyDb::to_db(&new_key))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to import raw api key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::{ApiKeyDb, NewApiKeyDb};

            diesel::insert_into(api_key::table)
                .values(NewApiKeyDb::to_db(&new_key))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to import raw api key", err))
        }
    }
}

pub(crate) fn update_api_key_metadata(
    conn: &mut PortableRepositoryConnection<'_>,
    api_key_id: i64,
    data: &UpdateApiKeyData,
    updated_at: i64,
) -> DbResult<ApiKey> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key;
            use crate::database::api_key::_postgres_model::{ApiKeyDb, UpdateApiKeyDataDb};

            diesel::update(api_key::table.find(api_key_id))
                .set((
                    UpdateApiKeyDataDb::to_db(data),
                    api_key::dsl::updated_at.eq(updated_at),
                ))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported api key", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key;
            use crate::database::api_key::_sqlite_model::{ApiKeyDb, UpdateApiKeyDataDb};

            diesel::update(api_key::table.find(api_key_id))
                .set((
                    UpdateApiKeyDataDb::to_db(data),
                    api_key::dsl::updated_at.eq(updated_at),
                ))
                .returning(ApiKeyDb::as_returning())
                .get_result::<ApiKeyDb>(*conn)
                .map(ApiKeyDb::from_db)
                .map_err(|err| map_write_error("Failed to update imported api key", err))
        }
    }
}

pub(crate) fn insert_api_key_acl_rule(
    conn: &mut PortableRepositoryConnection<'_>,
    row: &NewApiKeyAclRule,
) -> DbResult<ApiKeyAclRule> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::api_key_acl_rule;
            use crate::database::api_key_acl_rule::_postgres_model::{
                ApiKeyAclRuleDb, NewApiKeyAclRuleDb,
            };

            diesel::insert_into(api_key_acl_rule::table)
                .values(NewApiKeyAclRuleDb::to_db(row))
                .returning(ApiKeyAclRuleDb::as_returning())
                .get_result::<ApiKeyAclRuleDb>(*conn)
                .map(ApiKeyAclRuleDb::from_db)
                .map_err(|err| map_write_error("Failed to import api key ACL rule", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::api_key_acl_rule;
            use crate::database::api_key_acl_rule::_sqlite_model::{
                ApiKeyAclRuleDb, NewApiKeyAclRuleDb,
            };

            diesel::insert_into(api_key_acl_rule::table)
                .values(NewApiKeyAclRuleDb::to_db(row))
                .returning(ApiKeyAclRuleDb::as_returning())
                .get_result::<ApiKeyAclRuleDb>(*conn)
                .map(ApiKeyAclRuleDb::from_db)
                .map_err(|err| map_write_error("Failed to import api key ACL rule", err))
        }
    }
}

pub(crate) fn insert_api_key_model_override(
    conn: &mut PortableRepositoryConnection<'_>,
    row: &NewApiKeyModelOverride,
) -> DbResult<ApiKeyModelOverride> {
    validate_model_override_source_name(conn, &row.source_name)?;
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::{api_key_model_override, model_route};
            use crate::database::model_route::_postgres_model::{
                ApiKeyModelOverrideDb, NewApiKeyModelOverrideDb,
            };

            model_route::table
                .filter(
                    model_route::dsl::id
                        .eq(row.target_route_id)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(model_route::dsl::id)
                .first::<i64>(*conn)
                .optional()
                .map_err(|err| map_write_error("Failed to lookup model route for override", err))?
                .ok_or_else(|| {
                    BaseError::NotFound(Some(format!(
                        "Model route {} not found",
                        row.target_route_id
                    )))
                })?;

            diesel::insert_into(api_key_model_override::table)
                .values(NewApiKeyModelOverrideDb::to_db(row))
                .returning(ApiKeyModelOverrideDb::as_returning())
                .get_result::<ApiKeyModelOverrideDb>(*conn)
                .map(ApiKeyModelOverrideDb::from_db)
                .map_err(|err| map_write_error("Failed to import api key model override", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::{api_key_model_override, model_route};
            use crate::database::model_route::_sqlite_model::{
                ApiKeyModelOverrideDb, NewApiKeyModelOverrideDb,
            };

            model_route::table
                .filter(
                    model_route::dsl::id
                        .eq(row.target_route_id)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(model_route::dsl::id)
                .first::<i64>(*conn)
                .optional()
                .map_err(|err| map_write_error("Failed to lookup model route for override", err))?
                .ok_or_else(|| {
                    BaseError::NotFound(Some(format!(
                        "Model route {} not found",
                        row.target_route_id
                    )))
                })?;

            diesel::insert_into(api_key_model_override::table)
                .values(NewApiKeyModelOverrideDb::to_db(row))
                .returning(ApiKeyModelOverrideDb::as_returning())
                .get_result::<ApiKeyModelOverrideDb>(*conn)
                .map(ApiKeyModelOverrideDb::from_db)
                .map_err(|err| map_write_error("Failed to import api key model override", err))
        }
    }
}

fn validate_raw_api_key_input(input: &RawApiKeyImportInput) -> DbResult<()> {
    if input.raw_api_key.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "raw api key must not be empty".to_string(),
        )));
    }
    if input.name.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "api key name must not be empty".to_string(),
        )));
    }
    Ok(())
}

fn validate_model_override_source_name(
    conn: &mut PortableRepositoryConnection<'_>,
    source_name: &str,
) -> DbResult<()> {
    if source_name.trim().is_empty() {
        return Err(BaseError::ParamInvalid(Some(
            "model override source_name must not be empty".to_string(),
        )));
    }

    let direct_model_names = match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::{model, provider};

            provider::table
                .inner_join(model::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                .filter(
                    provider::dsl::deleted_at
                        .is_null()
                        .and(provider::dsl::is_enabled.eq(true))
                        .and(model::dsl::deleted_at.is_null())
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .select((provider::dsl::provider_key, model::dsl::model_name))
                .load::<(String, String)>(*conn)
                .map_err(|err| map_write_error("Failed to load direct provider/model names", err))?
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::{model, provider};

            provider::table
                .inner_join(model::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                .filter(
                    provider::dsl::deleted_at
                        .is_null()
                        .and(provider::dsl::is_enabled.eq(true))
                        .and(model::dsl::deleted_at.is_null())
                        .and(model::dsl::is_enabled.eq(true)),
                )
                .select((provider::dsl::provider_key, model::dsl::model_name))
                .load::<(String, String)>(*conn)
                .map_err(|err| map_write_error("Failed to load direct provider/model names", err))?
        }
    };

    if direct_model_names
        .iter()
        .any(|(provider_key, model_name)| format!("{provider_key}/{model_name}") == source_name)
    {
        return Err(BaseError::ParamInvalid(Some(format!(
            "name '{}' conflicts with an active direct provider/model address",
            source_name
        ))));
    }

    Ok(())
}

fn find_provider_key_by_id(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
) -> DbResult<Option<String>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::provider;

            provider::table
                .filter(
                    provider::dsl::id
                        .eq(provider_id)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select(provider::dsl::provider_key)
                .first::<String>(*conn)
                .optional()
                .map_err(|err| {
                    map_write_error("Failed to lookup provider ref for portable export", err)
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::provider;

            provider::table
                .filter(
                    provider::dsl::id
                        .eq(provider_id)
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select(provider::dsl::provider_key)
                .first::<String>(*conn)
                .optional()
                .map_err(|err| {
                    map_write_error("Failed to lookup provider ref for portable export", err)
                })
        }
    }
}

fn find_model_ref_by_id(
    conn: &mut PortableRepositoryConnection<'_>,
    model_id: i64,
) -> DbResult<Option<PortableModelRef>> {
    let row = match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::{model, provider};

            model::table
                .inner_join(provider::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                .filter(
                    model::dsl::id
                        .eq(model_id)
                        .and(model::dsl::deleted_at.is_null())
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select((provider::dsl::provider_key, model::dsl::model_name))
                .first::<(String, String)>(*conn)
                .optional()
                .map_err(|err| {
                    map_write_error("Failed to lookup model ref for portable export", err)
                })?
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::{model, provider};

            model::table
                .inner_join(provider::table.on(model::dsl::provider_id.eq(provider::dsl::id)))
                .filter(
                    model::dsl::id
                        .eq(model_id)
                        .and(model::dsl::deleted_at.is_null())
                        .and(provider::dsl::deleted_at.is_null()),
                )
                .select((provider::dsl::provider_key, model::dsl::model_name))
                .first::<(String, String)>(*conn)
                .optional()
                .map_err(|err| {
                    map_write_error("Failed to lookup model ref for portable export", err)
                })?
        }
    };

    Ok(row.map(|(provider_key, model_name)| PortableModelRef {
        provider_key,
        model_name,
    }))
}

fn find_route_name_by_id(
    conn: &mut PortableRepositoryConnection<'_>,
    route_id: i64,
) -> DbResult<Option<String>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::model_route;

            model_route::table
                .filter(
                    model_route::dsl::id
                        .eq(route_id)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(model_route::dsl::route_name)
                .first::<String>(*conn)
                .optional()
                .map_err(|err| {
                    map_write_error("Failed to lookup route ref for portable export", err)
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::model_route;

            model_route::table
                .filter(
                    model_route::dsl::id
                        .eq(route_id)
                        .and(model_route::dsl::deleted_at.is_null()),
                )
                .select(model_route::dsl::route_name)
                .first::<String>(*conn)
                .optional()
                .map_err(|err| {
                    map_write_error("Failed to lookup route ref for portable export", err)
                })
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        controller::BaseError,
        database::{
            TestDbContext,
            api_key::{ApiKey, hash_api_key},
            get_connection,
        },
        service::admin::portable_config::repository::with_transaction,
    };

    use super::{RawApiKeyImportInput, insert_raw_api_key};

    #[test]
    fn raw_api_key_import_writes_secret_and_shadow_fields_in_transaction() {
        let test_db_context = TestDbContext::new_sqlite("portable-raw-api-key-import.sqlite");

        test_db_context.run_sync(|| {
            let raw_key = "cyder-imported-secret-1234567890";
            let mut conn = get_connection().expect("connection");
            let inserted = with_transaction(&mut conn, |tx| {
                insert_raw_api_key(tx, &RawApiKeyImportInput::test(raw_key, "imported", 1000))
            })
            .expect("raw api key import should commit");

            assert_eq!(inserted.api_key, raw_key);
            let expected_hash = hash_api_key(raw_key);
            assert_eq!(
                inserted.api_key_hash.as_deref(),
                Some(expected_hash.as_str())
            );
            assert_eq!(inserted.key_prefix, "cyder-import");
            assert_eq!(inserted.key_last4, "7890");

            let loaded = ApiKey::get_by_hash(&expected_hash).expect("imported key should load");
            assert_eq!(loaded.id, inserted.id);
            assert_eq!(loaded.api_key, raw_key);
        });
    }

    #[test]
    fn raw_api_key_import_rolls_back_on_later_error() {
        let test_db_context = TestDbContext::new_sqlite("portable-raw-api-key-rollback.sqlite");

        test_db_context.run_sync(|| {
            let raw_key = "cyder-rollback-secret-1234567890";
            let mut conn = get_connection().expect("connection");
            let result = with_transaction(&mut conn, |tx| {
                let _inserted =
                    insert_raw_api_key(tx, &RawApiKeyImportInput::test(raw_key, "rollback", 1000))?;
                Err::<(), BaseError>(BaseError::ParamInvalid(Some(
                    "forced portable import failure".to_string(),
                )))
            });

            assert!(matches!(result, Err(BaseError::ParamInvalid(_))));
            assert!(matches!(
                ApiKey::get_by_hash(&hash_api_key(raw_key)),
                Err(BaseError::NotFound(_))
            ));
        });
    }
}
