use super::orchestrator::RequestAttemptDraft;
use crate::{
    cost::{CostLedger, CostRatingContext, CostSnapshot, UsageNormalization, rate_cost},
    database::{
        api_key_rollup::{
            ApiKeyRollupDaily, ApiKeyRollupMonthly, NewApiKeyRollupDaily, NewApiKeyRollupMonthly,
        },
        request_attempt::RequestAttempt,
        request_log::RequestLog,
    },
    schema::enum_def::{
        LlmApiType, RequestAttemptStatus, RequestStatus, SchedulerAction, StorageType,
    },
    service::app_state::{ApiKeyCompletionDelta, AppState},
    service::cache::types::{CacheApiKey, CacheCostCatalogVersion, CacheModel, CacheProvider},
    service::storage::{Storage, get_storage, types::PutObjectOptions},
    service::transform::unified::{
        UnifiedTransformDiagnostic, UnifiedTransformDiagnosticLossLevel,
    },
    utils::{
        ID_GENERATOR,
        storage::{
            LogBodyCaptureState, REQUEST_LOG_BUNDLE_V2_VERSION, RequestLogBundleAttemptSection,
            RequestLogBundleCandidateManifest, RequestLogBundleRequestSection,
            RequestLogBundleRequestSnapshot, RequestLogBundleTransformDiagnosticItem,
            RequestLogBundleTransformDiagnosticPhase, RequestLogBundleTransformDiagnostics,
            RequestLogBundleTransformDiagnosticsSummary, RequestLogBundleV2,
            RequestLogBundleV2Builder, RequestLogBundleV2DiagnosticAssets,
            generate_storage_path_from_id,
        },
        usage::UsageInfo,
    },
};
use bytes::Bytes;
use chrono::{Datelike, TimeZone, Utc};
use cyder_tools::log::{debug, error};
use flate2::{Compression, write::GzEncoder};
use reqwest::StatusCode;
use rmp_serde::to_vec_named;
use std::sync::Arc;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::{env, io::Write, path::PathBuf, time::Duration};
use tokio::{
    fs::{self, File},
    io::AsyncWriteExt,
    sync::{mpsc, oneshot},
    time::sleep,
};

#[derive(Debug, Clone)]
pub enum LoggedBody {
    InMemory {
        bytes: Bytes,
        capture_state: LogBodyCaptureState,
    },
    Spooled {
        path: PathBuf,
        size_bytes: usize,
        capture_state: LogBodyCaptureState,
    },
}

impl LoggedBody {
    pub fn from_bytes(bytes: Bytes) -> Self {
        Self::InMemory {
            bytes,
            capture_state: LogBodyCaptureState::Complete,
        }
    }

    pub fn from_bytes_with_state(bytes: Bytes, capture_state: LogBodyCaptureState) -> Self {
        Self::InMemory {
            bytes,
            capture_state,
        }
    }

    pub fn capture_state(&self) -> LogBodyCaptureState {
        match self {
            Self::InMemory { capture_state, .. } | Self::Spooled { capture_state, .. } => {
                *capture_state
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LogBodyKind {
    UserRequest,
    LlmRequest,
    LlmResponse,
    UserResponse,
}

impl LogBodyKind {
    fn as_path_segment(self) -> &'static str {
        match self {
            Self::UserRequest => "user_request",
            Self::LlmRequest => "llm_request",
            Self::LlmResponse => "llm_response",
            Self::UserResponse => "user_response",
        }
    }
}

#[derive(Debug)]
pub struct StreamingBodyWriter {
    path: PathBuf,
    file: File,
    size_bytes: usize,
    cleanup_on_drop: bool,
}

impl StreamingBodyWriter {
    pub async fn new(kind: LogBodyKind, log_id: i64) -> std::io::Result<Self> {
        let mut dir = env::temp_dir();
        dir.push("cyder-api");
        dir.push("request-log-spool");
        fs::create_dir_all(&dir).await?;

        let path = dir.join(format!(
            "{}-{}-{}.body",
            log_id,
            kind.as_path_segment(),
            ID_GENERATOR.generate_id()
        ));
        let file = File::create(&path).await?;
        Ok(Self {
            path,
            file,
            size_bytes: 0,
            cleanup_on_drop: true,
        })
    }

    pub async fn append(&mut self, chunk: &[u8]) -> std::io::Result<()> {
        self.file.write_all(chunk).await?;
        self.file.flush().await?;
        self.size_bytes += chunk.len();
        Ok(())
    }

    pub fn snapshot(&self, capture_state: LogBodyCaptureState) -> LoggedBody {
        LoggedBody::Spooled {
            path: self.path.clone(),
            size_bytes: self.size_bytes,
            capture_state,
        }
    }

    pub async fn finish(
        mut self,
        capture_state: LogBodyCaptureState,
    ) -> std::io::Result<LoggedBody> {
        self.cleanup_on_drop = false;
        self.file.flush().await?;
        self.file.sync_data().await?;

        Ok(LoggedBody::Spooled {
            path: self.path.clone(),
            size_bytes: self.size_bytes,
            capture_state,
        })
    }

    pub async fn abort(mut self) -> std::io::Result<()> {
        self.cleanup_on_drop = false;
        self.file.flush().await?;
        match fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err),
        }
    }
}

impl Drop for StreamingBodyWriter {
    fn drop(&mut self) {
        if !self.cleanup_on_drop {
            return;
        }

        match std::fs::remove_file(&self.path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {}
        }
    }
}

#[derive(Debug, Clone)]
pub struct RequestLogContext {
    // from create_request_log
    pub id: i64,
    pub api_key_id: i64,
    pub provider_id: i64,
    pub provider_key: String,
    pub provider_name: String,
    pub model_id: i64,
    pub provider_api_key_id: Option<i64>,
    pub requested_model_name: String,
    pub resolved_name_scope: String,
    pub resolved_route_id: Option<i64>,
    pub resolved_route_name: Option<String>,
    pub model_name: String,
    pub real_model_name: String,
    pub user_api_type: LlmApiType,
    pub llm_api_type: LlmApiType,
    pub request_received_at: i64,
    pub client_ip: Option<String>,
    pub llm_request_sent_at: Option<i64>,

    // from log_final_update
    pub request_url: Option<String>,
    pub llm_status: Option<StatusCode>,
    pub response_headers_json: Option<String>,
    pub is_stream: bool,
    pub first_chunk_ts: Option<i64>,
    pub completion_ts: Option<i64>,
    pub usage: Option<UsageInfo>,
    pub usage_normalization: Option<UsageNormalization>,
    pub cost_catalog_id: Option<i64>,
    pub cost_catalog_version: Option<CacheCostCatalogVersion>,
    pub overall_status: RequestStatus,
    pub user_request_body: Option<LoggedBody>,
    pub llm_request_body: Option<LoggedBody>,
    pub llm_response_body: Option<LoggedBody>,
    pub user_response_body: Option<LoggedBody>,
    pub final_error_code: Option<String>,
    pub final_error_message: Option<String>,
    pub request_snapshot: Option<RequestLogBundleRequestSnapshot>,
    pub candidate_manifest: Option<RequestLogBundleCandidateManifest>,
    pub transform_diagnostics: Vec<RequestLogBundleTransformDiagnosticItem>,
    pub(super) skipped_attempts: Vec<RequestAttemptDraft>,
    pub(super) attempts: Vec<RequestAttemptDraft>,
}

impl RequestLogContext {
    pub fn new(
        system_api_key: &CacheApiKey,
        provider: &CacheProvider,
        model: &CacheModel,
        provider_api_key_id: Option<i64>,
        requested_model_name: &str,
        resolved_name_scope: &str,
        resolved_route_id: Option<i64>,
        resolved_route_name: Option<&str>,
        start_time: i64,
        client_ip_addr: &Option<String>,
        user_api_type: LlmApiType,
        llm_api_type: LlmApiType,
    ) -> Self {
        let real_model_name = model
            .real_model_name
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&model.model_name);

        Self {
            id: ID_GENERATOR.generate_id(),
            api_key_id: system_api_key.id,
            provider_id: provider.id,
            provider_key: provider.provider_key.clone(),
            provider_name: provider.name.clone(),
            model_id: model.id,
            provider_api_key_id,
            requested_model_name: requested_model_name.to_string(),
            resolved_name_scope: resolved_name_scope.to_string(),
            resolved_route_id,
            resolved_route_name: resolved_route_name.map(str::to_string),
            model_name: model.model_name.clone(),
            real_model_name: real_model_name.to_string(),
            user_api_type,
            llm_api_type,
            request_received_at: start_time,
            client_ip: client_ip_addr.clone(),
            llm_request_sent_at: None,
            request_url: None,
            llm_status: None,
            response_headers_json: None,
            is_stream: false,
            first_chunk_ts: None,
            completion_ts: None,
            usage: None,
            usage_normalization: None,
            cost_catalog_id: model.cost_catalog_id,
            cost_catalog_version: None,
            overall_status: RequestStatus::Pending,
            user_request_body: None,
            llm_request_body: None,
            llm_response_body: None,
            user_response_body: None,
            final_error_code: None,
            final_error_message: None,
            request_snapshot: None,
            candidate_manifest: None,
            transform_diagnostics: Vec::new(),
            skipped_attempts: Vec::new(),
            attempts: Vec::new(),
        }
    }

    pub(super) fn new_for_skipped_candidates(
        system_api_key: &CacheApiKey,
        requested_model_name: &str,
        resolved_name_scope: &str,
        resolved_route_id: Option<i64>,
        resolved_route_name: Option<&str>,
        start_time: i64,
        client_ip_addr: &Option<String>,
        user_api_type: LlmApiType,
        first_skipped_attempt: &RequestAttemptDraft,
    ) -> Self {
        Self {
            id: ID_GENERATOR.generate_id(),
            api_key_id: system_api_key.id,
            provider_id: first_skipped_attempt
                .provider_id
                .expect("skipped capability attempts should carry provider_id"),
            provider_key: first_skipped_attempt
                .provider_key_snapshot
                .clone()
                .unwrap_or_default(),
            provider_name: first_skipped_attempt
                .provider_name_snapshot
                .clone()
                .unwrap_or_default(),
            model_id: first_skipped_attempt
                .model_id
                .expect("skipped capability attempts should carry model_id"),
            provider_api_key_id: None,
            requested_model_name: requested_model_name.to_string(),
            resolved_name_scope: resolved_name_scope.to_string(),
            resolved_route_id,
            resolved_route_name: resolved_route_name.map(str::to_string),
            model_name: first_skipped_attempt
                .model_name_snapshot
                .clone()
                .unwrap_or_default(),
            real_model_name: first_skipped_attempt
                .real_model_name_snapshot
                .clone()
                .unwrap_or_default(),
            user_api_type,
            llm_api_type: first_skipped_attempt.llm_api_type.unwrap_or(user_api_type),
            request_received_at: start_time,
            client_ip: client_ip_addr.clone(),
            llm_request_sent_at: None,
            request_url: None,
            llm_status: None,
            response_headers_json: None,
            is_stream: false,
            first_chunk_ts: None,
            completion_ts: None,
            usage: None,
            usage_normalization: None,
            cost_catalog_id: None,
            cost_catalog_version: None,
            overall_status: RequestStatus::Pending,
            user_request_body: None,
            llm_request_body: None,
            llm_response_body: None,
            user_response_body: None,
            final_error_code: None,
            final_error_message: None,
            request_snapshot: None,
            candidate_manifest: None,
            transform_diagnostics: Vec::new(),
            skipped_attempts: Vec::new(),
            attempts: Vec::new(),
        }
    }

