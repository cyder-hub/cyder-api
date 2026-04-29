use async_trait::async_trait;
use std::fmt;
use std::time::Duration;
use uuid::Uuid;

use crate::config::ProviderGovernanceConfig;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderHealthStatus {
    Healthy,
    Open,
    HalfOpen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderHealthSnapshot {
    pub status: ProviderHealthStatus,
    pub consecutive_failures: u32,
    pub half_open_probe_in_flight: bool,
    pub opened_at: Option<i64>,
    pub last_failure_at: Option<i64>,
    pub last_recovered_at: Option<i64>,
    pub last_error: Option<String>,
}

impl Default for ProviderHealthSnapshot {
    fn default() -> Self {
        Self::synthetic_healthy()
    }
}

impl ProviderHealthSnapshot {
    pub fn synthetic_healthy() -> Self {
        Self {
            status: ProviderHealthStatus::Healthy,
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
pub struct ProviderCircuitProbePermit {
    provider_id: i64,
    decision_id: String,
    lease_id: String,
    issued_at_ms: i64,
    probe_expires_at_ms: i64,
}

impl ProviderCircuitProbePermit {
    pub(crate) fn new(
        provider_id: i64,
        decision_id: String,
        lease_id: String,
        issued_at_ms: i64,
        probe_expires_at_ms: i64,
    ) -> Self {
        Self {
            provider_id,
            decision_id,
            lease_id,
            issued_at_ms,
            probe_expires_at_ms,
        }
    }

    pub fn provider_id(&self) -> i64 {
        self.provider_id
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
pub enum ProviderCircuitRejection {
    OpenCooldown,
    HalfOpenProbeInFlight,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderCircuitDecision {
    pub snapshot: ProviderHealthSnapshot,
    pub allowed: bool,
    pub rejection: Option<ProviderCircuitRejection>,
    pub retry_after: Option<Duration>,
    pub probe_permit: Option<ProviderCircuitProbePermit>,
}

impl ProviderCircuitDecision {
    pub(crate) fn allowed(
        snapshot: ProviderHealthSnapshot,
        probe_permit: Option<ProviderCircuitProbePermit>,
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
        snapshot: ProviderHealthSnapshot,
        rejection: ProviderCircuitRejection,
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
pub enum ProviderCircuitError {
    Backend(String),
}

impl fmt::Display for ProviderCircuitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProviderCircuitError::Backend(message) => {
                write!(f, "provider circuit backend error: {message}")
            }
        }
    }
}

impl std::error::Error for ProviderCircuitError {}

#[derive(Clone, Debug)]
pub(crate) struct ProviderHealthState {
    pub(crate) status: ProviderHealthStatus,
    pub(crate) consecutive_failures: u32,
    pub(crate) opened_at: Option<i64>,
    pub(crate) half_open_probe: Option<ProviderCircuitProbePermit>,
    pub(crate) last_failure_at: Option<i64>,
    pub(crate) last_recovered_at: Option<i64>,
    pub(crate) last_error: Option<String>,
}

impl Default for ProviderHealthState {
    fn default() -> Self {
        Self {
            status: ProviderHealthStatus::Healthy,
            consecutive_failures: 0,
            opened_at: None,
            half_open_probe: None,
            last_failure_at: None,
            last_recovered_at: None,
            last_error: None,
        }
    }
}

impl ProviderHealthState {
    pub(crate) fn snapshot(&self) -> ProviderHealthSnapshot {
        ProviderHealthSnapshot {
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
        provider_id: i64,
        now_ms: i64,
        probe_lease_ttl: Duration,
    ) -> ProviderCircuitProbePermit {
        self.status = ProviderHealthStatus::HalfOpen;
        let ttl_ms = i64::try_from(probe_lease_ttl.as_millis()).unwrap_or(i64::MAX);
        let expires_at_ms = now_ms.saturating_add(ttl_ms);
        let permit = ProviderCircuitProbePermit::new(
            provider_id,
            Uuid::new_v4().to_string(),
            Uuid::new_v4().to_string(),
            now_ms,
            expires_at_ms,
        );
        self.half_open_probe = Some(permit.clone());
        permit
    }

    fn probe_permit_matches(&self, permit: Option<&ProviderCircuitProbePermit>) -> bool {
        let Some(permit) = permit else {
            return false;
        };
        self.half_open_probe.as_ref().is_some_and(|active| {
            active.provider_id == permit.provider_id && active.lease_id == permit.lease_id
        })
    }

    pub(crate) fn allow_request(
        &mut self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        now_ms: i64,
        probe_lease_ttl: Duration,
    ) -> ProviderCircuitDecision {
        if !config.is_enabled() {
            return ProviderCircuitDecision::allowed(
                ProviderHealthSnapshot::synthetic_healthy(),
                None,
            );
        }

        self.prune_expired_probe(now_ms);

        match self.status {
            ProviderHealthStatus::Healthy => {
                ProviderCircuitDecision::allowed(self.snapshot(), None)
            }
            ProviderHealthStatus::Open => {
                let retry_after = self.retry_after_for_open(config, now_ms);
                if !retry_after.is_zero() {
                    return ProviderCircuitDecision::rejected(
                        self.snapshot(),
                        ProviderCircuitRejection::OpenCooldown,
                        Some(retry_after),
                    );
                }

                let permit = self.create_probe_permit(provider_id, now_ms, probe_lease_ttl);
                ProviderCircuitDecision::allowed(self.snapshot(), Some(permit))
            }
            ProviderHealthStatus::HalfOpen => {
                if self.half_open_probe.is_some() {
                    return ProviderCircuitDecision::rejected(
                        self.snapshot(),
                        ProviderCircuitRejection::HalfOpenProbeInFlight,
                        None,
                    );
                }

                let permit = self.create_probe_permit(provider_id, now_ms, probe_lease_ttl);
                ProviderCircuitDecision::allowed(self.snapshot(), Some(permit))
            }
        }
    }

    pub(crate) fn record_success(
        &mut self,
        config: &ProviderGovernanceConfig,
        now_ms: i64,
        permit: Option<&ProviderCircuitProbePermit>,
    ) {
        if !config.is_enabled() {
            return;
        }

        self.prune_expired_probe(now_ms);
        if self.status == ProviderHealthStatus::Open {
            return;
        }
        if self.status == ProviderHealthStatus::HalfOpen && !self.probe_permit_matches(permit) {
            return;
        }

        let was_unhealthy = self.status != ProviderHealthStatus::Healthy;
        self.status = ProviderHealthStatus::Healthy;
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
        permit: Option<&ProviderCircuitProbePermit>,
    ) {
        if !config.is_enabled() {
            return;
        }

        self.prune_expired_probe(now_ms);

        let half_open_probe_failed =
            self.status == ProviderHealthStatus::HalfOpen && self.probe_permit_matches(permit);
        if self.status == ProviderHealthStatus::HalfOpen && !half_open_probe_failed {
            return;
        }

        self.last_failure_at = Some(now_ms);
        self.last_error = Some(error_message);

        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if half_open_probe_failed
            || self.consecutive_failures >= config.consecutive_failure_threshold
        {
            self.status = ProviderHealthStatus::Open;
            self.opened_at = Some(now_ms);
            self.half_open_probe = None;
        }
    }
}

#[async_trait]
pub trait ProviderCircuitStore: Send + Sync {
    async fn allow_request(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
    ) -> Result<ProviderCircuitDecision, ProviderCircuitError>;

    async fn record_success(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError>;

    async fn record_failure(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        error_message: String,
        permit: Option<&ProviderCircuitProbePermit>,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError>;

    async fn snapshot(
        &self,
        provider_id: i64,
    ) -> Result<ProviderHealthSnapshot, ProviderCircuitError>;
}
