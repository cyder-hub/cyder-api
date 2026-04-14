use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, ListResult, get_connection};
use crate::controller::BaseError;
use crate::schema::enum_def::{LlmApiType, RequestStatus, StorageType};
use crate::{db_execute, db_object};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLog {
        pub id: i64,
        pub system_api_key_id: i64,
        pub provider_id: i64,
        pub model_id: i64,
        pub provider_api_key_id: i64,
        pub model_name: String,
        pub real_model_name: String,
        pub request_received_at: i64,
        pub llm_request_sent_at: i64,
        pub llm_response_first_chunk_at: Option<i64>,
        pub llm_response_completed_at: Option<i64>,
        pub client_ip: Option<String>,
        pub llm_request_uri: Option<String>,
        pub llm_response_status: Option<i32>,
        pub status: Option<RequestStatus>,
        pub is_stream: bool,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub cost_catalog_id: Option<i64>,
        pub cost_catalog_version_id: Option<i64>,
        pub cost_snapshot_json: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
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
        pub storage_type: Option<StorageType>,
        pub user_request_body: Option<String>,
        pub llm_request_body: Option<String>,
        pub llm_response_body: Option<String>,
        pub user_response_body: Option<String>,
        pub user_api_type: LlmApiType,
        pub llm_api_type: LlmApiType,
    }

    #[derive(Queryable, Selectable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLogListItem {
        pub id: i64,
        pub system_api_key_id: i64,
        pub provider_id: i64,
        pub model_name: String,
        pub request_received_at: i64,
        pub llm_request_sent_at: i64,
        pub llm_response_first_chunk_at: Option<i64>,
        pub llm_response_completed_at: Option<i64>,
        pub status: Option<RequestStatus>,
        pub is_stream: bool,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub total_input_tokens: Option<i32>,
        pub total_output_tokens: Option<i32>,
        pub reasoning_tokens: Option<i32>,
        pub total_tokens: Option<i32>,
    }
}

#[derive(Deserialize, Debug, Default)]
pub struct RequestLogQueryPayload {
    pub system_api_key_id: Option<i64>,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub status: Option<RequestStatus>,
    pub start_time: Option<i64>, // For request_received_at >= start_time
    pub end_time: Option<i64>,   // For request_received_at <= end_time
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub search: Option<String>,
}

impl RequestLog {
    /// Inserts a new request log entry with initial details.
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

    /// Retrieves a request log by its ID.
    pub fn get_by_id(log_id: i64) -> DbResult<RequestLog> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            let log_db = request_log::table
                .find(log_id)
                .select(RequestLogDb::as_select())
                .first::<RequestLogDb>(conn)
                .map_err(|e| match e {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Request log with id {} not found",
                        log_id
                    ))),
                    _ => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching request log {}: {}",
                        log_id, e
                    ))),
                })?;
            Ok(log_db.from_db())
        })
    }

    /// Lists request logs with filtering and pagination.
    pub fn list(payload: RequestLogQueryPayload) -> DbResult<ListResult<RequestLogListItem>> {
        let conn = &mut get_connection()?;
        let page_size = payload.page_size.unwrap_or(20); // Default page size
        let page = payload.page.unwrap_or(1);
        let offset = (page - 1) * page_size;

        db_execute!(conn, {
            let mut query = request_log::table.into_boxed();
            let mut count_query = request_log::table.into_boxed();

            if let Some(val) = payload.system_api_key_id {
                query = query.filter(request_log::dsl::system_api_key_id.eq(val));
                count_query = count_query.filter(request_log::dsl::system_api_key_id.eq(val));
            }
            if let Some(val) = payload.provider_id {
                query = query.filter(request_log::dsl::provider_id.eq(val));
                count_query = count_query.filter(request_log::dsl::provider_id.eq(val));
            }
            if let Some(val) = payload.model_id {
                query = query.filter(request_log::dsl::model_id.eq(val));
                count_query = count_query.filter(request_log::dsl::model_id.eq(val));
            }
            if let Some(val) = payload.status {
                query = query.filter(request_log::dsl::status.eq(val.clone()));
                count_query = count_query.filter(request_log::dsl::status.eq(val));
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    if let Ok(id_search) = search_term.parse::<i64>() {
                        let search_filter = request_log::dsl::id
                            .eq(id_search)
                            .or(request_log::dsl::model_name.like(pattern.clone()));
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
                    } else {
                        let search_filter = request_log::dsl::model_name.like(pattern);
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
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

            let results_db = query
                .order(request_log::dsl::request_received_at.desc())
                .limit(page_size)
                .offset(offset)
                .select(RequestLogListItemDb::as_select())
                .load::<RequestLogListItemDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list request logs: {}", e)))
                })?;

            let list = results_db
                .into_iter()
                .map(|db_log| db_log.from_db())
                .collect();

            Ok(ListResult {
                total,
                page,
                page_size,
                list,
            })
        })
    }

    pub fn list_full(payload: RequestLogQueryPayload) -> DbResult<ListResult<RequestLog>> {
        let conn = &mut get_connection()?;
        let page_size = payload.page_size.unwrap_or(20); // Default page size
        let page = payload.page.unwrap_or(1);
        let offset = (page - 1) * page_size;

        db_execute!(conn, {
            let mut query = request_log::table.into_boxed();
            let mut count_query = request_log::table.into_boxed();

            if let Some(val) = payload.system_api_key_id {
                query = query.filter(request_log::dsl::system_api_key_id.eq(val));
                count_query = count_query.filter(request_log::dsl::system_api_key_id.eq(val));
            }
            if let Some(val) = payload.provider_id {
                query = query.filter(request_log::dsl::provider_id.eq(val));
                count_query = count_query.filter(request_log::dsl::provider_id.eq(val));
            }
            if let Some(val) = payload.model_id {
                query = query.filter(request_log::dsl::model_id.eq(val));
                count_query = count_query.filter(request_log::dsl::model_id.eq(val));
            }
            if let Some(val) = payload.status {
                query = query.filter(request_log::dsl::status.eq(val.clone()));
                count_query = count_query.filter(request_log::dsl::status.eq(val));
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    if let Ok(id_search) = search_term.parse::<i64>() {
                        let search_filter = request_log::dsl::id
                            .eq(id_search)
                            .or(request_log::dsl::model_name.like(pattern.clone()));
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
                    } else {
                        let search_filter = request_log::dsl::model_name.like(pattern);
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
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

            let results_db = query
                .order(request_log::dsl::request_received_at.desc())
                .limit(page_size)
                .offset(offset)
                .select(RequestLogDb::as_select())
                .load::<RequestLogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list request logs: {}", e)))
                })?;

            let list = results_db
                .into_iter()
                .map(|db_log| db_log.from_db())
                .collect();

            Ok(ListResult {
                total,
                page,
                page_size,
                list,
            })
        })
    }
}
