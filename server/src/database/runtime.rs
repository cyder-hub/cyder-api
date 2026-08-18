use std::{
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use diesel::{ConnectionError, SqliteConnection};
use diesel_async::{
    AsyncConnection, AsyncPgConnection, SimpleAsyncConnection,
    pooled_connection::{AsyncDieselConnectionManager, ManagerConfig, bb8::Pool},
    sync_connection_wrapper::SyncConnectionWrapper,
};
use futures::{FutureExt, future::BoxFuture};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, oneshot};

use crate::config::DatabaseIoConfig;

use super::{
    DbType,
    error::{DatabaseRuntimeInitError, PersistenceError},
    parse_db_type,
};

type PgPool = Pool<AsyncPgConnection>;
type SqlitePool = Pool<SyncConnectionWrapper<SqliteConnection>>;
type PgPooledConnection =
    diesel_async::pooled_connection::bb8::PooledConnection<'static, AsyncPgConnection>;
type SqlitePooledConnection = diesel_async::pooled_connection::bb8::PooledConnection<
    'static,
    SyncConnectionWrapper<SqliteConnection>,
>;

const RUNTIME_OPEN: u8 = 0;
const RUNTIME_CLOSING: u8 = 1;
const RUNTIME_CLOSED: u8 = 2;
const STATUS_EVENT_INTERVAL_MS: i64 = 60_000;

tokio::task_local! {
    static DATABASE_OPERATION_SCOPE: ();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseBackendKind {
    Postgres,
    Sqlite,
}

impl fmt::Display for DatabaseBackendKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Postgres => "postgres",
            Self::Sqlite => "sqlite",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DatabaseWorkload {
    Foreground,
    Background,
}

impl fmt::Display for DatabaseWorkload {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Foreground => "foreground",
            Self::Background => "background",
        })
    }
}

pub(crate) enum RuntimeConnection {
    Postgres(PgPooledConnection),
    Sqlite(SqlitePooledConnection),
}

impl RuntimeConnection {
    #[cfg(test)]
    pub(crate) const fn backend(&self) -> DatabaseBackendKind {
        match self {
            Self::Postgres(_) => DatabaseBackendKind::Postgres,
            Self::Sqlite(_) => DatabaseBackendKind::Sqlite,
        }
    }
}

enum RuntimePool {
    Postgres(PgPool),
    Sqlite(SqlitePool),
}

impl RuntimePool {
    const fn backend(&self) -> DatabaseBackendKind {
        match self {
            Self::Postgres(_) => DatabaseBackendKind::Postgres,
            Self::Sqlite(_) => DatabaseBackendKind::Sqlite,
        }
    }

