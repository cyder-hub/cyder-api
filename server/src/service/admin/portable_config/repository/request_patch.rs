use diesel::prelude::*;

use crate::database::{
    DbResult,
    request_patch::{NewRequestPatchRule, RequestPatchRule},
};

use super::{PortableRepositoryConnection, map_write_error};

pub(crate) fn list_provider_request_patch_rules_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    provider_id: i64,
) -> DbResult<Vec<RequestPatchRule>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::request_patch_rule;
            use crate::database::request_patch::_postgres_model::RequestPatchRuleDb;

            request_patch_rule::table
                .filter(
                    request_patch_rule::dsl::provider_id
                        .eq(provider_id)
                        .and(request_patch_rule::dsl::model_id.is_null())
                        .and(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .order((
                    request_patch_rule::dsl::created_at.asc(),
                    request_patch_rule::dsl::id.asc(),
                ))
                .select(RequestPatchRuleDb::as_select())
                .load::<RequestPatchRuleDb>(*conn)
                .map(|rows| rows.into_iter().map(RequestPatchRuleDb::from_db).collect())
                .map_err(|err| {
                    map_write_error(
                        "Failed to list provider request patches for portable export",
                        err,
                    )
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::request_patch_rule;
            use crate::database::request_patch::_sqlite_model::RequestPatchRuleDb;

            request_patch_rule::table
                .filter(
                    request_patch_rule::dsl::provider_id
                        .eq(provider_id)
                        .and(request_patch_rule::dsl::model_id.is_null())
                        .and(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .order((
                    request_patch_rule::dsl::created_at.asc(),
                    request_patch_rule::dsl::id.asc(),
                ))
                .select(RequestPatchRuleDb::as_select())
                .load::<RequestPatchRuleDb>(*conn)
                .map(|rows| rows.into_iter().map(RequestPatchRuleDb::from_db).collect())
                .map_err(|err| {
                    map_write_error(
                        "Failed to list provider request patches for portable export",
                        err,
                    )
                })
        }
    }
}

pub(crate) fn list_model_request_patch_rules_for_export(
    conn: &mut PortableRepositoryConnection<'_>,
    model_id: i64,
) -> DbResult<Vec<RequestPatchRule>> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::request_patch_rule;
            use crate::database::request_patch::_postgres_model::RequestPatchRuleDb;

            request_patch_rule::table
                .filter(
                    request_patch_rule::dsl::model_id
                        .eq(model_id)
                        .and(request_patch_rule::dsl::provider_id.is_null())
                        .and(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .order((
                    request_patch_rule::dsl::created_at.asc(),
                    request_patch_rule::dsl::id.asc(),
                ))
                .select(RequestPatchRuleDb::as_select())
                .load::<RequestPatchRuleDb>(*conn)
                .map(|rows| rows.into_iter().map(RequestPatchRuleDb::from_db).collect())
                .map_err(|err| {
                    map_write_error(
                        "Failed to list model request patches for portable export",
                        err,
                    )
                })
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::request_patch_rule;
            use crate::database::request_patch::_sqlite_model::RequestPatchRuleDb;

            request_patch_rule::table
                .filter(
                    request_patch_rule::dsl::model_id
                        .eq(model_id)
                        .and(request_patch_rule::dsl::provider_id.is_null())
                        .and(request_patch_rule::dsl::deleted_at.is_null()),
                )
                .order((
                    request_patch_rule::dsl::created_at.asc(),
                    request_patch_rule::dsl::id.asc(),
                ))
                .select(RequestPatchRuleDb::as_select())
                .load::<RequestPatchRuleDb>(*conn)
                .map(|rows| rows.into_iter().map(RequestPatchRuleDb::from_db).collect())
                .map_err(|err| {
                    map_write_error(
                        "Failed to list model request patches for portable export",
                        err,
                    )
                })
        }
    }
}

pub(crate) fn insert_request_patch_rule(
    conn: &mut PortableRepositoryConnection<'_>,
    new_rule: &NewRequestPatchRule,
) -> DbResult<RequestPatchRule> {
    match conn {
        PortableRepositoryConnection::Postgres(conn) => {
            use crate::database::_postgres_schema::request_patch_rule;
            use crate::database::request_patch::_postgres_model::{
                NewRequestPatchRuleDb, RequestPatchRuleDb,
            };

            diesel::insert_into(request_patch_rule::table)
                .values(NewRequestPatchRuleDb::to_db(new_rule))
                .returning(RequestPatchRuleDb::as_returning())
                .get_result::<RequestPatchRuleDb>(*conn)
                .map(RequestPatchRuleDb::from_db)
                .map_err(|err| map_write_error("Failed to import request patch rule", err))
        }
        PortableRepositoryConnection::Sqlite(conn) => {
            use crate::database::_sqlite_schema::request_patch_rule;
            use crate::database::request_patch::_sqlite_model::{
                NewRequestPatchRuleDb, RequestPatchRuleDb,
            };

            diesel::insert_into(request_patch_rule::table)
                .values(NewRequestPatchRuleDb::to_db(new_rule))
                .returning(RequestPatchRuleDb::as_returning())
                .get_result::<RequestPatchRuleDb>(*conn)
                .map(RequestPatchRuleDb::from_db)
                .map_err(|err| map_write_error("Failed to import request patch rule", err))
        }
    }
}
