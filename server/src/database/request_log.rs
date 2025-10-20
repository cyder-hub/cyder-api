use chrono::Utc;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{get_connection, DbResult, ListResult};
use crate::controller::BaseError;
use crate::{db_execute, db_object};
use crate::schema::enum_def::RequestStatus;

db_object! {
    #[derive(Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLog {
        pub id: i64,
        pub system_api_key_id: Option<i64>,
        pub provider_id: Option<i64>,
        pub model_id: Option<i64>,
        pub provider_api_key_id: Option<i64>,
        pub model_name: Option<String>,
        pub real_model_name: Option<String>,
        pub request_received_at: i64,
        pub llm_request_sent_at: Option<i64>,
        pub llm_response_first_chunk_at: Option<i64>,
        pub llm_response_completed_at: Option<i64>,
        pub response_sent_to_client_at: Option<i64>,
        pub client_ip: Option<String>,
        pub external_request_uri: Option<String>, // From schema
        pub llm_request_uri: Option<String>,    // From schema
        pub llm_response_status: Option<i32>,   // From schema (diesel type: Nullable<Int4>)
        pub llm_request_body: Option<String>,   // From schema (diesel type: Nullable<Text>)
        pub llm_response_body: Option<String>,  // From schema (diesel type: Nullable<Text>)
        pub status: Option<RequestStatus>,             // From schema (diesel type: Nullable<Text>)
        pub is_stream: bool,                    // From schema (diesel type: Bool)
        pub calculated_cost: Option<i64>,       // From schema (diesel type: Nullable<Int8>)
        pub cost_currency: Option<String>,      // From schema (diesel type: Nullable<Text>)
        pub created_at: i64,                    // From schema (diesel type: Int8)
        pub updated_at: i64,                    // From schema (diesel type: Int8)
        pub prompt_tokens: Option<i32>,         // From schema (diesel type: Nullable<Int4>)
        pub completion_tokens: Option<i32>,     // From schema (diesel type: Nullable<Int4>)
        pub reasoning_tokens: Option<i32>,      // From schema (diesel type: Nullable<Int4>)
        pub total_tokens: Option<i32>,          // From schema (diesel type: Nullable<Int4>)
        pub channel: Option<String>,
        pub external_id: Option<String>,
    }

    // Struct for inserting the initial part of a request log.
    #[derive(Insertable, Deserialize, Debug)]
    #[diesel(table_name = request_log)]
    pub struct NewRequestLog {
        pub id: i64,
        pub system_api_key_id: i64,
        pub provider_id: i64,
        pub model_id: i64,
        pub provider_api_key_id: i64,
        pub model_name: String,
        pub real_model_name: String, // Nullable in schema
        pub request_received_at: i64,
        pub llm_request_sent_at: i64,
        pub client_ip: Option<String>,
        pub external_request_uri: Option<String>,
        pub status: RequestStatus,
        pub created_at: i64,
        pub updated_at: i64,
        pub channel: Option<String>,
        pub external_id: Option<String>,
    }

    // Struct for updating a request log with completion details.
    #[derive(AsChangeset, Deserialize, Debug, Default)]
    #[diesel(table_name = request_log)]
    pub struct UpdateRequestLogData {
        pub prompt_tokens: Option<i32>,
        pub completion_tokens: Option<i32>,
        pub reasoning_tokens: Option<i32>,
        pub total_tokens: Option<i32>,
        pub llm_response_first_chunk_at: Option<i64>,
        pub llm_response_completed_at: Option<i64>,
        pub response_sent_to_client_at: Option<i64>,
        pub status: Option<RequestStatus>,          // From schema: Nullable<Text>
        pub calculated_cost: Option<i64>,    // From schema: Nullable<Int8>
        pub cost_currency: Option<Option<String>>,   // From schema: Nullable<Text>
        pub is_stream: Option<bool>,                 // From schema: Bool
        pub llm_request_uri: Option<Option<String>>, // From schema: Nullable<Text>
        pub llm_request_body: Option<Option<String>>,// From schema: Nullable<Text>
        pub llm_response_body: Option<Option<String>>,// From schema: Nullable<Text>
        pub llm_response_status: Option<Option<i32>>,// From schema: Nullable<Int4>
        // updated_at is handled manually
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
    pub fn insert(new_log_data: &NewRequestLog) -> DbResult<RequestLog> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_log_db = diesel::insert_into(request_log::table)
                .values(NewRequestLogDb::to_db(new_log_data))
                .returning(RequestLogDb::as_returning())
                .get_result::<RequestLogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!("Failed to insert request log: {}", e)))
                })?;
            Ok(inserted_log_db.from_db())
        })
    }

    /// Updates an existing request log with completion details.
    pub fn update_completion_details(
        log_id: i64,
        update_data: &UpdateRequestLogData,
    ) -> DbResult<RequestLog> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_log_db = diesel::update(request_log::table.find(log_id))
                .set((
                    UpdateRequestLogDataDb::to_db(update_data),
                    request_log::dsl::updated_at.eq(current_time),
                ))
                .returning(RequestLogDb::as_returning())
                .get_result::<RequestLogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update request log {}: {}",
                        log_id, e
                    )))
                })?;
            Ok(updated_log_db.from_db())
        })
    }

    /// Retrieves a request log by its ID.
    pub fn get_by_id(log_id: i64) -> DbResult<RequestLog> {
        let conn = &mut get_connection();
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
    pub fn list(payload: RequestLogQueryPayload) -> DbResult<ListResult<RequestLog>> {
        let conn = &mut get_connection();
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
            if let Some(val) = payload.status  {
                query = query.filter(request_log::dsl::status.eq(val.clone()));
                count_query = count_query.filter(request_log::dsl::status.eq(val));
            }
            if let Some(search_term) = payload.search.as_ref() {
                if !search_term.is_empty() {
                    let pattern = format!("%{}%", search_term);
                    let search_filter = request_log::dsl::model_name
                        .like(pattern)
                        .or(request_log::dsl::channel.eq(search_term))
                        .or(request_log::dsl::external_id.eq(search_term));
                    query = query.filter(search_filter.clone());
                    count_query = count_query.filter(search_filter);
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
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to count request logs: {}", e))))?;

            let results_db = query
                .order(request_log::dsl::request_received_at.desc())
                .limit(page_size)
                .offset(offset)
                .select(RequestLogDb::as_select())
                .load::<RequestLogDb>(conn)
                .map_err(|e| BaseError::DatabaseFatal(Some(format!("Failed to list request logs: {}", e))))?;

            let list = results_db.into_iter().map(|db_log| db_log.from_db()).collect();

            Ok(ListResult {
                total,
                page,
                page_size,
                list,
            })
        })
    }

    /// Creates an initial request log entry as the first step of a two-step commit.
    ///
    /// The caller is responsible for providing an `id` (e.g., generated by a snowflake algorithm),
    /// setting `initial_data.is_success` to `false`, and ensuring fields not known at this stage
    /// (like `initial_data.real_model_name`, `initial_data.external_request_body_full`) are `None`.
    pub fn create_initial_request(initial_data: &NewRequestLog) -> DbResult<RequestLog> {
        let conn = &mut get_connection();
        db_execute!(conn, {
            let inserted_log_db = diesel::insert_into(request_log::table)
                .values(NewRequestLogDb::to_db(initial_data))
                .returning(RequestLogDb::as_returning())
                .get_result::<RequestLogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to insert initial request log via create_initial_request: {}",
                        e
                    )))
                })?;
            Ok(inserted_log_db.from_db())
        })
    }

    /// Updates an existing request log with completion details as the second step of a two-step commit.
    ///
    /// This includes details obtained after interacting with the LLM and sending the response to the client.
    pub fn update_request_with_completion_details(
        log_id: i64,
        completion_data: &UpdateRequestLogData, // This struct now includes real_model_name
    ) -> DbResult<RequestLog> {
        let conn = &mut get_connection();
        let current_time = Utc::now().timestamp_millis();

        db_execute!(conn, {
            let updated_log_db = diesel::update(request_log::table.find(log_id))
                .set((
                    UpdateRequestLogDataDb::to_db(completion_data),
                    request_log::dsl::updated_at.eq(current_time),
                ))
                .returning(RequestLogDb::as_returning())
                .get_result::<RequestLogDb>(conn)
                .map_err(|e| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to update request log {} with completion details via update_request_with_completion_details: {}",
                        log_id, e
                    )))
                })?;
            Ok(updated_log_db.from_db())
        })
    }

}