    async fn get_owned(&self) -> Result<RuntimeConnection, PersistenceError> {
        match self {
            Self::Postgres(pool) => pool
                .get_owned()
                .await
                .map(RuntimeConnection::Postgres)
                .map_err(|_| PersistenceError::PoolAcquire {
                    backend: DatabaseBackendKind::Postgres,
                }),
            Self::Sqlite(pool) => pool
                .get_owned()
                .await
                .map(RuntimeConnection::Sqlite)
                .map_err(|_| PersistenceError::PoolAcquire {
                    backend: DatabaseBackendKind::Sqlite,
                }),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseRuntimeSnapshot {
    pub backend: DatabaseBackendKind,
    pub active: usize,
    pub waiting: usize,
    pub waiting_foreground: usize,
    pub queue_rejected: u64,
    pub acquire_timeout: u64,
    pub operation_deadline_exceeded: u64,
    pub execution_error: u64,
    pub last_success_at_ms: Option<i64>,
    pub last_error_at_ms: Option<i64>,
    pub last_recovery_at_ms: Option<i64>,
    pub shutting_down: bool,
}

struct RuntimeTelemetry {
    queue_rejected: AtomicU64,
    acquire_timeout: AtomicU64,
    operation_deadline_exceeded: AtomicU64,
    execution_error: AtomicU64,
    last_success_at_ms: AtomicI64,
    last_error_at_ms: AtomicI64,
    last_recovery_at_ms: AtomicI64,
    degraded: AtomicBool,
    last_status_event_at_ms: AtomicI64,
}

impl Default for RuntimeTelemetry {
    fn default() -> Self {
        Self {
            queue_rejected: AtomicU64::new(0),
            acquire_timeout: AtomicU64::new(0),
            operation_deadline_exceeded: AtomicU64::new(0),
            execution_error: AtomicU64::new(0),
            last_success_at_ms: AtomicI64::new(-1),
            last_error_at_ms: AtomicI64::new(-1),
            last_recovery_at_ms: AtomicI64::new(-1),
            degraded: AtomicBool::new(false),
            last_status_event_at_ms: AtomicI64::new(-1),
        }
    }
}

impl RuntimeTelemetry {
    fn record_queue_rejected(&self, backend: DatabaseBackendKind, workload: DatabaseWorkload) {
        self.queue_rejected.fetch_add(1, Ordering::Relaxed);
        self.record_error(backend, workload, "queue_full");
    }

    fn record_acquire_timeout(&self, backend: DatabaseBackendKind, workload: DatabaseWorkload) {
        self.acquire_timeout.fetch_add(1, Ordering::Relaxed);
        self.record_error(backend, workload, "acquire_timeout");
    }

    fn record_deadline(&self, backend: DatabaseBackendKind, workload: DatabaseWorkload) {
        self.operation_deadline_exceeded
            .fetch_add(1, Ordering::Relaxed);
        self.record_error(backend, workload, "operation_deadline_exceeded");
    }

    fn record_execution_error(&self, backend: DatabaseBackendKind, workload: DatabaseWorkload) {
        self.execution_error.fetch_add(1, Ordering::Relaxed);
        self.record_error(backend, workload, "execution_error");
    }

    fn record_error(
        &self,
        backend: DatabaseBackendKind,
        workload: DatabaseWorkload,
        outcome: &'static str,
    ) {
        let now = now_ms();
        self.last_error_at_ms.store(now, Ordering::Relaxed);
        self.degraded.store(true, Ordering::Release);
        let previous = self.last_status_event_at_ms.load(Ordering::Relaxed);
        if (previous < 0 || now.saturating_sub(previous) >= STATUS_EVENT_INTERVAL_MS)
            && self
                .last_status_event_at_ms
                .compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            crate::warn_event!(
                "database.runtime_degraded",
                backend = backend.to_string(),
                workload = workload.to_string(),
                outcome = outcome,
            );
        }
    }

    fn record_success(&self, backend: DatabaseBackendKind, workload: DatabaseWorkload) {
        let now = now_ms();
        self.last_success_at_ms.store(now, Ordering::Relaxed);
        if self.degraded.swap(false, Ordering::AcqRel) {
            self.last_recovery_at_ms.store(now, Ordering::Relaxed);
            crate::info_event!(
                "database.runtime_recovered",
                backend = backend.to_string(),
                workload = workload.to_string(),
                outcome = "success",
            );
        }
    }
}

struct DatabaseRuntimeInner {
    pool: RuntimePool,
    capacity: Arc<Semaphore>,
    waiters: Arc<Semaphore>,
    background_capacity: Arc<Semaphore>,
    queue_wait_timeout: Duration,
    operation_deadline: Duration,
    state: AtomicU8,
    active: AtomicUsize,
    operation_lifecycle: Mutex<()>,
    waiting: AtomicUsize,
    waiting_foreground: AtomicUsize,
    state_notify: Notify,
    admission_notify: Notify,
    telemetry: RuntimeTelemetry,
}

#[derive(Clone)]
pub struct DatabaseRuntime {
    inner: Arc<DatabaseRuntimeInner>,
}

impl fmt::Debug for DatabaseRuntime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DatabaseRuntime")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

struct WaiterGuard {
    inner: Arc<DatabaseRuntimeInner>,
    workload: DatabaseWorkload,
    _permit: OwnedSemaphorePermit,
}

impl Drop for WaiterGuard {
    fn drop(&mut self) {
        self.inner.waiting.fetch_sub(1, Ordering::AcqRel);
        if self.workload == DatabaseWorkload::Foreground {
            self.inner.waiting_foreground.fetch_sub(1, Ordering::AcqRel);
        }
        self.inner.admission_notify.notify_waiters();
    }
}

struct AdmissionPermit {
    inner: Arc<DatabaseRuntimeInner>,
    _capacity: OwnedSemaphorePermit,
    _background: Option<OwnedSemaphorePermit>,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.inner.admission_notify.notify_waiters();
    }
}

struct ActiveOperationGuard {
    inner: Arc<DatabaseRuntimeInner>,
    _admission: AdmissionPermit,
}

impl ActiveOperationGuard {
    fn try_new(
        inner: Arc<DatabaseRuntimeInner>,
        admission: AdmissionPermit,
    ) -> Result<Self, AdmissionPermit> {
        let lifecycle = inner
            .operation_lifecycle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.state.load(Ordering::Acquire) != RUNTIME_OPEN {
            return Err(admission);
        }
        inner.active.fetch_add(1, Ordering::AcqRel);
        drop(lifecycle);
        Ok(Self {
            inner,
            _admission: admission,
        })
    }
}

impl Drop for ActiveOperationGuard {
    fn drop(&mut self) {
        self.inner.active.fetch_sub(1, Ordering::AcqRel);
        self.inner.state_notify.notify_waiters();
        self.inner.admission_notify.notify_waiters();
    }
}

impl DatabaseRuntime {
    pub async fn connect(
        db_url: &str,
        pool_size: u32,
        config: DatabaseIoConfig,
    ) -> Result<Self, DatabaseRuntimeInitError> {
        if !(1..=128).contains(&pool_size) || config.validate().is_err() {
            return Err(DatabaseRuntimeInitError::InvalidConfig);
        }
        let queue_wait_timeout = Duration::from_secs(config.queue_wait_timeout_seconds);
        let operation_deadline = Duration::from_secs(config.operation_deadline_seconds);
        let pool = match parse_db_type(db_url) {
            DbType::Postgres => RuntimePool::Postgres(
                build_postgres_pool(db_url, pool_size, queue_wait_timeout, operation_deadline)
                    .await?,
            ),
            DbType::Sqlite => RuntimePool::Sqlite(
                build_sqlite_pool(
                    db_url,
                    pool_size,
                    queue_wait_timeout,
                    config.sqlite_busy_timeout(),
                )
                .await?,
            ),
        };
        let background_limit = if pool_size > 1 { pool_size - 1 } else { 1 };
        Ok(Self {
            inner: Arc::new(DatabaseRuntimeInner {
                pool,
                capacity: Arc::new(Semaphore::new(pool_size as usize)),
                waiters: Arc::new(Semaphore::new(config.max_waiters as usize)),
                background_capacity: Arc::new(Semaphore::new(background_limit as usize)),
                queue_wait_timeout,
                operation_deadline,
                state: AtomicU8::new(RUNTIME_OPEN),
                active: AtomicUsize::new(0),
                operation_lifecycle: Mutex::new(()),
                waiting: AtomicUsize::new(0),
                waiting_foreground: AtomicUsize::new(0),
                state_notify: Notify::new(),
                admission_notify: Notify::new(),
                telemetry: RuntimeTelemetry::default(),
            }),
        })
    }

    pub fn backend(&self) -> DatabaseBackendKind {
        self.inner.pool.backend()
    }

    pub fn snapshot(&self) -> DatabaseRuntimeSnapshot {
        DatabaseRuntimeSnapshot {
            backend: self.backend(),
            active: self.inner.active.load(Ordering::Acquire),
            waiting: self.inner.waiting.load(Ordering::Acquire),
            waiting_foreground: self.inner.waiting_foreground.load(Ordering::Acquire),
            queue_rejected: self.inner.telemetry.queue_rejected.load(Ordering::Relaxed),
            acquire_timeout: self.inner.telemetry.acquire_timeout.load(Ordering::Relaxed),
            operation_deadline_exceeded: self
                .inner
                .telemetry
                .operation_deadline_exceeded
                .load(Ordering::Relaxed),
            execution_error: self.inner.telemetry.execution_error.load(Ordering::Relaxed),
            last_success_at_ms: optional_timestamp(&self.inner.telemetry.last_success_at_ms),
            last_error_at_ms: optional_timestamp(&self.inner.telemetry.last_error_at_ms),
            last_recovery_at_ms: optional_timestamp(&self.inner.telemetry.last_recovery_at_ms),
            shutting_down: self.inner.state.load(Ordering::Acquire) != RUNTIME_OPEN,
        }
    }

    pub(crate) fn operation_deadline(&self) -> Duration {
        self.inner.operation_deadline
    }

    #[cfg(test)]
    pub(crate) async fn run<R, F>(
        &self,
        workload: DatabaseWorkload,
        operation: F,
    ) -> Result<R, PersistenceError>
    where
        R: Send + 'static,
        F: for<'connection> FnOnce(
                &'connection mut RuntimeConnection,
            )
                -> BoxFuture<'connection, Result<R, PersistenceError>>
            + Send
            + 'static,
    {
        self.run_domain(workload, operation)
            .await?
            .map_err(|error| error)
    }

