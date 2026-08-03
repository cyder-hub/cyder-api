use std::{
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, RwLock},
    time::Duration,
};

use chrono::Utc;
use reqwest::StatusCode;
use tokio::{
    sync::{mpsc, oneshot},
    time::sleep,
};

use crate::{
    cost::{CostLedger, CostRatingContext, CostSnapshot, UsageNormalization, rate_cost},
    database::request_log::{RequestLog, RequestLogRecord},
    proxy::{
        ExecutionStage, ResponseVisibility,
        request_context::{ClientRequestId, ProxyRequestContext, RequestId},
    },
    schema::enum_def::{DownstreamProtocol, RequestStatus, UpstreamProtocol},
    service::{
        app_state::AppState,
        cache::types::{CacheApiKey, CacheCostCatalogVersion, CacheModel, CacheProvider},
        runtime::ApiKeyCompletionDelta,
    },
    utils::{ID_GENERATOR, usage::UsageInfo},
};

#[cfg(test)]
use crate::database::TestDbContext;

#[derive(Debug, Clone)]
pub struct RequestLogContext {
    pub id: i64,
    pub request_id: RequestId,
    pub client_request_id: Option<ClientRequestId>,
    pub api_key_id: i64,
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub model_id: i64,
    pub provider_api_key_id: Option<i64>,
    pub requested_model_name: String,
    pub base_requested_model_name: String,
    pub resolved_reasoning_suffix: Option<String>,
    pub resolved_reasoning_preset: Option<String>,
    pub model_name: String,
    pub real_model_name: String,
    pub downstream_protocol: DownstreamProtocol,
    pub upstream_protocol: UpstreamProtocol,
    pub request_received_at: i64,
    pub client_ip: Option<String>,
    pub llm_request_sent_at: Option<i64>,
    pub request_url: Option<String>,
    pub llm_status: Option<StatusCode>,
    pub is_stream: bool,
    pub first_chunk_ts: Option<i64>,
    pub completion_ts: Option<i64>,
    pub usage: Option<UsageInfo>,
    pub usage_normalization: Option<UsageNormalization>,
    pub cost_catalog_id: Option<i64>,
    pub cost_catalog_version: Option<CacheCostCatalogVersion>,
    pub overall_status: RequestStatus,
    pub final_error_code: Option<String>,
    pub final_error_message: Option<String>,
    pub final_error_stage: Option<ExecutionStage>,
    pub response_visibility: ResponseVisibility,
}

impl RequestLogContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api_key: &CacheApiKey,
        provider: &CacheProvider,
        model: &CacheModel,
        provider_api_key_id: Option<i64>,
        requested_model_name: &str,
        request_context: &ProxyRequestContext,
        client_ip_addr: &Option<String>,
        downstream_protocol: DownstreamProtocol,
        upstream_protocol: UpstreamProtocol,
    ) -> Self {
        let real_model_name = model
            .real_model_name
            .as_deref()
            .filter(|value| !value.is_empty())
            .unwrap_or(&model.model_name);
        Self {
            id: ID_GENERATOR.generate_id(),
            request_id: request_context.request_id.clone(),
            client_request_id: request_context.client_request_id.clone(),
            api_key_id: api_key.id,
            provider_id: provider.id,
            provider_key: provider.provider_key.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id,
            provider_api_key_id,
            requested_model_name: requested_model_name.to_string(),
            base_requested_model_name: requested_model_name.to_string(),
            resolved_reasoning_suffix: None,
            resolved_reasoning_preset: None,
            model_name: model.model_name.clone(),
            real_model_name: real_model_name.to_string(),
            downstream_protocol,
            upstream_protocol,
            request_received_at: request_context.received_at_ms,
            client_ip: client_ip_addr.clone(),
            llm_request_sent_at: None,
            request_url: None,
            llm_status: None,
            is_stream: false,
            first_chunk_ts: None,
            completion_ts: None,
            usage: None,
            usage_normalization: None,
            cost_catalog_id: model.cost_catalog_id,
            cost_catalog_version: None,
            overall_status: RequestStatus::Pending,
            final_error_code: None,
            final_error_message: None,
            final_error_stage: None,
            response_visibility: request_context.response_visibility.current(),
        }
    }

    pub(super) fn set_model_resolution_trace(
        &mut self,
        base_requested_model_name: &str,
        resolved_reasoning_suffix: Option<&str>,
        resolved_reasoning_preset: Option<&str>,
    ) {
        self.base_requested_model_name = base_requested_model_name.to_string();
        self.resolved_reasoning_suffix = resolved_reasoning_suffix.map(str::to_string);
        self.resolved_reasoning_preset = resolved_reasoning_preset.map(str::to_string);
    }
}

