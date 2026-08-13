use std::{
    collections::BTreeMap,
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
        runtime::route_resolver::SourceSelectionReason,
        runtime::transport::{
            lifecycle::ProxyTerminationCoordinator,
            timing::{TimingSnapshot, TransportTimingState},
        },
    },
    schema::enum_def::{
        DownstreamProtocol, ModelKind, RequestStatus, UpstreamProfileType, UpstreamProtocol,
    },
    service::{
        app_state::AppState,
        cache::types::{
            CacheApiKey, CacheCostCatalogVersion, CacheModel, CacheProvider, CacheUpstreamSource,
        },
        provider_http::normalize_provider_base_url,
        runtime::ApiKeyCompletionDelta,
        transform::{
            TransformAction, TransformFailure, TransformOutcomeKind, TransformOutcomeSummary,
            TransformSemanticUnit, TransformSeverity,
        },
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
    pub source_id: i64,
    pub source_selection_reason: Option<String>,
    pub source_profile_type: UpstreamProfileType,
    pub source_base_url: Option<String>,
    pub provider_api_key_id: Option<i64>,
    pub requested_model_name: String,
    pub base_requested_model_name: String,
    pub resolved_patch_suffix: Option<String>,
    pub model_name: String,
    pub real_model_name: String,
    pub model_kind: crate::schema::enum_def::ModelKind,
    pub downstream_protocol: DownstreamProtocol,
    pub upstream_protocol: UpstreamProtocol,
    pub request_received_at: i64,
    pub client_ip: Option<String>,
    pub completed_at: Option<i64>,
    pub request_url: Option<String>,
    pub llm_status: Option<StatusCode>,
    pub is_stream: bool,
    pub(crate) transport_timing: Option<TransportTimingState>,
    pub usage: Option<UsageInfo>,
    pub usage_normalization: Option<UsageNormalization>,
    pub cost_catalog_id: Option<i64>,
    pub cost_catalog_version: Option<CacheCostCatalogVersion>,
    pub overall_status: RequestStatus,
    pub final_error_code: Option<String>,
    pub final_error_message: Option<String>,
    pub final_error_stage: Option<ExecutionStage>,
    pub response_visibility: ResponseVisibility,
    pub(crate) completion_coordinator: Option<ProxyTerminationCoordinator>,
}

impl RequestLogContext {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        api_key: &CacheApiKey,
        provider: &CacheProvider,
        model: &CacheModel,
        source: &CacheUpstreamSource,
        provider_api_key_id: Option<i64>,
        requested_model_name: &str,
        request_context: &ProxyRequestContext,
        client_ip_addr: &Option<String>,
        downstream_protocol: DownstreamProtocol,
        upstream_protocol: UpstreamProtocol,
        selection_reason: SourceSelectionReason,
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
            source_id: source.id,
            source_selection_reason: Some(selection_reason.as_key().to_string()),
            source_profile_type: source.profile_type,
            source_base_url: None,
            provider_api_key_id,
            requested_model_name: requested_model_name.to_string(),
            base_requested_model_name: requested_model_name.to_string(),
            resolved_patch_suffix: None,
            model_name: model.model_name.clone(),
            real_model_name: real_model_name.to_string(),
            model_kind: model.model_kind,
            downstream_protocol,
            upstream_protocol,
            request_received_at: request_context.received_at_ms,
            client_ip: client_ip_addr.clone(),
            completed_at: None,
            request_url: None,
            llm_status: None,
            is_stream: false,
            transport_timing: None,
            usage: None,
            usage_normalization: None,
            cost_catalog_id: model.cost_catalog_id,
            cost_catalog_version: None,
            overall_status: RequestStatus::Pending,
            final_error_code: None,
            final_error_message: None,
            final_error_stage: None,
            response_visibility: request_context.response_visibility.current(),
            completion_coordinator: None,
        }
    }

    pub(crate) fn set_completion_coordinator(&mut self, coordinator: ProxyTerminationCoordinator) {
        self.completion_coordinator = Some(coordinator);
    }

    pub(crate) fn attach_transport_timing(&mut self, timing: TransportTimingState) {
        self.transport_timing = Some(timing);
    }

    pub(super) fn set_model_resolution_trace(
        &mut self,
        base_requested_model_name: &str,
        resolved_patch_suffix: Option<&str>,
    ) {
        self.base_requested_model_name = base_requested_model_name.to_string();
        self.resolved_patch_suffix = resolved_patch_suffix.map(str::to_string);
    }

    pub(crate) fn set_source_base_url_snapshot(&mut self, normalized_base_url: &str) {
        self.source_base_url = safe_source_base_url_snapshot(normalized_base_url);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::proxy) enum TransformLogStage {
    Request,
    Response,
    Stream,
}

