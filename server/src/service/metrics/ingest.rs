#[cfg(test)]
mod tests {
    use crate::config::MetricsConfig;
    use crate::database::TestDbContext;
    use crate::database::api_key::{ApiKey, CreateApiKeyPayload};
    use crate::database::metrics::{
        query_attempt_window_aggregates, query_cost_window_aggregates, query_http_status_breakdown,
        query_request_window_aggregates,
    };
    use crate::database::model::{Model, ModelCapabilityFlags};
    use crate::database::provider::{NewProvider, NewProviderApiKey, Provider, ProviderApiKey};
    use crate::database::request_attempt::RequestAttempt;
    use crate::database::request_log::RequestLog;
    use crate::schema::enum_def::{
        LlmApiType, ProviderApiKeyMode, ProviderType, RequestAttemptStatus, RequestStatus,
        SchedulerAction, StorageType,
    };
    use crate::service::metrics::MetricsService;
    use crate::service::metrics::types::MetricsRepairParams;

    fn request_log() -> RequestLog {
        RequestLog {
            id: 900,
            api_key_id: 9,
            requested_model_name: Some("gpt".to_string()),
            base_requested_model_name: None,
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            resolved_name_scope: None,
            resolved_route_id: None,
            resolved_route_name: None,
            user_api_type: LlmApiType::Openai,
            overall_status: RequestStatus::Error,
            final_error_code: Some("upstream_error".to_string()),
            final_error_message: None,
            attempt_count: 1,
            retry_count: 1,
            fallback_count: 0,
            request_received_at: 60_100,
            first_attempt_started_at: Some(60_200),
            response_started_to_client_at: None,
            completed_at: Some(60_700),
            is_stream: false,
            client_ip: None,
            final_attempt_id: Some(901),
            final_provider_id: Some(7),
            final_provider_api_key_id: Some(8),
            final_model_id: Some(11),
            final_provider_key_snapshot: Some("pk".to_string()),
            final_provider_name_snapshot: Some("Provider".to_string()),
            final_model_name_snapshot: Some("Model".to_string()),
            final_real_model_name_snapshot: None,
            final_llm_api_type: Some(LlmApiType::Openai),
            estimated_cost_nanos: Some(100),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_id: None,
            cost_catalog_version_id: None,
            cost_snapshot_json: None,
            total_input_tokens: Some(10),
            total_output_tokens: Some(20),
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: Some(3),
            total_tokens: Some(33),
            has_transform_diagnostics: true,
            transform_diagnostic_count: 1,
            transform_diagnostic_max_loss_level: Some("reject".to_string()),
            bundle_version: None,
            bundle_storage_type: Some(StorageType::FileSystem),
            bundle_storage_key: None,
            created_at: 60_700,
            updated_at: 60_700,
        }
    }

    fn attempt() -> RequestAttempt {
        RequestAttempt {
            id: 901,
            request_log_id: 900,
            attempt_index: 0,
            candidate_position: 0,
            provider_id: Some(7),
            provider_api_key_id: Some(8),
            model_id: Some(11),
            provider_key_snapshot: Some("pk".to_string()),
            provider_name_snapshot: Some("Provider".to_string()),
            model_name_snapshot: Some("Model".to_string()),
            real_model_name_snapshot: None,
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: RequestAttemptStatus::Error,
            scheduler_action: SchedulerAction::RetrySameCandidate,
            error_code: Some("upstream_error".to_string()),
            error_message: None,
            request_uri: None,
            request_headers_json: None,
            response_headers_json: None,
            http_status: Some(500),
            started_at: Some(60_200),
            first_byte_at: None,
            completed_at: Some(60_600),
            response_started_to_client: false,
            backoff_ms: None,
            applied_request_patch_ids_json: None,
            request_patch_summary_json: None,
            estimated_cost_nanos: Some(50),
            estimated_cost_currency: Some("USD".to_string()),
            cost_catalog_version_id: None,
            total_input_tokens: Some(10),
            total_output_tokens: Some(0),
            input_text_tokens: None,
            output_text_tokens: None,
            input_image_tokens: None,
            output_image_tokens: None,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: Some(0),
            total_tokens: Some(10),
            llm_request_blob_id: None,
            llm_request_patch_id: None,
            llm_response_blob_id: None,
            llm_response_capture_state: None,
            created_at: 60_600,
            updated_at: 60_600,
        }
    }

