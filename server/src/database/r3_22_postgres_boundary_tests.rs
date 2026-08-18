use std::{env, sync::Arc, time::Duration};

use axum::http::{HeaderMap, StatusCode};
use diesel::{QueryableByName, sql_types::BigInt};
use diesel_async::{AsyncConnection, RunQueryDsl, SimpleAsyncConnection};
use tokio::sync::Notify;

use crate::{
    config::{CONFIG, DatabaseIoConfig},
    database::{
        TestDatabase,
        api_key::CreateApiKeyPayload,
        metrics::count_ingested_request_log_markers,
        provider::{BootstrapProviderInput, Provider},
        request_log::RequestLog,
        runtime::{DatabaseBackendKind, DatabaseRuntime, DatabaseWorkload, RuntimeConnection},
        upstream_source::NewUpstreamSource,
    },
    proxy::{
        ApiKeyPosition, ProxyRequestContext, ResponseVisibility, admit_api_key_request,
        check_system_api_key, logging::RequestLogContext,
    },
    schema::enum_def::{
        Action, DownstreamProtocol, ModelKind, ProviderApiKeyMode, RequestStatus,
        UpstreamProfileType, UpstreamProtocol,
    },
    service::{
        app_state::create_test_app_state,
        secret_encryption::{SecretDomain, SensitiveSecret, prepare_secrets_before_startup},
        source_selector::SourceSelectionReason,
    },
    utils::ID_GENERATOR,
};

use super::error::PersistenceError;

const POSTGRES_BOUNDARY_URL_ENV: &str = "CYDER_R322_POSTGRES_SMOKE_URL";
const POSTGRES_BOUNDARY_DATABASE: &str = "cyder_r322_async_boundary";
const POSTGRES_BOUNDARY_USER: &str = "cyder_r322";

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    value: i64,
}

#[derive(QueryableByName)]
struct TimeoutSettingsRow {
    #[diesel(sql_type = BigInt)]
    statement_timeout_ms: i64,
    #[diesel(sql_type = BigInt)]
    idle_transaction_timeout_ms: i64,
}

fn boundary_config() -> DatabaseIoConfig {
    DatabaseIoConfig {
        max_waiters: 0,
        queue_wait_timeout_seconds: 1,
        operation_deadline_seconds: 1,
        sqlite_busy_timeout_seconds: 1,
    }
}

fn execution_error() -> PersistenceError {
    PersistenceError::execution(DatabaseBackendKind::Postgres, DatabaseWorkload::Foreground)
}

async fn run_postgres_batch(
    runtime: &DatabaseRuntime,
    statement: &'static str,
) -> Result<(), PersistenceError> {
    runtime
        .run(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                let RuntimeConnection::Postgres(connection) = connection else {
                    panic!("PostgreSQL boundary suite requires a PostgreSQL runtime");
                };
                connection
                    .batch_execute(statement)
                    .await
                    .map_err(|_| execution_error())
            })
        })
        .await
}

async fn postgres_count(runtime: &DatabaseRuntime, query: &'static str) -> i64 {
    runtime
        .run(DatabaseWorkload::Foreground, move |connection| {
            Box::pin(async move {
                let RuntimeConnection::Postgres(connection) = connection else {
                    panic!("PostgreSQL boundary suite requires a PostgreSQL runtime");
                };
                diesel::sql_query(query)
                    .get_result::<CountRow>(&mut **connection)
                    .await
                    .map(|row| row.value)
                    .map_err(|_| execution_error())
            })
        })
        .await
        .expect("PostgreSQL boundary count should query")
}

async fn timeout_settings(runtime: &DatabaseRuntime) -> TimeoutSettingsRow {
    runtime
        .run(DatabaseWorkload::Foreground, |connection| {
            Box::pin(async move {
                let RuntimeConnection::Postgres(connection) = connection else {
                    panic!("PostgreSQL boundary suite requires a PostgreSQL runtime");
                };
                diesel::sql_query(
                    "SELECT \
                        (SELECT setting::bigint FROM pg_settings WHERE name = 'statement_timeout') \
                            AS statement_timeout_ms, \
                        (SELECT setting::bigint FROM pg_settings \
                            WHERE name = 'idle_in_transaction_session_timeout') \
                            AS idle_transaction_timeout_ms",
                )
                .get_result::<TimeoutSettingsRow>(&mut **connection)
                .await
                .map_err(|_| execution_error())
            })
        })
        .await
        .expect("PostgreSQL timeout settings should query")
}

