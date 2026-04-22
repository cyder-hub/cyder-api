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
    use diesel::Connection;
    use diesel::connection::SimpleConnection;
    use diesel::sql_types::{BigInt, Integer, Nullable, Text};
    use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
    use tempfile::tempdir;

    const SQLITE_MIGRATIONS: EmbeddedMigrations = embed_migrations!("migrations/sqlite");

    fn sqlite_connection() -> (tempfile::TempDir, SqliteConnection) {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("request-attempt.sqlite");
        std::fs::File::create(&db_path).expect("db file should be created");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut conn =
            SqliteConnection::establish(&db_url).expect("sqlite connection should be established");
        conn.run_pending_migrations(SQLITE_MIGRATIONS)
            .expect("migrations should run");
        (temp_dir, conn)
    }

    fn apply_sql(connection: &mut SqliteConnection, sql_text: &str) {
        if let Err(err) = connection.batch_execute(sql_text) {
            panic!("sql should execute successfully: {err}\n{sql_text}");
        }
    }

    fn sqlite_connection_before_routing_resilience() -> (tempfile::TempDir, SqliteConnection) {
        let temp_dir = tempdir().expect("temp dir should be created");
        let db_path = temp_dir.path().join("legacy-routing.sqlite");
        std::fs::File::create(&db_path).expect("db file should be created");
        let db_url = db_path.to_string_lossy().into_owned();
        let mut conn =
            SqliteConnection::establish(&db_url).expect("sqlite connection should be established");

        for sql in [
            include_str!("../../migrations/sqlite/2025-03-20-062357_initial_setup/up.sql"),
            include_str!("../../migrations/sqlite/2025-07-02-140210_api_key_jwt/up.sql"),
            include_str!("../../migrations/sqlite/2026-01-28-233111_request_log_optimize/up.sql"),
            include_str!("../../migrations/sqlite/2026-02-03-230221_request_log_field_opt/up.sql"),
            include_str!(
                "../../migrations/sqlite/2026-04-08-090000_expand_llm_api_type_for_request_log/up.sql"
            ),
            include_str!("../../migrations/sqlite/2026-04-10-120000_cost_schema_foundation/up.sql"),
            include_str!(
                "../../migrations/sqlite/2026-04-14-090000_cost_catalog_version_freeze_flags/up.sql"
            ),
            include_str!("../../migrations/sqlite/2026-04-17-100000_model_route_foundation/up.sql"),
            include_str!(
                "../../migrations/sqlite/2026-04-17-120000_api_key_governance_foundation/up.sql"
            ),
            include_str!(
                "../../migrations/sqlite/2026-04-17-130000_request_log_route_trace/up.sql"
            ),
            include_str!(
                "../../migrations/sqlite/2026-04-20-120000_request_patch_rule_foundation/up.sql"
            ),
        ] {
            apply_sql(&mut conn, sql);
        }

        (temp_dir, conn)
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

    #[derive(QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }

    #[derive(QueryableByName)]
    struct LegacyMigrationRow {
        #[diesel(sql_type = BigInt)]
        id: i64,
        #[diesel(sql_type = Text)]
        overall_status: String,
        #[diesel(sql_type = Nullable<Text>)]
        final_error_code: Option<String>,
        #[diesel(sql_type = BigInt)]
        final_attempt_id: i64,
        #[diesel(sql_type = Integer)]
        attempt_count: i32,
        #[diesel(sql_type = Nullable<Integer>)]
        bundle_version: Option<i32>,
        #[diesel(sql_type = Nullable<Text>)]
        bundle_storage_key: Option<String>,
        #[diesel(sql_type = Text)]
        attempt_status: String,
        #[diesel(sql_type = Text)]
        scheduler_action: String,
        #[diesel(sql_type = Nullable<Text>)]
        attempt_error_code: Option<String>,
        #[diesel(sql_type = Nullable<Text>)]
        request_uri: Option<String>,
        #[diesel(sql_type = Nullable<Integer>)]
        http_status: Option<i32>,
        #[diesel(sql_type = Integer)]
        response_started_to_client: i32,
        #[diesel(sql_type = Nullable<Text>)]
        provider_key_snapshot: Option<String>,
        #[diesel(sql_type = Nullable<Text>)]
        applied_request_patch_ids_json: Option<String>,
    }

    fn seed_legacy_request_logs(conn: &mut SqliteConnection) {
        conn.batch_execute(
            "INSERT INTO system_api_key (
                id, api_key, name, description, access_control_policy_id, usage_limit_policy_id,
                is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'ck-legacy', 'Legacy key', NULL, NULL, NULL,
                1, NULL, 1, 1
            );

            INSERT INTO api_key (
                id, api_key, api_key_hash, key_prefix, key_last4, name, description,
                default_action, is_enabled, expires_at, rate_limit_rpm, max_concurrent_requests,
                quota_daily_requests, quota_daily_tokens, quota_monthly_tokens,
                budget_daily_nanos, budget_daily_currency, budget_monthly_nanos,
                budget_monthly_currency, deleted_at, created_at, updated_at
            ) VALUES (
                1, 'ck-legacy', 'legacy-hash', 'ck-legacy', 'gacy', 'Legacy key', NULL,
                'ALLOW', 1, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, 1, 1
            );

            INSERT INTO provider (
                id, provider_key, name, endpoint, use_proxy, is_enabled, deleted_at,
                created_at, updated_at, provider_type, provider_api_key_mode
            ) VALUES (
                10, 'openai-main', 'OpenAI Main', 'https://api.example.com/v1', 0, 1, NULL,
                1, 1, 'OPENAI', 'QUEUE'
            );

            INSERT INTO provider_api_key (
                id, provider_id, api_key, description, deleted_at, is_enabled, created_at, updated_at
            ) VALUES (
                20, 10, 'sk-provider', NULL, NULL, 1, 1, 1
            );

            INSERT INTO model (
                id, provider_id, cost_catalog_id, model_name, real_model_name,
                is_enabled, deleted_at, created_at, updated_at
            ) VALUES (
                30, 10, NULL, 'gpt-test', 'gpt-test-real',
                1, NULL, 1, 1
            );",
        )
        .expect("legacy dependencies should insert");

        conn.batch_execute(
            "INSERT INTO request_log (
                id, system_api_key_id, provider_id, model_id, provider_api_key_id,
                requested_model_name, resolved_name_scope, resolved_route_id, resolved_route_name,
                model_name, real_model_name, request_received_at, llm_request_sent_at,
                llm_response_first_chunk_at, llm_response_completed_at, client_ip,
                llm_request_uri, llm_response_status, status, is_stream,
                estimated_cost_nanos, estimated_cost_currency, cost_catalog_id,
                cost_catalog_version_id, cost_snapshot_json, created_at, updated_at,
                total_input_tokens, total_output_tokens, input_text_tokens, output_text_tokens,
                input_image_tokens, output_image_tokens, cache_read_tokens, cache_write_tokens,
                reasoning_tokens, total_tokens, storage_type,
                user_request_body, llm_request_body, llm_response_body, user_response_body,
                user_api_type, llm_api_type,
                applied_request_patch_ids_json, request_patch_summary_json
            ) VALUES
            (
                123456, 1, 10, 30, 20,
                'gpt-test', 'direct', NULL, NULL,
                'gpt-test', 'gpt-test-real', 1744100800000, 1744100800100,
                1744100800200, 1744100800300, '127.0.0.1',
                'https://api.example.com/v1/chat/completions', 200, 'SUCCESS', 0,
                1000, 'USD', NULL,
                NULL, NULL, 1744100800000, 1744100800400,
                10, 20, 10, 20,
                0, 0, 0, 0,
                0, 30, 'FILE_SYSTEM',
                NULL, NULL, NULL, NULL,
                'OPENAI', 'OPENAI',
                '[101]', '{\"rules\":1}'
            ),
            (
                123457, 1, 10, 30, 20,
                'gpt-test', 'direct', NULL, NULL,
                'gpt-test', 'gpt-test-real', 1744100800000, 1744100800100,
                NULL, 1744100800300, '127.0.0.1',
                'https://api.example.com/v1/chat/completions', 500, 'ERROR', 0,
                2000, 'USD', NULL,
                NULL, NULL, 1744100800000, 1744100800400,
                11, 21, 11, 21,
                0, 0, 0, 0,
                0, 32, 'S3',
                NULL, NULL, NULL, NULL,
                'OPENAI', 'OPENAI',
                NULL, NULL
            ),
            (
                123458, 1, 10, 30, 20,
                'gpt-test', 'direct', NULL, NULL,
                'gpt-test', 'gpt-test-real', 1744100800000, 1744100800100,
                NULL, NULL, '127.0.0.1',
                NULL, NULL, 'CANCELLED', 0,
                NULL, NULL, NULL,
                NULL, NULL, 1744100800000, 1744100800400,
                NULL, NULL, NULL, NULL,
                NULL, NULL, NULL, NULL,
                NULL, NULL, NULL,
                NULL, NULL, NULL, NULL,
                'OPENAI', 'OPENAI',
                NULL, NULL
            ),
            (
                123459, 1, 10, 30, 20,
                'gpt-test', 'direct', NULL, NULL,
                'gpt-test', 'gpt-test-real', 1744100800000, 1744100800100,
                NULL, NULL, '127.0.0.1',
                'https://api.example.com/v1/chat/completions', NULL, 'PENDING', 0,
                NULL, NULL, NULL,
                NULL, NULL, 1744100800000, 1744100800400,
                NULL, NULL, NULL, NULL,
                NULL, NULL, NULL, NULL,
                NULL, NULL, 'FILE_SYSTEM',
                NULL, NULL, NULL, NULL,
                'OPENAI', 'OPENAI',
                NULL, NULL
            );",
        )
        .expect("legacy request logs should insert");
    }

    #[test]
    fn sqlite_routing_resilience_migration_splits_legacy_request_logs_into_attempts() {
        let (_temp_dir, mut conn) = sqlite_connection_before_routing_resilience();
        seed_legacy_request_logs(&mut conn);

        apply_sql(
            &mut conn,
            include_str!(
                "../../migrations/sqlite/2026-04-21-090000_routing_resilience_foundation/up.sql"
            ),
        );

        let request_count = diesel::sql_query("SELECT COUNT(*) AS count FROM request_log")
            .get_result::<CountRow>(&mut conn)
            .expect("request_log count should load")
            .count;
        let attempt_count = diesel::sql_query("SELECT COUNT(*) AS count FROM request_attempt")
            .get_result::<CountRow>(&mut conn)
            .expect("request_attempt count should load")
            .count;
        assert_eq!(request_count, 4);
        assert_eq!(attempt_count, 4);

        let rows = diesel::sql_query(
            "SELECT
                rl.id,
                rl.overall_status,
                rl.final_error_code,
                rl.final_attempt_id,
                rl.attempt_count,
                rl.bundle_version,
                rl.bundle_storage_key,
                ra.attempt_status,
                ra.scheduler_action,
                ra.error_code AS attempt_error_code,
                ra.request_uri,
                ra.http_status,
                CAST(ra.response_started_to_client AS INTEGER) AS response_started_to_client,
                ra.provider_key_snapshot,
                ra.applied_request_patch_ids_json
             FROM request_log AS rl
             JOIN request_attempt AS ra
               ON ra.request_log_id = rl.id
             ORDER BY rl.id ASC",
        )
        .load::<LegacyMigrationRow>(&mut conn)
        .expect("legacy migration rows should load");

        assert_eq!(rows.len(), 4);
        for row in &rows {
            assert_eq!(row.final_attempt_id, row.id);
            assert_eq!(row.attempt_count, 1);
            assert_eq!(row.provider_key_snapshot.as_deref(), Some("openai-main"));
        }

        let success = &rows[0];
        assert_eq!(success.overall_status, "SUCCESS");
        assert_eq!(success.final_error_code, None);
        assert_eq!(success.attempt_status, "SUCCESS");
        assert_eq!(success.scheduler_action, "RETURN_SUCCESS");
        assert_eq!(
            success.bundle_storage_key.as_deref(),
            Some("2025/04/08/12/123456.mp.gz")
        );
        assert_eq!(success.bundle_version, Some(1));
        assert_eq!(success.http_status, Some(200));
        assert_eq!(success.response_started_to_client, 1);
        assert_eq!(
            success.request_uri.as_deref(),
            Some("https://api.example.com/v1/chat/completions")
        );
        assert_eq!(
            success.applied_request_patch_ids_json.as_deref(),
            Some("[101]")
        );

        let error = &rows[1];
        assert_eq!(error.overall_status, "ERROR");
        assert_eq!(
            error.final_error_code.as_deref(),
            Some("legacy_request_log_error")
        );
        assert_eq!(error.attempt_status, "ERROR");
        assert_eq!(error.scheduler_action, "FAIL_FAST");
        assert_eq!(
            error.attempt_error_code.as_deref(),
            Some("legacy_request_log_error")
        );
        assert_eq!(
            error.bundle_storage_key.as_deref(),
            Some("logs/2025/04/08/123457.mp.gz")
        );

        let cancelled = &rows[2];
        assert_eq!(cancelled.overall_status, "CANCELLED");
        assert_eq!(
            cancelled.final_error_code.as_deref(),
            Some("client_cancelled_error")
        );
        assert_eq!(cancelled.attempt_status, "CANCELLED");
        assert_eq!(cancelled.bundle_version, None);

        let pending = &rows[3];
        assert_eq!(pending.overall_status, "ERROR");
        assert_eq!(
            pending.final_error_code.as_deref(),
            Some("legacy_pending_request_log_error")
        );
        assert_eq!(pending.attempt_status, "ERROR");
        assert_eq!(
            pending.attempt_error_code.as_deref(),
            Some("legacy_pending_request_log_error")
        );
    }
}