impl TransformLogStage {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Request => "request",
            Self::Response => "response",
            Self::Stream => "stream",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TransformSummaryLogFields {
    stage: &'static str,
    request_id: String,
    log_id: i64,
    source_id: i64,
    source_profile_type: &'static str,
    model_kind: &'static str,
    downstream_protocol: &'static str,
    upstream_protocol: &'static str,
    selection_reason: Option<String>,
    transform_applied: bool,
    total_fact_count: u64,
    retained_fact_count: usize,
    dropped_diagnostic_count: u64,
    max_severity: &'static str,
    outcome_counts: String,
    action_counts: String,
    semantic_counts: String,
    safe_summary_count: usize,
    safe_summary_bytes: usize,
    safe_summary_top_level_fields: usize,
    safe_summary_event_count: usize,
    safe_summary_sha256: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct TransformLogIdentity<'a> {
    request_id: &'a str,
    log_id: i64,
    source_id: i64,
    source_profile_type: UpstreamProfileType,
    model_kind: ModelKind,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    selection_reason: Option<&'a str>,
}

impl<'a> From<&'a RequestLogContext> for TransformLogIdentity<'a> {
    fn from(context: &'a RequestLogContext) -> Self {
        Self {
            request_id: context.request_id.as_str(),
            log_id: context.id,
            source_id: context.source_id,
            source_profile_type: context.source_profile_type,
            model_kind: context.model_kind,
            downstream_protocol: context.downstream_protocol,
            upstream_protocol: context.upstream_protocol,
            selection_reason: context.source_selection_reason.as_deref(),
        }
    }
}

fn transform_summary_log_fields(
    stage: TransformLogStage,
    identity: TransformLogIdentity<'_>,
    summary: &TransformOutcomeSummary,
) -> TransformSummaryLogFields {
    let safe_summaries = summary
        .facts
        .iter()
        .filter_map(|fact| fact.safe_summary.as_ref())
        .collect::<Vec<_>>();
    TransformSummaryLogFields {
        stage: stage.as_str(),
        request_id: identity.request_id.to_string(),
        log_id: identity.log_id,
        source_id: identity.source_id,
        source_profile_type: upstream_profile_type_name(identity.source_profile_type),
        model_kind: model_kind_name(identity.model_kind),
        downstream_protocol: downstream_protocol_name(identity.downstream_protocol),
        upstream_protocol: upstream_protocol_name(identity.upstream_protocol),
        selection_reason: identity.selection_reason.map(str::to_string),
        transform_applied: !protocols_share_wire(
            identity.downstream_protocol,
            identity.upstream_protocol,
        ),
        total_fact_count: summary.total_fact_count,
        retained_fact_count: summary.facts.len(),
        dropped_diagnostic_count: summary.dropped_diagnostic_count,
        max_severity: summary
            .max_severity
            .map(TransformSeverity::as_str)
            .unwrap_or("none"),
        outcome_counts: format_transform_counts(
            &summary.outcome_counts,
            |value: TransformOutcomeKind| value.as_str(),
        ),
        action_counts: format_transform_counts(&summary.action_counts, |value: TransformAction| {
            value.as_str()
        }),
        semantic_counts: format_transform_counts(
            &summary.semantic_counts,
            |value: TransformSemanticUnit| value.as_str(),
        ),
        safe_summary_count: safe_summaries.len(),
        safe_summary_bytes: safe_summaries
            .iter()
            .fold(0usize, |total, value| total.saturating_add(value.bytes)),
        safe_summary_top_level_fields: safe_summaries.iter().fold(0usize, |total, value| {
            total.saturating_add(value.top_level_field_count)
        }),
        safe_summary_event_count: safe_summaries.iter().fold(0usize, |total, value| {
            total.saturating_add(value.event_count)
        }),
        safe_summary_sha256: safe_summaries.first().map(|value| value.sha256.clone()),
    }
}

fn format_transform_counts<K: Copy + Ord>(
    counts: &BTreeMap<K, u64>,
    key: impl Fn(K) -> &'static str,
) -> String {
    counts
        .iter()
        .map(|(value, count)| format!("{}:{count}", key(*value)))
        .collect::<Vec<_>>()
        .join(",")
}

