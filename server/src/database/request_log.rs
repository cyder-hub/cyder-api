use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, ListResult, get_connection};
use crate::controller::BaseError;
use crate::schema::enum_def::{LlmApiType, RequestStatus};
use crate::{db_execute, db_object};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, AsChangeset, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLog {
        pub id: i64,
        pub api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub base_requested_model_name: Option<String>,
        pub resolved_reasoning_suffix: Option<String>,
        pub resolved_reasoning_preset: Option<String>,
        pub user_api_type: LlmApiType,
        #[diesel(column_name = status)]
        pub overall_status: RequestStatus,
        pub final_error_code: Option<String>,
        pub final_error_message: Option<String>,
        pub request_received_at: i64,
        pub upstream_request_sent_at: Option<i64>,
        #[diesel(column_name = llm_response_first_chunk_at)]
        pub response_started_to_client_at: Option<i64>,
        #[diesel(column_name = llm_response_completed_at)]
        pub completed_at: Option<i64>,
        pub is_stream: bool,
        pub client_ip: Option<String>,
        pub provider_id: Option<i64>,
        pub provider_api_key_id: Option<i64>,
        pub model_id: Option<i64>,
        pub provider_key_snapshot: Option<String>,
        pub provider_name_snapshot: Option<String>,
        pub model_name_snapshot: Option<String>,
        pub real_model_name_snapshot: Option<String>,
        pub llm_api_type: Option<LlmApiType>,
        pub upstream_http_status: Option<i32>,
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
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLogListItem {
        pub id: i64,
        pub api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub base_requested_model_name: Option<String>,
        pub resolved_reasoning_suffix: Option<String>,
        pub resolved_reasoning_preset: Option<String>,
        #[diesel(column_name = status)]
        pub overall_status: RequestStatus,
        pub request_received_at: i64,
        pub upstream_request_sent_at: Option<i64>,
        #[diesel(column_name = llm_response_first_chunk_at)]
        pub response_started_to_client_at: Option<i64>,
        #[diesel(column_name = llm_response_completed_at)]
        pub completed_at: Option<i64>,
        pub is_stream: bool,
        pub provider_id: Option<i64>,
        pub provider_name_snapshot: Option<String>,
        pub model_id: Option<i64>,
        pub model_name_snapshot: Option<String>,
        pub real_model_name_snapshot: Option<String>,
        pub upstream_http_status: Option<i32>,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub total_input_tokens: Option<i32>,
        pub total_output_tokens: Option<i32>,
        pub output_text_tokens: Option<i32>,
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
    pub user_api_type: Option<LlmApiType>,
    pub final_error_code: Option<String>,
    pub latency_ms_min: Option<i64>,
    pub latency_ms_max: Option<i64>,
    pub total_tokens_min: Option<i32>,
    pub total_tokens_max: Option<i32>,
    pub estimated_cost_nanos_min: Option<i64>,
    pub estimated_cost_nanos_max: Option<i64>,
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
            if let Some(val) = payload.user_api_type {
                query = query.filter(request_log::dsl::user_api_type.eq(val));
                count_query = count_query.filter(request_log::dsl::user_api_type.eq(val));
            }
            if let Some(val) = payload.final_error_code.as_ref() {
                if !val.is_empty() {
                    query = query.filter(request_log::dsl::final_error_code.eq(Some(val)));
                    count_query =
                        count_query.filter(request_log::dsl::final_error_code.eq(Some(val)));
                }
            }
            if let Some(val) = payload.latency_ms_min {
                let filter = request_log::dsl::llm_response_completed_at
                    .is_not_null()
                    .and(
                        request_log::dsl::llm_response_completed_at
                            .assume_not_null()
                            .ge(request_log::dsl::request_received_at + val),
                    );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.latency_ms_max {
                let filter = request_log::dsl::llm_response_completed_at
                    .is_not_null()
                    .and(
                        request_log::dsl::llm_response_completed_at
                            .assume_not_null()
                            .le(request_log::dsl::request_received_at + val),
                    );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.total_tokens_min {
                let filter = request_log::dsl::total_tokens
                    .is_not_null()
                    .and(request_log::dsl::total_tokens.assume_not_null().ge(val));
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.total_tokens_max {
                let filter = request_log::dsl::total_tokens
                    .is_not_null()
                    .and(request_log::dsl::total_tokens.assume_not_null().le(val));
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.estimated_cost_nanos_min {
                let filter = request_log::dsl::estimated_cost_nanos.is_not_null().and(
                    request_log::dsl::estimated_cost_nanos
                        .assume_not_null()
                        .ge(val),
                );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.estimated_cost_nanos_max {
                let filter = request_log::dsl::estimated_cost_nanos.is_not_null().and(
                    request_log::dsl::estimated_cost_nanos
                        .assume_not_null()
                        .le(val),
                );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    let text_filter = request_log::dsl::model_name_snapshot
                        .is_not_null()
                        .and(
                            request_log::dsl::model_name_snapshot
                                .assume_not_null()
                                .like(pattern.clone()),
                        )
                        .or(request_log::dsl::requested_model_name.is_not_null().and(
                            request_log::dsl::requested_model_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::base_requested_model_name
                            .is_not_null()
                            .and(
                                request_log::dsl::base_requested_model_name
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ))
                        .or(request_log::dsl::resolved_reasoning_suffix
                            .is_not_null()
                            .and(
                                request_log::dsl::resolved_reasoning_suffix
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ))
                        .or(request_log::dsl::resolved_reasoning_preset
                            .is_not_null()
                            .and(
                                request_log::dsl::resolved_reasoning_preset
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ))
                        .or(request_log::dsl::provider_name_snapshot.is_not_null().and(
                            request_log::dsl::provider_name_snapshot
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::final_error_code.is_not_null().and(
                            request_log::dsl::final_error_code
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
            if let Some(val) = payload.user_api_type {
                query = query.filter(request_log::dsl::user_api_type.eq(val));
                count_query = count_query.filter(request_log::dsl::user_api_type.eq(val));
            }
            if let Some(val) = payload.final_error_code.as_ref() {
                if !val.is_empty() {
                    query = query.filter(request_log::dsl::final_error_code.eq(Some(val)));
                    count_query =
                        count_query.filter(request_log::dsl::final_error_code.eq(Some(val)));
                }
            }
            if let Some(val) = payload.latency_ms_min {
                let filter = request_log::dsl::llm_response_completed_at
                    .is_not_null()
                    .and(
                        request_log::dsl::llm_response_completed_at
                            .assume_not_null()
                            .ge(request_log::dsl::request_received_at + val),
                    );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.latency_ms_max {
                let filter = request_log::dsl::llm_response_completed_at
                    .is_not_null()
                    .and(
                        request_log::dsl::llm_response_completed_at
                            .assume_not_null()
                            .le(request_log::dsl::request_received_at + val),
                    );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.total_tokens_min {
                let filter = request_log::dsl::total_tokens
                    .is_not_null()
                    .and(request_log::dsl::total_tokens.assume_not_null().ge(val));
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.total_tokens_max {
                let filter = request_log::dsl::total_tokens
                    .is_not_null()
                    .and(request_log::dsl::total_tokens.assume_not_null().le(val));
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.estimated_cost_nanos_min {
                let filter = request_log::dsl::estimated_cost_nanos.is_not_null().and(
                    request_log::dsl::estimated_cost_nanos
                        .assume_not_null()
                        .ge(val),
                );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(val) = payload.estimated_cost_nanos_max {
                let filter = request_log::dsl::estimated_cost_nanos.is_not_null().and(
                    request_log::dsl::estimated_cost_nanos
                        .assume_not_null()
                        .le(val),
                );
                query = query.filter(filter.clone());
                count_query = count_query.filter(filter);
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    let text_filter = request_log::dsl::model_name_snapshot
                        .is_not_null()
                        .and(
                            request_log::dsl::model_name_snapshot
                                .assume_not_null()
                                .like(pattern.clone()),
                        )
                        .or(request_log::dsl::requested_model_name.is_not_null().and(
                            request_log::dsl::requested_model_name
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::base_requested_model_name
                            .is_not_null()
                            .and(
                                request_log::dsl::base_requested_model_name
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ))
                        .or(request_log::dsl::resolved_reasoning_suffix
                            .is_not_null()
                            .and(
                                request_log::dsl::resolved_reasoning_suffix
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ))
                        .or(request_log::dsl::resolved_reasoning_preset
                            .is_not_null()
                            .and(
                                request_log::dsl::resolved_reasoning_preset
                                    .assume_not_null()
                                    .like(pattern.clone()),
                            ))
                        .or(request_log::dsl::provider_name_snapshot.is_not_null().and(
                            request_log::dsl::provider_name_snapshot
                                .assume_not_null()
                                .like(pattern.clone()),
                        ))
                        .or(request_log::dsl::final_error_code.is_not_null().and(
                            request_log::dsl::final_error_code
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