fn hold_operation(
    runtime: Arc<DatabaseRuntime>,
    started: Arc<Notify>,
    release: Arc<Notify>,
) -> tokio::task::JoinHandle<Result<(), PersistenceError>> {
    tokio::spawn(async move {
        runtime
            .run(DatabaseWorkload::Foreground, move |_connection| {
                Box::pin(async move {
                    started.notify_one();
                    release.notified().await;
                    Ok(())
                })
            })
            .await
    })
}

async fn wait_for_active(runtime: &DatabaseRuntime, expected: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        while runtime.snapshot().active != expected {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("PostgreSQL active operation count should converge");
}

#[tokio::test]
#[ignore = "requires a destructive dedicated PostgreSQL 17 database"]
async fn r3_22_postgres_async_boundary() {
    let database_url = env::var(POSTGRES_BOUNDARY_URL_ENV).unwrap_or_else(|_| {
        panic!(
            "{POSTGRES_BOUNDARY_URL_ENV} must point to the dedicated PostgreSQL 17 boundary database"
        )
    });
    let version = TestDatabase::reset_dedicated_postgres17_schema(
        &database_url,
        POSTGRES_BOUNDARY_DATABASE,
        POSTGRES_BOUNDARY_USER,
    );
    assert!(version.starts_with("17"));

    let database =
        TestDatabase::new_postgres_with_config(&database_url, 2, boundary_config()).await;
    let runtime = database.runtime();
    assert_eq!(runtime.backend(), DatabaseBackendKind::Postgres);
    runtime
        .readiness_probe()
        .await
        .expect("PostgreSQL runtime should be ready after clean migrations");

    let mut upgrade_startup = database.startup_connection();
    let secret_summary =
        prepare_secrets_before_startup(&CONFIG.secret_encryption, &mut upgrade_startup)
            .expect("PostgreSQL startup secret preparation should succeed");
    assert_eq!(secret_summary.downstream.total(), 0);
    assert_eq!(secret_summary.provider.total(), 0);
    drop(upgrade_startup);

    run_postgres_batch(
        &runtime,
        "CREATE TABLE r3_22_runtime_probe (id BIGINT PRIMARY KEY, value BIGINT NOT NULL);",
    )
    .await
    .expect("PostgreSQL representative table should create");
    run_postgres_batch(
        &runtime,
        "INSERT INTO r3_22_runtime_probe (id, value) VALUES (1, 10);",
    )
    .await
    .expect("PostgreSQL representative write should commit");
    let rollback = runtime
        .run(DatabaseWorkload::Foreground, |connection| {
            Box::pin(async move {
                let RuntimeConnection::Postgres(connection) = connection else {
                    panic!("PostgreSQL boundary suite requires a PostgreSQL runtime");
                };
                connection
                    .transaction::<(), diesel::result::Error, _>(async move |connection| {
                        connection
                            .batch_execute(
                                "INSERT INTO r3_22_runtime_probe (id, value) VALUES (2, 20);",
                            )
                            .await?;
                        Err(diesel::result::Error::RollbackTransaction)
                    })
                    .await
                    .map_err(|_| execution_error())
            })
        })
        .await;
    assert!(matches!(rollback, Err(PersistenceError::Execution { .. })));
    assert_eq!(
        postgres_count(
            &runtime,
            "SELECT COUNT(*) AS value FROM r3_22_runtime_probe",
        )
        .await,
        1
    );

    let configured_timeouts = timeout_settings(&runtime).await;
    assert_eq!(configured_timeouts.statement_timeout_ms, 1_000);
    assert_eq!(configured_timeouts.idle_transaction_timeout_ms, 1_000);
    let deadline_count = runtime.snapshot().operation_deadline_exceeded;
    let statement_timeout =
        run_postgres_batch(&runtime, "SET statement_timeout = 100; SELECT pg_sleep(1);").await;
    assert!(matches!(
        statement_timeout,
        Err(PersistenceError::Execution { .. })
    ));
    assert_eq!(
        runtime.snapshot().operation_deadline_exceeded,
        deadline_count,
        "server statement_timeout must fire before the runtime deadline"
    );

    let idle_disconnect = runtime
        .run(DatabaseWorkload::Foreground, |connection| {
            Box::pin(async move {
                let RuntimeConnection::Postgres(connection) = connection else {
                    panic!("PostgreSQL boundary suite requires a PostgreSQL runtime");
                };
                connection
                    .batch_execute(
                        "SET statement_timeout = 0; \
                         SET idle_in_transaction_session_timeout = 100;",
                    )
                    .await
                    .map_err(|_| execution_error())?;
                connection
                    .transaction::<(), diesel::result::Error, _>(async move |connection| {
                        tokio::time::sleep(Duration::from_millis(250)).await;
                        connection.batch_execute("SELECT 1;").await
                    })
                    .await
                    .map_err(|_| execution_error())
            })
        })
        .await;
    assert!(matches!(
        idle_disconnect,
        Err(PersistenceError::Execution { .. })
    ));
    assert_eq!(
        postgres_count(&runtime, "SELECT 1::bigint AS value").await,
        1,
        "pool must replace the server-terminated transaction connection"
    );
    let recovered_timeouts = timeout_settings(&runtime).await;
    assert_eq!(recovered_timeouts.statement_timeout_ms, 1_000);
    assert_eq!(recovered_timeouts.idle_transaction_timeout_ms, 1_000);
    assert!(runtime.snapshot().last_recovery_at_ms.is_some());

    let release = Arc::new(Notify::new());
    let first_started = Arc::new(Notify::new());
    let first = hold_operation(
        Arc::clone(&runtime),
        Arc::clone(&first_started),
        Arc::clone(&release),
    );
    first_started.notified().await;
    let second_started = Arc::new(Notify::new());
    let second = hold_operation(
        Arc::clone(&runtime),
        Arc::clone(&second_started),
        Arc::clone(&release),
    );
    second_started.notified().await;
    wait_for_active(&runtime, 2).await;
    assert_eq!(
        runtime.readiness_probe().await,
        Err(PersistenceError::ReadinessUnavailable {
            backend: DatabaseBackendKind::Postgres,
        })
    );
    assert_eq!(
        runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async { Ok(()) })
            })
            .await,
        Err(PersistenceError::QueueFull {
            workload: DatabaseWorkload::Foreground,
        })
    );
    release.notify_waiters();
    first
        .await
        .expect("first saturation holder should join")
        .unwrap();
    second
        .await
        .expect("second saturation holder should join")
        .unwrap();
    wait_for_active(&runtime, 0).await;

    let app_state = create_test_app_state(database.clone()).await;
    let created_api_key = app_state
        .admin
        .api_key
        .create_api_key(CreateApiKeyPayload {
            name: "r3.22-postgres-boundary".to_string(),
            description: Some("PostgreSQL async boundary smoke".to_string()),
            default_action: Some(Action::Allow),
            is_enabled: Some(true),
            expires_at: None,
            rate_limit_rpm: None,
            max_concurrent_requests: Some(1),
            quota_daily_requests: None,
            quota_daily_tokens: None,
            quota_monthly_tokens: None,
            budget_daily_nanos: None,
            budget_daily_currency: None,
            budget_monthly_nanos: None,
            budget_monthly_currency: None,
            acl_rules: None,
        })
        .await
        .expect("Manager should create a PostgreSQL-backed API key");
    app_state.catalog.clear_cache().await;
    let authenticated = check_system_api_key(
        &app_state,
        &created_api_key.reveal.api_key,
        ApiKeyPosition::AuthorizationHeader,
    )
    .await
    .expect("Proxy cache miss should load the PostgreSQL-backed API key");
    let lease = admit_api_key_request(&app_state, &authenticated.api_key)
        .await
        .expect("first governed request should be admitted")
        .expect("configured concurrency should return a lease");
    assert!(
        admit_api_key_request(&app_state, &authenticated.api_key)
            .await
            .is_err(),
        "second concurrent request must fail closed"
    );
    drop(lease);

    let rotated = app_state
        .admin
        .api_key
        .rotate_api_key(created_api_key.detail.id)
        .await
        .expect("Manager rotation transaction should commit");
    assert!(
        check_system_api_key(
            &app_state,
            &created_api_key.reveal.api_key,
            ApiKeyPosition::AuthorizationHeader,
        )
        .await
        .is_err(),
        "post-commit invalidation must evict the old API key hash"
    );
    let authenticated = check_system_api_key(
        &app_state,
        &rotated.api_key,
        ApiKeyPosition::AuthorizationHeader,
    )
    .await
    .expect("rotated API key should authenticate through PostgreSQL");

    let provider_id = ID_GENERATOR.generate_id();
    let provider_api_key_id = ID_GENERATOR.generate_id();
    let source_id = ID_GENERATOR.generate_id();
    let provider_secret = SensitiveSecret::new("sk-r3-22-postgres".to_string());
    let mut source = NewUpstreamSource::test_defaults(UpstreamProfileType::Openai);
    source.id = source_id;
    source.provider_id = provider_id;
    source.base_url = "https://example.test/v1".to_string();
    let provider = Provider::bootstrap(
        &runtime,
        &BootstrapProviderInput {
            provider_id,
            provider_key: "r3-22-postgres".to_string(),
            name: "R3.22 PostgreSQL".to_string(),
            source,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            provider_api_key_id,
            api_key_description: Some("boundary key".to_string()),
            key_prefix: "sk-r".to_string(),
            key_last4: "gres".to_string(),
            encrypted_secret: app_state
                .secret_encryption
                .encrypt_current(
                    SecretDomain::ProviderApiKey(provider_api_key_id),
                    &provider_secret,
                )
                .expect("provider secret should encrypt"),
            secret_hmac: app_state
                .secret_encryption
                .provider_secret_fingerprint(provider_id, &provider_secret)
                .expect("provider secret should fingerprint"),
            model_name: "r3-22-model".to_string(),
            real_model_name: None,
            model_kind: ModelKind::Chat,
        },
    )
    .await
    .expect("PostgreSQL provider bootstrap transaction should commit");
    app_state.catalog.clear_cache().await;
    let cached_provider = app_state
        .catalog
        .get_provider_by_id(provider_id)
        .await
        .expect("provider cache load should succeed")
        .expect("provider should exist");
    let cached_model = app_state
        .catalog
        .get_model_by_id(provider.created_model.id)
        .await
        .expect("model cache load should succeed")
        .expect("model should exist");
    assert_eq!(cached_provider.upstream_sources.len(), 1);
    let cached_source = &cached_provider.upstream_sources[0];

    let request_context = ProxyRequestContext::from_headers(&HeaderMap::new());
    let mut log_context = RequestLogContext::new(
        &authenticated.api_key,
        &cached_provider,
        &cached_model,
        cached_source,
        Some(provider.created_key.id),
        "r3-22-model",
        &request_context,
        &None,
        DownstreamProtocol::Openai,
        UpstreamProtocol::Openai,
        SourceSelectionReason::ProtocolMatch,
    );
    log_context.completed_at = Some(log_context.request_received_at + 10);
    log_context.llm_status = Some(StatusCode::OK);
    log_context.overall_status = RequestStatus::Success;
    log_context.response_visibility = ResponseVisibility::NotVisible;
    let request_log_id = log_context.id;
    app_state.infra.log_manager().log(log_context).await;
    app_state.flush_proxy_logs().await;
    let persisted = RequestLog::get_by_id(&runtime, request_log_id)
        .await
        .expect("PostgreSQL Request Log should persist");
    assert_eq!(persisted.id, request_log_id);
    assert_eq!(
        count_ingested_request_log_markers(&runtime).await.unwrap(),
        1
    );
    assert_eq!(
        app_state
            .metrics
            .count_pending_reconciliation(0, i64::MAX)
            .await
            .expect("PostgreSQL pending Metrics count should query"),
        0
    );

    app_state.start_background_workers();
    app_state.shutdown_persistence().await;
    let shutdown = runtime.snapshot();
    assert!(shutdown.shutting_down);
    assert_eq!(shutdown.active, 0);
    assert_eq!(shutdown.waiting, 0);
}
