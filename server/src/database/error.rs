use thiserror::Error;

use super::runtime::{DatabaseBackendKind, DatabaseWorkload};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DatabaseRuntimeInitError {
    #[error("database runtime configuration is invalid")]
    InvalidConfig,
    #[error("failed to initialize the {backend} database runtime")]
    Pool { backend: DatabaseBackendKind },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PersistenceError {
    #[error("database runtime is shutting down")]
    ShuttingDown,
    #[error("nested database runtime acquisition is forbidden")]
    NestedOperation,
    #[error("database admission queue is full for {workload}")]
    QueueFull { workload: DatabaseWorkload },
    #[error("database acquisition timed out for {workload}")]
    AcquireTimeout { workload: DatabaseWorkload },
    #[error("database connection acquisition failed for {backend}")]
    PoolAcquire { backend: DatabaseBackendKind },
    #[error("database operation exceeded its deadline for {backend}/{workload}")]
    OperationDeadlineExceeded {
        backend: DatabaseBackendKind,
        workload: DatabaseWorkload,
    },
    #[error("database operation failed for {backend}/{workload}")]
    Execution {
        backend: DatabaseBackendKind,
        workload: DatabaseWorkload,
    },
    #[error("database readiness probe cannot start immediately for {backend}")]
    ReadinessUnavailable { backend: DatabaseBackendKind },
    #[error("database operation result channel closed")]
    ResultChannelClosed,
}

impl PersistenceError {
    #[cfg(test)]
    pub(crate) const fn execution(
        backend: DatabaseBackendKind,
        workload: DatabaseWorkload,
    ) -> Self {
        Self::Execution { backend, workload }
    }
}
