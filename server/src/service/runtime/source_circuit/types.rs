use async_trait::async_trait;
use std::fmt;
use std::time::Duration;
use uuid::Uuid;

use crate::config::ProviderGovernanceConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceHealthStatus {
    Healthy,
    Open,
    HalfOpen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceHealthSnapshot {
    pub status: SourceHealthStatus,
    pub consecutive_failures: u32,
    pub half_open_probe_in_flight: bool,
    pub opened_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_recovered_at: Option<i64>,
    pub last_error: Option<String>,
}

impl Default for SourceHealthSnapshot {
    fn default() -> Self {
        Self::synthetic_healthy()
    }
}

impl SourceHealthSnapshot {
    pub fn synthetic_healthy() -> Self {
        Self {
            status: SourceHealthStatus::Healthy,
            consecutive_failures: 0,
            half_open_probe_in_flight: false,
            opened_at: None,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCircuitProbePermit {
    source_id: i64,
    decision_id: String,
    lease_id: String,
    issued_at_ms: i64,
    probe_expires_at_ms: i64,
}

impl SourceCircuitProbePermit {
    pub(crate) fn new(
        source_id: i64,
        decision_id: String,
        lease_id: String,
        issued_at_ms: i64,
        probe_expires_at_ms: i64,
    ) -> Self {
        Self {
            source_id,
            decision_id,
            lease_id,
            issued_at_ms,
            probe_expires_at_ms,
        }
    }

    pub fn source_id(&self) -> i64 {
        self.source_id
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }

    pub fn lease_id(&self) -> &str {
        &self.lease_id
    }

    pub fn issued_at_ms(&self) -> i64 {
        self.issued_at_ms
    }

    pub fn probe_expires_at_ms(&self) -> i64 {
        self.probe_expires_at_ms
    }

    pub fn expires_at_ms(&self) -> i64 {
        self.probe_expires_at_ms
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceCircuitRejection {
    OpenCooldown,
    HalfOpenProbeInFlight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceCircuitDecision {
    pub snapshot: SourceHealthSnapshot,
    pub allowed: bool,
    pub rejection: Option<SourceCircuitRejection>,
    pub retry_after: Option<Duration>,
    pub probe_permit: Option<SourceCircuitProbePermit>,
}

impl SourceCircuitDecision {
    pub(crate) fn allowed(
        snapshot: SourceHealthSnapshot,
        probe_permit: Option<SourceCircuitProbePermit>,
    ) -> Self {
        Self {
            snapshot,
            allowed: true,
            rejection: None,
            retry_after: None,
            probe_permit,
        }
    }

    pub(crate) fn rejected(
        snapshot: SourceHealthSnapshot,
        rejection: SourceCircuitRejection,
        retry_after: Option<Duration>,
    ) -> Self {
        Self {
            snapshot,
            allowed: false,
            rejection: Some(rejection),
            retry_after,
            probe_permit: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceCircuitError {
    Backend(String),
}

impl fmt::Display for SourceCircuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceCircuitError::Backend(message) => {
                write!(f, "source circuit backend error: {message}")
            }
        }
    }
}

impl std::error::Error for SourceCircuitError {}

#[derive(Clone, Debug)]
pub(crate) struct SourceHealthState {
    pub(crate) status: SourceHealthStatus,
    pub(crate) consecutive_failures: u32,
    pub(crate) opened_at: Option<i64>,
    pub(crate) half_open_probe: Option<SourceCircuitProbePermit>,
    pub(crate) last_failure_at: Option<i64>,
    pub(crate) last_recovered_at: Option<i64>,
    pub(crate) last_error: Option<String>,
}

impl Default for SourceHealthState {
    fn default() -> Self {
        Self {
            status: SourceHealthStatus::Healthy,
            consecutive_failures: 0,
            opened_at: None,
            half_open_probe: None,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
        }
    }
}

impl SourceHealthState {
    pub(crate) fn snapshot(&self) -> SourceHealthSnapshot {
        SourceHealthSnapshot {
            status: self.status,
            consecutive_failures: self.consecutive_failures,
            half_open_probe_in_flight: self.half_open_probe.is_some(),
            opened_at: self.opened_at,
            last_failure_at: self.last_failure_at,
            last_recovered_at: self.last_recovered_at,
            last_error: self.last_error.clone(),
        }
    }

    pub(crate) fn prune_expired_probe(&mut self, now_ms: i64) {
        if self
            .half_open_probe
            .as_ref()
            .is_some_and(|permit| permit.probe_expires_at_ms <= now_ms)
        {
            self.half_open_probe = None;
        }
    }

    fn retry_after_for_open(&self, config: &ProviderGovernanceConfig, now_ms: i64) -> Duration {
        let cooldown_ms = i64::try_from(config.open_cooldown().as_millis()).unwrap_or(i64::MAX);
        let opened_at = self.opened_at.unwrap_or(now_ms);
        let elapsed_ms = now_ms.saturating_sub(opened_at);
        let remaining_ms = cooldown_ms.saturating_sub(elapsed_ms).max(0);
        Duration::from_millis(u64::try_from(remaining_ms).unwrap_or(u64::MAX))
    }

    fn create_probe_permit(
        &mut self,
        source_id: i64,
        now_ms: i64,
        probe_lease_ttl: Duration,
    ) -> SourceCircuitProbePermit {
        self.status = SourceHealthStatus::HalfOpen;
        let ttl_ms = i64::try_from(probe_lease_ttl.as_millis()).unwrap_or(i64::MAX);
        let expires_at_ms = now_ms.saturating_add(ttl_ms);
        let permit = SourceCircuitProbePermit::new(
            source_id,
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            now_ms,
            expires_at_ms,
        );
        self.half_open_probe = Some(permit.clone());
        permit
    }

    fn probe_permit_matches(&self, permit: Option<&SourceCircuitProbePermit>) -> bool {
        let Some(permit) = permit else {
            return false;
        };
        self.half_open_probe.as_ref().is_some_and(|active| {
            active.source_id == permit.source_id && active.lease_id == permit.lease_id
        })
    }

    pub(crate) fn allow_request(
        &mut self,
        source_id: i64,
        config: &ProviderGovernanceConfig,
        now_ms: i64,
        probe_lease_ttl: Duration,
    ) -> SourceCircuitDecision {
        if !config.is_enabled() {
            return SourceCircuitDecision::allowed(SourceHealthSnapshot::synthetic_healthy(), None);
        }

        self.prune_expired_probe(now_ms);

        match self.status {
            SourceHealthStatus::Healthy => SourceCircuitDecision::allowed(self.snapshot(), None),
            SourceHealthStatus::Open => {
                let retry_after = self.retry_after_for_open(config, now_ms);
                if !retry_after.is_zero() {
                    return SourceCircuitDecision::rejected(
                        self.snapshot(),
                        SourceCircuitRejection::OpenCooldown,
                        Some(retry_after),
                    );
                }

                let permit = self.create_probe_permit(source_id, now_ms, probe_lease_ttl);
                SourceCircuitDecision::allowed(self.snapshot(), Some(permit))
            }
            SourceHealthStatus::HalfOpen => {
                if self.half_open_probe.is_some() {
                    return SourceCircuitDecision::rejected(
                        self.snapshot(),
                        SourceCircuitRejection::HalfOpenProbeInFlight,
                        None,
                    );
                }

                let permit = self.create_probe_permit(source_id, now_ms, probe_lease_ttl);
                SourceCircuitDecision::allowed(self.snapshot(), Some(permit))
            }
        }
    }

    pub(crate) fn record_success(
        &mut self,
        config: &ProviderGovernanceConfig,
        now_ms: i64,
        permit: Option<&SourceCircuitProbePermit>,
    ) {
        if !config.is_enabled() {
            return;
        }

        self.prune_expired_probe(now_ms);
        let matching_probe =
            self.status == SourceHealthStatus::HalfOpen && self.probe_permit_matches(permit);
        if permit.is_some() && !matching_probe {
            return;
        }
        if matches!(
            self.status,
            SourceHealthStatus::Open | SourceHealthStatus::HalfOpen
        ) && !matching_probe
        {
            return;
        }

        let was_unhealthy = self.status != SourceHealthStatus::Healthy;
        self.status = SourceHealthStatus::Healthy;
        self.consecutive_failures = 0;
        self.opened_at = None;
        self.half_open_probe = None;
        if was_unhealthy {
            self.last_recovered_at = Some(now_ms);
        }
        self.last_error = None;
    }

    pub(crate) fn record_failure(
        &mut self,
        config: &ProviderGovernanceConfig,
        now_ms: i64,
        error_message: String,
        permit: Option<&SourceCircuitProbePermit>,
    ) {
        if !config.is_enabled() {
            return;
        }

        self.prune_expired_probe(now_ms);

        let half_open_probe_failed =
            self.status == SourceHealthStatus::HalfOpen && self.probe_permit_matches(permit);
        if permit.is_some() && !half_open_probe_failed {
            return;
        }
        if self.status == SourceHealthStatus::HalfOpen && !half_open_probe_failed {
            return;
        }

        self.last_failure_at = Some(now_ms);
        self.last_error = Some(error_message);

        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if half_open_probe_failed
            || self.consecutive_failures >= config.consecutive_failure_threshold
        {
            self.status = SourceHealthStatus::Open;
            self.opened_at = Some(now_ms);
            self.half_open_probe = None;
        }
    }

    pub(crate) fn release_probe(
        &mut self,
        config: &ProviderGovernanceConfig,
        now_ms: i64,
        permit: Option<&SourceCircuitProbePermit>,
    ) {
        if !config.is_enabled() {
            return;
        }

        self.prune_expired_probe(now_ms);
        if self.status == SourceHealthStatus::HalfOpen && self.probe_permit_matches(permit) {
            self.half_open_probe = None;
        }
    }
}

#[async_trait]
pub trait SourceCircuitStore: Send + Sync {
    async fn allow_request(
        &self,
        source_id: i64,
        config: &ProviderGovernanceConfig,
    ) -> Result<SourceCircuitDecision, SourceCircuitError>;

    async fn record_success(
        &self,
        source_id: i64,
        config: &ProviderGovernanceConfig,
        permit: Option<&SourceCircuitProbePermit>,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError>;

    async fn record_failure(
        &self,
        source_id: i64,
        config: &ProviderGovernanceConfig,
        error_message: String,
        permit: Option<&SourceCircuitProbePermit>,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError>;

    async fn release_probe(
        &self,
        source_id: i64,
        config: &ProviderGovernanceConfig,
        permit: Option<&SourceCircuitProbePermit>,
    ) -> Result<SourceHealthSnapshot, SourceCircuitError>;

    async fn clear(&self, source_id: i64) -> Result<(), SourceCircuitError>;

    async fn snapshot(&self, source_id: i64) -> Result<SourceHealthSnapshot, SourceCircuitError>;
}
