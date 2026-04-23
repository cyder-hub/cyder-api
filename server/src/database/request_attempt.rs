use diesel::{SqliteConnection, prelude::*};
use serde::Serialize;

use super::{DbConnection, DbResult, get_connection};
use crate::controller::BaseError;
use crate::schema::enum_def::{LlmApiType, RequestAttemptStatus, SchedulerAction};
use crate::{db_execute, db_object};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_attempt)]
    pub struct RequestAttempt {
        pub id: i64,
        pub request_log_id: i64,
        pub attempt_index: i32,
        pub candidate_position: i32,
        pub provider_id: Option<i64>,
        pub provider_api_key_id: Option<i64>,
        pub model_id: Option<i64>,
        pub provider_key_snapshot: Option<String>,
        pub provider_name_snapshot: Option<String>,
        pub model_name_snapshot: Option<String>,
        pub real_model_name_snapshot: Option<String>,
        pub llm_api_type: Option<LlmApiType>,
        pub attempt_status: RequestAttemptStatus,
        pub scheduler_action: SchedulerAction,
        pub error_code: Option<String>,
        pub error_message: Option<String>,
        pub request_uri: Option<String>,
        pub request_headers_json: Option<String>,
        pub response_headers_json: Option<String>,
        pub http_status: Option<i32>,
        pub started_at: Option<i64>,
        pub first_byte_at: Option<i64>,
        pub completed_at: Option<i64>,
        pub response_started_to_client: bool,
        pub backoff_ms: Option<i32>,
        pub applied_request_patch_ids_json: Option<String>,
        pub request_patch_summary_json: Option<String>,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub cost_catalog_version_id: Option<i64>,
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
        pub llm_request_blob_id: Option<i32>,
        pub llm_request_patch_id: Option<i32>,
        pub llm_response_blob_id: Option<i32>,
        pub llm_response_capture_state: Option<String>,
        pub created_at: i64,
        pub updated_at: i64,
    }

    #[derive(Queryable, Selectable, Serialize, Debug, Clone)]
    #[diesel(table_name = request_attempt)]
    pub struct RequestAttemptListItem {
        pub id: i64,
        pub request_log_id: i64,
        pub attempt_index: i32,
        pub candidate_position: i32,
        pub provider_id: Option<i64>,
        pub provider_name_snapshot: Option<String>,
        pub model_id: Option<i64>,
        pub model_name_snapshot: Option<String>,
        pub real_model_name_snapshot: Option<String>,
        pub llm_api_type: Option<LlmApiType>,
        pub attempt_status: RequestAttemptStatus,
        pub scheduler_action: SchedulerAction,
        pub error_code: Option<String>,
        pub http_status: Option<i32>,
        pub started_at: Option<i64>,
        pub first_byte_at: Option<i64>,
        pub completed_at: Option<i64>,
        pub response_started_to_client: bool,
        pub backoff_ms: Option<i32>,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub total_input_tokens: Option<i32>,
        pub total_output_tokens: Option<i32>,
        pub reasoning_tokens: Option<i32>,
        pub total_tokens: Option<i32>,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

pub type RequestAttemptDetail = RequestAttempt;

impl RequestAttempt {
    pub fn insert_many(new_attempts: &[RequestAttempt]) -> DbResult<Vec<RequestAttempt>> {
        if new_attempts.is_empty() {
            return Ok(Vec::new());
        }

        let conn = &mut get_connection()?;
        match conn {
            DbConnection::Postgres(conn) => {
                use crate::database::_postgres_schema::request_attempt;
                use _postgres_model::*;

                let rows: Vec<RequestAttemptDb> =
                    new_attempts.iter().map(RequestAttemptDb::to_db).collect();

                diesel::insert_into(request_attempt::table)
                    .values(&rows)
                    .returning(RequestAttemptDb::as_returning())
                    .get_results::<RequestAttemptDb>(conn)
                    .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                    .map_err(|err| {
                        map_request_attempt_write_error("Failed to insert request attempts", err)
                    })
            }
            DbConnection::Sqlite(conn) => insert_many_sqlite(conn, new_attempts),
        }
    }

    pub fn get_by_id(attempt_id: i64) -> DbResult<RequestAttemptDetail> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_attempt::table
                .find(attempt_id)
                .select(RequestAttemptDb::as_select())
                .first::<RequestAttemptDb>(conn)
                .map(|row| row.from_db())
                .map_err(|err| match err {
                    diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
                        "Request attempt with id {} not found",
                        attempt_id
                    ))),
                    other => BaseError::DatabaseFatal(Some(format!(
                        "Error fetching request attempt {}: {}",
                        attempt_id, other
                    ))),
                })
        })
    }

    pub fn list_by_request_log_id(log_id: i64) -> DbResult<Vec<RequestAttemptDetail>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_attempt::table
                .filter(request_attempt::dsl::request_log_id.eq(log_id))
                .order(request_attempt::dsl::attempt_index.asc())
                .select(RequestAttemptDb::as_select())
                .load::<RequestAttemptDb>(conn)
                .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list request attempts for request_log {}: {}",
                        log_id, err
                    )))
                })
        })
    }

    pub fn list_items_by_request_log_id(log_id: i64) -> DbResult<Vec<RequestAttemptListItem>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_attempt::table
                .filter(request_attempt::dsl::request_log_id.eq(log_id))
                .order(request_attempt::dsl::attempt_index.asc())
                .select(RequestAttemptListItemDb::as_select())
                .load::<RequestAttemptListItemDb>(conn)
                .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list request attempt summaries for request_log {}: {}",
                        log_id, err
                    )))
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

