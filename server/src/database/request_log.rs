use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{DbResult, ListResult, get_connection};
use crate::controller::BaseError;
use crate::schema::enum_def::{LlmApiType, RequestStatus, StorageType};
use crate::{db_execute, db_object};

// Legacy database compatibility boundary.
// `request_log.system_api_key_id` remains the physical column so historical log
// rows keep their original key linkage, but business-facing DTOs should
// converge on `api_key` naming rather than re-exporting `system_api_key`.
db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLog {
        pub id: i64,
        // This legacy column name is intentionally kept during the first
        // `api_key` migration stage. Its values must stay stable so historical
        // request-to-key linkage remains joinable after the table rewrite.
        pub system_api_key_id: i64,
        pub provider_id: i64,
        pub model_id: i64,
        pub provider_api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub resolved_name_scope: Option<String>,
        pub resolved_route_id: Option<i64>,
        pub resolved_route_name: Option<String>,
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
        pub applied_request_patch_ids_json: Option<String>,
        pub request_patch_summary_json: Option<String>,
        pub user_api_type: LlmApiType,
        pub llm_api_type: LlmApiType,
    }

    // Legacy list row kept only for Diesel field mapping against the physical
    // `request_log.system_api_key_id` column. Upper layers should use the
    // canonical `RequestLogListItem` read model below instead.
    #[derive(Queryable, Selectable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct LegacyRequestLogListItemRow {
        pub id: i64,
        // Keep the same semantics as `RequestLog.system_api_key_id`.
        pub system_api_key_id: i64,
        pub provider_id: i64,
        pub requested_model_name: Option<String>,
        pub resolved_name_scope: Option<String>,
        pub resolved_route_name: Option<String>,
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
    pub api_key_id: Option<i64>,
    pub provider_id: Option<i64>,
    pub model_id: Option<i64>,
    pub status: Option<RequestStatus>,
    pub start_time: Option<i64>, // For request_received_at >= start_time
    pub end_time: Option<i64>,   // For request_received_at <= end_time
    pub page: Option<i64>,
    pub page_size: Option<i64>,
    pub search: Option<String>,
}

#[derive(Serialize, Debug, Clone)]
pub struct RequestLogRecord {
    pub id: i64,
    pub api_key_id: i64,
    pub provider_id: i64,
    pub model_id: i64,
    pub provider_api_key_id: i64,
    pub requested_model_name: Option<String>,
    pub resolved_name_scope: Option<String>,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
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
    pub applied_request_patch_ids_json: Option<String>,
    pub request_patch_summary_json: Option<String>,
    pub user_api_type: LlmApiType,
    pub llm_api_type: LlmApiType,
}