    pub(super) fn set_request_snapshot(&mut self, snapshot: RequestLogBundleRequestSnapshot) {
        self.request_snapshot = Some(snapshot);
    }

    pub(super) fn set_candidate_manifest(&mut self, manifest: RequestLogBundleCandidateManifest) {
        self.candidate_manifest = Some(manifest);
    }

    pub(super) fn seed_transform_diagnostics(
        &mut self,
        diagnostics: &[RequestLogBundleTransformDiagnosticItem],
    ) {
        self.transform_diagnostics = diagnostics.to_vec();
    }

    pub(super) fn append_transform_diagnostics(
        &mut self,
        phase: RequestLogBundleTransformDiagnosticPhase,
        diagnostics: &[UnifiedTransformDiagnostic],
    ) {
        self.transform_diagnostics.extend(
            diagnostics
                .iter()
                .cloned()
                .map(|diagnostic| RequestLogBundleTransformDiagnosticItem { phase, diagnostic }),
        );
    }

    pub(super) fn replace_transform_diagnostics_phase(
        &mut self,
        phase: RequestLogBundleTransformDiagnosticPhase,
        diagnostics: &[UnifiedTransformDiagnostic],
    ) {
        self.transform_diagnostics
            .retain(|item| item.phase != phase);
        self.append_transform_diagnostics(phase, diagnostics);
    }

    pub(super) fn set_attempts_for_logging(
        &mut self,
        skipped_attempts: &[RequestAttemptDraft],
        current_attempt: Option<RequestAttemptDraft>,
    ) {
        self.skipped_attempts = skipped_attempts.to_vec();
        self.attempts = skipped_attempts.to_vec();
        if let Some(attempt) = current_attempt {
            self.attempts.push(attempt);
        }
    }
}

const ROLLUP_UNSPECIFIED_CURRENCY: &str = "NUL";

fn normalize_rollup_currency(currency: Option<&str>) -> String {
    currency
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_uppercase)
        .unwrap_or_else(|| ROLLUP_UNSPECIFIED_CURRENCY.to_string())
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

fn transform_diagnostic_loss_level_rank(loss_level: &UnifiedTransformDiagnosticLossLevel) -> u8 {
    match loss_level {
        UnifiedTransformDiagnosticLossLevel::Lossless => 0,
        UnifiedTransformDiagnosticLossLevel::LossyMinor => 1,
        UnifiedTransformDiagnosticLossLevel::LossyMajor => 2,
        UnifiedTransformDiagnosticLossLevel::Reject => 3,
    }
}

fn transform_diagnostic_loss_level_db_str(
    loss_level: &UnifiedTransformDiagnosticLossLevel,
) -> &'static str {
    match loss_level {
        UnifiedTransformDiagnosticLossLevel::Lossless => "lossless",
        UnifiedTransformDiagnosticLossLevel::LossyMinor => "lossy_minor",
        UnifiedTransformDiagnosticLossLevel::LossyMajor => "lossy_major",
        UnifiedTransformDiagnosticLossLevel::Reject => "reject",
    }
}

pub(super) fn completion_delta_from_log_context(
    context: &RequestLogContext,
) -> ApiKeyCompletionDelta {
    let cost_outcome = LogManager::build_cost_outcome(context);
    ApiKeyCompletionDelta {
        api_key_id: context.api_key_id,
        occurred_at: context.completion_ts.unwrap_or(context.request_received_at),
        total_tokens: total_tokens_for_context(context),
        billed_amount_nanos: cost_outcome.estimated_cost_nanos.unwrap_or_default(),
        billed_currency: cost_outcome.estimated_cost_currency,
    }
}

pub(super) async fn record_request_completion_and_log(
    app_state: &Arc<AppState>,
    context: RequestLogContext,
) {
    if let Err(err) = app_state
        .record_api_key_completion(&completion_delta_from_log_context(&context))
        .await
    {
        error!(
            "Failed to record api key completion delta for key {}: {}",
            context.api_key_id, err
        );
    }

    get_log_manager().log(context).await;
}

pub struct LogManager {
    sender: mpsc::Sender<LogCommand>,
    metrics: LogManagerMetrics,
}

enum LogCommand {
    Record(RequestLogContext),
    Flush(oneshot::Sender<()>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogProcessingStage {
    Accepted,
    BodyPersisting,
    BodyPersisted,
    BodyPersistFailed,
    MetadataPersisted,
    MetadataPersistFailed,
    Completed,
    NeedsCompensation,
}

#[derive(Debug, Clone)]
pub struct LogManagerMetrics {
    enqueued: Arc<AtomicU64>,
    processed: Arc<AtomicU64>,
    pending: Arc<AtomicU64>,
    in_flight: Arc<AtomicU64>,
    retries: Arc<AtomicU64>,
    channel_full_events: Arc<AtomicU64>,
    enqueue_failures: Arc<AtomicU64>,
    storage_failures: Arc<AtomicU64>,
    db_failures: Arc<AtomicU64>,
    cleanup_failures: Arc<AtomicU64>,
    compensation_needed: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Default)]
pub struct LogManagerMetricsSnapshot {
    pub enqueued: u64,
    pub processed: u64,
    pub pending: u64,
    pub in_flight: u64,
    pub retries: u64,
    pub channel_full_events: u64,
    pub enqueue_failures: u64,
    pub storage_failures: u64,
    pub db_failures: u64,
    pub cleanup_failures: u64,
    pub compensation_needed: u64,
}

impl LogManagerMetrics {
    fn new() -> Self {
        Self {
            enqueued: Arc::new(AtomicU64::new(0)),
            processed: Arc::new(AtomicU64::new(0)),
            pending: Arc::new(AtomicU64::new(0)),
            in_flight: Arc::new(AtomicU64::new(0)),
            retries: Arc::new(AtomicU64::new(0)),
            channel_full_events: Arc::new(AtomicU64::new(0)),
            enqueue_failures: Arc::new(AtomicU64::new(0)),
            storage_failures: Arc::new(AtomicU64::new(0)),
            db_failures: Arc::new(AtomicU64::new(0)),
            cleanup_failures: Arc::new(AtomicU64::new(0)),
            compensation_needed: Arc::new(AtomicU64::new(0)),
        }
    }

    fn record_enqueued(&self) {
        self.enqueued.fetch_add(1, Ordering::Relaxed);
        self.pending.fetch_add(1, Ordering::Relaxed);
    }

    fn record_started(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
        self.in_flight.fetch_add(1, Ordering::Relaxed);
    }

    fn record_processed(&self) {
        self.processed.fetch_add(1, Ordering::Relaxed);
        self.in_flight.fetch_sub(1, Ordering::Relaxed);
    }

    fn record_retry(&self) {
        self.retries.fetch_add(1, Ordering::Relaxed);
    }

    fn record_channel_full(&self) {
        self.channel_full_events.fetch_add(1, Ordering::Relaxed);
    }

    fn record_enqueue_failure(&self) {
        self.enqueue_failures.fetch_add(1, Ordering::Relaxed);
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }

    fn record_storage_failure(&self) {
        self.storage_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn record_db_failure(&self) {
        self.db_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn record_cleanup_failure(&self) {
        self.cleanup_failures.fetch_add(1, Ordering::Relaxed);
    }

    fn record_compensation_needed(&self) {
        self.compensation_needed.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> LogManagerMetricsSnapshot {
        LogManagerMetricsSnapshot {
            enqueued: self.enqueued.load(Ordering::Relaxed),
            processed: self.processed.load(Ordering::Relaxed),
            pending: self.pending.load(Ordering::Relaxed),
            in_flight: self.in_flight.load(Ordering::Relaxed),
            retries: self.retries.load(Ordering::Relaxed),
            channel_full_events: self.channel_full_events.load(Ordering::Relaxed),
            enqueue_failures: self.enqueue_failures.load(Ordering::Relaxed),
            storage_failures: self.storage_failures.load(Ordering::Relaxed),
            db_failures: self.db_failures.load(Ordering::Relaxed),
            cleanup_failures: self.cleanup_failures.load(Ordering::Relaxed),
            compensation_needed: self.compensation_needed.load(Ordering::Relaxed),
        }
    }
}

impl LogManager {
    fn new() -> Self {
        let (sender, mut receiver) = mpsc::channel::<LogCommand>(100);
        let metrics = LogManagerMetrics::new();
        let worker_metrics = metrics.clone();

        tokio::spawn(async move {
            while let Some(command) = receiver.recv().await {
                match command {
                    LogCommand::Record(context) => {
                        worker_metrics.record_started();
                        Self::process_log(context, &worker_metrics).await;
                        worker_metrics.record_processed();
                    }
                    LogCommand::Flush(done) => {
                        if done.send(()).is_err() {
                            error!("[log_manager] flush waiter dropped before completion");
                        }
                    }
                }
            }
        });

        Self { sender, metrics }
    }

    pub async fn log(&self, context: RequestLogContext) {
        self.metrics.record_enqueued();
        let command = LogCommand::Record(context);
        match self.sender.try_send(command) {
            Ok(()) => {}
            Err(tokio::sync::mpsc::error::TrySendError::Full(command)) => {
                self.metrics.record_channel_full();
                error!(
                    "[log_manager] stage={:?} reason=channel_full pending={} in_flight={}",
                    LogProcessingStage::Accepted,
                    self.metrics.snapshot().pending,
                    self.metrics.snapshot().in_flight
                );
                if let Err(e) = self.sender.send(command).await {
                    self.metrics.record_enqueue_failure();
                    error!("[log_manager] failed_to_enqueue_log: {:?}", e);
                    let LogCommand::Record(context) = e.0 else {
                        return;
                    };
                    self.metrics.record_started();
                    Self::process_log(context, &self.metrics).await;
                    self.metrics.record_processed();
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(command)) => {
                self.metrics.record_enqueue_failure();
                error!("[log_manager] failed_to_enqueue_log: channel closed");
                if let LogCommand::Record(context) = command {
                    self.metrics.record_started();
                    Self::process_log(context, &self.metrics).await;
                    self.metrics.record_processed();
                }
            }
        }
    }

    pub async fn flush(&self) {
        let (tx, rx) = oneshot::channel();
        if let Err(e) = self.sender.send(LogCommand::Flush(tx)).await {
            error!("[log_manager] failed_to_enqueue_flush: {:?}", e);
            return;
        }
        if let Err(e) = rx.await {
            error!("[log_manager] flush_wait_failed: {:?}", e);
        }
    }

    pub fn metrics(&self) -> LogManagerMetricsSnapshot {
        self.metrics.snapshot()
    }

    fn log_stage_event(
        metrics: &LogManagerMetrics,
        log_id: i64,
        stage: LogProcessingStage,
        detail: &str,
    ) {
        let snapshot = metrics.snapshot();
        error!(
            "[log_manager] log_id={} stage={:?} detail={} pending={} in_flight={} retries={} storage_failures={} db_failures={} compensation_needed={}",
            log_id,
            stage,
            detail,
            snapshot.pending,
            snapshot.in_flight,
            snapshot.retries,
            snapshot.storage_failures,
            snapshot.db_failures,
            snapshot.compensation_needed
        );
    }

    async fn put_object_with_retry(
        storage: &dyn Storage,
        key: &str,
        data: Bytes,
        options: Option<PutObjectOptions<'_>>,
        metrics: &LogManagerMetrics,
        log_id: i64,
        artifact_name: &str,
    ) -> bool {
        const MAX_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_ATTEMPTS {
            match storage.put_object(key, data.clone(), options.clone()).await {
                Ok(()) => return true,
                Err(e) => {
                    metrics.record_storage_failure();
                    Self::log_stage_event(
                        metrics,
                        log_id,
                        LogProcessingStage::BodyPersistFailed,
                        &format!(
                            "artifact={} attempt={}/{} storage_put_failed error={}",
                            artifact_name, attempt, MAX_ATTEMPTS, e
                        ),
                    );
                    if attempt < MAX_ATTEMPTS {
                        metrics.record_retry();
                        sleep(Duration::from_millis(100 * attempt as u64)).await;
                    }
                }
            }
        }

        false
    }

    fn insert_request_log_with_attempts_with_retry(
        request_log: &RequestLog,
        request_attempts: &[RequestAttempt],
        metrics: &LogManagerMetrics,
        log_id: i64,
    ) -> bool {
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 1..=MAX_ATTEMPTS {
            match RequestLog::insert_with_attempts(request_log, request_attempts) {
                Ok(_) => return true,
                Err(e) => {
                    metrics.record_db_failure();
                    Self::log_stage_event(
                        metrics,
                        log_id,
                        LogProcessingStage::MetadataPersistFailed,
                        &format!(
                            "attempt={}/{} request_log_and_attempts_insert_failed error={:?}",
                            attempt, MAX_ATTEMPTS, e
                        ),
                    );
                    if attempt < MAX_ATTEMPTS {
                        metrics.record_retry();
                        std::thread::sleep(Duration::from_millis(100 * attempt as u64));
                    }
                }
            }
        }

        false
    }

    fn add_api_key_rollup_delta_with_retry(
        request_log: &RequestLog,
        metrics: &LogManagerMetrics,
        log_id: i64,
    ) -> bool {
        const MAX_ATTEMPTS: usize = 3;
        let now = Utc::now().timestamp_millis();
        let request_at = request_log.request_received_at;
        let currency = normalize_rollup_currency(request_log.estimated_cost_currency.as_deref());
        let daily_delta = NewApiKeyRollupDaily {
            api_key_id: request_log.api_key_id,
            day_bucket: request_at.div_euclid(86_400_000) * 86_400_000,
            currency: currency.clone(),
            request_count: 1,
            total_input_tokens: i64::from(request_log.total_input_tokens.unwrap_or_default()),
            total_output_tokens: i64::from(request_log.total_output_tokens.unwrap_or_default()),
            total_reasoning_tokens: i64::from(request_log.reasoning_tokens.unwrap_or_default()),
            total_tokens: i64::from(request_log.total_tokens.unwrap_or_default()),
            billed_amount_nanos: request_log.estimated_cost_nanos.unwrap_or_default(),
            last_request_at: Some(request_at),
            created_at: now,
            updated_at: now,
        };
        let monthly_delta = NewApiKeyRollupMonthly {
            api_key_id: request_log.api_key_id,
            month_bucket: {
                let timestamp = chrono::Utc
                    .timestamp_millis_opt(request_at)
                    .single()
                    .unwrap_or_else(chrono::Utc::now);
                chrono::Utc
                    .with_ymd_and_hms(timestamp.year(), timestamp.month(), 1, 0, 0, 0)
                    .single()
                    .expect("month bucket should be valid")
                    .timestamp_millis()
            },
            currency,
            request_count: 1,
            total_input_tokens: i64::from(request_log.total_input_tokens.unwrap_or_default()),
            total_output_tokens: i64::from(request_log.total_output_tokens.unwrap_or_default()),
            total_reasoning_tokens: i64::from(request_log.reasoning_tokens.unwrap_or_default()),
            total_tokens: i64::from(request_log.total_tokens.unwrap_or_default()),
            billed_amount_nanos: request_log.estimated_cost_nanos.unwrap_or_default(),
            last_request_at: Some(request_at),
            created_at: now,
            updated_at: now,
        };

        for attempt in 1..=MAX_ATTEMPTS {
            let daily_result = ApiKeyRollupDaily::add_delta(&daily_delta);
            let monthly_result = ApiKeyRollupMonthly::add_delta(&monthly_delta);
            match (daily_result, monthly_result) {
                (Ok(_), Ok(_)) => return true,
                (daily_err, monthly_err) => {
                    metrics.record_db_failure();
                    Self::log_stage_event(
                        metrics,
                        log_id,
                        LogProcessingStage::MetadataPersistFailed,
                        &format!(
                            "attempt={}/{} api_key_rollup_update_failed daily={:?} monthly={:?}",
                            attempt,
                            MAX_ATTEMPTS,
                            daily_err.err(),
                            monthly_err.err()
                        ),
                    );
                    if attempt < MAX_ATTEMPTS {
                        metrics.record_retry();
                        std::thread::sleep(Duration::from_millis(100 * attempt as u64));
                    }
                }
            }
        }

        false
    }

    fn gzip_bytes(data: &[u8]) -> Option<Bytes> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        if let Err(e) = encoder.write_all(data) {
            error!("Failed to gzip log artifact bytes: {:?}", e);
            return None;
        };

        match encoder.finish() {
            Ok(v) => Some(Bytes::from(v)),
            Err(e) => {
                error!("Failed to finish gzip log artifact bytes: {:?}", e);
                None
            }
        }
    }

    async fn read_logged_body(body: &LoggedBody) -> Option<Bytes> {
        match body {
            LoggedBody::InMemory { bytes, .. } => Some(bytes.clone()),
            LoggedBody::Spooled { path, .. } => match fs::read(path).await {
                Ok(bytes) => Some(Bytes::from(bytes)),
                Err(e) => {
                    error!("Failed to read spooled body {:?}: {:?}", path, e);
                    None
                }
            },
        }
    }

    async fn cleanup_logged_body(body: &LoggedBody) {
        if let LoggedBody::Spooled { path, .. } = body {
            if let Err(e) = fs::remove_file(path).await {
                error!("Failed to remove spooled body {:?}: {:?}", path, e);
            }
        }
    }

    async fn store_bundle(
        storage: &dyn Storage,
        key: &str,
        id: i64,
        bundle: &RequestLogBundleV2,
        metrics: &LogManagerMetrics,
    ) -> bool {
        let serialized_body = match to_vec_named(bundle) {
            Ok(v) => v,
            Err(e) => {
                metrics.record_storage_failure();
                Self::log_stage_event(
                    metrics,
                    id,
                    LogProcessingStage::BodyPersistFailed,
                    &format!("bundle_serialize_failed error={e:?}"),
                );
                return false;
            }
        };

        let compressed_body = match Self::gzip_bytes(&serialized_body) {
            Some(body) => body,
            None => {
                metrics.record_storage_failure();
                Self::log_stage_event(
                    metrics,
                    id,
                    LogProcessingStage::BodyPersistFailed,
                    "bundle_gzip_failed",
                );
                return false;
            }
        };

        debug!("Storing log bundle for log_id {}: {:?}", id, key);

        Self::put_object_with_retry(
            storage,
            key,
            compressed_body,
            Some(PutObjectOptions {
                content_type: Some("application/msgpack"),
                content_encoding: Some("gzip"),
            }),
            metrics,
            id,
            "bundle",
        )
        .await
    }

    fn has_downstream_request(context: &RequestLogContext) -> bool {
        context.request_url.is_some()
            || context.llm_request_sent_at.is_some()
            || context.llm_status.is_some()
            || context.llm_request_body.is_some()
            || context.llm_response_body.is_some()
    }

    fn terminal_attempt_index(attempts: &[RequestAttemptDraft]) -> Option<usize> {
        attempts.iter().rposition(|attempt| {
            attempt.attempt_status != RequestAttemptStatus::Skipped
                || attempt.request_uri.is_some()
                || attempt.started_at.is_some()
        })
    }

    fn final_attempt(attempts: &[RequestAttempt]) -> Option<&RequestAttempt> {
        attempts
            .iter()
            .rfind(|attempt| attempt.attempt_status != RequestAttemptStatus::Skipped)
            .or_else(|| attempts.last())
    }

    fn fill_attempt_usage_from_context(
        attempt: &mut RequestAttemptDraft,
        context: &RequestLogContext,
    ) {
        if let Some(usage) = context.usage_normalization.as_ref() {
            attempt.total_input_tokens = attempt
                .total_input_tokens
                .or(Some(usage.total_input_tokens as i32));
            attempt.total_output_tokens = attempt
                .total_output_tokens
                .or(Some(usage.total_output_tokens as i32));
            attempt.input_text_tokens = attempt
                .input_text_tokens
                .or(Some(usage.input_text_tokens as i32));
            attempt.output_text_tokens = attempt
                .output_text_tokens
                .or(Some(usage.output_text_tokens as i32));
            attempt.input_image_tokens = attempt
                .input_image_tokens
                .or(Some(usage.input_image_tokens as i32));
            attempt.output_image_tokens = attempt
                .output_image_tokens
                .or(Some(usage.output_image_tokens as i32));
            attempt.cache_read_tokens = attempt
                .cache_read_tokens
                .or(Some(usage.cache_read_tokens as i32));
            attempt.cache_write_tokens = attempt
                .cache_write_tokens
                .or(Some(usage.cache_write_tokens as i32));
            attempt.reasoning_tokens = attempt
                .reasoning_tokens
                .or(Some(usage.reasoning_tokens as i32));
            attempt.total_tokens = attempt.total_tokens.or(Some(
                (usage.total_input_tokens + usage.total_output_tokens) as i32,
            ));
        } else if let Some(usage) = context.usage.as_ref() {
            attempt.total_input_tokens = attempt.total_input_tokens.or(Some(usage.input_tokens));
            attempt.total_output_tokens = attempt.total_output_tokens.or(Some(usage.output_tokens));
            attempt.input_image_tokens = attempt
                .input_image_tokens
                .or(Some(usage.input_image_tokens));
            attempt.output_image_tokens = attempt
                .output_image_tokens
                .or(Some(usage.output_image_tokens));
            attempt.cache_read_tokens = attempt.cache_read_tokens.or(Some(usage.cached_tokens));
            attempt.reasoning_tokens = attempt.reasoning_tokens.or(Some(usage.reasoning_tokens));
            attempt.total_tokens = attempt.total_tokens.or(Some(usage.total_tokens));
        }
    }

    fn log_body_capture_state_as_db_str(capture_state: LogBodyCaptureState) -> &'static str {
        match capture_state {
            LogBodyCaptureState::Complete => "COMPLETE",
            LogBodyCaptureState::Incomplete => "INCOMPLETE",
            LogBodyCaptureState::NotCaptured => "NOT_CAPTURED",
        }
    }

    fn merge_context_into_terminal_attempt(
        attempt: &mut RequestAttemptDraft,
        context: &RequestLogContext,
        llm_response_capture_state: Option<LogBodyCaptureState>,
        now: i64,
    ) {
        attempt.provider_id = attempt.provider_id.or(Some(context.provider_id));
        attempt.provider_api_key_id = attempt.provider_api_key_id.or(context.provider_api_key_id);
        attempt.model_id = attempt.model_id.or(Some(context.model_id));
        if attempt.provider_key_snapshot.is_none() {
            attempt.provider_key_snapshot = Some(context.provider_key.clone());
        }
        if attempt.provider_name_snapshot.is_none() {
            attempt.provider_name_snapshot = Some(context.provider_name.clone());
        }
        if attempt.model_name_snapshot.is_none() {
            attempt.model_name_snapshot = Some(context.model_name.clone());
        }
        if attempt.real_model_name_snapshot.is_none() {
            attempt.real_model_name_snapshot = Some(context.real_model_name.clone());
        }
        attempt.llm_api_type = attempt.llm_api_type.or(Some(context.llm_api_type));
        attempt.request_uri = attempt.request_uri.clone().or(context.request_url.clone());
        attempt.response_headers_json = attempt
            .response_headers_json
            .clone()
            .or(context.response_headers_json.clone());
        attempt.http_status = attempt
            .http_status
            .or_else(|| context.llm_status.map(|status| i32::from(status.as_u16())));
        attempt.started_at = attempt.started_at.or(context.llm_request_sent_at);
        attempt.first_byte_at = attempt.first_byte_at.or(context.first_chunk_ts);
        attempt.completed_at = attempt.completed_at.or(context.completion_ts).or(Some(now));
        attempt.response_started_to_client |= context.first_chunk_ts.is_some();

        match context.overall_status {
            RequestStatus::Success => {
                attempt.attempt_status = RequestAttemptStatus::Success;
                attempt.scheduler_action = SchedulerAction::ReturnSuccess;
            }
            RequestStatus::Cancelled => {
                attempt.attempt_status = RequestAttemptStatus::Cancelled;
                if attempt.scheduler_action == SchedulerAction::ReturnSuccess {
                    attempt.scheduler_action = SchedulerAction::FailFast;
                }
            }
            RequestStatus::Error => {
                if attempt.attempt_status == RequestAttemptStatus::Skipped
                    || attempt.request_uri.is_some()
                    || attempt.started_at.is_some()
                {
                    attempt.attempt_status = RequestAttemptStatus::Error;
                }
                if attempt.scheduler_action == SchedulerAction::ReturnSuccess {
                    attempt.scheduler_action = SchedulerAction::FailFast;
                }
            }
            RequestStatus::Pending => {}
        }

        if attempt.error_code.is_none() {
            attempt.error_code = context.final_error_code.clone();
        }
        if attempt.error_message.is_none() {
            attempt.error_message = context.final_error_message.clone();
        }

        Self::fill_attempt_usage_from_context(attempt, context);
        let cost_outcome = Self::build_cost_outcome(context);
        attempt.estimated_cost_nanos = attempt
            .estimated_cost_nanos
            .or(cost_outcome.estimated_cost_nanos);
        attempt.estimated_cost_currency = attempt
            .estimated_cost_currency
            .clone()
            .or(cost_outcome.estimated_cost_currency);
        attempt.cost_catalog_version_id = attempt
            .cost_catalog_version_id
            .or(cost_outcome.cost_catalog_version_id)
            .or_else(|| {
                context
                    .cost_catalog_version
                    .as_ref()
                    .map(|version| version.id)
            });
        if let Some(capture_state) = llm_response_capture_state {
            attempt.llm_response_capture_state =
                Some(Self::log_body_capture_state_as_db_str(capture_state).to_string());
        }
        if attempt.llm_request_body_for_log.is_none() {
            attempt.llm_request_body_for_log = context.llm_request_body.clone();
        }
        if attempt.llm_response_body_for_log.is_none() {
            attempt.llm_response_body_for_log = context.llm_response_body.clone();
        }
    }

    fn synthesize_downstream_attempt(
        context: &RequestLogContext,
        now: i64,
        llm_response_capture_state: Option<LogBodyCaptureState>,
    ) -> RequestAttemptDraft {
        let mut attempt = RequestAttemptDraft {
            candidate_position: context.skipped_attempts.len() as i32 + 1,
            provider_id: Some(context.provider_id),
            provider_api_key_id: context.provider_api_key_id,
            model_id: Some(context.model_id),
            provider_key_snapshot: Some(context.provider_key.clone()),
            provider_name_snapshot: Some(context.provider_name.clone()),
            model_name_snapshot: Some(context.model_name.clone()),
            real_model_name_snapshot: Some(context.real_model_name.clone()),
            llm_api_type: Some(context.llm_api_type),
            ..RequestAttemptDraft::default()
        };
        Self::merge_context_into_terminal_attempt(
            &mut attempt,
            context,
            llm_response_capture_state,
            now,
        );
        attempt
    }

    fn request_attempt_drafts_for_context(
        context: &RequestLogContext,
        now: i64,
        llm_response_capture_state: Option<LogBodyCaptureState>,
    ) -> Vec<RequestAttemptDraft> {
        let mut attempts = if context.attempts.is_empty() {
            context.skipped_attempts.clone()
        } else {
            context.attempts.clone()
        };

        if Self::has_downstream_request(context) {
            if let Some(index) = Self::terminal_attempt_index(&attempts) {
                Self::merge_context_into_terminal_attempt(
                    &mut attempts[index],
                    context,
                    llm_response_capture_state,
                    now,
                );
            } else {
                attempts.push(Self::synthesize_downstream_attempt(
                    context,
                    now,
                    llm_response_capture_state,
                ));
            }
        }

        attempts
    }

    fn build_request_attempts_for_logging(
        context: &RequestLogContext,
        now: i64,
        llm_response_capture_state: Option<LogBodyCaptureState>,
    ) -> Vec<RequestAttempt> {
        Self::request_attempt_drafts_for_context(context, now, llm_response_capture_state)
            .iter()
            .enumerate()
            .map(|(index, attempt)| {
                attempt.to_request_attempt_with_id(
                    ID_GENERATOR.generate_id(),
                    context.id,
                    (index + 1) as i32,
                    now,
                )
            })
            .collect()
    }

    fn clear_attempt_bundle_refs(attempts: &mut [RequestAttempt]) {
        for attempt in attempts {
            attempt.llm_request_blob_id = None;
            attempt.llm_request_patch_id = None;
            attempt.llm_response_blob_id = None;
        }
    }

    fn logged_body_in_memory_bytes(body: &LoggedBody) -> Option<Bytes> {
        match body {
            LoggedBody::InMemory { bytes, .. } => Some(bytes.clone()),
            LoggedBody::Spooled { .. } => None,
        }
    }

    fn apply_attempt_rollup_to_request_log(
        request_log: &mut RequestLog,
        attempts: &[RequestAttempt],
    ) {
        request_log.attempt_count = attempts.len() as i32;
        request_log.retry_count = attempts
            .iter()
            .filter(|attempt| attempt.scheduler_action == SchedulerAction::RetrySameCandidate)
            .count() as i32;
        request_log.fallback_count = attempts
            .iter()
            .filter(|attempt| attempt.scheduler_action == SchedulerAction::FallbackNextCandidate)
            .count() as i32;
        request_log.first_attempt_started_at =
            attempts.iter().find_map(|attempt| attempt.started_at);
        if request_log.response_started_to_client_at.is_none() {
            request_log.response_started_to_client_at =
                attempts.iter().find_map(|attempt| attempt.first_byte_at);
        }
        if request_log.completed_at.is_none() {
            request_log.completed_at = attempts
                .iter()
                .rev()
                .find_map(|attempt| attempt.completed_at);
        }

        if let Some(final_attempt) = Self::final_attempt(attempts) {
            request_log.final_attempt_id = Some(final_attempt.id);
            request_log.final_provider_id = final_attempt.provider_id;
            request_log.final_provider_api_key_id = final_attempt.provider_api_key_id;
            request_log.final_model_id = final_attempt.model_id;
            request_log.final_provider_key_snapshot = final_attempt.provider_key_snapshot.clone();
            request_log.final_provider_name_snapshot = final_attempt.provider_name_snapshot.clone();
            request_log.final_model_name_snapshot = final_attempt.model_name_snapshot.clone();
            request_log.final_real_model_name_snapshot =
                final_attempt.real_model_name_snapshot.clone();
            request_log.final_llm_api_type = final_attempt.llm_api_type;
            if request_log.final_error_code.is_none() {
                request_log.final_error_code = final_attempt.error_code.clone();
            }
            if request_log.final_error_message.is_none() {
                request_log.final_error_message = final_attempt.error_message.clone();
            }
        }
    }

    async fn process_log(context: RequestLogContext, metrics: &LogManagerMetrics) {
        let log_id = context.id;
        let created_at = context.request_received_at;
        let skip_response_body_persistence =
            !should_persist_response_bodies(&context.overall_status);

        Self::log_stage_event(metrics, log_id, LogProcessingStage::BodyPersisting, "start");

        let storage = get_storage().await;
        let storage_type = storage.get_storage_type();
        let user_request_body = match context.user_request_body.as_ref() {
            Some(body) => match Self::read_logged_body(body).await {
                Some(bytes) => Some(bytes),
                None => {
                    metrics.record_storage_failure();
                    Self::log_stage_event(
                        metrics,
                        log_id,
                        LogProcessingStage::BodyPersistFailed,
                        "user_request_body_read_failed",
                    );
                    None
                }
            },
            None => None,
        };
        let llm_request_body = match context.llm_request_body.as_ref() {
            Some(body) => match Self::read_logged_body(body).await {
                Some(bytes) => Some(bytes),
                None => {
                    metrics.record_storage_failure();
                    Self::log_stage_event(
                        metrics,
                        log_id,
                        LogProcessingStage::BodyPersistFailed,
                        "llm_request_body_read_failed",
                    );
                    None
                }
            },
            None => None,
        };
        let llm_response_body = match context.llm_response_body.as_ref() {
            Some(body) if !skip_response_body_persistence => {
                match Self::read_logged_body(body).await {
                    Some(bytes) => Some(bytes),
                    None => {
                        metrics.record_storage_failure();
                        Self::log_stage_event(
                            metrics,
                            log_id,
                            LogProcessingStage::BodyPersistFailed,
                            "llm_response_body_read_failed",
                        );
                        None
                    }
                }
            }
            _ => None,
        };
        let user_response_body = match context.user_response_body.as_ref() {
            Some(body) if !skip_response_body_persistence => {
                match Self::read_logged_body(body).await {
                    Some(bytes) => Some(bytes),
                    None => {
                        metrics.record_storage_failure();
                        Self::log_stage_event(
                            metrics,
                            log_id,
                            LogProcessingStage::BodyPersistFailed,
                            "user_response_body_read_failed",
                        );
                        None
                    }
                }
            }
            _ => None,
        };

        let llm_response_capture_state = response_capture_state_for_bundle(
            context.llm_response_body.as_ref(),
            llm_response_body.as_ref(),
            context.request_url.is_some() || context.llm_status.is_some(),
        );
        let user_response_capture_state = response_capture_state_for_bundle(
            context.user_response_body.as_ref(),
            user_response_body.as_ref(),
            context.request_url.is_some() || context.user_response_body.is_some(),
        );
        let now = Utc::now().timestamp_millis();
        let mut request_attempts =
            Self::build_request_attempts_for_logging(&context, now, llm_response_capture_state);
        let bundle = Self::build_request_log_bundle_v2(
            log_id,
            created_at,
            &context,
            &mut request_attempts,
            user_request_body,
            llm_request_body,
            llm_response_body,
            llm_response_capture_state,
            user_response_body,
            user_response_capture_state,
        );
        let bundle_storage_key = generate_storage_path_from_id(created_at, log_id, &storage_type);
        let bundle_stored =
            Self::store_bundle(&**storage, &bundle_storage_key, log_id, &bundle, metrics).await;
        let final_storage_type = bundle_stored.then_some(storage_type);
        if !bundle_stored {
            Self::clear_attempt_bundle_refs(&mut request_attempts);
        }

        if bundle_stored {
            Self::log_stage_event(
                metrics,
                log_id,
                LogProcessingStage::BodyPersisted,
                "body_persisted",
            );
        } else {
            metrics.record_compensation_needed();
            Self::log_stage_event(
                metrics,
                log_id,
                LogProcessingStage::NeedsCompensation,
                "log_bundle_not_persisted",
            );
        }

        if let Some(body) = context.user_request_body.as_ref() {
            if matches!(body, LoggedBody::Spooled { .. }) {
                let path = match body {
                    LoggedBody::Spooled { path, .. } => Some(path.clone()),
                    _ => None,
                };
                Self::cleanup_logged_body(body).await;
                if let Some(path) = path {
                    if fs::metadata(path).await.is_ok() {
                        metrics.record_cleanup_failure();
                    }
                }
            }
        }
        if let Some(body) = context.llm_request_body.as_ref() {
            if matches!(body, LoggedBody::Spooled { .. }) {
                let path = match body {
                    LoggedBody::Spooled { path, .. } => Some(path.clone()),
                    _ => None,
                };
                Self::cleanup_logged_body(body).await;
                if let Some(path) = path {
                    if fs::metadata(path).await.is_ok() {
                        metrics.record_cleanup_failure();
                    }
                }
            }
        }
        if let Some(body) = context.llm_response_body.as_ref() {
            if matches!(body, LoggedBody::Spooled { .. }) {
                let path = match body {
                    LoggedBody::Spooled { path, .. } => Some(path.clone()),
                    _ => None,
                };
                Self::cleanup_logged_body(body).await;
                if let Some(path) = path {
                    if fs::metadata(path).await.is_ok() {
                        metrics.record_cleanup_failure();
                    }
                }
            }
        }
        if let Some(body) = context.user_response_body.as_ref() {
            if matches!(body, LoggedBody::Spooled { .. }) {
                let path = match body {
                    LoggedBody::Spooled { path, .. } => Some(path.clone()),
                    _ => None,
                };
                Self::cleanup_logged_body(body).await;
                if let Some(path) = path {
                    if fs::metadata(path).await.is_ok() {
                        metrics.record_cleanup_failure();
                    }
                }
            }
        }

        let mut request_log = Self::build_request_log(&context, final_storage_type, now);
        Self::apply_attempt_rollup_to_request_log(&mut request_log, &request_attempts);

        if Self::insert_request_log_with_attempts_with_retry(
            &request_log,
            &request_attempts,
            metrics,
            log_id,
        ) {
            Self::log_stage_event(
                metrics,
                log_id,
                LogProcessingStage::MetadataPersisted,
                "request_log_and_attempts_inserted",
            );
            if Self::add_api_key_rollup_delta_with_retry(&request_log, metrics, log_id) {
                Self::log_stage_event(metrics, log_id, LogProcessingStage::Completed, "done");
            } else {
                metrics.record_compensation_needed();
                Self::log_stage_event(
                    metrics,
                    log_id,
                    LogProcessingStage::NeedsCompensation,
                    "api_key_rollup_update_failed_after_retries",
                );
            }
        } else {
            metrics.record_compensation_needed();
            Self::log_stage_event(
                metrics,
                log_id,
                LogProcessingStage::NeedsCompensation,
                "request_log_insert_failed_after_retries",
            );
        }
    }

    fn build_request_log(
        context: &RequestLogContext,
        final_storage_type: Option<StorageType>,
        now: i64,
    ) -> RequestLog {
        let cost_outcome = Self::build_cost_outcome(context);
        let transform_diagnostics =
            Self::build_transform_diagnostics_asset(&context.transform_diagnostics);
        let bundle_storage_key = final_storage_type.as_ref().map(|storage_type| {
            generate_storage_path_from_id(context.request_received_at, context.id, storage_type)
        });
        let attempt_drafts = Self::request_attempt_drafts_for_context(context, now, None);
        let first_attempt_started_at = attempt_drafts
            .iter()
            .find_map(|attempt| attempt.started_at)
            .or(context.llm_request_sent_at);

        RequestLog {
            id: context.id,
            api_key_id: context.api_key_id,
            requested_model_name: Some(context.requested_model_name.clone()),
            resolved_name_scope: Some(context.resolved_name_scope.clone()),
            resolved_route_id: context.resolved_route_id,
            resolved_route_name: context.resolved_route_name.clone(),
            user_api_type: context.user_api_type,
            overall_status: context.overall_status.clone(),
            final_error_code: context.final_error_code.clone(),
            final_error_message: context.final_error_message.clone(),
            attempt_count: attempt_drafts.len() as i32,
            retry_count: attempt_drafts
                .iter()
                .filter(|attempt| attempt.scheduler_action == SchedulerAction::RetrySameCandidate)
                .count() as i32,
            fallback_count: attempt_drafts
                .iter()
                .filter(|attempt| {
                    attempt.scheduler_action == SchedulerAction::FallbackNextCandidate
                })
                .count() as i32,
            request_received_at: context.request_received_at,
            first_attempt_started_at,
            response_started_to_client_at: context.first_chunk_ts,
            completed_at: context.completion_ts.or(Some(now)),
            client_ip: context.client_ip.clone(),
            final_attempt_id: None,
            final_provider_id: Some(context.provider_id),
            final_provider_api_key_id: context.provider_api_key_id,
            final_model_id: Some(context.model_id),
            final_provider_key_snapshot: Some(context.provider_key.clone()),
            final_provider_name_snapshot: Some(context.provider_name.clone()),
            final_model_name_snapshot: Some(context.model_name.clone()),
            final_real_model_name_snapshot: Some(context.real_model_name.clone()),
            final_llm_api_type: Some(context.llm_api_type),
            estimated_cost_nanos: cost_outcome.estimated_cost_nanos,
            estimated_cost_currency: cost_outcome.estimated_cost_currency,
            cost_catalog_id: context.cost_catalog_id,
            cost_catalog_version_id: cost_outcome.cost_catalog_version_id,
            cost_snapshot_json: cost_outcome.cost_snapshot_json,
            total_input_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.total_input_tokens as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.input_tokens)),
            total_output_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.total_output_tokens as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.output_tokens)),
            input_text_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.input_text_tokens as i32),
            output_text_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.output_text_tokens as i32),
            input_image_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.input_image_tokens as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.input_image_tokens)),
            output_image_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.output_image_tokens as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.output_image_tokens)),
            cache_read_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.cache_read_tokens as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.cached_tokens)),
            cache_write_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.cache_write_tokens as i32),
            reasoning_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| u.reasoning_tokens as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.reasoning_tokens)),
            total_tokens: context
                .usage_normalization
                .as_ref()
                .map(|u| (u.total_input_tokens + u.total_output_tokens) as i32)
                .or_else(|| context.usage.as_ref().map(|u| u.total_tokens)),
            has_transform_diagnostics: !context.transform_diagnostics.is_empty(),
            transform_diagnostic_count: context.transform_diagnostics.len() as i32,
            transform_diagnostic_max_loss_level: transform_diagnostics
                .summary
                .max_loss_level
                .as_ref()
                .map(transform_diagnostic_loss_level_db_str)
                .map(str::to_string),
            bundle_version: final_storage_type
                .as_ref()
                .map(|_| REQUEST_LOG_BUNDLE_V2_VERSION as i32),
            bundle_storage_type: final_storage_type,
            bundle_storage_key,
            created_at: context.request_received_at,
            updated_at: now,
        }
    }

    fn build_request_log_bundle_v2(
        log_id: i64,
        created_at: i64,
        context: &RequestLogContext,
        request_attempts: &mut [RequestAttempt],
        user_request_body: Option<Bytes>,
        llm_request_body: Option<Bytes>,
        llm_response_body: Option<Bytes>,
        llm_response_capture_state: Option<LogBodyCaptureState>,
        user_response_body: Option<Bytes>,
        user_response_capture_state: Option<LogBodyCaptureState>,
    ) -> RequestLogBundleV2 {
        let mut builder = RequestLogBundleV2Builder::new();
        let user_request_blob_id = user_request_body
            .clone()
            .map(|body| builder.add_user_request_body(body));
        let user_response_blob_id = user_response_body
            .clone()
            .map(|body| builder.add_response_body(body));
        let body_attempt_index = request_attempts.iter().rposition(|attempt| {
            attempt.request_uri.is_some()
                || attempt.started_at.is_some()
                || attempt.attempt_status != RequestAttemptStatus::Skipped
        });
        let attempt_drafts = Self::request_attempt_drafts_for_context(
            context,
            context.completion_ts.unwrap_or(created_at),
            llm_response_capture_state,
        );

        for (index, request_attempt) in request_attempts.iter_mut().enumerate() {
            let attempt_request_body = attempt_drafts
                .get(index)
                .and_then(|attempt| attempt.llm_request_body_for_log.as_ref())
                .and_then(Self::logged_body_in_memory_bytes)
                .or_else(|| {
                    (Some(index) == body_attempt_index)
                        .then(|| llm_request_body.clone())
                        .flatten()
                });

            if let Some(body) = attempt_request_body {
                let llm_api_type = request_attempt.llm_api_type.unwrap_or(context.llm_api_type);
                let body_ref = builder.add_llm_request_body(
                    context.user_api_type,
                    llm_api_type,
                    request_attempt.attempt_index,
                    body,
                );
                request_attempt.llm_request_blob_id = Some(body_ref.blob_id);
                request_attempt.llm_request_patch_id = body_ref.patch_id;
            }

            let attempt_response_body = attempt_drafts
                .get(index)
                .and_then(|attempt| attempt.llm_response_body_for_log.as_ref())
                .and_then(Self::logged_body_in_memory_bytes)
                .or_else(|| {
                    (Some(index) == body_attempt_index)
                        .then(|| llm_response_body.clone())
                        .flatten()
                });

            if let Some(body) = attempt_response_body {
                request_attempt.llm_response_blob_id = Some(builder.add_response_body(body));
            }

            let attempt_capture_state = attempt_drafts
                .get(index)
                .and_then(|attempt| attempt.llm_response_capture_state.as_deref())
                .and_then(log_body_capture_state_from_db_str)
                .or_else(|| {
                    (Some(index) == body_attempt_index)
                        .then_some(llm_response_capture_state)
                        .flatten()
                });
            request_attempt.llm_response_capture_state =
                attempt_capture_state.map(|capture_state| {
                    Self::log_body_capture_state_as_db_str(capture_state).to_string()
                });
        }

        if request_attempts
            .iter()
            .all(|attempt| attempt.llm_request_blob_id.is_none())
        {
            if let (Some(index), Some(body)) = (body_attempt_index, llm_request_body) {
                let llm_api_type = request_attempts[index]
                    .llm_api_type
                    .unwrap_or(context.llm_api_type);
                let body_ref = builder.add_llm_request_body(
                    context.user_api_type,
                    llm_api_type,
                    request_attempts[index].attempt_index,
                    body,
                );
                request_attempts[index].llm_request_blob_id = Some(body_ref.blob_id);
                request_attempts[index].llm_request_patch_id = body_ref.patch_id;
            }
        }

        if request_attempts
            .iter()
            .all(|attempt| attempt.llm_response_blob_id.is_none())
        {
            if let (Some(index), Some(body)) = (body_attempt_index, llm_response_body) {
                request_attempts[index].llm_response_blob_id =
                    Some(builder.add_response_body(body));
            }
        }

        if let Some(index) = body_attempt_index {
            if request_attempts[index].llm_response_capture_state.is_none() {
                request_attempts[index].llm_response_capture_state = llm_response_capture_state
                    .map(|capture_state| {
                        Self::log_body_capture_state_as_db_str(capture_state).to_string()
                    });
            }
        }

        let attempt_sections = request_attempts
            .iter()
            .map(|attempt| RequestLogBundleAttemptSection {
                attempt_id: Some(attempt.id),
                attempt_index: attempt.attempt_index,
                llm_request_blob_id: attempt.llm_request_blob_id,
                llm_request_patch_id: attempt.llm_request_patch_id,
                llm_response_blob_id: attempt.llm_response_blob_id,
                llm_response_capture_state: attempt
                    .llm_response_capture_state
                    .as_deref()
                    .and_then(log_body_capture_state_from_db_str),
            })
            .collect::<Vec<_>>();

        builder.finish(
            log_id,
            created_at,
            RequestLogBundleRequestSection {
                user_request_blob_id,
                user_response_blob_id,
                user_response_capture_state,
            },
            attempt_sections,
            RequestLogBundleV2DiagnosticAssets {
                request_snapshot: context.request_snapshot.clone(),
                candidate_manifest: context.candidate_manifest.clone(),
                transform_diagnostics: Some(Self::build_transform_diagnostics_asset(
                    &context.transform_diagnostics,
                )),
            },
        )
    }

    fn build_transform_diagnostics_asset(
        items: &[RequestLogBundleTransformDiagnosticItem],
    ) -> RequestLogBundleTransformDiagnostics {
        let mut summary = RequestLogBundleTransformDiagnosticsSummary {
            count: items.len() as u32,
            max_loss_level: None,
            kinds: Vec::new(),
            phases: Vec::new(),
        };

        for item in items {
            if !summary.kinds.contains(&item.diagnostic.diagnostic_kind) {
                summary.kinds.push(item.diagnostic.diagnostic_kind.clone());
            }
            if !summary.phases.contains(&item.phase) {
                summary.phases.push(item.phase);
            }

            let should_replace_max = summary.max_loss_level.as_ref().map_or(true, |current| {
                transform_diagnostic_loss_level_rank(&item.diagnostic.loss_level)
                    > transform_diagnostic_loss_level_rank(current)
            });
            if should_replace_max {
                summary.max_loss_level = Some(item.diagnostic.loss_level.clone());
            }
        }

        RequestLogBundleTransformDiagnostics {
            summary,
            items: items.to_vec(),
        }
    }

    fn build_cost_outcome(context: &RequestLogContext) -> CostOutcome {
        let Some(normalization) = context.usage_normalization.as_ref() else {
            return CostOutcome::default();
        };

        let Some(version) = context.cost_catalog_version.as_ref() else {
            return CostOutcome::default();
        };

        let ledger = CostLedger::from(normalization);
        let rating = match rate_cost(
            &ledger,
            &CostRatingContext {
                total_input_tokens: normalization.total_input_tokens,
            },
            version,
        ) {
            Ok(result) => result,
            Err(err) => {
                return CostOutcome::from_snapshot(CostSnapshot {
                    schema_version: crate::cost::COST_SNAPSHOT_SCHEMA_VERSION_V1,
                    cost_catalog_id: version.catalog_id,
                    cost_catalog_version_id: version.id,
                    total_cost_nanos: 0,
                    currency: version.currency.clone(),
                    detail_lines: vec![],
                    unmatched_items: vec![],
                    warnings: vec![format!("cost rating failed: {:?}", err)],
                });
            }
        };

        let mut warnings = normalization.warnings.clone();
        warnings.extend(rating.warnings.clone());
        let snapshot = CostSnapshot {
            schema_version: crate::cost::COST_SNAPSHOT_SCHEMA_VERSION_V1,
            cost_catalog_id: version.catalog_id,
            cost_catalog_version_id: version.id,
            total_cost_nanos: rating.total_cost_nanos,
            currency: rating.currency.clone(),
            detail_lines: rating.detail_lines,
            unmatched_items: rating.unmatched_items,
            warnings,
        };

        CostOutcome::from_snapshot(snapshot)
    }
}