    #[test]
    fn record_request_log_ingests_rollups_idempotently() {
        let context = TestDbContext::new_sqlite("metrics-ingest.sqlite");
        context.run_sync(|| {
            let service = MetricsService::new(MetricsConfig::default());
            let request_log = request_log();
            let attempts = vec![attempt()];

            let first = service
                .record_request_log(&request_log, &attempts)
                .expect("first ingest should succeed");
            assert!(first.ingested);
            assert!(!first.skipped_existing);
            assert_eq!(first.request_rollup_deltas, 6);
            assert_eq!(first.attempt_rollup_deltas, 6);
            assert_eq!(first.http_status_deltas, 6);

            let second = service
                .record_request_log(&request_log, &attempts)
                .expect("second ingest should be skipped");
            assert!(!second.ingested);
            assert!(second.skipped_existing);

            let request = query_request_window_aggregates(0, 120_000, Some("provider"), Some("7"))
                .expect("request aggregate");
            assert_eq!(request.len(), 1);
            assert_eq!(request[0].request_count, 1);
            assert_eq!(request[0].error_count, 1);
            assert_eq!(request[0].retry_count, 1);
            assert_eq!(request[0].transform_diagnostic_reject_count, 1);

            let attempt = query_attempt_window_aggregates(0, 120_000, Some("provider"), Some("7"))
                .expect("attempt aggregate");
            assert_eq!(attempt.len(), 1);
            assert_eq!(attempt[0].attempt_count, 1);
            assert_eq!(attempt[0].retry_same_candidate_count, 1);
            assert_eq!(attempt[0].total_latency_sum_ms, 400);

            let statuses =
                query_http_status_breakdown(0, 120_000, "provider", "7").expect("status breakdown");
            assert_eq!(statuses.len(), 1);
            assert_eq!(statuses[0].status_code, 500);
            assert_eq!(statuses[0].count, 1);

            let request_cost = query_cost_window_aggregates(0, 120_000, "request", "provider", "7")
                .expect("request cost");
            assert_eq!(request_cost[0].amount_nanos, 100);
            let attempt_cost = query_cost_window_aggregates(0, 120_000, "attempt", "provider", "7")
                .expect("attempt cost");
            assert_eq!(attempt_cost[0].amount_nanos, 50);
        });
    }

    #[test]
    fn worker_tick_reconciles_recent_uningested_request_log() {
        let context = TestDbContext::new_sqlite("metrics-worker-reconciliation.sqlite");
        context.run_sync(|| {
            let api_key = ApiKey::create(&CreateApiKeyPayload {
                name: "worker-test".to_string(),
                description: None,
                default_action: None,
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
            })
            .expect("api key fixture should insert");
            let mut request_log = request_log();
            request_log.api_key_id = api_key.detail.id;
            request_log.final_provider_id = None;
            request_log.final_provider_api_key_id = None;
            request_log.final_model_id = None;
            request_log.final_attempt_id = Some(901);
            let mut attempt = attempt();
            attempt.attempt_index = 1;
            attempt.candidate_position = 1;
            attempt.provider_id = None;
            attempt.provider_api_key_id = None;
            attempt.model_id = None;
            attempt.first_byte_at = Some(60_300);
            attempt.created_at = 60_000;
            attempt.updated_at = 60_700;
            let attempts = vec![attempt];
            RequestLog::insert_with_attempts(&request_log, &attempts)
                .expect("request log fixture should insert");
            let service = MetricsService::new(MetricsConfig {
                reconciliation_batch_size: 10,
                reconciliation_worker_recent_window_seconds: 20,
                reconciliation_worker_safety_lag_seconds: 0,
                ..MetricsConfig::default()
            });

            let result = service.tick_reconciliation_worker_at(70_000);

            assert_eq!(result.processed, 1);
            assert_eq!(result.skipped, 0);
            assert_eq!(result.failed, 0);
            let request =
                query_request_window_aggregates(0, 120_000, Some("global"), Some("global"))
                    .expect("request aggregate");
            assert_eq!(request.len(), 1);
            assert_eq!(request[0].request_count, 1);
        });
    }

    #[test]
    fn repair_removes_existing_rollups_and_markers_before_replay() {
        let context = TestDbContext::new_sqlite("metrics-repair.sqlite");
        context.run_sync(|| {
            let service = MetricsService::new(MetricsConfig::default());
            let request_log = request_log();
            let attempts = vec![attempt()];
            service
                .record_request_log(&request_log, &attempts)
                .expect("ingest should succeed");

            let summary = service
                .repair_request_logs(MetricsRepairParams {
                    start_time: 60_000,
                    end_time: 61_000,
                    limit: 500,
                    dry_run: false,
                })
                .expect("repair should succeed");

            assert_eq!(summary.deleted_ingest_markers, 1);
            assert!(summary.deleted_request_rollups > 0);
            assert!(summary.deleted_attempt_rollups > 0);
            assert!(summary.deleted_http_status_rollups > 0);
            assert!(summary.deleted_cost_rollups > 0);

            let request = query_request_window_aggregates(0, 120_000, Some("provider"), Some("7"))
                .expect("request aggregate should query");
            assert!(request.is_empty());
        });
    }

