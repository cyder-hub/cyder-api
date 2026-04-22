use diesel::{Connection, prelude::*};
use serde::{Deserialize, Serialize};

use super::{DbConnection, DbResult, ListResult, get_connection};
use crate::controller::BaseError;
use crate::database::request_attempt::RequestAttempt;
use crate::schema::enum_def::{LlmApiType, RequestStatus, StorageType};
use crate::{db_execute, db_object};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, AsChangeset, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLog {
        pub id: i64,
        pub api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub resolved_name_scope: Option<String>,
        pub resolved_route_id: Option<i64>,
        pub resolved_route_name: Option<String>,
        pub user_api_type: LlmApiType,
        #[diesel(column_name = status)]
        pub overall_status: RequestStatus,
        pub final_error_code: Option<String>,
        pub final_error_message: Option<String>,
        pub attempt_count: i32,
        pub retry_count: i32,
        pub fallback_count: i32,
        pub request_received_at: i64,
        #[diesel(column_name = llm_request_sent_at)]
        pub first_attempt_started_at: Option<i64>,
        #[diesel(column_name = llm_response_first_chunk_at)]
        pub response_started_to_client_at: Option<i64>,
        #[diesel(column_name = llm_response_completed_at)]
        pub completed_at: Option<i64>,
        pub client_ip: Option<String>,
        pub final_attempt_id: Option<i64>,
        #[diesel(column_name = provider_id)]
        pub final_provider_id: Option<i64>,
        #[diesel(column_name = provider_api_key_id)]
        pub final_provider_api_key_id: Option<i64>,
        #[diesel(column_name = model_id)]
        pub final_model_id: Option<i64>,
        pub final_provider_key_snapshot: Option<String>,
        pub final_provider_name_snapshot: Option<String>,
        #[diesel(column_name = model_name)]
        pub final_model_name_snapshot: Option<String>,
        #[diesel(column_name = real_model_name)]
        pub final_real_model_name_snapshot: Option<String>,
        #[diesel(column_name = llm_api_type)]
        pub final_llm_api_type: Option<LlmApiType>,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub cost_catalog_id: Option<i64>,
        pub cost_catalog_version_id: Option<i64>,
        pub cost_snapshot_json: Option<String>,
        pub total_input_tokens: Option<i32>,
        pub total_output_tokens: Option<i32>,
        pub input_text_tokens: Option<i32>,
        pub output_text_tokens: Option<i32>,
        pub input_image_tokens: Option<i32>,
        pub output_image_tokens: Option<i32>,
        pub cache_read_tokens: Option<i32>,
        pub cache_write_tokens: Option<i32>,
        pub reasoning_tokens: Option<i32>,
        pub total_tokens: Option<i32>,
        pub bundle_version: Option<i32>,
        #[diesel(column_name = storage_type)]
        pub bundle_storage_type: Option<StorageType>,
        pub bundle_storage_key: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLogListItem {
        pub id: i64,
        pub api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub resolved_name_scope: Option<String>,
        pub resolved_route_name: Option<String>,
        #[diesel(column_name = status)]
        pub overall_status: RequestStatus,
        pub attempt_count: i32,
        pub retry_count: i32,
        pub fallback_count: i32,
        pub request_received_at: i64,
        #[diesel(column_name = llm_request_sent_at)]
        pub first_attempt_started_at: Option<i64>,
        #[diesel(column_name = llm_response_first_chunk_at)]
        pub response_started_to_client_at: Option<i64>,
        #[diesel(column_name = llm_response_completed_at)]
        pub completed_at: Option<i64>,
        #[diesel(column_name = provider_id)]
        pub final_provider_id: Option<i64>,
        pub final_provider_name_snapshot: Option<String>,
        #[diesel(column_name = model_id)]
        pub final_model_id: Option<i64>,
        #[diesel(column_name = model_name)]
        pub final_model_name_snapshot: Option<String>,
        #[diesel(column_name = real_model_name)]
        pub final_real_model_name_snapshot: Option<String>,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub total_input_tokens: Option<i32>,
        pub total_output_tokens: Option<i32>,
        pub reasoning_tokens: Option<i32>,
        pub total_tokens: Option<i32>,
    }
}