#[derive(Default)]
struct CostOutcome {
    estimated_cost_nanos: Option<i64>,
    estimated_cost_currency: Option<String>,
    cost_catalog_version_id: Option<i64>,
    cost_snapshot_json: Option<String>,
}

impl CostOutcome {
    fn from_snapshot(snapshot: CostSnapshot) -> Self {
        let snapshot_json = serde_json::to_string(&snapshot).ok();
        let estimated_cost_nanos = snapshot_json.as_ref().map(|_| snapshot.total_cost_nanos);
        Self {
            estimated_cost_nanos,
            estimated_cost_currency: Some(snapshot.currency.clone()),
            cost_catalog_version_id: Some(snapshot.cost_catalog_version_id),
            cost_snapshot_json: snapshot_json,
        }
    }
}

fn should_persist_response_bodies(status: &RequestStatus) -> bool {
    *status != RequestStatus::Cancelled
}

fn response_capture_state_for_bundle(
    logged_body: Option<&LoggedBody>,
    persisted_body: Option<&Bytes>,
    response_context_present: bool,
) -> Option<LogBodyCaptureState> {
    if persisted_body.is_some() {
        return logged_body
            .map(LoggedBody::capture_state)
            .or(Some(LogBodyCaptureState::NotCaptured));
    }

    if logged_body.is_some() || response_context_present {
        Some(LogBodyCaptureState::NotCaptured)
    } else {
        None
    }
}