fn total_tokens_for_context(context: &RequestLogContext) -> i64 {
    context
        .usage_normalization
        .as_ref()
        .map(|usage| (usage.total_input_tokens + usage.total_output_tokens) as i64)
        .or_else(|| {
            context
                .usage
                .as_ref()
                .map(|usage| i64::from(usage.total_tokens))
        })
        .unwrap_or_default()
}

pub(super) fn completion_delta_from_log_context(
    context: &RequestLogContext,
) -> ApiKeyCompletionDelta {
    let cost = build_cost_outcome(context);
    ApiKeyCompletionDelta {
        api_key_id: context.api_key_id,
        occurred_at: context.completion_ts.unwrap_or(context.request_received_at),
        total_tokens: total_tokens_for_context(context),
        billed_amount_nanos: cost.estimated_cost_nanos.unwrap_or_default(),
        billed_currency: cost.estimated_cost_currency,
    }
}

pub(super) async fn record_request_completion_and_log(
    app_state: &Arc<AppState>,
    context: RequestLogContext,
) {
    if let Err(err) = app_state
        .api_key_governance
        .record_api_key_completion(&completion_delta_from_log_context(&context))
        .await
    {
        crate::error_event!(
            "logging.api_key_completion_record_failed",
            request_id = &context.request_id,
            log_id = context.id,
            api_key_id = context.api_key_id,
            error = err,
        );
    }
    app_state.infra.log_manager().log(context).await;
}

#[async_trait::async_trait]
pub trait RequestLogPersistedSink: Send + Sync {
    async fn on_request_log_persisted(&self, context: RequestLogPersistedContext);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestLogPersistedContext {
    pub request_log_id: i64,
    pub request_id: String,
    #[cfg(test)]
    pub final_error_stage: Option<ExecutionStage>,
    #[cfg(test)]
    pub response_visibility: ResponseVisibility,
}

pub struct LogManager {
    sender: mpsc::Sender<LogCommand>,
    metrics: LogManagerMetrics,
    request_log_persisted_sink: Arc<RwLock<Option<Arc<dyn RequestLogPersistedSink>>>>,
}

enum LogCommand {
    Record(RequestLogContext),
    Flush(oneshot::Sender<()>),
}

#[derive(Clone)]
enum LogManagerRuntime {
    Global,
    #[cfg(test)]
    Test(TestDbContext),
}

impl LogManagerRuntime {
    async fn run<F>(&self, future: F) -> F::Output
    where
        F: std::future::Future,
    {
        match self {
            Self::Global => future.await,
            #[cfg(test)]
            Self::Test(test_db_context) => test_db_context.run_async(future).await,
        }
    }
}

#[derive(Debug, Clone)]
pub struct LogManagerMetrics {
    enqueued: Arc<AtomicU64>,
    processed: Arc<AtomicU64>,
    pending: Arc<AtomicU64>,
    in_flight: Arc<AtomicU64>,
    retries: Arc<AtomicU64>,
    enqueue_failures: Arc<AtomicU64>,
    db_failures: Arc<AtomicU64>,
}

impl LogManagerMetrics {
    fn new() -> Self {
        Self {
            enqueued: Arc::new(AtomicU64::new(0)),
            processed: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(AtomicU64::new(0)),
            in_flight: Arc::new(AtomicU64::new(0)),
            retries: Arc::new(AtomicU64::new(0)),
            enqueue_failures: Arc::new(AtomicU64::new(0)),
            db_failures: Arc::new(AtomicU64::new(0)),
        }
    }
}

fn decrement(counter: &AtomicU64) {
    let _ = counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
        value.checked_sub(1)
    });
}

impl LogManager {
    pub fn new() -> Self {
        Self::new_with_runtime(LogManagerRuntime::Global)
    }

    #[cfg(test)]
    pub fn new_for_test(test_db_context: TestDbContext) -> Self {
        Self::new_with_runtime(LogManagerRuntime::Test(test_db_context))
    }