    pub(crate) async fn run_db<R, F>(
        &self,
        workload: DatabaseWorkload,
        operation: F,
    ) -> super::DbResult<R>
    where
        R: Send + 'static,
        F: for<'connection> FnOnce(
                &'connection mut RuntimeConnection,
            ) -> BoxFuture<'connection, super::DbResult<R>>
            + Send
            + 'static,
    {
        self.run_domain(workload, operation)
            .await
            .map_err(crate::controller::BaseError::from)?
    }

    pub(crate) async fn run_domain_error<R, E, F>(
        &self,
        workload: DatabaseWorkload,
        operation: F,
    ) -> Result<R, E>
    where
        R: Send + 'static,
        E: From<PersistenceError> + Send + 'static,
        F: for<'connection> FnOnce(
                &'connection mut RuntimeConnection,
            ) -> BoxFuture<'connection, Result<R, E>>
            + Send
            + 'static,
    {
        self.run_domain(workload, operation)
            .await
            .map_err(E::from)?
    }

    async fn run_domain<R, E, F>(
        &self,
        workload: DatabaseWorkload,
        operation: F,
    ) -> Result<Result<R, E>, PersistenceError>
    where
        R: Send + 'static,
        E: Send + 'static,
        F: for<'connection> FnOnce(
                &'connection mut RuntimeConnection,
            ) -> BoxFuture<'connection, Result<R, E>>
            + Send
            + 'static,
    {
        if DATABASE_OPERATION_SCOPE.try_with(|()| ()).is_ok() {
            return Err(PersistenceError::NestedOperation);
        }
        let admission = self.admit(workload).await?;
        let acquire = self.inner.pool.get_owned();
        let mut connection =
            match tokio::time::timeout(self.inner.queue_wait_timeout, acquire).await {
                Ok(Ok(connection)) => connection,
                Ok(Err(error)) => {
                    self.inner
                        .telemetry
                        .record_execution_error(self.backend(), workload);
                    return Err(error);
                }
                Err(_) => {
                    self.inner
                        .telemetry
                        .record_acquire_timeout(self.backend(), workload);
                    return Err(PersistenceError::AcquireTimeout { workload });
                }
            };
        let inner = Arc::clone(&self.inner);
        let backend = self.backend();
        let deadline = inner.operation_deadline;
        let active = ActiveOperationGuard::try_new(Arc::clone(&inner), admission)
            .map_err(|_admission| PersistenceError::ShuttingDown)?;
        let (sender, receiver) = oneshot::channel();
        tokio::spawn(async move {
            let _active = active;
            let operation_future = DATABASE_OPERATION_SCOPE.scope((), operation(&mut connection));
            tokio::pin!(operation_future);
            let deadline_sleep = tokio::time::sleep(deadline);
            tokio::pin!(deadline_sleep);
            let (result, deadline_exceeded) = tokio::select! {
                result = &mut operation_future => (result, false),
                () = &mut deadline_sleep => {
                    inner.telemetry.record_deadline(backend, workload);
                    (operation_future.await, true)
                }
            };
            let result = if deadline_exceeded {
                Err(PersistenceError::OperationDeadlineExceeded { backend, workload })
            } else {
                match &result {
                    Ok(_) => inner.telemetry.record_success(backend, workload),
                    Err(_) => inner.telemetry.record_execution_error(backend, workload),
                }
                Ok(result)
            };
            let _ = sender.send(result);
        });

        receiver
            .await
            .map_err(|_| PersistenceError::ResultChannelClosed)?
    }

    pub async fn readiness_probe(&self) -> Result<(), PersistenceError> {
        if self.inner.state.load(Ordering::Acquire) != RUNTIME_OPEN {
            return Err(PersistenceError::ShuttingDown);
        }
        let capacity = Arc::clone(&self.inner.capacity)
            .try_acquire_owned()
            .map_err(|_| PersistenceError::ReadinessUnavailable {
                backend: self.backend(),
            })?;
        let backend = self.backend();
        let acquire = self.inner.pool.get_owned();
        let result = tokio::time::timeout(self.inner.queue_wait_timeout, acquire).await;
        drop(capacity);
        match result {
            Ok(Ok(_connection)) => {
                self.inner
                    .telemetry
                    .record_success(backend, DatabaseWorkload::Foreground);
                Ok(())
            }
            Ok(Err(error)) => {
                self.inner
                    .telemetry
                    .record_execution_error(backend, DatabaseWorkload::Foreground);
                Err(error)
            }
            Err(_) => {
                self.inner
                    .telemetry
                    .record_acquire_timeout(backend, DatabaseWorkload::Foreground);
                Err(PersistenceError::AcquireTimeout {
                    workload: DatabaseWorkload::Foreground,
                })
            }
        }
    }

    pub async fn close_and_drain(&self) {
        {
            let _lifecycle = self
                .inner
                .operation_lifecycle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let _ = self.inner.state.compare_exchange(
                RUNTIME_OPEN,
                RUNTIME_CLOSING,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
        }
        self.inner.admission_notify.notify_waiters();
        let drain = async {
            loop {
                let notified = self.inner.state_notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.inner.active.load(Ordering::Acquire) == 0 {
                    break;
                }
                notified.await;
            }
        };
        tokio::pin!(drain);
        let overdue = tokio::time::sleep(self.inner.operation_deadline);
        tokio::pin!(overdue);
        tokio::select! {
            () = &mut drain => {}
            () = &mut overdue => {
                crate::warn_event!(
                    "database.shutdown_drain_overdue",
                    backend = self.backend().to_string(),
                    active_operations = self.inner.active.load(Ordering::Acquire),
                    operation_deadline_ms = self.inner.operation_deadline.as_millis(),
                );
                drain.await;
            }
        }
        self.inner.state.store(RUNTIME_CLOSED, Ordering::Release);
        self.inner.admission_notify.notify_waiters();
    }

    async fn admit(&self, workload: DatabaseWorkload) -> Result<AdmissionPermit, PersistenceError> {
        if self.inner.state.load(Ordering::Acquire) != RUNTIME_OPEN {
            return Err(PersistenceError::ShuttingDown);
        }
        if let Some(permit) = self.try_start(workload) {
            return Ok(permit);
        }

        let waiter_permit = Arc::clone(&self.inner.waiters)
            .try_acquire_owned()
            .map_err(|_| {
                self.inner
                    .telemetry
                    .record_queue_rejected(self.backend(), workload);
                PersistenceError::QueueFull { workload }
            })?;
        self.inner.waiting.fetch_add(1, Ordering::AcqRel);
        if workload == DatabaseWorkload::Foreground {
            self.inner.waiting_foreground.fetch_add(1, Ordering::AcqRel);
        }
        let waiter = WaiterGuard {
            inner: Arc::clone(&self.inner),
            workload,
            _permit: waiter_permit,
        };

        let wait = async {
            loop {
                let notified = self.inner.admission_notify.notified();
                tokio::pin!(notified);
                notified.as_mut().enable();
                if self.inner.state.load(Ordering::Acquire) != RUNTIME_OPEN {
                    return Err(PersistenceError::ShuttingDown);
                }
                if let Some(permit) = self.try_start(workload) {
                    return Ok(permit);
                }
                notified.await;
            }
        };
        let result = tokio::time::timeout(self.inner.queue_wait_timeout, wait).await;
        drop(waiter);
        match result {
            Ok(result) => result,
            Err(_) => {
                self.inner
                    .telemetry
                    .record_acquire_timeout(self.backend(), workload);
                Err(PersistenceError::AcquireTimeout { workload })
            }
        }
    }

    fn try_start(&self, workload: DatabaseWorkload) -> Option<AdmissionPermit> {
        if self.inner.state.load(Ordering::Acquire) != RUNTIME_OPEN {
            return None;
        }
        let background = if workload == DatabaseWorkload::Background {
            if self.inner.waiting_foreground.load(Ordering::Acquire) > 0 {
                return None;
            }
            let permit = Arc::clone(&self.inner.background_capacity)
                .try_acquire_owned()
                .ok()?;
            if self.inner.waiting_foreground.load(Ordering::Acquire) > 0 {
                drop(permit);
                return None;
            }
            Some(permit)
        } else {
            None
        };
        let capacity = match Arc::clone(&self.inner.capacity).try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                drop(background);
                return None;
            }
        };
        if workload == DatabaseWorkload::Background
            && self.inner.waiting_foreground.load(Ordering::Acquire) > 0
        {
            drop(capacity);
            drop(background);
            return None;
        }
        Some(AdmissionPermit {
            inner: Arc::clone(&self.inner),
            _capacity: capacity,
            _background: background,
        })
    }
}

