use crate::{
    cost::{CostLedger, CostRatingContext, CostSnapshot, UsageNormalization, rate_cost},
    database::{
        api_key_rollup::{
            ApiKeyRollupDaily, ApiKeyRollupMonthly, NewApiKeyRollupDaily, NewApiKeyRollupMonthly,
        },
        request_log::RequestLog,
    },
    schema::enum_def::{LlmApiType, RequestStatus, StorageType},
    service::app_state::{ApiKeyCompletionDelta, AppState},
    service::cache::types::{
        CacheCostCatalogVersion, CacheModel, CacheProvider, CacheSystemApiKey,
    },
    service::storage::{Storage, get_storage, types::PutObjectOptions},
    utils::{
        ID_GENERATOR,
        storage::{LogBodyCaptureState, LogBundle, generate_storage_path_from_id},
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
    pub system_api_key_id: i64,
    pub provider_id: i64,
    pub model_id: i64,
    pub provider_api_key_id: i64,
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
}

impl RequestLogContext {
    pub fn new(
        system_api_key: &CacheSystemApiKey,
        provider: &CacheProvider,
        model: &CacheModel,
        provider_api_key_id: i64,
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
            system_api_key_id: system_api_key.id,
            provider_id: provider.id,
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
        }
    }
}

pub(super) fn build_initial_request_log_context(
    system_api_key: &CacheSystemApiKey,
    provider: &CacheProvider,
    model: &CacheModel,
    provider_api_key_id: i64,
    requested_model_name: &str,
    resolved_name_scope: &str,
    resolved_route_id: Option<i64>,
    resolved_route_name: Option<&str>,
    start_time: i64,
    client_ip_addr: &Option<String>,
    user_api_type: LlmApiType,
    llm_api_type: LlmApiType,
    user_request_body: Option<Bytes>,
) -> RequestLogContext {
    let mut context = RequestLogContext::new(
        system_api_key,
        provider,
        model,
        provider_api_key_id,
        requested_model_name,
        resolved_name_scope,
        resolved_route_id,
        resolved_route_name,
        start_time,
        client_ip_addr,
        user_api_type,
        llm_api_type,
    );
    context.user_request_body = user_request_body.map(LoggedBody::from_bytes);
    context
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

pub(super) fn completion_delta_from_log_context(
    context: &RequestLogContext,
) -> ApiKeyCompletionDelta {
    let cost_outcome = LogManager::build_cost_outcome(context);
    ApiKeyCompletionDelta {
        api_key_id: context.system_api_key_id,
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
            context.system_api_key_id, err
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
                }
            }
            Err(tokio::sync::mpsc::error::TrySendError::Closed(_command)) => {
                self.metrics.record_enqueue_failure();
                error!("[log_manager] failed_to_enqueue_log: channel closed");
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

    fn insert_request_log_with_retry(
        request_log: &RequestLog,
        metrics: &LogManagerMetrics,
        log_id: i64,
    ) -> bool {
        const MAX_ATTEMPTS: usize = 3;

        for attempt in 1..=MAX_ATTEMPTS {
            match RequestLog::insert(request_log) {
                Ok(_) => return true,
                Err(e) => {
                    metrics.record_db_failure();
                    Self::log_stage_event(
                        metrics,
                        log_id,
                        LogProcessingStage::MetadataPersistFailed,
                        &format!(
                            "attempt={}/{} request_log_insert_failed error={:?}",
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
            api_key_id: request_log.system_api_key_id,
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
            api_key_id: request_log.system_api_key_id,
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
        storage_type: &crate::schema::enum_def::StorageType,
        created_at: i64,
        id: i64,
        bundle: &LogBundle,
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

        let key = generate_storage_path_from_id(created_at, id, storage_type);

        debug!("Storing log bundle for log_id {}: {:?}", id, key);

        Self::put_object_with_retry(
            storage,
            &key,
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

        let bundle = LogBundle {
            version: 1,
            log_id,
            created_at,
            user_request_body,
            llm_request_body,
            llm_response_body: llm_response_body.clone(),
            llm_response_capture_state: llm_response_body.as_ref().and_then(|_| {
                context
                    .llm_response_body
                    .as_ref()
                    .map(LoggedBody::capture_state)
            }),
            user_response_body: user_response_body.clone(),
            user_response_capture_state: user_response_body.as_ref().and_then(|_| {
                context
                    .user_response_body
                    .as_ref()
                    .map(LoggedBody::capture_state)
            }),
        };
        let bundle_stored = Self::store_bundle(
            &**storage,
            &storage_type,
            created_at,
            log_id,
            &bundle,
            metrics,
        )
        .await;
        let final_storage_type = bundle_stored.then_some(storage_type);

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

        let now = Utc::now().timestamp_millis();

        let request_log = Self::build_request_log(&context, final_storage_type, now);

        if Self::insert_request_log_with_retry(&request_log, metrics, log_id) {
            Self::log_stage_event(
                metrics,
                log_id,
                LogProcessingStage::MetadataPersisted,
                "request_log_inserted",
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

        RequestLog {
            id: context.id,
            system_api_key_id: context.system_api_key_id,
            provider_id: context.provider_id,
            model_id: context.model_id,
            provider_api_key_id: context.provider_api_key_id,
            requested_model_name: Some(context.requested_model_name.clone()),
            resolved_name_scope: Some(context.resolved_name_scope.clone()),
            resolved_route_id: context.resolved_route_id,
            resolved_route_name: context.resolved_route_name.clone(),
            model_name: context.model_name.clone(),
            real_model_name: context.real_model_name.clone(),
            request_received_at: context.request_received_at,
            llm_request_sent_at: context.llm_request_sent_at.unwrap_or(now),
            llm_response_first_chunk_at: context.first_chunk_ts,
            llm_response_completed_at: context.completion_ts,
            client_ip: context.client_ip.clone(),
            llm_request_uri: context.request_url.clone(),
            llm_response_status: context.llm_status.map(|s| s.as_u16() as i32),
            status: Some(context.overall_status.clone()),
            is_stream: context.is_stream,
            estimated_cost_nanos: cost_outcome.estimated_cost_nanos,
            estimated_cost_currency: cost_outcome.estimated_cost_currency,
            cost_catalog_id: context.cost_catalog_id,
            cost_catalog_version_id: cost_outcome.cost_catalog_version_id,
            cost_snapshot_json: cost_outcome.cost_snapshot_json,
            created_at: context.request_received_at,
            updated_at: now,
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
            storage_type: final_storage_type,
            user_request_body: None,
            llm_request_body: None,
            llm_response_body: None,
            user_response_body: None,
            user_api_type: context.user_api_type,
            llm_api_type: context.llm_api_type,
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

static LOG_MANAGER: LazyLock<LogManager> = LazyLock::new(LogManager::new);

pub fn get_log_manager() -> &'static LogManager {
    &LOG_MANAGER
}

#[cfg(test)]
mod tests {
    use super::{
        LogBodyKind, LogManager, LogManagerMetrics, RequestLogContext, StreamingBodyWriter,
        should_persist_response_bodies,
    };
    use crate::cost::UsageNormalization;
    use crate::schema::enum_def::{
        LlmApiType, ProviderApiKeyMode, ProviderType, RequestStatus, StorageType,
    };
    use crate::service::cache::types::{
        CacheCostCatalogVersion, CacheModel, CacheProvider, CacheSystemApiKey,
    };
    use crate::utils::storage::{LogBodyCaptureState, LogBundle};
    use crate::utils::usage::UsageInfo;
    use bytes::Bytes;

    fn make_log_context() -> RequestLogContext {
        let system_api_key = CacheSystemApiKey {
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
            is_enabled: true,
        };

        RequestLogContext::new(
            &system_api_key,
            &provider,
            &model,
            4,
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
    fn log_bundle_tracks_only_response_capture_state() {
        let bundle = LogBundle {
            version: 1,
            log_id: 42,
            created_at: 1_744_100_800_000,
            user_request_body: Some(Bytes::from_static(b"user request")),
            llm_request_body: Some(Bytes::from_static(b"llm request")),
            llm_response_body: Some(Bytes::from_static(b"llm response")),
            llm_response_capture_state: Some(LogBodyCaptureState::Incomplete),
            user_response_body: Some(Bytes::from_static(b"user response")),
            user_response_capture_state: Some(LogBodyCaptureState::Complete),
        };

        assert_eq!(
            bundle.user_request_body,
            Some(Bytes::from_static(b"user request"))
        );
        assert_eq!(
            bundle.llm_response_capture_state,
            Some(LogBodyCaptureState::Incomplete)
        );
        assert_eq!(
            bundle.user_response_capture_state,
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
        assert_eq!(request_log.llm_api_type, LlmApiType::Anthropic);
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
            request_log.llm_request_uri.as_deref(),
            Some("https://example.com/v1/chat/completions")
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
    }

    #[test]
    fn build_request_log_preserves_route_trace_for_early_failures() {
        let mut context = make_log_context();
        context.overall_status = RequestStatus::Error;
        context.completion_ts = Some(1800);
        context.llm_request_sent_at = None;
        context.request_url = None;

        let request_log = LogManager::build_request_log(&context, None, 2000);

        assert_eq!(request_log.status, Some(RequestStatus::Error));
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
        assert_eq!(request_log.model_name, "gpt-test");
        assert_eq!(request_log.real_model_name, "real-gpt-test");
        assert!(request_log.llm_request_uri.is_none());
        assert!(request_log.llm_response_status.is_none());
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