fn log_body_capture_state_from_db_str(value: &str) -> Option<LogBodyCaptureState> {
    match value {
        "COMPLETE" => Some(LogBodyCaptureState::Complete),
        "INCOMPLETE" => Some(LogBodyCaptureState::Incomplete),
        "NOT_CAPTURED" => Some(LogBodyCaptureState::NotCaptured),
        _ => None,
    }
}

static LOG_MANAGER: LazyLock<LogManager> = LazyLock::new(LogManager::new);

pub fn get_log_manager() -> &'static LogManager {
    &LOG_MANAGER
}

#[cfg(test)]
mod tests {
    use super::{
        LogBodyKind, LogManager, LogManagerMetrics, LoggedBody, RequestLogContext,
        StreamingBodyWriter, response_capture_state_for_bundle, should_persist_response_bodies,
    };
    use crate::cost::UsageNormalization;
    use crate::proxy::orchestrator::{
        CAPABILITY_MISMATCH_SKIPPED_ERROR, NO_CANDIDATE_AVAILABLE_ERROR, RequestAttemptDraft,
    };
    use crate::schema::enum_def::{
        LlmApiType, ProviderApiKeyMode, ProviderType, RequestAttemptStatus, RequestStatus,
        SchedulerAction, StorageType,
    };
    use crate::service::cache::types::{
        CacheApiKey, CacheCostCatalogVersion, CacheModel, CacheProvider,
    };
    use crate::utils::storage::{
        LogBodyCaptureState, REQUEST_LOG_BUNDLE_V2_VERSION, RequestLogBundleTransformDiagnostics,
        RequestLogBundleTransformDiagnosticsSummary,
    };
    use crate::utils::usage::UsageInfo;
    use bytes::Bytes;