async fn build_postgres_pool(
    db_url: &str,
    pool_size: u32,
    connection_timeout: Duration,
    operation_deadline: Duration,
) -> Result<PgPool, DatabaseRuntimeInitError> {
    let statement_timeout_ms = operation_deadline.as_millis();
    let mut manager_config = ManagerConfig::default();
    manager_config.custom_setup = Box::new(move |url| {
        let url = url.to_string();
        async move {
            let mut connection = AsyncPgConnection::establish(&url).await?;
            connection
                .batch_execute(&format!(
                    "SET statement_timeout = {statement_timeout_ms}; \
                         SET idle_in_transaction_session_timeout = {statement_timeout_ms};"
                ))
                .await
                .map_err(|error| ConnectionError::BadConnection(error.to_string()))?;
            Ok(connection)
        }
        .boxed()
    });
    let manager = AsyncDieselConnectionManager::new_with_config(db_url, manager_config);
    Pool::builder()
        .max_size(pool_size)
        .min_idle(Some(1))
        .connection_timeout(connection_timeout)
        .retry_connection(false)
        .build(manager)
        .await
        .map_err(|_| DatabaseRuntimeInitError::Pool {
            backend: DatabaseBackendKind::Postgres,
        })
}

async fn build_sqlite_pool(
    db_url: &str,
    pool_size: u32,
    connection_timeout: Duration,
    busy_timeout: Duration,
) -> Result<SqlitePool, DatabaseRuntimeInitError> {
    let busy_timeout_ms = u64::try_from(busy_timeout.as_millis()).unwrap_or(u64::MAX);
    let mut manager_config = ManagerConfig::default();
    manager_config.custom_setup = Box::new(move |url| {
        let url = url.to_string();
        async move {
            let mut connection = SyncConnectionWrapper::<SqliteConnection>::establish(&url).await?;
            connection
                .batch_execute(&format!(
                    "PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; \
                         PRAGMA busy_timeout = {busy_timeout_ms};"
                ))
                .await
                .map_err(|error| ConnectionError::BadConnection(error.to_string()))?;
            Ok(connection)
        }
        .boxed()
    });
    let manager = AsyncDieselConnectionManager::new_with_config(db_url, manager_config);
    Pool::builder()
        .max_size(pool_size)
        .min_idle(Some(1))
        .connection_timeout(connection_timeout)
        .retry_connection(false)
        .build(manager)
        .await
        .map_err(|_| DatabaseRuntimeInitError::Pool {
            backend: DatabaseBackendKind::Sqlite,
        })
}