fn insert_many_sqlite(
    conn: &mut SqliteConnection,
    new_attempts: &[RequestAttempt],
) -> DbResult<Vec<RequestAttempt>> {
    use crate::database::_sqlite_schema::request_attempt;
    use _sqlite_model::*;

    conn.transaction::<Vec<RequestAttempt>, BaseError, _>(|conn| {
        let mut inserted = Vec::with_capacity(new_attempts.len());
        for attempt in new_attempts {
            let row = RequestAttemptDb::to_db(attempt);
            let inserted_row = diesel::insert_into(request_attempt::table)
                .values(&row)
                .returning(RequestAttemptDb::as_returning())
                .get_result::<RequestAttemptDb>(conn)
                .map_err(|err| {
                    map_request_attempt_write_error("Failed to insert request attempt", err)
                })?;
            inserted.push(inserted_row.from_db());
        }
        Ok(inserted)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::connection::SimpleConnection;

    fn sqlite_connection() -> (tempfile::TempDir, SqliteConnection) {
        crate::database::open_test_sqlite_connection_with_migrations("request-attempt.sqlite")
    }

    fn seed_request_log(conn: &mut SqliteConnection) {
        conn.batch_execute(
            "INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'ck-test', 'hash', 'ck-test', 'test', 'Test key', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );
            INSERT INTO request_log (
                id, api_key_id, user_api_type, overall_status, attempt_count,
                retry_count, fallback_count, request_received_at, created_at, updated_at
            ) VALUES (
                10, 1, 'OPENAI', 'SUCCESS', 4, 1, 1, 100, 100, 100
            );",
        )
        .expect("request log seed should insert");
    }

    fn attempt(id: i64, index: i32, status: RequestAttemptStatus) -> RequestAttempt {
        RequestAttempt {
            id,
            request_log_id: 10,
            attempt_index: index,
            candidate_position: index,
            provider_id: None,
            provider_api_key_id: None,
            model_id: None,
            provider_key_snapshot: None,
            provider_name_snapshot: None,
            model_name_snapshot: None,
            real_model_name_snapshot: None,
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: status,
            scheduler_action: match status {
                RequestAttemptStatus::Skipped => SchedulerAction::FallbackNextCandidate,
                RequestAttemptStatus::Success => SchedulerAction::ReturnSuccess,
                RequestAttemptStatus::Error => SchedulerAction::RetrySameCandidate,
                RequestAttemptStatus::Cancelled => SchedulerAction::FailFast,
            },
            error_code: None,
            error_message: None,
            request_uri: None,
            request_headers_json: None,
            response_headers_json: None,
            http_status: None,
            started_at: Some(100 + i64::from(index)),
            first_byte_at: None,
            completed_at: Some(110 + i64::from(index)),
            response_started_to_client: false,
            backoff_ms: None,
            applied_request_patch_ids_json: None,
            request_patch_summary_json: None,
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            cost_catalog_version_id: None,
            total_input_tokens: None,
            total_output_tokens: None,
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            total_tokens: None,
            llm_request_blob_id: None,
            llm_request_patch_id: None,
            llm_response_blob_id: None,
            llm_response_capture_state: None,
            created_at: 100,
            updated_at: 120,
        }
    }

    #[test]
    fn sqlite_request_attempt_persists_statuses_and_orders_by_attempt_index() {
        let (_temp_dir, mut conn) = sqlite_connection();
        seed_request_log(&mut conn);

        let attempts = vec![
            attempt(4, 4, RequestAttemptStatus::Cancelled),
            attempt(2, 2, RequestAttemptStatus::Success),
            attempt(1, 1, RequestAttemptStatus::Skipped),
            attempt(3, 3, RequestAttemptStatus::Error),
        ];
        let inserted = insert_many_sqlite(&mut conn, &attempts).expect("attempts should insert");
        assert_eq!(inserted.len(), 4);

        use crate::database::_sqlite_schema::request_attempt;
        use _sqlite_model::*;

        let loaded = request_attempt::table
            .filter(request_attempt::dsl::request_log_id.eq(10))
            .order(request_attempt::dsl::attempt_index.asc())
            .select(RequestAttemptDb::as_select())
            .load::<RequestAttemptDb>(&mut conn)
            .expect("attempts should load")
            .into_iter()
            .map(RequestAttemptDb::from_db)
            .collect::<Vec<_>>();

        assert_eq!(
            loaded
                .iter()
                .map(|attempt| attempt.attempt_index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(
            loaded
                .iter()
                .map(|attempt| attempt.attempt_status)
                .collect::<Vec<_>>(),
            vec![
                RequestAttemptStatus::Skipped,
                RequestAttemptStatus::Success,
                RequestAttemptStatus::Error,
                RequestAttemptStatus::Cancelled,
            ]
        );

        assert!(
            insert_many_sqlite(&mut conn, &[attempt(5, 1, RequestAttemptStatus::Error)]).is_err()
        );
    }
}