    fn make_log_context() -> RequestLogContext {
        let system_api_key = CacheApiKey {
            id: 1,
            api_key_hash: "hash".to_string(),
            key_prefix: "cyder-prefix".to_string(),
            key_last4: "1234".to_string(),
            name: "system".to_string(),
            description: None,
            default_action: crate::schema::enum_def::Action::Allow,
            is_enabled: true,
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
            acl_rules: vec![],
        };
        let provider = CacheProvider {
            id: 2,
            provider_key: "provider".to_string(),
            name: "Provider".to_string(),
            endpoint: "https://example.com".to_string(),
            use_proxy: false,
            provider_type: ProviderType::Openai,
            provider_api_key_mode: ProviderApiKeyMode::Queue,
            is_enabled: true,
        };
        let model = CacheModel {
            id: 3,
            provider_id: 2,
            model_name: "gpt-test".to_string(),
            real_model_name: Some("real-gpt-test".to_string()),
            cost_catalog_id: None,
            supports_streaming: true,
            supports_tools: true,
            supports_reasoning: true,
            supports_image_input: true,
            supports_embeddings: true,
            supports_rerank: true,
            is_enabled: true,
        };

        RequestLogContext::new(
            &system_api_key,
            &provider,
            &model,
            Some(4),
            "manual-smoke-route",
            "global_route",
            Some(8),
            Some("manual-smoke-route"),
            1234,
            &Some("127.0.0.1".to_string()),
            LlmApiType::Responses,
            LlmApiType::Anthropic,
        )
    }