fn downstream_protocol_name(protocol: DownstreamProtocol) -> &'static str {
    match protocol {
        DownstreamProtocol::Openai => "openai",
        DownstreamProtocol::Responses => "responses",
        DownstreamProtocol::Anthropic => "anthropic",
        DownstreamProtocol::Gemini => "gemini",
    }
}

fn upstream_protocol_name(protocol: UpstreamProtocol) -> &'static str {
    match protocol {
        UpstreamProtocol::Openai => "openai",
        UpstreamProtocol::Responses => "responses",
        UpstreamProtocol::Anthropic => "anthropic",
        UpstreamProtocol::Gemini => "gemini",
        UpstreamProtocol::Ollama => "ollama",
    }
}

fn upstream_profile_type_name(profile_type: UpstreamProfileType) -> &'static str {
    match profile_type {
        UpstreamProfileType::Openai => "openai",
        UpstreamProfileType::OpenaiCompatible => "openai_compatible",
        UpstreamProfileType::Gemini => "gemini",
        UpstreamProfileType::Vertex => "vertex",
        UpstreamProfileType::Ollama => "ollama",
        UpstreamProfileType::Anthropic => "anthropic",
        UpstreamProfileType::Responses => "responses",
        UpstreamProfileType::GeminiOpenai => "gemini_openai",
    }
}

fn model_kind_name(model_kind: ModelKind) -> &'static str {
    match model_kind {
        ModelKind::Chat => "chat",
        ModelKind::Embedding => "embedding",
        ModelKind::Rerank => "rerank",
    }
}

fn protocols_share_wire(
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
) -> bool {
    matches!(
        (downstream_protocol, upstream_protocol),
        (DownstreamProtocol::Openai, UpstreamProtocol::Openai)
            | (DownstreamProtocol::Responses, UpstreamProtocol::Responses)
            | (DownstreamProtocol::Anthropic, UpstreamProtocol::Anthropic)
            | (DownstreamProtocol::Gemini, UpstreamProtocol::Gemini)
    )
}

fn transform_summary_event_fields(
    fields: &TransformSummaryLogFields,
) -> Vec<(&'static str, Option<String>)> {
    vec![
        ("request_id", Some(fields.request_id.clone())),
        ("log_id", Some(fields.log_id.to_string())),
        ("source_id", Some(fields.source_id.to_string())),
        (
            "source_profile_type",
            Some(fields.source_profile_type.to_string()),
        ),
        ("model_kind", Some(fields.model_kind.to_string())),
        ("stage", Some(fields.stage.to_string())),
        (
            "downstream_protocol",
            Some(fields.downstream_protocol.to_string()),
        ),
        (
            "upstream_protocol",
            Some(fields.upstream_protocol.to_string()),
        ),
        ("selection_reason", fields.selection_reason.clone()),
        (
            "transform_applied",
            Some(fields.transform_applied.to_string()),
        ),
        (
            "total_fact_count",
            Some(fields.total_fact_count.to_string()),
        ),
        (
            "retained_fact_count",
            Some(fields.retained_fact_count.to_string()),
        ),
        (
            "dropped_diagnostic_count",
            Some(fields.dropped_diagnostic_count.to_string()),
        ),
        ("max_severity", Some(fields.max_severity.to_string())),
        ("outcome_counts", Some(fields.outcome_counts.clone())),
        ("action_counts", Some(fields.action_counts.clone())),
        ("semantic_counts", Some(fields.semantic_counts.clone())),
        (
            "safe_summary_count",
            Some(fields.safe_summary_count.to_string()),
        ),
        (
            "safe_summary_bytes",
            Some(fields.safe_summary_bytes.to_string()),
        ),
        (
            "safe_summary_top_level_fields",
            Some(fields.safe_summary_top_level_fields.to_string()),
        ),
        (
            "safe_summary_event_count",
            Some(fields.safe_summary_event_count.to_string()),
        ),
        ("safe_summary_sha256", fields.safe_summary_sha256.clone()),
    ]
}

pub(in crate::proxy) fn log_transform_summary(
    stage: TransformLogStage,
    context: &RequestLogContext,
    summary: &TransformOutcomeSummary,
) {
    let fields = transform_summary_log_fields(stage, context.into(), summary);
    let event_fields = transform_summary_event_fields(&fields);
    let level = match summary.max_severity.unwrap_or(TransformSeverity::Debug) {
        TransformSeverity::Debug => crate::logging::StructuredEventLevel::Debug,
        TransformSeverity::Warning => crate::logging::StructuredEventLevel::Warning,
        TransformSeverity::Error => crate::logging::StructuredEventLevel::Error,
    };
    crate::logging::log_structured_event(level, "proxy.transform_summary", &event_fields);
}

