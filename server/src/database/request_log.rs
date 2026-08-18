use diesel::prelude::*;
use serde::{Deserialize, Serialize};

use super::{
    DbResult, ListResult,
    runtime::{DatabaseRuntime, DatabaseWorkload, db_execute as async_db_execute},
};
use crate::controller::BaseError;
use crate::db_object;
use crate::schema::enum_def::{
    DownstreamProtocol, ModelKind, RequestStatus, UpstreamProfileType, UpstreamProtocol,
};

db_object! {
    #[derive(Insertable, Queryable, Selectable, Identifiable, AsChangeset, Serialize, Debug, Clone)]
    #[diesel(table_name = request_log)]
    pub struct RequestLog {
        pub id: i64,
        pub request_id: String,
        pub client_request_id: Option<String>,
        pub api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub base_requested_model_name: Option<String>,
        pub resolved_patch_suffix: Option<String>,
        pub downstream_protocol: DownstreamProtocol,
        #[diesel(column_name = status)]
        pub overall_status: RequestStatus,
        pub final_error_code: Option<String>,
        pub final_error_message: Option<String>,
        pub request_received_at: i64,
        pub upstream_request_sent_at: Option<i64>,
        pub upstream_response_headers_at: Option<i64>,
        pub upstream_first_body_chunk_at: Option<i64>,
        pub first_response_body_at: Option<i64>,
        pub first_token_at: Option<i64>,
        pub max_upstream_response_idle_ms: Option<i64>,
        pub completed_at: Option<i64>,
        pub is_stream: bool,
        pub client_ip: Option<String>,
        pub provider_id: Option<i64>,
        pub provider_api_key_id: Option<i64>,
        pub model_id: Option<i64>,
        pub source_id: Option<i64>,
        pub source_selection_reason: Option<String>,
        pub provider_key_snapshot: Option<String>,
        pub provider_name_snapshot: Option<String>,
        pub model_name_snapshot: Option<String>,
        pub real_model_name_snapshot: Option<String>,
        pub model_kind_snapshot: Option<ModelKind>,
        pub source_profile_type_snapshot: Option<UpstreamProfileType>,
        pub source_base_url_snapshot: Option<String>,
        pub upstream_protocol: Option<UpstreamProtocol>,
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
        pub request_id: String,
        pub client_request_id: Option<String>,
        pub api_key_id: i64,
        pub requested_model_name: Option<String>,
        pub base_requested_model_name: Option<String>,
        pub resolved_patch_suffix: Option<String>,
        #[diesel(column_name = status)]
        pub overall_status: RequestStatus,
        pub request_received_at: i64,
        pub upstream_request_sent_at: Option<i64>,
        pub first_response_body_at: Option<i64>,
        pub first_token_at: Option<i64>,
        pub completed_at: Option<i64>,
        pub is_stream: bool,
        pub provider_id: Option<i64>,
        pub provider_name_snapshot: Option<String>,
        pub model_id: Option<i64>,
        pub model_name_snapshot: Option<String>,
        pub real_model_name_snapshot: Option<String>,
        pub model_kind_snapshot: Option<ModelKind>,
        pub source_id: Option<i64>,
        pub source_selection_reason: Option<String>,
        pub source_profile_type_snapshot: Option<UpstreamProfileType>,
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
    pub source_id: Option<i64>,
    pub status: Option<RequestStatus>,
    pub downstream_protocol: Option<DownstreamProtocol>,
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
    pub async fn insert(
        database: &DatabaseRuntime,
        new_log_data: &RequestLog,
    ) -> DbResult<RequestLog> {
        let new_log_data = new_log_data.clone();
        database
            .run_db(DatabaseWorkload::Background, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        use diesel_async::AsyncConnection;
                        conn.transaction(async move |conn| {
                            let query = diesel::insert_into(request_log::table)
                                .values(RequestLogDb::to_db(&new_log_data))
                                .returning(RequestLogDb::as_returning());
                            let inserted_log_db =
                                diesel_async::RunQueryDsl::get_result::<RequestLogDb>(
                                    query,
                                    &mut *conn,
                                )
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to insert request log: {}",
                                        error
                                    )))
                                })?;

                            if let Some(cost_catalog_version_id) =
                                new_log_data.cost_catalog_version_id
                            {
                                let query = diesel::update(
                                    cost_catalog_versions::table.filter(
                                        cost_catalog_versions::dsl::id
                                            .eq(cost_catalog_version_id)
                                            .and(
                                                cost_catalog_versions::dsl::first_used_at.is_null(),
                                            ),
                                    ),
                                )
                                .set((
                                    cost_catalog_versions::dsl::first_used_at
                                        .eq(Some(new_log_data.request_received_at)),
                                    cost_catalog_versions::dsl::updated_at
                                        .eq(new_log_data.updated_at),
                                ));
                                diesel_async::RunQueryDsl::execute(query, &mut *conn)
                                    .await
                                    .map_err(|error| {
                                        BaseError::DatabaseFatal(Some(format!(
                                            "Failed to freeze cost catalog version {} after request log insert: {}",
                                            cost_catalog_version_id, error
                                        )))
                                    })?;
                            }

                            Ok(inserted_log_db.from_db())
                        })
                        .await
                    })
                })
            })
            .await
    }

    pub async fn get_by_id(database: &DatabaseRuntime, log_id: i64) -> DbResult<RequestLogRecord> {
        Self::get_by_id_with_workload_runtime(database, log_id, DatabaseWorkload::Foreground).await
    }

    pub(crate) async fn get_by_id_for_metrics_runtime(
        database: &DatabaseRuntime,
        log_id: i64,
    ) -> DbResult<RequestLogRecord> {
        Self::get_by_id_with_workload_runtime(database, log_id, DatabaseWorkload::Background).await
    }

    async fn get_by_id_with_workload_runtime(
        database: &DatabaseRuntime,
        log_id: i64,
        workload: DatabaseWorkload,
    ) -> DbResult<RequestLogRecord> {
        database
            .run_db(workload, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let query = request_log::table
                            .find(log_id)
                            .select(RequestLogDb::as_select());
                        diesel_async::RunQueryDsl::first::<RequestLogDb>(query, &mut **conn)
                            .await
                            .map(|row| row.from_db())
                            .map_err(|error| match error {
                                diesel::result::Error::NotFound => BaseError::NotFound(Some(
                                    format!("Request log with id {} not found", log_id),
                                )),
                                other => BaseError::DatabaseFatal(Some(format!(
                                    "Error fetching request log {}: {}",
                                    log_id, other
                                ))),
                            })
                    })
                })
            })
            .await
    }

    pub async fn list(
        database: &DatabaseRuntime,
        payload: RequestLogQueryPayload,
    ) -> DbResult<ListResult<RequestLogListItem>> {
        let page_size = payload.page_size.unwrap_or(20);
        let page = payload.page.unwrap_or(1);
        let offset = (page - 1) * page_size;
        database
            .run_db(DatabaseWorkload::Foreground, move |connection| {
                Box::pin(async move {
                    async_db_execute!(connection as conn, {
                        let mut query = request_log::table.into_boxed();
                        let mut count_query = request_log::table.into_boxed();

                        if let Some(value) = payload.api_key_id {
                            query = query.filter(request_log::dsl::api_key_id.eq(value));
                            count_query =
                                count_query.filter(request_log::dsl::api_key_id.eq(value));
                        }
                        if let Some(value) = payload.provider_id {
                            query = query.filter(request_log::dsl::provider_id.eq(Some(value)));
                            count_query =
                                count_query.filter(request_log::dsl::provider_id.eq(Some(value)));
                        }
                        if let Some(value) = payload.model_id {
                            query = query.filter(request_log::dsl::model_id.eq(Some(value)));
                            count_query =
                                count_query.filter(request_log::dsl::model_id.eq(Some(value)));
                        }
                        if let Some(value) = payload.source_id {
                            query = query.filter(request_log::dsl::source_id.eq(Some(value)));
                            count_query =
                                count_query.filter(request_log::dsl::source_id.eq(Some(value)));
                        }
                        if let Some(value) = payload.status {
                            query = query.filter(request_log::dsl::status.eq(value.clone()));
                            count_query = count_query.filter(request_log::dsl::status.eq(value));
                        }
                        if let Some(value) = payload.downstream_protocol {
                            query = query.filter(request_log::dsl::downstream_protocol.eq(value));
                            count_query =
                                count_query.filter(request_log::dsl::downstream_protocol.eq(value));
                        }
                        if let Some(value) = payload.final_error_code.as_ref()
                            && !value.is_empty()
                        {
                            query =
                                query.filter(request_log::dsl::final_error_code.eq(Some(value)));
                            count_query = count_query
                                .filter(request_log::dsl::final_error_code.eq(Some(value)));
                        }
                        if let Some(value) = payload.latency_ms_min {
                            let filter = request_log::dsl::completed_at.is_not_null().and(
                                request_log::dsl::completed_at
                                    .assume_not_null()
                                    .ge(request_log::dsl::request_received_at + value),
                            );
                            query = query.filter(filter.clone());
                            count_query = count_query.filter(filter);
                        }
                        if let Some(value) = payload.latency_ms_max {
                            let filter = request_log::dsl::completed_at.is_not_null().and(
                                request_log::dsl::completed_at
                                    .assume_not_null()
                                    .le(request_log::dsl::request_received_at + value),
                            );
                            query = query.filter(filter.clone());
                            count_query = count_query.filter(filter);
                        }
                        if let Some(value) = payload.total_tokens_min {
                            let filter = request_log::dsl::total_tokens
                                .is_not_null()
                                .and(request_log::dsl::total_tokens.assume_not_null().ge(value));
                            query = query.filter(filter.clone());
                            count_query = count_query.filter(filter);
                        }
                        if let Some(value) = payload.total_tokens_max {
                            let filter = request_log::dsl::total_tokens
                                .is_not_null()
                                .and(request_log::dsl::total_tokens.assume_not_null().le(value));
                            query = query.filter(filter.clone());
                            count_query = count_query.filter(filter);
                        }
                        if let Some(value) = payload.estimated_cost_nanos_min {
                            let filter = request_log::dsl::estimated_cost_nanos.is_not_null().and(
                                request_log::dsl::estimated_cost_nanos
                                    .assume_not_null()
                                    .ge(value),
                            );
                            query = query.filter(filter.clone());
                            count_query = count_query.filter(filter);
                        }
                        if let Some(value) = payload.estimated_cost_nanos_max {
                            let filter = request_log::dsl::estimated_cost_nanos.is_not_null().and(
                                request_log::dsl::estimated_cost_nanos
                                    .assume_not_null()
                                    .le(value),
                            );
                            query = query.filter(filter.clone());
                            count_query = count_query.filter(filter);
                        }
                        if let Some(search_term) = payload.search.as_ref()
                            && !search_term.is_empty()
                        {
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
                                .or(request_log::dsl::resolved_patch_suffix.is_not_null().and(
                                    request_log::dsl::resolved_patch_suffix
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
                                ))
                                .or(request_log::dsl::request_id.eq(search_term))
                                .or(request_log::dsl::client_request_id.eq(Some(search_term)));

                            if let Ok(id_search) = search_term.parse::<i64>() {
                                let search_filter =
                                    request_log::dsl::id.eq(id_search).or(text_filter);
                                query = query.filter(search_filter.clone());
                                count_query = count_query.filter(search_filter);
                            } else {
                                query = query.filter(text_filter.clone());
                                count_query = count_query.filter(text_filter);
                            }
                        }
                        if let Some(start_time) = payload.start_time {
                            query =
                                query.filter(request_log::dsl::request_received_at.ge(start_time));
                            count_query = count_query
                                .filter(request_log::dsl::request_received_at.ge(start_time));
                        }
                        if let Some(end_time) = payload.end_time {
                            query =
                                query.filter(request_log::dsl::request_received_at.le(end_time));
                            count_query = count_query
                                .filter(request_log::dsl::request_received_at.le(end_time));
                        }

                        let count_query = count_query.select(diesel::dsl::count_star());
                        let total =
                            diesel_async::RunQueryDsl::first::<i64>(count_query, &mut **conn)
                                .await
                                .map_err(|error| {
                                    BaseError::DatabaseFatal(Some(format!(
                                        "Failed to count request logs: {}",
                                        error
                                    )))
                                })?;

                        let list_query = query
                            .order(request_log::dsl::request_received_at.desc())
                            .limit(page_size)
                            .offset(offset)
                            .select(RequestLogListItemDb::as_select());
                        let list = diesel_async::RunQueryDsl::load::<RequestLogListItemDb>(
                            list_query,
                            &mut **conn,
                        )
                        .await
                        .map(|rows| rows.into_iter().map(|row| row.from_db()).collect())
                        .map_err(|error| {
                            BaseError::DatabaseFatal(Some(format!(
                                "Failed to list request logs: {}",
                                error
                            )))
                        })?;

                        Ok(ListResult {
                            total,
                            page,
                            page_size,
                            list,
                        })
                    })
                })
            })
            .await
    }

    #[cfg(test)]
    pub async fn list_full(
        database: &DatabaseRuntime,
        payload: RequestLogQueryPayload,
    ) -> DbResult<ListResult<RequestLogRecord>> {
        let page = Self::list(database, payload).await?;
        let mut list = Vec::with_capacity(page.list.len());
        for item in page.list {
            list.push(Self::get_by_id(database, item.id).await?);
        }
        Ok(ListResult {
            total: page.total,
            page: page.page,
            page_size: page.page_size,
            list,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{RequestLog, RequestLogQueryPayload};
    use crate::{
        database::{
            TestDatabase,
            api_key::{ApiKey, ApiKeyIssuance, CreateApiKeyPayload},
            runtime::DatabaseRuntime,
        },
        schema::enum_def::{Action, DownstreamProtocol, RequestStatus, UpstreamProtocol},
        utils::ID_GENERATOR,
    };

    fn api_key_payload() -> CreateApiKeyPayload {
        CreateApiKeyPayload {
            name: format!("request-log-protocol-{}", ID_GENERATOR.generate_id()),
            description: None,
            default_action: Some(Action::Allow),
            is_enabled: Some(true),
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
            acl_rules: None,
        }
    }

    async fn create_api_key(database: &DatabaseRuntime) -> i64 {
        let secret = format!("cyder-request-log-{}", ID_GENERATOR.generate_id());
        let issuance = ApiKeyIssuance::new(ID_GENERATOR.generate_id(), &secret, None);
        ApiKey::create_issued(database, &api_key_payload(), &issuance)
            .await
            .expect("api key should create")
            .id
    }

    fn request_log(
        id: i64,
        api_key_id: i64,
        downstream_protocol: DownstreamProtocol,
        upstream_protocol: Option<UpstreamProtocol>,
    ) -> RequestLog {
        RequestLog {
            id,
            request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            client_request_id: Some("repository-test".to_string()),
            api_key_id,
            requested_model_name: Some("provider/model".to_string()),
            base_requested_model_name: Some("provider/model".to_string()),
            resolved_patch_suffix: None,
            downstream_protocol,
            overall_status: RequestStatus::Success,
            final_error_code: None,
            final_error_message: None,
            request_received_at: 1_000,
            upstream_request_sent_at: None,
            upstream_response_headers_at: None,
            upstream_first_body_chunk_at: None,
            first_response_body_at: None,
            first_token_at: None,
            max_upstream_response_idle_ms: None,
            completed_at: Some(1_001),
            is_stream: false,
            client_ip: None,
            provider_id: None,
            provider_api_key_id: None,
            model_id: None,
            source_id: None,
            source_selection_reason: None,
            provider_key_snapshot: None,
            provider_name_snapshot: None,
            model_name_snapshot: None,
            real_model_name_snapshot: None,
            model_kind_snapshot: None,
            source_profile_type_snapshot: None,
            source_base_url_snapshot: None,
            upstream_protocol,
            upstream_http_status: None,
            estimated_cost_nanos: None,
            estimated_cost_currency: None,
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
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
            created_at: 1_000,
            updated_at: 1_001,
        }
    }

    #[tokio::test]
    async fn sqlite_request_log_accepts_directional_protocol_domains_and_null_upstream() {
        let context =
            TestDatabase::new_sqlite_default("request-log-directional-protocols.sqlite").await;
        let database = context.runtime();
        let api_key = create_api_key(&database).await;
        let cases = [
            (DownstreamProtocol::Openai, None),
            (
                DownstreamProtocol::Responses,
                Some(UpstreamProtocol::Openai),
            ),
            (
                DownstreamProtocol::Anthropic,
                Some(UpstreamProtocol::Responses),
            ),
            (
                DownstreamProtocol::Gemini,
                Some(UpstreamProtocol::Anthropic),
            ),
            (DownstreamProtocol::Openai, Some(UpstreamProtocol::Gemini)),
            (
                DownstreamProtocol::Responses,
                Some(UpstreamProtocol::Openai),
            ),
        ];
        let first_id = ID_GENERATOR.generate_id();
        for (index, (downstream, upstream)) in cases.iter().cloned().enumerate() {
            RequestLog::insert(
                &database,
                &request_log(first_id + index as i64, api_key, downstream, upstream),
            )
            .await
            .expect("directional request log should insert");
        }

        let logs = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                page_size: Some(20),
                ..Default::default()
            },
        )
        .await
        .expect("request logs should list")
        .list;
        assert_eq!(logs.len(), cases.len());
        assert!(
            RequestLog::get_by_id(&database, first_id)
                .await
                .expect("request log detail should load")
                .upstream_protocol
                .is_none()
        );
    }

    #[tokio::test]
    async fn sqlite_request_log_persists_and_queries_all_terminal_statuses() {
        let context =
            TestDatabase::new_sqlite_default("request-log-terminal-statuses.sqlite").await;
        let database = context.runtime();
        let api_key_id = create_api_key(&database).await;
        let statuses = [
            RequestStatus::Success,
            RequestStatus::Error,
            RequestStatus::Cancelled,
        ];

        for (index, status) in statuses.iter().cloned().enumerate() {
            let id = ID_GENERATOR.generate_id();
            let mut log = request_log(
                id,
                api_key_id,
                DownstreamProtocol::Openai,
                Some(UpstreamProtocol::Openai),
            );
            log.overall_status = status.clone();
            if status != RequestStatus::Success {
                log.final_error_code = Some(format!("terminal-{index}"));
                log.final_error_message = Some("terminal request outcome".to_string());
            }
            RequestLog::insert(&database, &log)
                .await
                .expect("terminal request log should insert");
            let detail = RequestLog::get_by_id(&database, id)
                .await
                .expect("terminal request log detail should load");
            assert_eq!(detail.overall_status, status);
        }

        for status in statuses {
            let result = RequestLog::list(
                &database,
                RequestLogQueryPayload {
                    status: Some(status),
                    ..Default::default()
                },
            )
            .await
            .expect("terminal status filter should query");
            assert_eq!(result.total, 1);
        }
    }

    #[tokio::test]
    async fn sqlite_request_log_rejects_invalid_directional_protocol_values() {
        let database =
            TestDatabase::new_sqlite_default("request-log-invalid-protocols.sqlite").await;
        let api_key_id = create_api_key(database.runtime().as_ref()).await;

        let invalid_downstream = database.execute_sqlite_batch(format!(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol, overall_status,
                    request_received_at, is_stream, created_at, updated_at
                ) VALUES ({}, '018fa7d8-6a00-4c9a-8f7e-111111111111', {}, 'OLLAMA', 'SUCCESS', 1000, 0, 1000, 1000)",
                ID_GENERATOR.generate_id(),
                api_key_id
            )).await;
        assert!(invalid_downstream.is_err());

        let invalid_upstream = database.execute_sqlite_batch(format!(
                "INSERT INTO request_log (
                    id, request_id, api_key_id, downstream_protocol, upstream_protocol,
                    overall_status, request_received_at, is_stream, created_at, updated_at
                ) VALUES ({}, '018fa7d8-6a00-4c9a-9f7e-222222222222', {}, 'OPENAI', 'GEMINI_OPENAI', 'SUCCESS', 1000, 0, 1000, 1000)",
                ID_GENERATOR.generate_id(),
                api_key_id
            )).await;
        assert!(invalid_upstream.is_err());
    }

    #[tokio::test]
    async fn sqlite_request_log_searches_request_identity_exactly_and_preserves_existing_searches()
    {
        let context = TestDatabase::new_sqlite_default("request-log-identity-search.sqlite").await;
        let database = context.runtime();
        let api_key_id = create_api_key(&database).await;
        let first_id = ID_GENERATOR.generate_id();
        let second_id = ID_GENERATOR.generate_id();
        let third_id = ID_GENERATOR.generate_id();
        let first_request_id = "018fa7d8-6a00-4c9a-8f7e-111111111111";
        let second_request_id = "018fa7d8-6a00-4c9a-8f7e-222222222222";
        let third_request_id = "018fa7d8-6a00-4c9a-8f7e-333333333333";

        let mut first = request_log(
            first_id,
            api_key_id,
            DownstreamProtocol::Openai,
            Some(UpstreamProtocol::Openai),
        );
        first.request_id = first_request_id.to_string();
        first.client_request_id = Some("shared-client-request".to_string());
        first.model_name_snapshot = Some("searchable-model".to_string());

        let mut second = request_log(
            second_id,
            api_key_id,
            DownstreamProtocol::Responses,
            Some(UpstreamProtocol::Responses),
        );
        second.request_id = second_request_id.to_string();
        second.client_request_id = Some("shared-client-request".to_string());

        let mut third = request_log(
            third_id,
            api_key_id,
            DownstreamProtocol::Anthropic,
            Some(UpstreamProtocol::Anthropic),
        );
        third.request_id = third_request_id.to_string();
        third.client_request_id = Some("different-client-request".to_string());

        for log in [&first, &second, &third] {
            RequestLog::insert(&database, log)
                .await
                .expect("request log should insert");
        }

        let canonical = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                search: Some(first_request_id.to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("canonical request id search should succeed");
        assert_eq!(canonical.total, 1);
        assert_eq!(canonical.list[0].id, first_id);
        assert_eq!(canonical.list[0].request_id, first_request_id);
        assert_eq!(
            canonical.list[0].client_request_id.as_deref(),
            Some("shared-client-request")
        );

        let partial_canonical = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                search: Some("018fa7d8-6a00-4c9a".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("partial canonical request id search should succeed");
        assert_eq!(partial_canonical.total, 0);

        let duplicate_client = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                search: Some("shared-client-request".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("client request id search should succeed");
        assert_eq!(duplicate_client.total, 2);
        assert!(
            duplicate_client
                .list
                .iter()
                .all(|log| log.client_request_id.as_deref() == Some("shared-client-request"))
        );

        let partial_client = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                search: Some("shared-client".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("partial client request id search should succeed");
        assert_eq!(partial_client.total, 0);

        let text_search = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                search: Some("searchable".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("existing text search should succeed");
        assert_eq!(text_search.total, 1);
        assert_eq!(text_search.list[0].id, first_id);

        let numeric_search = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                search: Some(first_id.to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("existing numeric record id search should succeed");
        assert_eq!(numeric_search.total, 1);
        assert_eq!(numeric_search.list[0].id, first_id);

        let filtered_client = RequestLog::list(
            &database,
            RequestLogQueryPayload {
                downstream_protocol: Some(DownstreamProtocol::Openai),
                search: Some("shared-client-request".to_string()),
                ..Default::default()
            },
        )
        .await
        .expect("identity search should compose with existing filters");
        assert_eq!(filtered_client.total, 1);
        assert_eq!(filtered_client.list[0].id, first_id);
    }
}