impl From<RequestLog> for RequestLogRecord {
    fn from(value: RequestLog) -> Self {
        // Canonical business DTO boundary: keep the legacy
        // `request_log.system_api_key_id` storage detail inside the database
        // model, and expose only `api_key_id` above this conversion.
        Self {
            id: value.id,
            api_key_id: value.system_api_key_id,
            provider_id: value.provider_id,
            model_id: value.model_id,
            provider_api_key_id: value.provider_api_key_id,
            requested_model_name: value.requested_model_name,
            resolved_name_scope: value.resolved_name_scope,
            resolved_route_id: value.resolved_route_id,
            resolved_route_name: value.resolved_route_name,
            model_name: value.model_name,
            real_model_name: value.real_model_name,
            request_received_at: value.request_received_at,
            llm_request_sent_at: value.llm_request_sent_at,
            llm_response_first_chunk_at: value.llm_response_first_chunk_at,
            llm_response_completed_at: value.llm_response_completed_at,
            client_ip: value.client_ip,
            llm_request_uri: value.llm_request_uri,
            llm_response_status: value.llm_response_status,
            status: value.status,
            is_stream: value.is_stream,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            cost_catalog_id: value.cost_catalog_id,
            cost_catalog_version_id: value.cost_catalog_version_id,
            cost_snapshot_json: value.cost_snapshot_json,
            created_at: value.created_at,
            updated_at: value.updated_at,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            input_text_tokens: value.input_text_tokens,
            output_text_tokens: value.output_text_tokens,
            input_image_tokens: value.input_image_tokens,
            output_image_tokens: value.output_image_tokens,
            cache_read_tokens: value.cache_read_tokens,
            cache_write_tokens: value.cache_write_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
            storage_type: value.storage_type,
            user_request_body: value.user_request_body,
            llm_request_body: value.llm_request_body,
            llm_response_body: value.llm_response_body,
            user_response_body: value.user_response_body,
            applied_request_patch_ids_json: value.applied_request_patch_ids_json,
            request_patch_summary_json: value.request_patch_summary_json,
            user_api_type: value.user_api_type,
            llm_api_type: value.llm_api_type,
        }
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct RequestLogListItem {
    pub id: i64,
    pub api_key_id: i64,
    pub provider_id: i64,
    pub requested_model_name: Option<String>,
    pub resolved_name_scope: Option<String>,
    pub resolved_route_name: Option<String>,
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

impl From<LegacyRequestLogListItemRow> for RequestLogListItem {
    fn from(value: LegacyRequestLogListItemRow) -> Self {
        // Canonical list-item boundary mirroring the detail conversion above.
        Self {
            id: value.id,
            api_key_id: value.system_api_key_id,
            provider_id: value.provider_id,
            requested_model_name: value.requested_model_name,
            resolved_name_scope: value.resolved_name_scope,
            resolved_route_name: value.resolved_route_name,
            model_name: value.model_name,
            request_received_at: value.request_received_at,
            llm_request_sent_at: value.llm_request_sent_at,
            llm_response_first_chunk_at: value.llm_response_first_chunk_at,
            llm_response_completed_at: value.llm_response_completed_at,
            status: value.status,
            is_stream: value.is_stream,
            estimated_cost_nanos: value.estimated_cost_nanos,
            estimated_cost_currency: value.estimated_cost_currency,
            total_input_tokens: value.total_input_tokens,
            total_output_tokens: value.total_output_tokens,
            reasoning_tokens: value.reasoning_tokens,
            total_tokens: value.total_tokens,
        }
    }
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
    pub fn get_by_id(log_id: i64) -> DbResult<RequestLogRecord> {
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
            Ok(log_db.from_db().into())
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

            if let Some(val) = payload.api_key_id {
                // Query payload already uses canonical `api_key_id`; only this
                // database boundary should know it still maps to the legacy
                // `request_log.system_api_key_id` column.
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
                            .or(request_log::dsl::model_name.like(pattern.clone()))
                            .or(request_log::dsl::requested_model_name
                                .nullable()
                                .like(pattern.clone()));
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
                    } else {
                        let search_filter = request_log::dsl::model_name.like(pattern.clone()).or(
                            request_log::dsl::requested_model_name
                                .nullable()
                                .like(pattern),
                        );
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
                // Read through the legacy row shape, then immediately convert
                // into canonical `RequestLogListItem` values before returning.
                .select(LegacyRequestLogListItemRowDb::as_select())
                .load::<LegacyRequestLogListItemRowDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to list request logs: {}", e)))
                })?;

            let list = results_db
                .into_iter()
                .map(|db_log| RequestLogListItem::from(db_log.from_db()))
                .collect();

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
        let page_size = payload.page_size.unwrap_or(20); // Default page size
        let page = payload.page.unwrap_or(1);
        let offset = (page - 1) * page_size;

        db_execute!(conn, {
            let mut query = request_log::table.into_boxed();
            let mut count_query = request_log::table.into_boxed();

            if let Some(val) = payload.api_key_id {
                // Same canonical-to-legacy mapping as `list(...)`.
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
                            .or(request_log::dsl::model_name.like(pattern.clone()))
                            .or(request_log::dsl::requested_model_name
                                .nullable()
                                .like(pattern.clone()));
                        query = query.filter(search_filter.clone());
                        count_query = count_query.filter(search_filter);
                    } else {
                        let search_filter = request_log::dsl::model_name.like(pattern.clone()).or(
                            request_log::dsl::requested_model_name
                                .nullable()
                                .like(pattern),
                        );
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
                .map(|db_log| RequestLogRecord::from(db_log.from_db()))
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