fn optional_timestamp(value: &AtomicI64) -> Option<i64> {
    match value.load(Ordering::Relaxed) {
        value if value >= 0 => Some(value),
        _ => None,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(i64::MAX)
}

macro_rules! db_execute {
    ($connection:ident as $conn:ident, no_models, $block:block) => {{
        match $connection {
            $crate::database::runtime::RuntimeConnection::Postgres($conn) => {
                #[allow(unused_imports)]
                use diesel::prelude::*;
                #[allow(unused_imports)]
                use $crate::database::_postgres_schema::*;
                $block
            }
            $crate::database::runtime::RuntimeConnection::Sqlite($conn) => {
                #[allow(unused_imports)]
                use diesel::prelude::*;
                #[allow(unused_imports)]
                use $crate::database::_sqlite_schema::*;
                $block
            }
        }
    }};
    ($connection:ident as $conn:ident, $block:block) => {{
        match $connection {
            $crate::database::runtime::RuntimeConnection::Postgres($conn) => {
                #[allow(unused_imports)]
                use _postgres_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;
                #[allow(unused_imports)]
                use $crate::database::_postgres_schema::*;
                $block
            }
            $crate::database::runtime::RuntimeConnection::Sqlite($conn) => {
                #[allow(unused_imports)]
                use _sqlite_model::*;
                #[allow(unused_imports)]
                use diesel::prelude::*;
                #[allow(unused_imports)]
                use $crate::database::_sqlite_schema::*;
                $block
            }
        }
    }};
}

pub(crate) use db_execute;

#[cfg(test)]
macro_rules! db_transaction {
    ($connection:ident as $conn:ident, $block:block) => {{
        match $connection {
            $crate::database::runtime::RuntimeConnection::Postgres($conn) => {
                use diesel_async::AsyncConnection;
                $conn.transaction(async move |$conn| $block).await
            }
            $crate::database::runtime::RuntimeConnection::Sqlite($conn) => {
                use diesel_async::AsyncConnection;
                $conn.transaction(async move |$conn| $block).await
            }
        }
    }};
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar, Mutex};

    use diesel::{QueryableByName, connection::SimpleConnection, sql_types::BigInt};
    use tokio::sync::Notify;

    use crate::{config::DatabaseIoConfig, database::test_support::TestDatabase};

    use super::*;

    #[derive(QueryableByName)]
    struct CountValue {
        #[diesel(sql_type = BigInt)]
        value: i64,
    }

    #[derive(Clone)]
    struct BlockingSqliteFunction {
        started: Arc<AtomicBool>,
        release: Arc<(Mutex<bool>, Condvar)>,
    }

    impl BlockingSqliteFunction {
        fn new() -> Self {
            Self {
                started: Arc::new(AtomicBool::new(false)),
                release: Arc::new((Mutex::new(false), Condvar::new())),
            }
        }

        async fn wait_until_started(&self) {
            tokio::time::timeout(Duration::from_secs(2), async {
                while !self.started.load(Ordering::Acquire) {
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("blocking SQLite function should start");
        }

        fn release(&self) {
            let (released, condition) = &*self.release;
            *released.lock().expect("release lock should not poison") = true;
            condition.notify_all();
        }
    }

    async fn register_blocking_sqlite_function(
        connection: &mut RuntimeConnection,
        function_name: &'static str,
        function: BlockingSqliteFunction,
        workload: DatabaseWorkload,
    ) -> Result<(), PersistenceError> {
        let RuntimeConnection::Sqlite(connection) = connection else {
            panic!("blocking SQLite function requires a SQLite runtime");
        };
        let started = Arc::clone(&function.started);
        let release = Arc::clone(&function.release);
        connection
            .spawn_blocking(move |connection| {
                connection.register_noarg_sql_function::<BigInt, i64, _>(
                    function_name,
                    false,
                    move || {
                        started.store(true, Ordering::Release);
                        let (released, condition) = &*release;
                        let mut released = released.lock().expect("release lock should not poison");
                        while !*released {
                            released = condition
                                .wait(released)
                                .expect("release lock should not poison while waiting");
                        }
                        1
                    },
                )
            })
            .await
            .map_err(|_| PersistenceError::execution(DatabaseBackendKind::Sqlite, workload))
    }

    fn runtime_config(max_waiters: u32) -> DatabaseIoConfig {
        DatabaseIoConfig {
            max_waiters,
            queue_wait_timeout_seconds: 1,
            operation_deadline_seconds: 3,
            sqlite_busy_timeout_seconds: 1,
        }
    }

    async fn wait_for_snapshot(
        runtime: &DatabaseRuntime,
        predicate: impl Fn(&DatabaseRuntimeSnapshot) -> bool,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let snapshot = runtime.snapshot();
                if predicate(&snapshot) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime snapshot condition should become true");
    }

    #[tokio::test]
    async fn sqlite_runtime_reports_file_failure_and_recovers_when_path_is_restored() {
        let temp_dir = tempfile::tempdir().expect("temporary directory should create");
        let non_directory = temp_dir.path().join("not-a-directory");
        std::fs::write(&non_directory, b"block sqlite parent")
            .expect("blocking file should create");
        let database_url = non_directory
            .join("runtime.sqlite")
            .to_string_lossy()
            .into_owned();

        let result = DatabaseRuntime::connect(&database_url, 1, runtime_config(1)).await;
        assert!(matches!(
            result,
            Err(DatabaseRuntimeInitError::Pool {
                backend: DatabaseBackendKind::Sqlite,
            })
        ));

        std::fs::remove_file(&non_directory).expect("blocking path file should remove");
        std::fs::create_dir(&non_directory).expect("database parent directory should create");
        let runtime = DatabaseRuntime::connect(&database_url, 1, runtime_config(1))
            .await
            .expect("SQLite runtime should recover after the path is restored");
        runtime
            .readiness_probe()
            .await
            .expect("recovered SQLite runtime should be ready");
        assert!(runtime.snapshot().last_success_at_ms.is_some());
    }

    fn hold_operation(
        runtime: Arc<DatabaseRuntime>,
        workload: DatabaseWorkload,
        started: Arc<Notify>,
        release: Arc<Notify>,
    ) -> tokio::task::JoinHandle<Result<(), PersistenceError>> {
        tokio::spawn(async move {
            runtime
                .run(workload, move |_connection| {
                    Box::pin(async move {
                        started.notify_one();
                        release.notified().await;
                        Ok(())
                    })
                })
                .await
        })
    }

    #[tokio::test]
    async fn sqlite_runtime_executes_queries_and_transactions_with_one_connection() {
        let database = TestDatabase::new_sqlite("runtime-query.sqlite", 2, runtime_config(4)).await;
        let runtime = database.runtime();
        runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_execute!(connection as conn, no_models, {
                        conn.batch_execute(
                            "CREATE TABLE runtime_probe (id BIGINT PRIMARY KEY, value BIGINT NOT NULL);",
                        )
                        .await
                        .map_err(|_| PersistenceError::execution(backend, DatabaseWorkload::Foreground))?;
                    });
                    Ok(())
                })
            })
            .await
            .expect("runtime setup query should succeed");

        runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_transaction!(connection as conn, {
                        conn.batch_execute("INSERT INTO runtime_probe (id, value) VALUES (1, 10);")
                            .await?;
                        Ok::<_, diesel::result::Error>(())
                    })
                    .map_err(|_| {
                        PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                    })?;
                    Ok(())
                })
            })
            .await
            .expect("runtime transaction should commit");

        let rollback = runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_transaction!(connection as conn, {
                        conn.batch_execute("INSERT INTO runtime_probe (id, value) VALUES (2, 20);")
                            .await?;
                        Err::<(), _>(diesel::result::Error::RollbackTransaction)
                    })
                    .map_err(|_| {
                        PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                    })?;
                    Ok(())
                })
            })
            .await;
        assert!(matches!(rollback, Err(PersistenceError::Execution { .. })));

        let count = runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_execute!(connection as conn, no_models, {
                        diesel_async::RunQueryDsl::get_result::<CountValue>(
                            diesel::sql_query("SELECT COUNT(*) AS value FROM runtime_probe"),
                            &mut **conn,
                        )
                        .await
                        .map(|row| row.value)
                        .map_err(|_| {
                            PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                        })
                    })
                })
            })
            .await
            .expect("runtime count should query");
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn admission_rejects_when_max_waiters_is_zero_and_readiness_never_queues() {
        let database =
            TestDatabase::new_sqlite("runtime-no-waiters.sqlite", 1, runtime_config(0)).await;
        let runtime = database.runtime();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let holder = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Foreground,
            Arc::clone(&started),
            Arc::clone(&release),
        );
        started.notified().await;

        let rejected = runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async { Ok(()) })
            })
            .await;
        assert_eq!(
            rejected,
            Err(PersistenceError::QueueFull {
                workload: DatabaseWorkload::Foreground,
            })
        );
        assert_eq!(
            runtime.readiness_probe().await,
            Err(PersistenceError::ReadinessUnavailable {
                backend: DatabaseBackendKind::Sqlite,
            })
        );
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.waiting, 0);
        assert_eq!(snapshot.queue_rejected, 1);

        release.notify_one();
        holder.await.expect("holder task should join").unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn admission_wait_timeout_updates_telemetry_without_starting_operation() {
        let database =
            TestDatabase::new_sqlite("runtime-wait-timeout.sqlite", 1, runtime_config(1)).await;
        let runtime = database.runtime();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let holder = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Foreground,
            Arc::clone(&started),
            Arc::clone(&release),
        );
        started.notified().await;

        let queued_runtime = Arc::clone(&runtime);
        let queued = tokio::spawn(async move {
            queued_runtime
                .run(DatabaseWorkload::Foreground, |_connection| {
                    Box::pin(async { Ok(()) })
                })
                .await
        });
        wait_for_snapshot(&runtime, |snapshot| snapshot.waiting == 1).await;
        let rejected = runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async { Ok(()) })
            })
            .await;
        assert_eq!(
            rejected,
            Err(PersistenceError::QueueFull {
                workload: DatabaseWorkload::Foreground,
            })
        );
        let saturated = runtime.snapshot();
        assert_eq!(saturated.active, 1);
        assert_eq!(saturated.waiting, 1);
        assert_eq!(saturated.waiting_foreground, 1);
        assert_eq!(saturated.queue_rejected, 1);

        tokio::time::advance(Duration::from_secs(1)).await;
        let timed_out = queued.await.expect("queued operation should join");
        assert_eq!(
            timed_out,
            Err(PersistenceError::AcquireTimeout {
                workload: DatabaseWorkload::Foreground,
            })
        );
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.active, 1);
        assert_eq!(snapshot.waiting, 0);
        assert_eq!(snapshot.waiting_foreground, 0);
        assert_eq!(snapshot.queue_rejected, 1);
        assert_eq!(snapshot.acquire_timeout, 1);
        assert!(snapshot.last_error_at_ms.is_some());
        assert!(!format!("{snapshot:?}").contains("runtime-wait-timeout.sqlite"));

        release.notify_one();
        holder.await.expect("holder task should join").unwrap();
    }

    #[tokio::test]
    async fn caller_cancellation_while_queued_releases_the_waiter_slot() {
        let database =
            TestDatabase::new_sqlite("runtime-queued-cancel.sqlite", 1, runtime_config(1)).await;
        let runtime = database.runtime();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let holder = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Foreground,
            Arc::clone(&started),
            Arc::clone(&release),
        );
        started.notified().await;

        let queued_runtime = Arc::clone(&runtime);
        let queued = tokio::spawn(async move {
            queued_runtime
                .run(DatabaseWorkload::Foreground, |_connection| {
                    Box::pin(async { Ok(()) })
                })
                .await
        });
        wait_for_snapshot(&runtime, |snapshot| snapshot.waiting == 1).await;
        queued.abort();
        let _ = queued.await;
        wait_for_snapshot(&runtime, |snapshot| snapshot.waiting == 0).await;
        assert_eq!(runtime.snapshot().active, 1);

        release.notify_one();
        holder.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn foreground_waiter_starts_before_background_waiter() {
        let database =
            TestDatabase::new_sqlite("runtime-foreground-first.sqlite", 1, runtime_config(4)).await;
        let runtime = database.runtime();
        let holder_started = Arc::new(Notify::new());
        let holder_release = Arc::new(Notify::new());
        let holder = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Foreground,
            Arc::clone(&holder_started),
            Arc::clone(&holder_release),
        );
        holder_started.notified().await;

        let background_started = Arc::new(Notify::new());
        let background_release = Arc::new(Notify::new());
        let background = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Background,
            Arc::clone(&background_started),
            Arc::clone(&background_release),
        );
        wait_for_snapshot(&runtime, |snapshot| snapshot.waiting == 1).await;

        let foreground_started = Arc::new(Notify::new());
        let foreground_release = Arc::new(Notify::new());
        let foreground = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Foreground,
            Arc::clone(&foreground_started),
            Arc::clone(&foreground_release),
        );
        wait_for_snapshot(&runtime, |snapshot| {
            snapshot.waiting == 2 && snapshot.waiting_foreground == 1
        })
        .await;

        holder_release.notify_one();
        foreground_started.notified().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(50), background_started.notified())
                .await
                .is_err(),
            "background operation must not pass a waiting foreground operation"
        );
        foreground_release.notify_one();
        background_started.notified().await;
        background_release.notify_one();

        holder.await.unwrap().unwrap();
        foreground.await.unwrap().unwrap();
        background.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn background_limit_reserves_capacity_for_foreground() {
        let database =
            TestDatabase::new_sqlite("runtime-background-limit.sqlite", 3, runtime_config(4)).await;
        let runtime = database.runtime();
        let release = Arc::new(Notify::new());
        let mut holders = Vec::new();
        for _ in 0..2 {
            let started = Arc::new(Notify::new());
            holders.push(hold_operation(
                Arc::clone(&runtime),
                DatabaseWorkload::Background,
                Arc::clone(&started),
                Arc::clone(&release),
            ));
            started.notified().await;
        }

        let third_started = Arc::new(Notify::new());
        let third = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Background,
            Arc::clone(&third_started),
            Arc::clone(&release),
        );
        wait_for_snapshot(&runtime, |snapshot| snapshot.waiting == 1).await;

        runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async { Ok(()) })
            })
            .await
            .expect("reserved foreground capacity should remain available");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), third_started.notified())
                .await
                .is_err()
        );

        release.notify_waiters();
        third_started.notified().await;
        release.notify_waiters();
        for holder in holders {
            holder.await.unwrap().unwrap();
        }
        third.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn nested_acquisition_is_rejected_without_entering_the_waiter_queue() {
        let database =
            TestDatabase::new_sqlite("runtime-nested.sqlite", 2, runtime_config(4)).await;
        let runtime = database.runtime();
        let nested_runtime = Arc::clone(&runtime);
        let nested = runtime
            .run(DatabaseWorkload::Foreground, move |_connection| {
                Box::pin(async move {
                    nested_runtime
                        .run(DatabaseWorkload::Foreground, |_connection| {
                            Box::pin(async { Ok(()) })
                        })
                        .await
                })
            })
            .await;
        assert_eq!(nested, Err(PersistenceError::NestedOperation));
        assert_eq!(runtime.snapshot().waiting, 0);
    }

    #[tokio::test]
    async fn caller_cancellation_keeps_started_operation_tracked_until_shutdown_drain() {
        let database =
            TestDatabase::new_sqlite("runtime-cancel.sqlite", 1, runtime_config(2)).await;
        let runtime = database.runtime();
        let started = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let caller = hold_operation(
            Arc::clone(&runtime),
            DatabaseWorkload::Foreground,
            Arc::clone(&started),
            Arc::clone(&release),
        );
        started.notified().await;
        caller.abort();
        let _ = caller.await;
        assert_eq!(runtime.snapshot().active, 1);

        let drain_runtime = Arc::clone(&runtime);
        let mut drain = tokio::spawn(async move { drain_runtime.close_and_drain().await });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut drain)
                .await
                .is_err(),
            "shutdown must wait for the started operation"
        );
        release.notify_one();
        drain.await.expect("drain task should join");
        assert_eq!(runtime.snapshot().active, 0);
        assert_eq!(
            runtime
                .run(DatabaseWorkload::Foreground, |_connection| {
                    Box::pin(async { Ok(()) })
                })
                .await,
            Err(PersistenceError::ShuttingDown)
        );
    }

    #[tokio::test]
    async fn operation_admitted_before_shutdown_cannot_register_after_drain() {
        let database =
            TestDatabase::new_sqlite("runtime-register-after-drain.sqlite", 1, runtime_config(2))
                .await;
        let runtime = database.runtime();
        let admission = runtime
            .admit(DatabaseWorkload::Foreground)
            .await
            .expect("operation should be admitted while runtime is open");

        runtime.close_and_drain().await;
        let registration = ActiveOperationGuard::try_new(Arc::clone(&runtime.inner), admission);
        assert!(registration.is_err());
        assert_eq!(runtime.snapshot().active, 0);
        assert!(runtime.snapshot().shutting_down);
    }

    #[tokio::test]
    async fn operation_deadline_is_observed_without_dropping_the_started_future() {
        let mut config = runtime_config(2);
        config.operation_deadline_seconds = 1;
        let database = TestDatabase::new_sqlite("runtime-deadline.sqlite", 1, config).await;
        let runtime = database.runtime();
        let completed = Arc::new(AtomicBool::new(false));
        let completed_in_operation = Arc::clone(&completed);
        let result = runtime
            .run(DatabaseWorkload::Foreground, move |_connection| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_millis(1_100)).await;
                    completed_in_operation.store(true, Ordering::Release);
                    Ok(())
                })
            })
            .await;
        assert_eq!(
            result,
            Err(PersistenceError::OperationDeadlineExceeded {
                backend: DatabaseBackendKind::Sqlite,
                workload: DatabaseWorkload::Foreground,
            })
        );
        assert!(completed.load(Ordering::Acquire));
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.active, 0);
        assert_eq!(snapshot.operation_deadline_exceeded, 1);
    }

    #[tokio::test]
    async fn blocking_sqlite_query_keeps_tokio_scheduler_runnable() {
        let database =
            TestDatabase::new_sqlite("runtime-non-blocking.sqlite", 1, runtime_config(2)).await;
        let runtime = database.runtime();
        let function = BlockingSqliteFunction::new();
        let operation_function = function.clone();
        let operation = tokio::spawn(async move {
            runtime
                .run(DatabaseWorkload::Foreground, move |connection| {
                    Box::pin(async move {
                        register_blocking_sqlite_function(
                            connection,
                            "r3_22_scheduler_probe",
                            operation_function,
                            DatabaseWorkload::Foreground,
                        )
                        .await?;
                        let RuntimeConnection::Sqlite(connection) = connection else {
                            panic!("scheduler probe requires a SQLite runtime");
                        };
                        diesel_async::RunQueryDsl::get_result::<CountValue>(
                            diesel::sql_query("SELECT r3_22_scheduler_probe() AS value"),
                            &mut **connection,
                        )
                        .await
                        .map(|row| row.value)
                        .map_err(|_| {
                            PersistenceError::execution(
                                DatabaseBackendKind::Sqlite,
                                DatabaseWorkload::Foreground,
                            )
                        })
                    })
                })
                .await
        });

        function.wait_until_started().await;
        let scheduler_ticks = Arc::new(AtomicUsize::new(0));
        let ticker_ticks = Arc::clone(&scheduler_ticks);
        let ticker = tokio::spawn(async move {
            for _ in 0..32 {
                tokio::task::yield_now().await;
                ticker_ticks.fetch_add(1, Ordering::AcqRel);
            }
        });
        tokio::time::timeout(Duration::from_millis(250), ticker)
            .await
            .expect("Tokio ticker must progress while SQLite is blocked")
            .expect("Tokio ticker should join");
        assert_eq!(scheduler_ticks.load(Ordering::Acquire), 32);
        assert!(
            !operation.is_finished(),
            "SQLite query must still be blocked while the Tokio ticker progresses"
        );

        function.release();
        assert_eq!(
            operation
                .await
                .expect("SQLite scheduler probe should join")
                .expect("SQLite scheduler probe should succeed"),
            1
        );
    }

    #[tokio::test]
    async fn overdue_sqlite_writes_reach_deterministic_commit_and_rollback() {
        let mut config = runtime_config(2);
        config.operation_deadline_seconds = 1;
        let database = TestDatabase::new_sqlite("runtime-write-deadline.sqlite", 1, config).await;
        database
            .execute_sqlite_batch(
                "CREATE TABLE runtime_deadline_write (id BIGINT PRIMARY KEY, value BIGINT NOT NULL);",
            )
            .await
            .expect("deadline write table should create");
        let runtime = database.runtime();
        tokio::time::pause();

        let commit_function = BlockingSqliteFunction::new();
        let operation_function = commit_function.clone();
        let commit_runtime = Arc::clone(&runtime);
        let commit = tokio::spawn(async move {
            commit_runtime
                .run(DatabaseWorkload::Foreground, move |connection| {
                    Box::pin(async move {
                        register_blocking_sqlite_function(
                            connection,
                            "r3_22_commit_probe",
                            operation_function,
                            DatabaseWorkload::Foreground,
                        )
                        .await?;
                        let backend = connection.backend();
                        db_transaction!(connection as conn, {
                            conn.batch_execute(
                                "INSERT INTO runtime_deadline_write (id, value) VALUES (1, 10);",
                            )
                            .await?;
                            diesel_async::RunQueryDsl::get_result::<CountValue>(
                                diesel::sql_query("SELECT r3_22_commit_probe() AS value"),
                                &mut **conn,
                            )
                            .await?;
                            Ok::<_, diesel::result::Error>(())
                        })
                        .map_err(|_| {
                            PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                        })
                    })
                })
                .await
        });
        commit_function.wait_until_started().await;
        tokio::time::advance(Duration::from_secs(2)).await;
        while runtime.snapshot().operation_deadline_exceeded < 1 {
            tokio::task::yield_now().await;
        }
        assert_eq!(runtime.snapshot().active, 1);
        commit_function.release();
        assert_eq!(
            commit.await.expect("overdue commit should join"),
            Err(PersistenceError::OperationDeadlineExceeded {
                backend: DatabaseBackendKind::Sqlite,
                workload: DatabaseWorkload::Foreground,
            })
        );

        let rollback_function = BlockingSqliteFunction::new();
        let operation_function = rollback_function.clone();
        let rollback_runtime = Arc::clone(&runtime);
        let rollback = tokio::spawn(async move {
            rollback_runtime
                .run(DatabaseWorkload::Foreground, move |connection| {
                    Box::pin(async move {
                        register_blocking_sqlite_function(
                            connection,
                            "r3_22_rollback_probe",
                            operation_function,
                            DatabaseWorkload::Foreground,
                        )
                        .await?;
                        let backend = connection.backend();
                        db_transaction!(connection as conn, {
                            conn.batch_execute(
                                "INSERT INTO runtime_deadline_write (id, value) VALUES (2, 20);",
                            )
                            .await?;
                            diesel_async::RunQueryDsl::get_result::<CountValue>(
                                diesel::sql_query("SELECT r3_22_rollback_probe() AS value"),
                                &mut **conn,
                            )
                            .await?;
                            Err::<(), _>(diesel::result::Error::RollbackTransaction)
                        })
                        .map_err(|_| {
                            PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                        })?;
                        Ok(())
                    })
                })
                .await
        });
        rollback_function.wait_until_started().await;
        tokio::time::advance(Duration::from_secs(2)).await;
        while runtime.snapshot().operation_deadline_exceeded < 2 {
            tokio::task::yield_now().await;
        }
        assert_eq!(runtime.snapshot().active, 1);
        rollback_function.release();
        assert_eq!(
            rollback.await.expect("overdue rollback should join"),
            Err(PersistenceError::OperationDeadlineExceeded {
                backend: DatabaseBackendKind::Sqlite,
                workload: DatabaseWorkload::Foreground,
            })
        );

        let count = runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_execute!(connection as conn, no_models, {
                        diesel_async::RunQueryDsl::get_result::<CountValue>(
                            diesel::sql_query(
                                "SELECT COUNT(*) AS value FROM runtime_deadline_write",
                            ),
                            &mut **conn,
                        )
                        .await
                        .map(|row| row.value)
                        .map_err(|_| {
                            PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                        })
                    })
                })
            })
            .await
            .expect("deadline write count should query");
        assert_eq!(
            count, 1,
            "overdue commit persists and overdue rollback does not"
        );
        assert_eq!(runtime.snapshot().active, 0);
    }

    #[tokio::test]
    async fn sqlite_busy_failure_recovers_after_the_write_lock_is_released() {
        let database =
            TestDatabase::new_sqlite("runtime-busy-recovery.sqlite", 1, runtime_config(2)).await;
        database
            .execute_sqlite_batch(
                "CREATE TABLE runtime_busy_write (id BIGINT PRIMARY KEY, value BIGINT NOT NULL);",
            )
            .await
            .expect("busy recovery table should create");
        let runtime = database.runtime();
        let mut lock_connection = database.startup_connection();
        lock_connection
            .sqlite_connection()
            .expect("write lock requires SQLite")
            .batch_execute(
                "BEGIN IMMEDIATE; INSERT INTO runtime_busy_write (id, value) VALUES (1, 10);",
            )
            .expect("fixture should hold a SQLite write lock");

        let failed = runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_execute!(connection as conn, no_models, {
                        conn.batch_execute(
                            "INSERT INTO runtime_busy_write (id, value) VALUES (2, 20);",
                        )
                        .await
                        .map_err(|_| {
                            PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                        })
                    })
                })
            })
            .await;
        assert!(matches!(failed, Err(PersistenceError::Execution { .. })));
        let degraded = runtime.snapshot();
        assert_eq!(degraded.execution_error, 1);
        assert_eq!(degraded.operation_deadline_exceeded, 0);

        lock_connection
            .sqlite_connection()
            .expect("write lock requires SQLite")
            .batch_execute("COMMIT;")
            .expect("fixture write lock should release");
        runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    let backend = connection.backend();
                    db_execute!(connection as conn, no_models, {
                        conn.batch_execute(
                            "INSERT INTO runtime_busy_write (id, value) VALUES (2, 20);",
                        )
                        .await
                        .map_err(|_| {
                            PersistenceError::execution(backend, DatabaseWorkload::Foreground)
                        })
                    })
                })
            })
            .await
            .expect("SQLite write should recover after the lock is released");
        let recovered = runtime.snapshot();
        assert!(recovered.last_recovery_at_ms.is_some());
        assert_eq!(recovered.active, 0);
    }

    #[tokio::test]
    async fn execution_error_then_success_records_recovery_without_sensitive_context() {
        let database =
            TestDatabase::new_sqlite("runtime-recovery.sqlite", 1, runtime_config(1)).await;
        let runtime = database.runtime();
        let failed = runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async {
                    Err::<(), _>(PersistenceError::Execution {
                        backend: DatabaseBackendKind::Sqlite,
                        workload: DatabaseWorkload::Foreground,
                    })
                })
            })
            .await;
        assert!(failed.is_err());
        let first_status_event = runtime
            .inner
            .telemetry
            .last_status_event_at_ms
            .load(Ordering::Relaxed);
        let second_failed = runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async {
                    Err::<(), _>(PersistenceError::Execution {
                        backend: DatabaseBackendKind::Sqlite,
                        workload: DatabaseWorkload::Foreground,
                    })
                })
            })
            .await;
        assert!(second_failed.is_err());
        assert_eq!(
            runtime
                .inner
                .telemetry
                .last_status_event_at_ms
                .load(Ordering::Relaxed),
            first_status_event,
            "degraded status events must be rate limited"
        );
        runtime
            .run(DatabaseWorkload::Foreground, |_connection| {
                Box::pin(async { Ok(()) })
            })
            .await
            .expect("recovery operation should succeed");
        let snapshot = runtime.snapshot();
        assert_eq!(snapshot.execution_error, 2);
        assert!(snapshot.last_error_at_ms.is_some());
        assert!(snapshot.last_success_at_ms.is_some());
        assert!(snapshot.last_recovery_at_ms.is_some());
        let rendered = format!("{runtime:?}");
        for forbidden in ["runtime-recovery.sqlite", "SELECT", "bind", "secret"] {
            assert!(!rendered.contains(forbidden));
        }
    }

    #[tokio::test]
    async fn test_databases_are_explicit_and_isolated_without_task_local_selection() {
        let (left, right) = tokio::join!(
            TestDatabase::new_sqlite("left.sqlite", 1, runtime_config(1)),
            TestDatabase::new_sqlite("right.sqlite", 1, runtime_config(1)),
        );
        assert!(!Arc::ptr_eq(&left.runtime(), &right.runtime()));
        left.runtime().readiness_probe().await.unwrap();
        right.runtime().readiness_probe().await.unwrap();
    }

    #[tokio::test]
    async fn sqlite_runtime_connections_apply_wal_foreign_keys_and_busy_timeout() {
        let database =
            TestDatabase::new_sqlite("runtime-session.sqlite", 1, runtime_config(1)).await;
        let runtime = database.runtime();
        let values = runtime
            .run(DatabaseWorkload::Foreground, |connection| {
                Box::pin(async move {
                    use diesel_async::RunQueryDsl as _;

                    let RuntimeConnection::Sqlite(connection) = connection else {
                        panic!("test runtime must use sqlite");
                    };
                    let journal = diesel::sql_query(
                        "SELECT CASE lower(journal_mode) WHEN 'wal' THEN 1 ELSE 0 END AS value FROM pragma_journal_mode",
                    )
                    .get_result::<CountValue>(&mut **connection)
                    .await
                    .map_err(|_| PersistenceError::execution(DatabaseBackendKind::Sqlite, DatabaseWorkload::Foreground))?
                    .value;
                    let foreign_keys = diesel::sql_query(
                        "SELECT foreign_keys AS value FROM pragma_foreign_keys",
                    )
                    .get_result::<CountValue>(&mut **connection)
                    .await
                    .map_err(|_| PersistenceError::execution(DatabaseBackendKind::Sqlite, DatabaseWorkload::Foreground))?
                    .value;
                    let busy_timeout = diesel::sql_query(
                        "SELECT timeout AS value FROM pragma_busy_timeout",
                    )
                    .get_result::<CountValue>(&mut **connection)
                    .await
                    .map_err(|_| PersistenceError::execution(DatabaseBackendKind::Sqlite, DatabaseWorkload::Foreground))?
                    .value;
                    Ok((journal, foreign_keys, busy_timeout))
                })
            })
            .await
            .expect("sqlite runtime pragmas should query");
        assert_eq!(values, (1, 1, 1_000));
    }
}
