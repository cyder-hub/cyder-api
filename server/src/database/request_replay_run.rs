use diesel::prelude::*;
use serde::Serialize;

use super::{DbResult, get_connection};
use crate::controller::BaseError;
use crate::schema::enum_def::{
    LlmApiType, RequestReplayKind, RequestReplayMode, RequestReplaySemanticBasis,
    RequestReplayStatus, StorageType,
};
use crate::{db_execute, db_object};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, AsChangeset, Serialize, Debug, Clone)]
    #[diesel(table_name = request_replay_run)]
    pub struct RequestReplayRun {
        pub id: i64,
        pub source_request_log_id: i64,
        pub source_attempt_id: Option<i64>,
        pub replay_kind: RequestReplayKind,
        pub replay_mode: RequestReplayMode,
        pub semantic_basis: RequestReplaySemanticBasis,
        pub status: RequestReplayStatus,
        pub executed_route_id: Option<i64>,
        pub executed_route_name: Option<String>,
        pub executed_provider_id: Option<i64>,
        pub executed_provider_api_key_id: Option<i64>,
        pub executed_model_id: Option<i64>,
        pub executed_llm_api_type: Option<LlmApiType>,
        pub downstream_request_uri: Option<String>,
        pub http_status: Option<i32>,
        pub error_code: Option<String>,
        pub error_message: Option<String>,
        pub total_input_tokens: Option<i32>,
        pub total_output_tokens: Option<i32>,
        pub reasoning_tokens: Option<i32>,
        pub total_tokens: Option<i32>,
        pub estimated_cost_nanos: Option<i64>,
        pub estimated_cost_currency: Option<String>,
        pub diff_summary_json: Option<String>,
        pub artifact_version: Option<i32>,
        pub artifact_storage_type: Option<StorageType>,
        pub artifact_storage_key: Option<String>,
        pub started_at: Option<i64>,
        pub first_byte_at: Option<i64>,
        pub completed_at: Option<i64>,
        pub created_at: i64,
        pub updated_at: i64,
    }
}

pub type RequestReplayRunRecord = RequestReplayRun;

impl RequestReplayRun {
    pub fn insert(new_run: &RequestReplayRun) -> DbResult<RequestReplayRunRecord> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::insert_into(request_replay_run::table)
                .values(RequestReplayRunDb::to_db(new_run))
                .returning(RequestReplayRunDb::as_returning())
                .get_result::<RequestReplayRunDb>(conn)
                .map(|row| row.from_db())
                .map_err(|err| {
                    map_request_replay_run_write_error("Failed to insert request replay run", err)
                })
        })
    }

    pub fn update(run: &RequestReplayRun) -> DbResult<RequestReplayRunRecord> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            diesel::update(request_replay_run::table.find(run.id))
                .set(RequestReplayRunDb::to_db(run))
                .returning(RequestReplayRunDb::as_returning())
                .get_result::<RequestReplayRunDb>(conn)
                .map(|row| row.from_db())
                .map_err(|err| map_request_replay_run_update_error(run.id, err))
        })
    }

    pub fn get_by_id(replay_run_id: i64) -> DbResult<RequestReplayRunRecord> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_replay_run::table
                .find(replay_run_id)
                .select(RequestReplayRunDb::as_select())
                .first::<RequestReplayRunDb>(conn)
                .map(|row| row.from_db())
                .map_err(|err| map_request_replay_run_read_error(replay_run_id, err))
        })
    }

    pub fn get_by_source_and_id(
        source_request_log_id: i64,
        replay_run_id: i64,
    ) -> DbResult<RequestReplayRunRecord> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_replay_run::table
                .filter(request_replay_run::dsl::source_request_log_id.eq(source_request_log_id))
                .filter(request_replay_run::dsl::id.eq(replay_run_id))
                .select(RequestReplayRunDb::as_select())
                .first::<RequestReplayRunDb>(conn)
                .map(|row| row.from_db())
                .map_err(|err| map_request_replay_run_read_error(replay_run_id, err))
        })
    }

    pub fn list_by_source_request_log_id(
        source_request_log_id: i64,
    ) -> DbResult<Vec<RequestReplayRunRecord>> {
        let conn = &mut get_connection()?;
        db_execute!(conn, {
            request_replay_run::table
                .filter(request_replay_run::dsl::source_request_log_id.eq(source_request_log_id))
                .order(request_replay_run::dsl::created_at.desc())
                .select(RequestReplayRunDb::as_select())
                .load::<RequestReplayRunDb>(conn)
                .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                .map_err(|err| {
                    BaseError::DatabaseFatal(Some(format!(
                        "Failed to list request replay runs for request_log {}: {}",
                        source_request_log_id, err
                    )))
                })
        })
    }
}