pub type RequestLogRecord = RequestLog;

#[derive(Deserialize, Debug, Default)]
pub struct RequestLogQueryPayload {
    pub api_key_id: Option<i64>,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub status: Option<RequestStatus>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub search: Option<String>,
}

impl RequestLog {
    pub fn insert(new_log_data: &RequestLog) -> DbResult<RequestLog> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            conn.transaction::<RequestLog, BaseError, _>(|conn| {
                let inserted_log_db = diesel::insert_into(request_log::table)
                    .values(RequestLogDb::to_db(new_log_data))
                    .returning(RequestLogDb::as_returning())
                    .get_result::<RequestLogDb>(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to insert request log: {}",
                            e
                        )))
                    })?;

                if let Some(cost_catalog_version_id) = new_log_data.cost_catalog_version_id {
                    diesel::update(
                        cost_catalog_versions::table.filter(
                            cost_catalog_versions::dsl::id
                                .eq(cost_catalog_version_id)
                                .and(cost_catalog_versions::dsl::first_used_at.is_null()),
                        ),
                    )
                    .set((
                        cost_catalog_versions::dsl::first_used_at
                            .eq(Some(new_log_data.request_received_at)),
                        cost_catalog_versions::dsl::updated_at.eq(new_log_data.updated_at),
                    ))
                    .execute(conn)
                    .map_err(|e| {
                        BaseError::DatabaseFatal(Some(format!(
                            "Failed to freeze cost catalog version {} after request log insert: {}",
                            cost_catalog_version_id, e
                        )))
                    })?;
                }

                Ok(inserted_log_db.from_db())
            })
        })
    }

    pub fn insert_with_attempts(
        new_log_data: &RequestLog,
        new_attempts: &[RequestAttempt],
    ) -> DbResult<RequestLog> {
        let conn = &mut get_connection()?;
        match conn {
            DbConnection::Postgres(conn) => {
                use crate::database::_postgres_schema::{
                    cost_catalog_versions, request_attempt, request_log,
                };
                use crate::database::request_attempt::_postgres_model::RequestAttemptDb;
                use _postgres_model::RequestLogDb;

                conn.transaction::<RequestLog, BaseError, _>(|conn| {
                    let mut initial_log = new_log_data.clone();
                    initial_log.final_attempt_id = None;

                    diesel::insert_into(request_log::table)
                        .values(RequestLogDb::to_db(&initial_log))
                        .execute(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to insert request log: {}",
                                e
                            )))
                        })?;

                    for attempt in new_attempts {
                        diesel::insert_into(request_attempt::table)
                            .values(RequestAttemptDb::to_db(attempt))
                            .execute(conn)
                            .map_err(|err| {
                                map_request_attempt_write_error(
                                    "Failed to insert request attempt",
                                    err,
                                )
                            })?;
                    }

                    let updated_log_db = diesel::update(request_log::table.find(new_log_data.id))
                        .set(RequestLogDb::to_db(new_log_data))
                        .returning(RequestLogDb::as_returning())
                        .get_result::<RequestLogDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to finalize request log {}: {}",
                                new_log_data.id, e
                            )))
                        })?;

                    if let Some(cost_catalog_version_id) = new_log_data.cost_catalog_version_id {
                        diesel::update(
                            cost_catalog_versions::table.filter(
                                cost_catalog_versions::dsl::id
                                    .eq(cost_catalog_version_id)
                                    .and(cost_catalog_versions::dsl::first_used_at.is_null()),
                            ),
                        )
                        .set((
                            cost_catalog_versions::dsl::first_used_at
                                .eq(Some(new_log_data.request_received_at)),
                            cost_catalog_versions::dsl::updated_at.eq(new_log_data.updated_at),
                        ))
                        .execute(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to freeze cost catalog version {} after request log insert: {}",
                                cost_catalog_version_id, e
                            )))
                        })?;
                    }

                    Ok(updated_log_db.from_db())
                })
            }
            DbConnection::Sqlite(conn) => {
                use crate::database::_sqlite_schema::{
                    cost_catalog_versions, request_attempt, request_log,
                };
                use crate::database::request_attempt::_sqlite_model::RequestAttemptDb;
                use _sqlite_model::RequestLogDb;

                conn.transaction::<RequestLog, BaseError, _>(|conn| {
                    let mut initial_log = new_log_data.clone();
                    initial_log.final_attempt_id = None;

                    diesel::insert_into(request_log::table)
                        .values(RequestLogDb::to_db(&initial_log))
                        .execute(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to insert request log: {}",
                                e
                            )))
                        })?;

                    for attempt in new_attempts {
                        diesel::insert_into(request_attempt::table)
                            .values(RequestAttemptDb::to_db(attempt))
                            .execute(conn)
                            .map_err(|err| {
                                map_request_attempt_write_error(
                                    "Failed to insert request attempt",
                                    err,
                                )
                            })?;
                    }

                    let updated_log_db = diesel::update(request_log::table.find(new_log_data.id))
                        .set(RequestLogDb::to_db(new_log_data))
                        .returning(RequestLogDb::as_returning())
                        .get_result::<RequestLogDb>(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to finalize request log {}: {}",
                                new_log_data.id, e
                            )))
                        })?;

                    if let Some(cost_catalog_version_id) = new_log_data.cost_catalog_version_id {
                        diesel::update(
                            cost_catalog_versions::table.filter(
                                cost_catalog_versions::dsl::id
                                    .eq(cost_catalog_version_id)
                                    .and(cost_catalog_versions::dsl::first_used_at.is_null()),
                            ),
                        )
                        .set((
                            cost_catalog_versions::dsl::first_used_at
                                .eq(Some(new_log_data.request_received_at)),
                            cost_catalog_versions::dsl::updated_at.eq(new_log_data.updated_at),
                        ))
                        .execute(conn)
                        .map_err(|e| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to freeze cost catalog version {} after request log insert: {}",
                                cost_catalog_version_id, e
                            )))
                        })?;
                    }

                    Ok(updated_log_db.from_db())
                })
            }
        }
    }

    pub fn get_by_id(log_id: i64) -> DbResult<RequestLogRecord> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_log::table
                .find(log_id)
                .select(RequestLogDb::as_select())
                .first::<RequestLogDb>(conn)
                .map(|row| row.from_db())
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Request log with id {} not found",
                        log_id
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching request log {}: {}",
                        log_id, other
                    ))),
                })
        })
    }

    pub fn list(payload: RequestLogQueryPayload) -> DbResult<ListResult<RequestLogListItem>> {
        let conn = &mut get_connection()?;
        let page_size = payload.page_size.unwrap_or(20);
        let page = payload.page.unwrap_or(1);
        let offset = (page - 1) * page_size;

        db_execute!(conn, {
            let mut query = request_log::table.into_boxed();
            let mut count_query = request_log::table.into_boxed();

            if let Some(val) = payload.api_key_id {
                query = query.filter(request_log::dsl::api_key_id.eq(val));
                count_query = count_query.filter(request_log::dsl::api_key_id.eq(val));
            }
            if let Some(val) = payload.provider_id {
                query = query.filter(request_log::dsl::provider_id.eq(Some(val)));
                count_query = count_query.filter(request_log::dsl::provider_id.eq(Some(val)));
            }
            if let Some(val) = payload.model_id {
                query = query.filter(request_log::dsl::model_id.eq(Some(val)));
                count_query = count_query.filter(request_log::dsl::model_id.eq(Some(val)));
            }
            if let Some(val) = payload.status {
                query = query.filter(request_log::dsl::status.eq(val.clone()));
                count_query = count_query.filter(request_log::dsl::status.eq(val));
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    let text_filter = request_log::dsl::model_name
                        .is_not_null()
                        .and(
                            request_log::dsl::model_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        )
                        .or(request_log::dsl::requested_model_name.is_not_null().and(
                            request_log::dsl::requested_model_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::resolved_route_name.is_not_null().and(
                            request_log::dsl::resolved_route_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::final_provider_name_snapshot
                            .is_not_null()
                            .and(
                                request_log::dsl::final_provider_name_snapshot
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ));

                    if let Ok(id_search) = search_term.parse::<i64>() {
                        let search_filter = request_log::dsl::id.eq(id_search).or(text_filter);
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
                    } else {
                        query = query.filter(text_filter.clone());
                        count_query = count_query.filter(text_filter);
                    }
                }
            }
            if let Some(st_time) = payload.start_time {
                query = query.filter(request_log::dsl::request_received_at.ge(st_time));
                count_query = count_query.filter(request_log::dsl::request_received_at.ge(st_time));
            }
            if let Some(et_time) = payload.end_time {
                query = query.filter(request_log::dsl::request_received_at.le(et_time));
                count_query = count_query.filter(request_log::dsl::request_received_at.le(et_time));
            }

            let total = count_query
                .select(diesel::dsl::count_star())
                .first::<i64>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to count request logs: {}", e)))
                })?;

            let list = query
                .order(request_log::dsl::request_received_at.desc())
                .limit(page_size)
                .offset(offset)
                .select(RequestLogListItemDb::as_select())
                .load::<RequestLogListItemDb>(conn)
                .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list request logs: {}", e)))
                })?;

            Ok(ListResult {
                total,
                page,
                page_size,
                list,
            })
        })
    }

    pub fn list_full(payload: RequestLogQueryPayload) -> DbResult<ListResult<RequestLogRecord>> {
        let conn = &mut get_connection()?;
        let page_size = payload.page_size.unwrap_or(20);
        let page = payload.page.unwrap_or(1);
        let offset = (page - 1) * page_size;

        db_execute!(conn, {
            let mut query = request_log::table.into_boxed();
            let mut count_query = request_log::table.into_boxed();

            if let Some(val) = payload.api_key_id {
                query = query.filter(request_log::dsl::api_key_id.eq(val));
                count_query = count_query.filter(request_log::dsl::api_key_id.eq(val));
            }
            if let Some(val) = payload.provider_id {
                query = query.filter(request_log::dsl::provider_id.eq(Some(val)));
                count_query = count_query.filter(request_log::dsl::provider_id.eq(Some(val)));
            }
            if let Some(val) = payload.model_id {
                query = query.filter(request_log::dsl::model_id.eq(Some(val)));
                count_query = count_query.filter(request_log::dsl::model_id.eq(Some(val)));
            }
            if let Some(val) = payload.status {
                query = query.filter(request_log::dsl::status.eq(val.clone()));
                count_query = count_query.filter(request_log::dsl::status.eq(val));
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    let text_filter = request_log::dsl::model_name
                        .is_not_null()
                        .and(
                            request_log::dsl::model_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        )
                        .or(request_log::dsl::requested_model_name.is_not_null().and(
                            request_log::dsl::requested_model_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::resolved_route_name.is_not_null().and(
                            request_log::dsl::resolved_route_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::final_provider_name_snapshot
                            .is_not_null()
                            .and(
                                request_log::dsl::final_provider_name_snapshot
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ));

                    if let Ok(id_search) = search_term.parse::<i64>() {
                        let search_filter = request_log::dsl::id.eq(id_search).or(text_filter);
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
                    } else {
                        query = query.filter(text_filter.clone());
                        count_query = count_query.filter(text_filter);
                    }
                }
            }
            if let Some(st_time) = payload.start_time {
                query = query.filter(request_log::dsl::request_received_at.ge(st_time));
                count_query = count_query.filter(request_log::dsl::request_received_at.ge(st_time));
            }
            if let Some(et_time) = payload.end_time {
                query = query.filter(request_log::dsl::request_received_at.le(et_time));
                count_query = count_query.filter(request_log::dsl::request_received_at.le(et_time));
            }

            let total = count_query
                .select(diesel::dsl::count_star())
                .first::<i64>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to count request logs: {}", e)))
                })?;

            let list = query
                .order(request_log::dsl::request_received_at.desc())
                .limit(page_size)
                .offset(offset)
                .select(RequestLogDb::as_select())
                .load::<RequestLogDb>(conn)
                .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list request logs: {}", e)))
                })?;

            Ok(ListResult {
                total,
                page,
                page_size,
                list,
            })
        })
    }
}

fn map_request_attempt_write_error(context: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(format!(
            "{context}: request_log_id + attempt_index must be unique"
        ))),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}