    #[test]
    fn request_log_bundle_v2_tracks_sections_and_blob_refs() {
        let mut context = make_log_context();
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.overall_status = RequestStatus::Success;
        let mut request_attempts = LogManager::build_request_attempts_for_logging(
            &context,
            2_000,
            Some(LogBodyCaptureState::Incomplete),
        );
        let bundle = LogManager::build_request_log_bundle_v2(
            42,
            1_744_100_800_000,
            &context,
            &mut request_attempts,
            Some(Bytes::from_static(b"user request")),
            Some(Bytes::from_static(b"llm request")),
            Some(Bytes::from_static(b"llm response")),
            Some(LogBodyCaptureState::Incomplete),
            Some(Bytes::from_static(b"user response")),
            Some(LogBodyCaptureState::Complete),
        );

        assert_eq!(bundle.version, REQUEST_LOG_BUNDLE_V2_VERSION);
        assert_eq!(bundle.request_section.user_request_blob_id, Some(1));
        assert_eq!(bundle.request_section.user_response_blob_id, Some(2));
        assert_eq!(
            bundle.request_section.user_response_capture_state,
            Some(LogBodyCaptureState::Complete)
        );
        assert_eq!(bundle.attempt_sections.len(), 1);
        assert_eq!(
            bundle.attempt_sections[0].attempt_id,
            Some(request_attempts[0].id)
        );
        assert_eq!(bundle.attempt_sections[0].llm_request_blob_id, Some(3));
        assert_eq!(
            bundle.attempt_sections[0].llm_response_capture_state,
            Some(LogBodyCaptureState::Incomplete)
        );
        assert_eq!(bundle.request_snapshot, None);
        assert_eq!(bundle.candidate_manifest, None);
        assert_eq!(
            bundle.transform_diagnostics,
            Some(RequestLogBundleTransformDiagnostics {
                summary: RequestLogBundleTransformDiagnosticsSummary::default(),
                items: Vec::new(),
            })
        );
        assert_eq!(bundle.blob_pool.len(), 4);
        assert_eq!(request_attempts[0].llm_request_blob_id, Some(3));
        assert_eq!(request_attempts[0].llm_response_blob_id, Some(4));

        LogManager::clear_attempt_bundle_refs(&mut request_attempts);
        assert_eq!(request_attempts[0].llm_request_blob_id, None);
        assert_eq!(request_attempts[0].llm_request_patch_id, None);
        assert_eq!(request_attempts[0].llm_response_blob_id, None);
    }