fn map_request_replay_run_write_error(context: &str, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        ) => BaseError::DatabaseDup(Some(format!("{context}: id must be unique"))),
        diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::ForeignKeyViolation,
            _,
        ) => BaseError::ParamInvalid(Some(format!(
            "{context}: source request_log or attempt does not exist"
        ))),
        other => BaseError::DatabaseFatal(Some(format!("{context}: {other}"))),
    }
}

fn map_request_replay_run_update_error(
    replay_run_id: i64,
    err: diesel::result::Error,
) -> BaseError {
    match err {
        diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
            "Request replay run with id {} not found",
            replay_run_id
        ))),
        other => map_request_replay_run_write_error("Failed to update request replay run", other),
    }
}

fn map_request_replay_run_read_error(replay_run_id: i64, err: diesel::result::Error) -> BaseError {
    match err {
        diesel::result::Error::NotFound => BaseError::NotFound(Some(format!(
            "Request replay run with id {} not found",
            replay_run_id
        ))),
        other => BaseError::DatabaseFatal(Some(format!(
            "Error fetching request replay run {}: {}",
            replay_run_id, other
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::connection::SimpleConnection;

    fn sqlite_connection() -> (tempfile::TempDir, diesel::SqliteConnection) {
        crate::database::open_test_sqlite_connection_with_migrations("request-replay-run.sqlite")
    }

    fn seed_source_request(conn: &mut diesel::SqliteConnection) {
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
                10, 1, 'OPENAI', 'SUCCESS', 1, 0, 0, 100, 100, 100
            );

            INSERT INTO request_attempt (
                id, request_log_id, attempt_index, candidate_position,
                attempt_status, scheduler_action, response_started_to_client,
                created_at, updated_at
            ) VALUES (
                101, 10, 1, 1,
                'SUCCESS', 'RETURN_SUCCESS', 1,
                100, 100
            );",
        )
        .expect("source request should insert");
    }

    fn replay_run(id: i64, created_at: i64) -> RequestReplayRun {
        RequestReplayRun {
            id,
            source_request_log_id: 10,
            source_attempt_id: Some(101),
            replay_kind: RequestReplayKind::AttemptUpstream,
            replay_mode: RequestReplayMode::Live,
            semantic_basis: RequestReplaySemanticBasis::HistoricalAttemptSnapshot,
            status: RequestReplayStatus::Running,
            created_at,
            updated_at: created_at,
            ..Default::default()
        }
    }

    fn insert_sqlite(
        conn: &mut diesel::SqliteConnection,
        run: &RequestReplayRun,
    ) -> DbResult<RequestReplayRunRecord> {
        use crate::database::_sqlite_schema::request_replay_run;
        use _sqlite_model::RequestReplayRunDb;

        diesel::insert_into(request_replay_run::table)
            .values(RequestReplayRunDb::to_db(run))
            .returning(RequestReplayRunDb::as_returning())
            .get_result::<RequestReplayRunDb>(conn)
            .map(|row| row.from_db())
            .map_err(|err| {
                map_request_replay_run_write_error("Failed to insert request replay run", err)
            })
    }

    fn update_sqlite(
        conn: &mut diesel::SqliteConnection,
        run: &RequestReplayRun,
    ) -> DbResult<RequestReplayRunRecord> {
        use crate::database::_sqlite_schema::request_replay_run;
        use _sqlite_model::RequestReplayRunDb;

        diesel::update(request_replay_run::table.find(run.id))
            .set(RequestReplayRunDb::to_db(run))
            .returning(RequestReplayRunDb::as_returning())
            .get_result::<RequestReplayRunDb>(conn)
            .map(|row| row.from_db())
            .map_err(|err| map_request_replay_run_update_error(run.id, err))
    }

    fn get_by_source_and_id_sqlite(
        conn: &mut diesel::SqliteConnection,
        source_request_log_id: i64,
        replay_run_id: i64,
    ) -> DbResult<RequestReplayRunRecord> {
        use crate::database::_sqlite_schema::request_replay_run;
        use _sqlite_model::RequestReplayRunDb;

        request_replay_run::table
            .filter(request_replay_run::dsl::source_request_log_id.eq(source_request_log_id))
            .filter(request_replay_run::dsl::id.eq(replay_run_id))
            .select(RequestReplayRunDb::as_select())
            .first::<RequestReplayRunDb>(conn)
            .map(|row| row.from_db())
            .map_err(|err| map_request_replay_run_read_error(replay_run_id, err))
    }

    fn list_by_source_sqlite(
        conn: &mut diesel::SqliteConnection,
        source_request_log_id: i64,
    ) -> DbResult<Vec<RequestReplayRunRecord>> {
        use crate::database::_sqlite_schema::request_replay_run;
        use _sqlite_model::RequestReplayRunDb;

        request_replay_run::table
            .filter(request_replay_run::dsl::source_request_log_id.eq(source_request_log_id))
            .order(request_replay_run::dsl::created_at.desc())
            .select(RequestReplayRunDb::as_select())
            .load::<RequestReplayRunDb>(conn)
            .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
            .map_err(|err| {
                BaseError::DatabaseFatal(Some(format!(
                    "Failed to list request replay runs for request_log {}: {}",
                    source_request_log_id, err
                )))
            })
    }

    #[test]
    fn sqlite_request_replay_run_create_update_get_and_list() {
        let (_temp_dir, mut conn) = sqlite_connection();
        seed_source_request(&mut conn);

        let first =
            insert_sqlite(&mut conn, &replay_run(1001, 200)).expect("replay run should insert");
        let second = insert_sqlite(&mut conn, &replay_run(1002, 300))
            .expect("second replay run should insert");

        assert_eq!(first.status, RequestReplayStatus::Running);
        assert_eq!(second.replay_kind, RequestReplayKind::AttemptUpstream);

        let mut updated = first.clone();
        updated.status = RequestReplayStatus::Success;
        updated.http_status = Some(200);
        updated.total_tokens = Some(42);
        updated.diff_summary_json = Some("{\"status_changed\":false}".to_string());
        updated.artifact_version = Some(1);
        updated.artifact_storage_type = Some(StorageType::FileSystem);
        updated.artifact_storage_key = Some("replays/2026/04/22/1001.mp.gz".to_string());
        updated.updated_at = 400;

        let updated = update_sqlite(&mut conn, &updated).expect("replay run should update");
        assert_eq!(updated.status, RequestReplayStatus::Success);
        assert_eq!(updated.artifact_version, Some(1));

        let loaded = get_by_source_and_id_sqlite(&mut conn, 10, 1001)
            .expect("replay run should load by source and id");
        assert_eq!(loaded.http_status, Some(200));
        assert_eq!(loaded.artifact_storage_type, Some(StorageType::FileSystem));

        let list = list_by_source_sqlite(&mut conn, 10).expect("replay runs should list");
        assert_eq!(
            list.iter().map(|run| run.id).collect::<Vec<_>>(),
            vec![1002, 1001]
        );

        let missing = get_by_source_and_id_sqlite(&mut conn, 999, 1001);
        assert!(matches!(missing, Err(BaseError::NotFound(_))));
    }

    #[test]
    fn sqlite_request_replay_run_requires_attempt_for_attempt_upstream() {
        let (_temp_dir, mut conn) = sqlite_connection();
        seed_source_request(&mut conn);

        let mut invalid = replay_run(1003, 200);
        invalid.source_attempt_id = None;

        let err = insert_sqlite(&mut conn, &invalid).expect_err("constraint should reject row");
        assert!(matches!(
            err,
            BaseError::DatabaseFatal(_) | BaseError::ParamInvalid(_)
        ));
    }
}
