use chrono::Utc;

use crate::{
    config::MetricsConfig,
    controller::BaseError,
    database::{
        metrics::{
            count_ingested_request_log_markers, count_uningested_request_logs_in_range,
            delete_metrics_data_in_range, ingest_metric_rollups,
            list_request_log_ids_in_range_after, list_uningested_request_log_ids,
        },
        request_attempt::RequestAttempt,
        request_log::RequestLog,
    },
    proxy::logging::RequestLogPersistedSink,
};

use super::{
    rollup::build_rollup_deltas,
    types::{
        MetricsIngestOutcome, MetricsIngestStatus, MetricsReconciliationParams,
        MetricsReconciliationSummary, MetricsRepairParams, MetricsRepairSummary,
        MetricsWorkerTickResult,
    },
};

#[derive(Debug, Clone)]
pub struct MetricsService {
    config: MetricsConfig,
}

impl MetricsService {
    pub fn new(config: MetricsConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &MetricsConfig {
        &self.config
    }

    pub async fn tick_reconciliation_worker(&self) -> MetricsWorkerTickResult {
        if !self.config.enabled {
            return MetricsWorkerTickResult::default();
        }

        let now_ms = Utc::now().timestamp_millis();
        self.tick_reconciliation_worker_at(now_ms)
    }

    pub(crate) fn tick_reconciliation_worker_at(&self, now_ms: i64) -> MetricsWorkerTickResult {
        if !self.config.enabled {
            return MetricsWorkerTickResult::default();
        }

        let safety_lag_ms =
            (self.config.reconciliation_worker_safety_lag_seconds as i64).saturating_mul(1_000);
        let recent_window_ms = (self
            .config
            .reconciliation_worker_recent_window_seconds
            .max(1) as i64)
            .saturating_mul(1_000);
        let end_time = now_ms.saturating_sub(safety_lag_ms);
        let start_time = end_time.saturating_sub(recent_window_ms);
        if start_time >= end_time {
            return MetricsWorkerTickResult::default();
        }

        match self.reconcile_request_logs(MetricsReconciliationParams {
            start_time,
            end_time,
            limit: self.config.reconciliation_batch_size.max(1),
            dry_run: false,
        }) {
            Ok(summary) => MetricsWorkerTickResult {
                processed: summary.ingested as u64,
                skipped: summary.skipped as u64,
                failed: summary.failed as u64,
            },
            Err(err) => {
                crate::error_event!(
                    "metrics.reconciliation_worker_failed",
                    start_time_ms = start_time,
                    end_time_ms = end_time,
                    error = format!("{err:?}")
                );
                MetricsWorkerTickResult {
                    failed: 1,
                    ..MetricsWorkerTickResult::default()
                }
            }
        }
    }

    pub fn ingest_status(&self) -> Result<MetricsIngestStatus, BaseError> {
        let generated_at = Utc::now().timestamp_millis();
        Ok(MetricsIngestStatus {
            ingested_request_log_count: count_ingested_request_log_markers()?,
            pending_reconciliation_count: None,
            generated_at,
        })
    }

    pub fn record_request_log(
        &self,
        request_log: &RequestLog,
        attempts: &[RequestAttempt],
    ) -> Result<MetricsIngestOutcome, BaseError> {
        if !self.config.enabled {
            return Ok(MetricsIngestOutcome {
                request_log_id: request_log.id,
                ingested: false,
                skipped_existing: false,
                ..Default::default()
            });
        }

        let now_ms = Utc::now().timestamp_millis();
        let marker = crate::database::metrics::MetricIngestedRequestLog {
            request_log_id: request_log.id,
            request_received_at: request_log.request_received_at,
            completed_at: request_log.completed_at,
            ingested_at: now_ms,
        };
        let deltas = build_rollup_deltas(
            request_log,
            attempts,
            self.config.rollup_bucket_seconds,
            now_ms,
        );
        if !ingest_metric_rollups(
            &marker,
            &deltas.request_rollups,
            &deltas.attempt_rollups,
            &deltas.http_status_rollups,
            &deltas.cost_rollups,
        )? {
            return Ok(MetricsIngestOutcome {
                request_log_id: request_log.id,
                ingested: false,
                skipped_existing: true,
                ..Default::default()
            });
        }

        Ok(MetricsIngestOutcome {
            request_log_id: request_log.id,
            ingested: true,
            skipped_existing: false,
            request_rollup_deltas: deltas.request_rollups.len(),
            attempt_rollup_deltas: deltas.attempt_rollups.len(),
            http_status_deltas: deltas.http_status_rollups.len(),
            cost_rollup_deltas: deltas.cost_rollups.len(),
        })
    }

    pub fn ingest_request_log_id(
        &self,
        request_log_id: i64,
    ) -> Result<MetricsIngestOutcome, BaseError> {
        let request_log = RequestLog::get_by_id(request_log_id)?;
        let attempts = RequestAttempt::list_by_request_log_id(request_log_id)?;
        self.record_request_log(&request_log, &attempts)
    }

    pub fn reconcile_request_logs(
        &self,
        params: MetricsReconciliationParams,
    ) -> Result<MetricsReconciliationSummary, BaseError> {
        if params.start_time >= params.end_time {
            return Err(BaseError::ParamInvalid(Some(
                "start_time must be before end_time".to_string(),
            )));
        }
        if params.limit == 0 || params.limit > self.config.reconciliation_batch_size.max(1) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "limit must be between 1 and {}",
                self.config.reconciliation_batch_size.max(1)
            ))));
        }

        let rows = list_uningested_request_log_ids(
            params.start_time,
            params.end_time,
            params.limit as i64,
        )?;
        let mut summary = MetricsReconciliationSummary {
            scanned: rows.len(),
            oldest_uningested: rows.first().map(|row| row.request_received_at),
            newest_uningested: rows.last().map(|row| row.request_received_at),
            ..Default::default()
        };

        if params.dry_run {
            summary.skipped = rows.len();
            return Ok(summary);
        }

        for row in rows {
            match self.ingest_request_log_id(row.id) {
                Ok(outcome) if outcome.ingested => summary.ingested += 1,
                Ok(_) => summary.skipped += 1,
                Err(err) => {
                    summary.failed += 1;
                    summary.errors.push(format!("{err:?}"));
                }
            }
        }

        Ok(summary)
    }

    pub fn repair_request_logs(
        &self,
        params: MetricsRepairParams,
    ) -> Result<MetricsRepairSummary, BaseError> {
        if params.start_time >= params.end_time {
            return Err(BaseError::ParamInvalid(Some(
                "start_time must be before end_time".to_string(),
            )));
        }
        if params.limit == 0 || params.limit > self.config.reconciliation_batch_size.max(1) {
            return Err(BaseError::ParamInvalid(Some(format!(
                "limit must be between 1 and {}",
                self.config.reconciliation_batch_size.max(1)
            ))));
        }

        let bucket_start = align_bucket_start(params.start_time, self.config.rollup_bucket_seconds);
        let bucket_end = align_bucket_end(params.end_time, self.config.rollup_bucket_seconds);

        if params.dry_run {
            let reconciliation =
                self.replay_request_logs_in_range(bucket_start, bucket_end, params.limit, true)?;
            return Ok(MetricsRepairSummary {
                requested_start_time: params.start_time,
                requested_end_time: params.end_time,
                expanded_replay_start_time: bucket_start,
                expanded_replay_end_time: bucket_end,
                reconciliation,
                ..Default::default()
            });
        }

        let deleted =
            delete_metrics_data_in_range(bucket_start, bucket_end, bucket_start, bucket_end)?;
        let reconciliation =
            self.replay_request_logs_in_range(bucket_start, bucket_end, params.limit, false)?;

        Ok(MetricsRepairSummary {
            requested_start_time: params.start_time,
            requested_end_time: params.end_time,
            expanded_replay_start_time: bucket_start,
            expanded_replay_end_time: bucket_end,
            deleted_ingest_markers: deleted.deleted_ingest_markers,
            deleted_request_rollups: deleted.deleted_request_rollups,
            deleted_attempt_rollups: deleted.deleted_attempt_rollups,
            deleted_http_status_rollups: deleted.deleted_http_status_rollups,
            deleted_cost_rollups: deleted.deleted_cost_rollups,
            reconciliation,
        })
    }

    pub fn count_pending_reconciliation(
        &self,
        start_time: i64,
        end_time: i64,
    ) -> Result<i64, BaseError> {
        count_uningested_request_logs_in_range(start_time, end_time)
    }

    fn replay_request_logs_in_range(
        &self,
        start_time: i64,
        end_time: i64,
        limit: usize,
        dry_run: bool,
    ) -> Result<MetricsReconciliationSummary, BaseError> {
        let mut summary = MetricsReconciliationSummary::default();
        let mut cursor_received_at = start_time;
        let mut cursor_id = i64::MIN;

        loop {
            let rows = list_request_log_ids_in_range_after(
                start_time,
                end_time,
                cursor_received_at,
                cursor_id,
                limit as i64,
            )?;
            let batch_len = rows.len();
            if batch_len == 0 {
                break;
            }

            summary.scanned += batch_len;
            if summary.oldest_uningested.is_none() {
                summary.oldest_uningested = rows.first().map(|row| row.request_received_at);
            }
            summary.newest_uningested = rows.last().map(|row| row.request_received_at);

            if dry_run {
                summary.skipped += batch_len;
            } else {
                for row in &rows {
                    match self.ingest_request_log_id(row.id) {
                        Ok(outcome) if outcome.ingested => summary.ingested += 1,
                        Ok(_) => summary.skipped += 1,
                        Err(err) => {
                            summary.failed += 1;
                            summary.errors.push(format!("{err:?}"));
                        }
                    }
                }
            }

            let Some(last) = rows.last() else {
                break;
            };
            cursor_received_at = last.request_received_at;
            cursor_id = last.id;

            if batch_len < limit {
                break;
            }
        }

        Ok(summary)
    }
}