pub(in crate::proxy) fn log_transform_failure(
    stage: TransformLogStage,
    context: &RequestLogContext,
    failure: &TransformFailure,
) {
    let fields = transform_summary_log_fields(stage, context.into(), &failure.summary);
    let mut event_fields = transform_summary_event_fields(&fields);
    event_fields.extend([
        ("failure_origin", Some(failure.origin.as_str().to_string())),
        ("failure_phase", Some(failure.phase.as_str().to_string())),
        (
            "semantic_unit",
            Some(failure.semantic_unit.as_str().to_string()),
        ),
        (
            "reason_code",
            Some(failure.reason_code.as_str().to_string()),
        ),
    ]);
    let level = match failure.origin {
        crate::service::transform::TransformFailureOrigin::DownstreamInput
        | crate::service::transform::TransformFailureOrigin::TargetCapability => {
            crate::logging::StructuredEventLevel::Debug
        }
        crate::service::transform::TransformFailureOrigin::UpstreamPayload
        | crate::service::transform::TransformFailureOrigin::TargetEncoding
        | crate::service::transform::TransformFailureOrigin::InternalInvariant => {
            crate::logging::StructuredEventLevel::Error
        }
    };
    crate::logging::log_structured_event(level, "proxy.transform_failure", &event_fields);
}

pub(in crate::proxy) fn log_upstream_usage_missing(
    context: &RequestLogContext,
    model: &str,
    status_code: StatusCode,
) {
    crate::warn_event!(
        "proxy.upstream_usage_missing",
        request_id = &context.request_id,
        log_id = context.id,
        source_id = context.source_id,
        source_profile_type = upstream_profile_type_name(context.source_profile_type),
        model_kind = model_kind_name(context.model_kind),
        downstream_protocol = downstream_protocol_name(context.downstream_protocol),
        upstream_protocol = upstream_protocol_name(context.upstream_protocol),
        model = model,
        status_code = status_code.as_u16(),
    );
}

pub(in crate::proxy) fn log_usage_normalization_warnings(
    context: &RequestLogContext,
    model: &str,
    status_code: StatusCode,
) {
    let Some(normalization) = context
        .usage_normalization
        .as_ref()
        .filter(|normalization| !normalization.warnings.is_empty())
    else {
        return;
    };
    crate::warn_event!(
        "proxy.usage_normalization_warning",
        request_id = &context.request_id,
        log_id = context.id,
        source_id = context.source_id,
        source_profile_type = upstream_profile_type_name(context.source_profile_type),
        model_kind = model_kind_name(context.model_kind),
        upstream_protocol = upstream_protocol_name(context.upstream_protocol),
        model = model,
        status_code = status_code.as_u16(),
        warning_count = normalization.warnings.len(),
        reported_total_mismatch = normalization
            .warnings
            .iter()
            .any(|warning| warning.starts_with("reported total_tokens ")),
        component_total_conflict = normalization
            .warnings
            .iter()
            .any(|warning| warning.contains("subcomponents exceeded total tokens")),
    );
}

fn safe_source_base_url_snapshot(base_url: &str) -> Option<String> {
    normalize_provider_base_url(base_url).ok()
}