    #[test]
    fn request_log_bundle_v2_uses_each_attempt_body_snapshot() {
        let mut context = make_log_context();
        context.user_api_type = LlmApiType::Openai;
        context.llm_api_type = LlmApiType::Openai;
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.overall_status = RequestStatus::Success;
        context.completion_ts = Some(2_000);

        let prompt = "This long prompt makes a model-only JSON patch smaller than storing the full request body for every attempt.";
        let user_request = Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "model": "route",
                "messages": [{"role": "user", "content": prompt}]
            }))
            .unwrap(),
        );
        let first_attempt_body = Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "model": "candidate-a",
                "messages": [{"role": "user", "content": prompt}]
            }))
            .unwrap(),
        );
        let second_attempt_body = Bytes::from(
            serde_json::to_vec(&serde_json::json!({
                "model": "candidate-b",
                "messages": [{"role": "user", "content": prompt}]
            }))
            .unwrap(),
        );

        context.attempts = vec![
            RequestAttemptDraft {
                candidate_position: 1,
                provider_id: Some(2),
                model_id: Some(3),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Error,
                scheduler_action: SchedulerAction::FallbackNextCandidate,
                request_uri: Some("https://example.com/v1/chat/completions".to_string()),
                started_at: Some(1_000),
                completed_at: Some(1_100),
                llm_request_body_for_log: Some(LoggedBody::from_bytes(first_attempt_body)),
                llm_response_body_for_log: Some(LoggedBody::from_bytes(Bytes::from_static(
                    br#"{"error":"rate limited"}"#,
                ))),
                llm_response_capture_state: Some("COMPLETE".to_string()),
                ..RequestAttemptDraft::default()
            },
            RequestAttemptDraft {
                candidate_position: 2,
                provider_id: Some(2),
                model_id: Some(4),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Success,
                scheduler_action: SchedulerAction::ReturnSuccess,
                request_uri: Some("https://example.com/v1/chat/completions".to_string()),
                started_at: Some(1_200),
                completed_at: Some(1_300),
                llm_request_body_for_log: Some(LoggedBody::from_bytes(second_attempt_body)),
                llm_response_body_for_log: Some(LoggedBody::from_bytes(Bytes::from_static(
                    br#"{"choices":[{"message":{"content":"ok"}}]}"#,
                ))),
                llm_response_capture_state: Some("COMPLETE".to_string()),
                ..RequestAttemptDraft::default()
            },
        ];

        let mut request_attempts =
            LogManager::build_request_attempts_for_logging(&context, 2_000, None);
        let bundle = LogManager::build_request_log_bundle_v2(
            42,
            1_744_100_800_000,
            &context,
            &mut request_attempts,
            Some(user_request),
            None,
            None,
            None,
            None,
            None,
        );

        assert_eq!(bundle.attempt_sections.len(), 2);
        assert!(bundle.attempt_sections[0].llm_request_blob_id.is_some());
        assert!(bundle.attempt_sections[1].llm_request_blob_id.is_some());
        assert!(bundle.attempt_sections[0].llm_request_patch_id.is_some());
        assert!(bundle.attempt_sections[1].llm_request_patch_id.is_some());
        assert!(bundle.attempt_sections[0].llm_response_blob_id.is_some());
        assert!(bundle.attempt_sections[1].llm_response_blob_id.is_some());
        assert_eq!(request_attempts[0].llm_request_patch_id, Some(1));
        assert_eq!(request_attempts[1].llm_request_patch_id, Some(2));
    }

    #[test]
    fn request_log_bundle_v2_marks_response_as_not_captured_when_body_is_absent() {
        let mut context = make_log_context();
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.llm_status = Some(reqwest::StatusCode::OK);
        context.overall_status = RequestStatus::Success;
        let mut request_attempts = LogManager::build_request_attempts_for_logging(
            &context,
            2_000,
            Some(LogBodyCaptureState::NotCaptured),
        );

        let bundle = LogManager::build_request_log_bundle_v2(
            42,
            1_744_100_800_000,
            &context,
            &mut request_attempts,
            Some(Bytes::from_static(b"user request")),
            Some(Bytes::from_static(b"llm request")),
            None,
            Some(LogBodyCaptureState::NotCaptured),
            None,
            Some(LogBodyCaptureState::NotCaptured),
        );

        assert_eq!(
            bundle.request_section.user_response_capture_state,
            Some(LogBodyCaptureState::NotCaptured)
        );
        assert_eq!(bundle.request_section.user_response_blob_id, None);
        assert_eq!(
            bundle.attempt_sections[0].llm_response_capture_state,
            Some(LogBodyCaptureState::NotCaptured)
        );
        assert_eq!(bundle.attempt_sections[0].llm_response_blob_id, None);
    }

    #[test]
    fn request_log_rollup_uses_preallocated_attempt_ids_and_scheduler_counts() {
        let mut context = make_log_context();
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.llm_request_sent_at = Some(1_500);
        context.first_chunk_ts = Some(1_600);
        context.completion_ts = Some(1_700);
        context.overall_status = RequestStatus::Success;
        let skipped_attempt = RequestAttemptDraft {
            candidate_position: 1,
            provider_id: Some(10),
            provider_name_snapshot: Some("Skipped Provider".to_string()),
            model_id: Some(11),
            model_name_snapshot: Some("skipped-model".to_string()),
            llm_api_type: Some(LlmApiType::Openai),
            attempt_status: RequestAttemptStatus::Skipped,
            scheduler_action: SchedulerAction::FallbackNextCandidate,
            error_code: Some(CAPABILITY_MISMATCH_SKIPPED_ERROR.to_string()),
            ..RequestAttemptDraft::default()
        };
        let terminal_attempt = RequestAttemptDraft {
            candidate_position: 2,
            provider_id: Some(20),
            provider_api_key_id: Some(21),
            model_id: Some(22),
            provider_key_snapshot: Some("final-provider".to_string()),
            provider_name_snapshot: Some("Final Provider".to_string()),
            model_name_snapshot: Some("final-model".to_string()),
            real_model_name_snapshot: Some("real-final-model".to_string()),
            llm_api_type: Some(LlmApiType::Anthropic),
            request_uri: context.request_url.clone(),
            started_at: Some(1_500),
            ..RequestAttemptDraft::default()
        };
        context.set_attempts_for_logging(&[skipped_attempt], Some(terminal_attempt));

        let request_attempts = LogManager::build_request_attempts_for_logging(
            &context,
            2_000,
            Some(LogBodyCaptureState::Complete),
        );
        let mut request_log =
            LogManager::build_request_log(&context, Some(StorageType::FileSystem), 2_000);
        LogManager::apply_attempt_rollup_to_request_log(&mut request_log, &request_attempts);

        assert_eq!(request_log.attempt_count, 2);
        assert_eq!(request_log.retry_count, 0);
        assert_eq!(request_log.fallback_count, 1);
        assert_eq!(request_log.final_attempt_id, Some(request_attempts[1].id));
        assert_eq!(request_log.final_provider_id, Some(20));
        assert_eq!(request_log.final_provider_api_key_id, Some(21));
        assert_eq!(
            request_log.final_model_name_snapshot.as_deref(),
            Some("final-model")
        );
    }

    #[test]
    fn request_log_rollup_counts_same_candidate_retry_success() {
        let mut context = make_log_context();
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.llm_request_sent_at = Some(1_500);
        context.first_chunk_ts = Some(1_650);
        context.completion_ts = Some(1_900);
        context.overall_status = RequestStatus::Success;
        context.attempts = vec![
            RequestAttemptDraft {
                candidate_position: 1,
                provider_id: Some(20),
                provider_api_key_id: Some(21),
                model_id: Some(22),
                provider_key_snapshot: Some("retry-provider".to_string()),
                provider_name_snapshot: Some("Retry Provider".to_string()),
                model_name_snapshot: Some("retry-model".to_string()),
                real_model_name_snapshot: Some("real-retry-model".to_string()),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Error,
                scheduler_action: SchedulerAction::RetrySameCandidate,
                error_code: Some("upstream_timeout".to_string()),
                started_at: Some(1_500),
                completed_at: Some(1_550),
                backoff_ms: Some(250),
                ..RequestAttemptDraft::default()
            },
            RequestAttemptDraft {
                candidate_position: 1,
                provider_id: Some(20),
                provider_api_key_id: Some(21),
                model_id: Some(22),
                provider_key_snapshot: Some("retry-provider".to_string()),
                provider_name_snapshot: Some("Retry Provider".to_string()),
                model_name_snapshot: Some("retry-model".to_string()),
                real_model_name_snapshot: Some("real-retry-model".to_string()),
                llm_api_type: Some(LlmApiType::Openai),
                attempt_status: RequestAttemptStatus::Success,
                scheduler_action: SchedulerAction::ReturnSuccess,
                request_uri: context.request_url.clone(),
                started_at: Some(1_650),
                first_byte_at: Some(1_700),
                completed_at: Some(1_900),
                response_started_to_client: true,
                total_input_tokens: Some(10),
                total_output_tokens: Some(20),
                total_tokens: Some(30),
                ..RequestAttemptDraft::default()
            },
        ];

        let request_attempts = LogManager::build_request_attempts_for_logging(
            &context,
            2_000,
            Some(LogBodyCaptureState::Complete),
        );
        let mut request_log =
            LogManager::build_request_log(&context, Some(StorageType::FileSystem), 2_000);
        LogManager::apply_attempt_rollup_to_request_log(&mut request_log, &request_attempts);

        assert_eq!(request_log.attempt_count, 2);
        assert_eq!(request_log.retry_count, 1);
        assert_eq!(request_log.fallback_count, 0);
        assert_eq!(request_log.final_attempt_id, Some(request_attempts[1].id));
        assert_eq!(request_log.final_provider_id, Some(20));
        assert_eq!(
            request_log.final_model_name_snapshot.as_deref(),
            Some("retry-model")
        );
    }

    #[test]
    fn terminal_attempt_merge_backfills_response_headers_from_context() {
        let mut context = make_log_context();
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.response_headers_json = Some(
            r#"{"content-type":"text/event-stream","x-request-id":"stream-req-1"}"#.to_string(),
        );
        context.llm_request_sent_at = Some(1_500);
        context.completion_ts = Some(1_700);
        context.overall_status = RequestStatus::Success;
        context.set_attempts_for_logging(
            &[],
            Some(RequestAttemptDraft {
                candidate_position: 1,
                provider_id: Some(20),
                model_id: Some(22),
                request_uri: context.request_url.clone(),
                started_at: Some(1_500),
                ..RequestAttemptDraft::default()
            }),
        );

        let request_attempts = LogManager::build_request_attempts_for_logging(
            &context,
            2_000,
            Some(LogBodyCaptureState::Complete),
        );

        assert_eq!(request_attempts.len(), 1);
        assert_eq!(
            request_attempts[0].response_headers_json.as_deref(),
            Some(r#"{"content-type":"text/event-stream","x-request-id":"stream-req-1"}"#)
        );
    }

    #[test]
    fn response_capture_state_for_bundle_marks_missing_body_as_not_captured() {
        assert_eq!(
            response_capture_state_for_bundle(None, None, true),
            Some(LogBodyCaptureState::NotCaptured)
        );
        assert_eq!(response_capture_state_for_bundle(None, None, false), None);
        assert_eq!(
            response_capture_state_for_bundle(
                Some(&LoggedBody::from_bytes(Bytes::from_static(b"ok"))),
                None,
                true,
            ),
            Some(LogBodyCaptureState::NotCaptured)
        );
        assert_eq!(
            response_capture_state_for_bundle(
                Some(&LoggedBody::from_bytes(Bytes::from_static(b"ok"))),
                Some(&Bytes::from_static(b"ok")),
                true,
            ),
            Some(LogBodyCaptureState::Complete)
        );
    }

    #[test]
    fn build_request_log_preserves_api_types_and_usage_fields() {
        let mut context = make_log_context();
        context.request_url = Some("https://example.com/v1/chat/completions".to_string());
        context.llm_request_sent_at = Some(1500);
        context.first_chunk_ts = Some(1600);
        context.completion_ts = Some(1700);
        context.is_stream = true;
        context.overall_status = RequestStatus::Success;
        context.usage = Some(UsageInfo {
            input_tokens: 10,
            output_tokens: 20,
            total_tokens: 30,
            reasoning_tokens: 5,
            input_image_tokens: 1,
            output_image_tokens: 2,
            cached_tokens: 3,
        });
        context.usage_normalization = Some(UsageNormalization {
            total_input_tokens: 10,
            total_output_tokens: 20,
            input_text_tokens: 6,
            output_text_tokens: 13,
            input_image_tokens: 1,
            output_image_tokens: 2,
            cache_read_tokens: 3,
            cache_write_tokens: 0,
            reasoning_tokens: 5,
            warnings: vec![],
        });
        context.cost_catalog_version = Some(CacheCostCatalogVersion {
            id: 9,
            catalog_id: 7,
            version: "v1".to_string(),
            currency: "USD".to_string(),
            source: None,
            effective_from: 0,
            effective_until: None,
            is_enabled: true,
            components: vec![],
        });
        let request_log =
            LogManager::build_request_log(&context, Some(StorageType::FileSystem), 2000);

        assert_eq!(request_log.user_api_type, LlmApiType::Responses);
        assert_eq!(request_log.final_llm_api_type, Some(LlmApiType::Anthropic));
        assert_eq!(
            request_log.requested_model_name.as_deref(),
            Some("manual-smoke-route")
        );
        assert_eq!(
            request_log.resolved_name_scope.as_deref(),
            Some("global_route")
        );
        assert_eq!(request_log.resolved_route_id, Some(8));
        assert_eq!(
            request_log.resolved_route_name.as_deref(),
            Some("manual-smoke-route")
        );
        assert_eq!(request_log.attempt_count, 1);
        assert_eq!(request_log.retry_count, 0);
        assert_eq!(request_log.fallback_count, 0);
        assert_eq!(request_log.final_provider_id, Some(2));
        assert_eq!(request_log.final_model_id, Some(3));
        assert_eq!(
            request_log.final_model_name_snapshot.as_deref(),
            Some("gpt-test")
        );
        assert_eq!(request_log.total_input_tokens, Some(10));
        assert_eq!(request_log.total_output_tokens, Some(20));
        assert_eq!(request_log.input_text_tokens, Some(6));
        assert_eq!(request_log.output_text_tokens, Some(13));
        assert_eq!(request_log.input_image_tokens, Some(1));
        assert_eq!(request_log.output_image_tokens, Some(2));
        assert_eq!(request_log.cache_read_tokens, Some(3));
        assert_eq!(request_log.estimated_cost_currency.as_deref(), Some("USD"));
        assert_eq!(request_log.cost_catalog_version_id, Some(9));
        assert_eq!(
            request_log.bundle_version,
            Some(REQUEST_LOG_BUNDLE_V2_VERSION as i32)
        );
        assert_eq!(
            request_log.bundle_storage_type,
            Some(StorageType::FileSystem)
        );
        assert!(request_log.bundle_storage_key.is_some());
    }

    #[test]
    fn build_request_log_preserves_route_trace_for_early_failures() {
        let mut context = make_log_context();
        context.overall_status = RequestStatus::Error;
        context.completion_ts = Some(1800);
        context.llm_request_sent_at = None;
        context.request_url = None;

        let request_log = LogManager::build_request_log(&context, None, 2000);

        assert_eq!(request_log.overall_status, RequestStatus::Error);
        assert_eq!(
            request_log.requested_model_name.as_deref(),
            Some("manual-smoke-route")
        );
        assert_eq!(
            request_log.resolved_name_scope.as_deref(),
            Some("global_route")
        );
        assert_eq!(request_log.resolved_route_id, Some(8));
        assert_eq!(
            request_log.resolved_route_name.as_deref(),
            Some("manual-smoke-route")
        );
        assert_eq!(
            request_log.final_model_name_snapshot.as_deref(),
            Some("gpt-test")
        );
        assert_eq!(
            request_log.final_real_model_name_snapshot.as_deref(),
            Some("real-gpt-test")
        );
        assert_eq!(request_log.attempt_count, 0);
        assert!(request_log.first_attempt_started_at.is_none());
        assert!(request_log.bundle_storage_key.is_none());
    }

    #[test]
    fn build_request_log_counts_skipped_attempts_and_preserves_final_error() {
        let mut context = make_log_context();
        context.overall_status = RequestStatus::Error;
        context.final_error_code = Some(NO_CANDIDATE_AVAILABLE_ERROR.to_string());
        context.final_error_message = Some("No execution candidate is available.".to_string());
        context.provider_api_key_id = None;
        context.skipped_attempts = vec![RequestAttemptDraft {
            candidate_position: 1,
            provider_id: Some(2),
            provider_api_key_id: None,
            model_id: Some(3),
            provider_key_snapshot: Some("provider".to_string()),
            provider_name_snapshot: Some("Provider".to_string()),
            model_name_snapshot: Some("gpt-test".to_string()),
            real_model_name_snapshot: Some("real-gpt-test".to_string()),
            llm_api_type: Some(LlmApiType::Anthropic),
            attempt_status: RequestAttemptStatus::Skipped,
            scheduler_action: SchedulerAction::FallbackNextCandidate,
            error_code: Some(CAPABILITY_MISMATCH_SKIPPED_ERROR.to_string()),
            error_message: Some("missing tools".to_string()),
            ..RequestAttemptDraft::default()
        }];

        let request_log = LogManager::build_request_log(&context, None, 2000);

        assert_eq!(request_log.attempt_count, 1);
        assert_eq!(request_log.fallback_count, 1);
        assert_eq!(
            request_log.final_error_code.as_deref(),
            Some(NO_CANDIDATE_AVAILABLE_ERROR)
        );
        assert_eq!(request_log.final_provider_api_key_id, None);
    }

    #[tokio::test]
    async fn streaming_body_writer_spools_and_finishes() {
        let mut writer = StreamingBodyWriter::new(LogBodyKind::LlmResponse, 42)
            .await
            .unwrap();
        writer.append(b"hello ").await.unwrap();
        writer.append(b"world").await.unwrap();

        let logged_body = writer
            .finish(LogBodyCaptureState::Incomplete)
            .await
            .unwrap();
        let body_bytes = LogManager::read_logged_body(&logged_body).await.unwrap();
        assert_eq!(body_bytes, Bytes::from_static(b"hello world"));
        assert_eq!(logged_body.capture_state(), LogBodyCaptureState::Incomplete);

        LogManager::cleanup_logged_body(&logged_body).await;
    }

    #[tokio::test]
    async fn streaming_body_writer_abort_removes_spooled_file() {
        let writer = StreamingBodyWriter::new(LogBodyKind::LlmResponse, 43)
            .await
            .unwrap();
        let snapshot = writer.snapshot(LogBodyCaptureState::Incomplete);
        writer.abort().await.unwrap();

        assert!(LogManager::read_logged_body(&snapshot).await.is_none());
    }

    #[test]
    fn cancelled_logs_skip_response_body_persistence() {
        assert!(!should_persist_response_bodies(&RequestStatus::Cancelled));
        assert!(should_persist_response_bodies(&RequestStatus::Success));
        assert!(should_persist_response_bodies(&RequestStatus::Error));
    }

    #[test]
    fn log_manager_metrics_snapshot_tracks_counters() {
        let metrics = LogManagerMetrics::new();
        metrics.record_enqueued();
        metrics.record_channel_full();
        metrics.record_started();
        metrics.record_retry();
        metrics.record_storage_failure();
        metrics.record_db_failure();
        metrics.record_compensation_needed();
        metrics.record_processed();

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.enqueued, 1);
        assert_eq!(snapshot.processed, 1);
        assert_eq!(snapshot.pending, 0);
        assert_eq!(snapshot.in_flight, 0);
        assert_eq!(snapshot.retries, 1);
        assert_eq!(snapshot.channel_full_events, 1);
        assert_eq!(snapshot.storage_failures, 1);
        assert_eq!(snapshot.db_failures, 1);
        assert_eq!(snapshot.compensation_needed, 1);
    }

    #[tokio::test]
    async fn log_manager_flush_returns_on_empty_queue() {
        let manager = LogManager::new();
        manager.flush().await;
        let snapshot = manager.metrics();
        assert_eq!(snapshot.pending, 0);
    }
}
