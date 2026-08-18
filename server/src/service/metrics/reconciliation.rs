#[cfg(test)]
mod tests {
    use crate::config::MetricsConfig;
    use crate::controller::BaseError;
    use crate::database::TestDatabase;
    use crate::database::api_key::{ApiKey, ApiKeyIssuance, CreateApiKeyPayload};
    use crate::database::request_log::RequestLog;
    use crate::schema::enum_def::{Action, DownstreamProtocol, RequestStatus, UpstreamProtocol};
    use crate::service::metrics::MetricsService;
    use crate::service::metrics::types::MetricsReconciliationParams;
    use crate::utils::ID_GENERATOR;

    async fn api_key_id(context: &TestDatabase) -> i64 {
        let secret = format!(
            "cyder-metrics-reconciliation-{}",
            ID_GENERATOR.generate_id()
        );
        let issuance = ApiKeyIssuance::new(ID_GENERATOR.generate_id(), &secret, None);
        ApiKey::create_issued(
            context.runtime().as_ref(),
            &CreateApiKeyPayload {
                name: format!("metrics-reconciliation-{}", ID_GENERATOR.generate_id()),
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
            },
            &issuance,
        )
        .await
        .expect("api key should create")
        .id
    }

    fn request_log(
        id: i64,
        api_key_id: i64,
        request_received_at: i64,
        status: RequestStatus,
    ) -> RequestLog {
        RequestLog {
            id,
            request_id: uuid::Uuid::new_v4().hyphenated().to_string(),
            client_request_id: None,
            api_key_id,
            requested_model_name: Some("test-model".to_string()),
            base_requested_model_name: Some("test-model".to_string()),
            resolved_patch_suffix: None,
            downstream_protocol: DownstreamProtocol::Openai,
            overall_status: status,
            final_error_code: None,
            final_error_message: None,
            request_received_at,
            upstream_request_sent_at: Some(request_received_at + 10),
            upstream_response_headers_at: Some(request_received_at + 20),
            upstream_first_body_chunk_at: Some(request_received_at + 30),
            first_response_body_at: Some(request_received_at + 30),
            first_token_at: Some(request_received_at + 40),
            max_upstream_response_idle_ms: Some(10),
            completed_at: Some(request_received_at + 100),
            is_stream: true,
            client_ip: None,
            provider_id: None,
            provider_api_key_id: None,
            model_id: None,
            source_id: None,
            source_selection_reason: None,
            provider_key_snapshot: None,
            provider_name_snapshot: None,
            model_name_snapshot: Some("test-model".to_string()),
            real_model_name_snapshot: None,
            model_kind_snapshot: None,
            source_profile_type_snapshot: None,
            source_base_url_snapshot: None,
            upstream_protocol: Some(UpstreamProtocol::Openai),
            upstream_http_status: Some(200),
            estimated_cost_nanos: Some(25),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
            total_input_tokens: Some(10),
            total_output_tokens: Some(20),
            input_text_tokens: Some(10),
            output_text_tokens: Some(20),
            input_image_tokens: Some(0),
            output_image_tokens: Some(0),
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: Some(5),
            total_tokens: Some(35),
            created_at: request_received_at,
            updated_at: request_received_at + 100,
        }
    }

    #[tokio::test]
    async fn reconciliation_rejects_missing_or_unbounded_range_shape() {
        let context = TestDatabase::new_sqlite_default("metrics-reconciliation-range.sqlite").await;
        let service = MetricsService::new(context.runtime(), MetricsConfig::default());
        let err = service
            .reconcile_request_logs(MetricsReconciliationParams {
                start_time: 2,
                end_time: 1,
                limit: 10,
                dry_run: true,
            })
            .await
            .expect_err("invalid range should fail");

        match err {
            BaseError::ParamInvalid(Some(message)) => {
                assert!(message.contains("start_time must be before end_time"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn reconciliation_enforces_configured_batch_limit() {
        let context = TestDatabase::new_sqlite_default("metrics-reconciliation-limit.sqlite").await;
        let service = MetricsService::new(
            context.runtime(),
            MetricsConfig {
                reconciliation_batch_size: 5,
                ..MetricsConfig::default()
            },
        );
        let err = service
            .reconcile_request_logs(MetricsReconciliationParams {
                start_time: 1,
                end_time: 2,
                limit: 6,
                dry_run: true,
            })
            .await
            .expect_err("oversized limit should fail");

        match err {
            BaseError::ParamInvalid(Some(message)) => {
                assert!(message.contains("limit must be between 1 and 5"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[tokio::test]
    async fn async_ingestion_is_idempotent_and_reconciliation_closes_pending_gap() {
        let context =
            TestDatabase::new_sqlite_default("metrics-reconciliation-runtime.sqlite").await;
        let api_key_id = api_key_id(&context).await;
        let database = context.runtime();
        let service = MetricsService::new(database.clone(), MetricsConfig::default());
        let first = request_log(
            ID_GENERATOR.generate_id(),
            api_key_id,
            60_000,
            RequestStatus::Success,
        );
        RequestLog::insert(&database, &first)
            .await
            .expect("first request log should insert");

        let immediate = service
            .ingest_request_log_id(first.id)
            .await
            .expect("immediate ingestion should succeed");
        assert!(immediate.ingested);
        let duplicate = service
            .ingest_request_log_id(first.id)
            .await
            .expect("duplicate ingestion should succeed");
        assert!(!duplicate.ingested);
        assert!(duplicate.skipped_existing);

        let second = request_log(
            ID_GENERATOR.generate_id(),
            api_key_id,
            61_000,
            RequestStatus::Error,
        );
        RequestLog::insert(&database, &second)
            .await
            .expect("second request log should insert");
        assert_eq!(
            service
                .count_pending_reconciliation(0, 120_000)
                .await
                .expect("pending count should load"),
            1
        );

        let preview = service
            .reconcile_request_logs(MetricsReconciliationParams {
                start_time: 0,
                end_time: 120_000,
                limit: 10,
                dry_run: true,
            })
            .await
            .expect("reconciliation preview should succeed");
        assert_eq!(preview.scanned, 1);
        assert_eq!(preview.skipped, 1);

        let reconciled = service
            .reconcile_request_logs(MetricsReconciliationParams {
                start_time: 0,
                end_time: 120_000,
                limit: 10,
                dry_run: false,
            })
            .await
            .expect("reconciliation should succeed");
        assert_eq!(reconciled.scanned, 1);
        assert_eq!(reconciled.ingested, 1);
        assert_eq!(reconciled.failed, 0);
        assert_eq!(
            service
                .count_pending_reconciliation(0, 120_000)
                .await
                .expect("pending count should load"),
            0
        );

        let global = service
            .query_request_window_metrics(0, 120_000, Some("global"), Some("global"))
            .await
            .expect("global request metrics should load");
        assert_eq!(global.len(), 1);
        assert_eq!(global[0].request_count, 2);
        assert_eq!(global[0].success_count, 1);
        assert_eq!(global[0].error_count, 1);
        assert_eq!(global[0].total_tokens, 70);
    }
}