fn align_bucket_start(timestamp_ms: i64, bucket_seconds: u64) -> i64 {
    let bucket_ms = (bucket_seconds.max(1) as i64).saturating_mul(1_000);
    timestamp_ms.div_euclid(bucket_ms) * bucket_ms
}

fn align_bucket_end(timestamp_ms: i64, bucket_seconds: u64) -> i64 {
    let bucket_ms = (bucket_seconds.max(1) as i64).saturating_mul(1_000);
    let last_inclusive = timestamp_ms.saturating_sub(1);
    last_inclusive.div_euclid(bucket_ms) * bucket_ms + bucket_ms
}

#[async_trait::async_trait]
impl RequestLogPersistedSink for MetricsService {
    async fn on_request_log_persisted(&self, request_log_id: i64) {
        match self.ingest_request_log_id(request_log_id) {
            Ok(outcome) => {
                crate::debug_event!(
                    "metrics.request_log_ingest_completed",
                    request_log_id = request_log_id,
                    ingested = outcome.ingested,
                    skipped_existing = outcome.skipped_existing,
                    request_rollup_deltas = outcome.request_rollup_deltas,
                    attempt_rollup_deltas = outcome.attempt_rollup_deltas,
                    http_status_deltas = outcome.http_status_deltas,
                    cost_rollup_deltas = outcome.cost_rollup_deltas,
                );
            }
            Err(err) => {
                crate::warn_event!(
                    "metrics.request_log_ingest_failed",
                    request_log_id = request_log_id,
                    error = format!("{err:?}")
                );
            }
        }
    }
}