    fn new_with_runtime(runtime: LogManagerRuntime) -> Self {
        let (sender, mut receiver) = mpsc::channel::<LogCommand>(100);
        let metrics = LogManagerMetrics::new();
        let worker_metrics = metrics.clone();
        let sink = Arc::new(RwLock::new(None));
        let worker_sink = Arc::clone(&sink);
        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                match command {
                    LogCommand::Record(context) => {
                        decrement(&worker_metrics.pending);
                        worker_metrics.in_flight.fetch_add(1, Ordering::Relaxed);
                        runtime
                            .run(process_log(context, &worker_metrics, &worker_sink))
                            .await;
                        decrement(&worker_metrics.in_flight);
                        worker_metrics.processed.fetch_add(1, Ordering::Relaxed);
                    }
                    LogCommand::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        Self {
            sender,
            metrics,
            request_log_persisted_sink: sink,
        }
    }

    pub fn set_request_log_persisted_sink(&self, sink: Arc<dyn RequestLogPersistedSink>) {
        if let Ok(mut guard) = self.request_log_persisted_sink.write() {
            *guard = Some(sink);
        }
    }

    pub async fn log(&self, context: RequestLogContext) {
        let request_id = context.request_id.clone();
        let log_id = context.id;
        self.metrics.enqueued.fetch_add(1, Ordering::Relaxed);
        self.metrics.pending.fetch_add(1, Ordering::Relaxed);
        if self.sender.send(LogCommand::Record(context)).await.is_err() {
            decrement(&self.metrics.pending);
            self.metrics
                .enqueue_failures
                .fetch_add(1, Ordering::Relaxed);
            crate::error_event!(
                "logging.request_log_enqueue_failed",
                request_id = &request_id,
                log_id = log_id,
            );
        }
    }

    pub async fn flush(&self) {
        let (sender, receiver) = oneshot::channel();
        if self.sender.send(LogCommand::Flush(sender)).await.is_ok() {
            let _ = receiver.await;
        }
    }
}

async fn process_log(
    context: RequestLogContext,
    metrics: &LogManagerMetrics,
    sink: &Arc<RwLock<Option<Arc<dyn RequestLogPersistedSink>>>>,
) {
    let request_log = build_request_log(&context, Utc::now().timestamp_millis());
    let mut inserted: Option<RequestLogRecord> = None;
    for retry in 0..3 {
        match RequestLog::insert(&request_log) {
            Ok(row) => {
                inserted = Some(row);
                break;
            }
            Err(err) if retry < 2 => {
                metrics.retries.fetch_add(1, Ordering::Relaxed);
                crate::warn_event!(
                    "logging.request_log_insert_retry",
                    request_id = &context.request_id,
                    log_id = context.id,
                    retry = retry + 1,
                    error = format!("{err:?}"),
                );
                sleep(Duration::from_millis(25 * (retry + 1))).await;
            }
            Err(err) => {
                metrics.db_failures.fetch_add(1, Ordering::Relaxed);
                crate::error_event!(
                    "logging.request_log_insert_failed",
                    request_id = &context.request_id,
                    log_id = context.id,
                    error = format!("{err:?}"),
                );
            }
        }
    }

    if let Some(row) = inserted {
        crate::debug_event!(
            "logging.request_log_inserted",
            request_id = &context.request_id,
            log_id = row.id,
        );
        let persisted_sink = sink.read().ok().and_then(|guard| guard.clone());
        if let Some(persisted_sink) = persisted_sink {
            persisted_sink
                .on_request_log_persisted(RequestLogPersistedContext {
                    request_log_id: row.id,
                    request_id: context.request_id.to_string(),
                    #[cfg(test)]
                    final_error_stage: context.final_error_stage,
                    #[cfg(test)]
                    response_visibility: context.response_visibility,
                })
                .await;
        }
    }
}

fn build_request_log(context: &RequestLogContext, now: i64) -> RequestLog {
    let cost = build_cost_outcome(context);
    RequestLog {
        id: context.id,
        request_id: context.request_id.to_string(),
        client_request_id: context.client_request_id.as_ref().map(ToString::to_string),
        api_key_id: context.api_key_id,
        requested_model_name: Some(context.requested_model_name.clone()),
        base_requested_model_name: Some(context.base_requested_model_name.clone()),
        resolved_reasoning_suffix: context.resolved_reasoning_suffix.clone(),
        resolved_reasoning_preset: context.resolved_reasoning_preset.clone(),
        downstream_protocol: context.downstream_protocol,
        overall_status: context.overall_status.clone(),
        final_error_code: context.final_error_code.clone(),
        final_error_message: context.final_error_message.clone(),
        request_received_at: context.request_received_at,
        upstream_request_sent_at: context.llm_request_sent_at,
        response_started_to_client_at: context.first_chunk_ts,
        completed_at: context.completion_ts.or(Some(now)),
        is_stream: context.is_stream,
        client_ip: context.client_ip.clone(),
        provider_id: Some(context.provider_id),
        provider_api_key_id: context.provider_api_key_id,
        model_id: Some(context.model_id),
        provider_key_snapshot: Some(context.provider_key.clone()),
        provider_name_snapshot: Some(context.provider_name.clone()),
        model_name_snapshot: Some(context.model_name.clone()),
        real_model_name_snapshot: Some(context.real_model_name.clone()),
        upstream_protocol: Some(context.upstream_protocol),
        upstream_http_status: context.llm_status.map(|status| i32::from(status.as_u16())),
        estimated_cost_nanos: cost.estimated_cost_nanos,
        estimated_cost_currency: cost.estimated_cost_currency,
        cost_catalog_id: context.cost_catalog_id,
        cost_catalog_version_id: cost.cost_catalog_version_id,
        cost_snapshot_json: cost.cost_snapshot_json,
        total_input_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.total_input_tokens as i32)
            .or_else(|| context.usage.as_ref().map(|usage| usage.input_tokens)),
        total_output_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.total_output_tokens as i32)
            .or_else(|| context.usage.as_ref().map(|usage| usage.output_tokens)),
        input_text_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.input_text_tokens as i32),
        output_text_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.output_text_tokens as i32),
        input_image_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.input_image_tokens as i32)
            .or_else(|| context.usage.as_ref().map(|usage| usage.input_image_tokens)),
        output_image_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.output_image_tokens as i32)
            .or_else(|| {
                context
                    .usage
                    .as_ref()
                    .map(|usage| usage.output_image_tokens)
            }),
        cache_read_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.cache_read_tokens as i32)
            .or_else(|| context.usage.as_ref().map(|usage| usage.cached_tokens)),
        cache_write_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.cache_write_tokens as i32),
        reasoning_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| usage.reasoning_tokens as i32)
            .or_else(|| context.usage.as_ref().map(|usage| usage.reasoning_tokens)),
        total_tokens: context
            .usage_normalization
            .as_ref()
            .map(|usage| (usage.total_input_tokens + usage.total_output_tokens) as i32)
            .or_else(|| context.usage.as_ref().map(|usage| usage.total_tokens)),
        created_at: context.request_received_at,
        updated_at: now,
    }
}