fn total_tokens_for_context(context: &RequestLogContext) -> i64 {
    context
        .usage_normalization
        .as_ref()
        .map(UsageNormalization::normalized_total_tokens)
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
        occurred_at: context.completed_at.unwrap_or(context.request_received_at),
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
    let timing = context
        .transport_timing
        .as_ref()
        .map(TransportTimingState::snapshot)
        .unwrap_or_else(TimingSnapshot::default);
    RequestLog {
        id: context.id,
        request_id: context.request_id.to_string(),
        client_request_id: context.client_request_id.as_ref().map(ToString::to_string),
        api_key_id: context.api_key_id,
        requested_model_name: Some(context.requested_model_name.clone()),
        base_requested_model_name: Some(context.base_requested_model_name.clone()),
        resolved_patch_suffix: context.resolved_patch_suffix.clone(),
        downstream_protocol: context.downstream_protocol,
        overall_status: context.overall_status.clone(),
        final_error_code: context.final_error_code.clone(),
        final_error_message: context.final_error_message.clone(),
        request_received_at: context.request_received_at,
        upstream_request_sent_at: timing.upstream_request_sent_at,
        upstream_response_headers_at: timing.response_headers_at,
        upstream_first_body_chunk_at: timing.upstream_first_raw_body_at,
        first_response_body_at: timing.first_response_body_at,
        first_token_at: if context.is_stream {
            timing.first_token_at
        } else {
            None
        },
        max_upstream_response_idle_ms: timing.max_upstream_response_idle_ms,
        completed_at: context.completed_at.or(Some(now)),
        is_stream: context.is_stream,
        client_ip: context.client_ip.clone(),
        provider_id: Some(context.provider_id),
        provider_api_key_id: context.provider_api_key_id,
        model_id: Some(context.model_id),
        source_id: Some(context.source_id),
        source_selection_reason: context.source_selection_reason.clone(),
        provider_key_snapshot: Some(context.provider_key.clone()),
        provider_name_snapshot: Some(context.provider_name.clone()),
        model_name_snapshot: Some(context.model_name.clone()),
        real_model_name_snapshot: Some(context.real_model_name.clone()),
        model_kind_snapshot: Some(context.model_kind),
        source_profile_type_snapshot: Some(context.source_profile_type),
        source_base_url_snapshot: context.source_base_url.clone(),
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
            .and_then(|usage| i32::try_from(usage.total_input_tokens).ok())
            .or_else(|| context.usage.as_ref().map(|usage| usage.input_tokens)),
        total_output_tokens: context
            .usage_normalization
            .as_ref()
            .filter(|usage| usage.output_tokens_applicable)
            .and_then(|usage| i32::try_from(usage.total_output_tokens).ok())
            .or_else(|| context.usage.as_ref().map(|usage| usage.output_tokens)),
        input_text_tokens: context
            .usage_normalization
            .as_ref()
            .and_then(|usage| i32::try_from(usage.input_text_tokens).ok()),
        output_text_tokens: context
            .usage_normalization
            .as_ref()
            .filter(|usage| usage.output_tokens_applicable)
            .and_then(|usage| i32::try_from(usage.output_text_tokens).ok()),
        input_image_tokens: context
            .usage_normalization
            .as_ref()
            .and_then(|usage| i32::try_from(usage.input_image_tokens).ok())
            .or_else(|| context.usage.as_ref().map(|usage| usage.input_image_tokens)),
        output_image_tokens: context
            .usage_normalization
            .as_ref()
            .filter(|usage| usage.output_tokens_applicable)
            .and_then(|usage| i32::try_from(usage.output_image_tokens).ok())
            .or_else(|| {
                context
                    .usage
                    .as_ref()
                    .map(|usage| usage.output_image_tokens)
            }),
        cache_read_tokens: context
            .usage_normalization
            .as_ref()
            .and_then(|usage| i32::try_from(usage.cache_read_tokens).ok())
            .or_else(|| context.usage.as_ref().map(|usage| usage.cached_tokens)),
        cache_write_tokens: context
            .usage_normalization
            .as_ref()
            .and_then(|usage| i32::try_from(usage.cache_write_tokens).ok())
            .or_else(|| context.usage.as_ref().map(|usage| usage.cache_write_tokens)),
        reasoning_tokens: context
            .usage_normalization
            .as_ref()
            .filter(|usage| usage.output_tokens_applicable)
            .and_then(|usage| i32::try_from(usage.reasoning_tokens).ok())
            .or_else(|| context.usage.as_ref().map(|usage| usage.reasoning_tokens)),
        total_tokens: context
            .usage_normalization
            .as_ref()
            .and_then(|usage| i32::try_from(usage.normalized_total_tokens()).ok())
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
    let Some(version) = context.cost_catalog_version.as_ref() else {
        return CostOutcome::default();
    };
    if context.overall_status != RequestStatus::Success {
        return CostOutcome::default();
    }
    let normalization = context.usage_normalization.as_ref();
    let ledger = CostLedger::for_successful_invocation(normalization);
    let snapshot = match rate_cost(
        &ledger,
        &CostRatingContext {
            total_input_tokens: normalization
                .map(|usage| usage.total_input_tokens)
                .unwrap_or_default(),
        },
        version,
    ) {
        Ok(rating) => {
            let mut warnings = normalization
                .map(|usage| usage.warnings.clone())
                .unwrap_or_default();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::transform::{
        TransformDiagnosticCollector, TransformDiagnosticFact, TransformPhase, TransformReasonCode,
        TransformSafeSummary,
    };

    #[test]
    fn source_base_url_snapshot_is_normalized_and_rejects_secret_bearing_urls() {
        assert_eq!(
            safe_source_base_url_snapshot("  HTTPS://API.EXAMPLE.COM:443/v1///  ").as_deref(),
            Some("https://api.example.com/v1")
        );
        assert!(safe_source_base_url_snapshot("https://user:secret@api.example.com/v1").is_none());
        assert!(safe_source_base_url_snapshot("https://api.example.com/v1?key=secret").is_none());
        assert!(safe_source_base_url_snapshot("https://api.example.com/v1#secret").is_none());
    }

    #[test]
    fn transform_summary_context_covers_all_selection_reasons_without_inferring_application() {
        let summary = TransformOutcomeSummary::default();
        for selection_reason in [
            "protocol_match",
            "provider_default_transform",
            "model_default_transform",
        ] {
            let fields = transform_summary_log_fields(
                TransformLogStage::Request,
                TransformLogIdentity {
                    request_id: "req-safe",
                    log_id: 41,
                    source_id: 42,
                    source_profile_type: UpstreamProfileType::Openai,
                    model_kind: ModelKind::Chat,
                    downstream_protocol: DownstreamProtocol::Openai,
                    upstream_protocol: UpstreamProtocol::Openai,
                    selection_reason: Some(selection_reason),
                },
                &summary,
            );
            assert_eq!(fields.selection_reason.as_deref(), Some(selection_reason));
            assert_eq!(fields.source_profile_type, "openai");
            assert_eq!(fields.model_kind, "chat");
            assert!(!fields.transform_applied);
        }

        let fields = transform_summary_log_fields(
            TransformLogStage::Response,
            TransformLogIdentity {
                request_id: "req-safe",
                log_id: 41,
                source_id: 42,
                source_profile_type: UpstreamProfileType::Responses,
                model_kind: ModelKind::Chat,
                downstream_protocol: DownstreamProtocol::Openai,
                upstream_protocol: UpstreamProtocol::Responses,
                selection_reason: Some("protocol_match"),
            },
            &summary,
        );
        assert!(fields.transform_applied);
    }

    #[test]
    fn transform_summary_log_is_bounded_aggregated_and_payload_free() {
        let mut collector = TransformDiagnosticCollector::default();
        for index in 0..35 {
            collector.record(TransformDiagnosticFact {
                sequence: 0,
                phase: TransformPhase::ResponseObserve,
                semantic_unit: TransformSemanticUnit::ResponseEnvelope,
                outcome: if index == 34 {
                    TransformOutcomeKind::ObservationDegraded
                } else {
                    TransformOutcomeKind::Passthrough
                },
                action: TransformAction::PassThrough,
                reason_code: TransformReasonCode::ObservationParseFailed,
                safe_summary: (index == 0).then(|| {
                    TransformSafeSummary::from_json(&serde_json::json!({
                        "secret": "must-not-appear",
                        "tool_arguments": {"private": true}
                    }))
                }),
            });
        }
        let summary = collector.into_summary();
        let fields = transform_summary_log_fields(
            TransformLogStage::Response,
            TransformLogIdentity {
                request_id: "req-safe",
                log_id: 41,
                source_id: 42,
                source_profile_type: UpstreamProfileType::Openai,
                model_kind: ModelKind::Chat,
                downstream_protocol: DownstreamProtocol::Openai,
                upstream_protocol: UpstreamProtocol::Openai,
                selection_reason: Some("protocol_match"),
            },
            &summary,
        );
        let rendered = crate::logging::render_structured_event(
            "proxy.transform_summary",
            &transform_summary_event_fields(&fields),
        );

        assert_eq!(fields.total_fact_count, 35);
        assert_eq!(fields.retained_fact_count, 32);
        assert_eq!(fields.dropped_diagnostic_count, 3);
        assert_eq!(fields.source_profile_type, "openai");
        assert_eq!(fields.model_kind, "chat");
        assert_eq!(fields.safe_summary_count, 1);
        assert_eq!(
            fields.safe_summary_sha256.as_deref().map(str::len),
            Some(64)
        );
        assert!(fields.outcome_counts.contains("passthrough:34"));
        assert!(fields.outcome_counts.contains("observation_degraded:1"));
        assert!(rendered.contains("source_profile_type=openai"));
        assert!(rendered.contains("model_kind=chat"));
        for forbidden in ["must-not-appear", "tool_arguments", "private"] {
            assert!(!rendered.contains(forbidden));
        }
    }
}
