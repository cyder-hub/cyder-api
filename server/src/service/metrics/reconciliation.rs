#[cfg(test)]
mod tests {
    use crate::config::MetricsConfig;
    use crate::controller::BaseError;
    use crate::service::metrics::MetricsService;
    use crate::service::metrics::types::MetricsReconciliationParams;

    #[test]
    fn reconciliation_rejects_missing_or_unbounded_range_shape() {
        let service = MetricsService::new(MetricsConfig::default());
        let err = service
            .reconcile_request_logs(MetricsReconciliationParams {
                start_time: 2,
                end_time: 1,
                limit: 10,
                dry_run: true,
            })
            .expect_err("invalid range should fail");

        match err {
            BaseError::ParamInvalid(Some(message)) => {
                assert!(message.contains("start_time must be before end_time"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn reconciliation_enforces_configured_batch_limit() {
        let service = MetricsService::new(MetricsConfig {
            reconciliation_batch_size: 5,
            ..MetricsConfig::default()
        });
        let err = service
            .reconcile_request_logs(MetricsReconciliationParams {
                start_time: 1,
                end_time: 2,
                limit: 6,
                dry_run: true,
            })
            .expect_err("oversized limit should fail");

        match err {
            BaseError::ParamInvalid(Some(message)) => {
                assert!(message.contains("limit must be between 1 and 5"));
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