#[derive(Default)]
struct CostOutcome {
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    cost_catalog_version_id: Option<i64>,
    cost_snapshot_json: Option<String>,
}

fn build_cost_outcome(context: &RequestLogContext) -> CostOutcome {
    let (Some(normalization), Some(version)) = (
        context.usage_normalization.as_ref(),
        context.cost_catalog_version.as_ref(),
    ) else {
        return CostOutcome::default();
    };
    let ledger = CostLedger::from(normalization);
    let snapshot = match rate_cost(
        &ledger,
        &CostRatingContext {
            total_input_tokens: normalization.total_input_tokens,
        },
        version,
    ) {
        Ok(rating) => {
            let mut warnings = normalization.warnings.clone();
            warnings.extend(rating.warnings);
            CostSnapshot {
                schema_version: crate::cost::COST_SNAPSHOT_SCHEMA_VERSION_V1,
                cost_catalog_id: version.catalog_id,
                cost_catalog_version_id: version.id,
                total_cost_nanos: rating.total_cost_nanos,
                currency: rating.currency,
                detail_lines: rating.detail_lines,
                unmatched_items: rating.unmatched_items,
                warnings,
            }
        }
        Err(err) => CostSnapshot {
            schema_version: crate::cost::COST_SNAPSHOT_SCHEMA_VERSION_V1,
            cost_catalog_id: version.catalog_id,
            cost_catalog_version_id: version.id,
            total_cost_nanos: 0,
            currency: version.currency.clone(),
            detail_lines: vec![],
            unmatched_items: vec![],
            warnings: vec![format!("cost rating failed: {err:?}")],
        },
    };
    CostOutcome {
        estimated_cost_nanos: Some(snapshot.total_cost_nanos),
        estimated_cost_currency: Some(snapshot.currency.clone()),
        cost_catalog_version_id: Some(snapshot.cost_catalog_version_id),
        cost_snapshot_json: serde_json::to_string(&snapshot).ok(),
    }
}