    #[test]
    fn repair_replays_expanded_bucket_range_for_non_aligned_request_range() {
        let context = TestDbContext::new_sqlite("metrics-repair-expanded-bucket.sqlite");
        context.run_sync(|| {
            let api_key = ApiKey::create(&CreateApiKeyPayload {
                name: "repair-expanded".to_string(),
                description: None,
                default_action: None,
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
            })
            .expect("api key fixture should insert");
            Provider::create(&NewProvider {
                id: 7,
                provider_key: "pk".to_string(),
                name: "Provider".to_string(),
                endpoint: "https://example.com".to_string(),
                use_proxy: false,
                is_enabled: true,
                created_at: 60_000,
                updated_at: 60_000,
                provider_type: ProviderType::Openai,
                provider_api_key_mode: ProviderApiKeyMode::Queue,
            })
            .expect("provider fixture should insert");
            ProviderApiKey::insert(&NewProviderApiKey {
                id: 8,
                provider_id: 7,
                api_key: "provider-secret".to_string(),
                description: Some("fixture".to_string()),
                is_enabled: true,
                created_at: 60_000,
                updated_at: 60_000,
            })
            .expect("provider api key fixture should insert");
            let model = Model::create(7, "Model", None, true, ModelCapabilityFlags::default())
                .expect("model fixture should insert");
            let service = MetricsService::new(MetricsConfig {
                reconciliation_batch_size: 2,
                ..MetricsConfig::default()
            });

            for (id, received_at) in [(910, 60_010), (920, 60_150), (930, 60_900)] {
                let mut log = request_log();
                log.id = id;
                log.api_key_id = api_key.detail.id;
                log.request_received_at = received_at;
                log.first_attempt_started_at = Some(received_at + 10);
                log.completed_at = Some(received_at + 200);
                log.created_at = received_at + 200;
                log.updated_at = received_at + 200;
                log.final_attempt_id = Some(id + 1);
                log.final_model_id = Some(model.id);
                let mut request_attempt = attempt();
                request_attempt.id = id + 1;
                request_attempt.request_log_id = id;
                request_attempt.attempt_index = 1;
                request_attempt.candidate_position = 1;
                request_attempt.model_id = Some(model.id);
                request_attempt.started_at = Some(received_at + 10);
                request_attempt.completed_at = Some(received_at + 100);
                request_attempt.created_at = received_at;
                request_attempt.updated_at = received_at + 100;
                RequestLog::insert_with_attempts(&log, &[request_attempt.clone()])
                    .expect("request log fixture should insert");
                service
                    .record_request_log(&log, &[request_attempt])
                    .expect("initial ingest should succeed");
            }

            let before =
                query_request_window_aggregates(60_000, 120_000, Some("provider"), Some("7"))
                    .expect("request aggregate should query");
            assert_eq!(before[0].request_count, 3);

            let dry_run = service
                .repair_request_logs(MetricsRepairParams {
                    start_time: 60_100,
                    end_time: 60_200,
                    limit: 2,
                    dry_run: true,
                })
                .expect("repair dry-run should succeed");
            assert_eq!(dry_run.requested_start_time, 60_100);
            assert_eq!(dry_run.requested_end_time, 60_200);
            assert_eq!(dry_run.expanded_replay_start_time, 60_000);
            assert_eq!(dry_run.expanded_replay_end_time, 120_000);
            assert_eq!(dry_run.deleted_request_rollups, 0);
            assert_eq!(dry_run.reconciliation.scanned, 3);
            assert_eq!(dry_run.reconciliation.skipped, 3);

            let summary = service
                .repair_request_logs(MetricsRepairParams {
                    start_time: 60_100,
                    end_time: 60_200,
                    limit: 2,
                    dry_run: false,
                })
                .expect("repair execute should succeed");
            assert_eq!(summary.deleted_ingest_markers, 3);
            assert_eq!(summary.reconciliation.scanned, 3);
            assert_eq!(summary.reconciliation.ingested, 3);

            let after =
                query_request_window_aggregates(60_000, 120_000, Some("provider"), Some("7"))
                    .expect("request aggregate should query");
            assert_eq!(after.len(), 1);
            assert_eq!(after[0].request_count, 3);

            let repeat = service
                .repair_request_logs(MetricsRepairParams {
                    start_time: 60_100,
                    end_time: 60_200,
                    limit: 2,
                    dry_run: false,
                })
                .expect("repeat repair should succeed");
            assert_eq!(repeat.reconciliation.scanned, 3);
            assert_eq!(repeat.reconciliation.ingested, 3);
            let repeated =
                query_request_window_aggregates(60_000, 120_000, Some("provider"), Some("7"))
                    .expect("request aggregate should query");
            assert_eq!(repeated[0].request_count, 3);
        });
    }
}
