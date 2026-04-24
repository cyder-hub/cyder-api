use async_trait::async_trait;
use std::time::{Duration, Instant};

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

#[derive(Clone, Debug)]
pub(crate) struct ProviderHealthState {
    pub(crate) status: ProviderHealthStatus,
    pub(crate) consecutive_failures: u32,
    pub(crate) opened_at_instant: Option<Instant>,
    pub(crate) opened_at: Option<i64>,
    pub(crate) half_open_probe_in_flight: bool,
    pub(crate) last_failure_at: Option<i64>,
    pub(crate) last_recovered_at: Option<i64>,
    pub(crate) last_error: Option<String>,
}

impl Default for ProviderHealthState {
    fn default() -> Self {
        Self {
            status: ProviderHealthStatus::Healthy,
            consecutive_failures: 0,
            opened_at_instant: None,
            opened_at: None,
            half_open_probe_in_flight: false,
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
            half_open_probe_in_flight: self.half_open_probe_in_flight,
            opened_at: self.opened_at,
            last_failure_at: self.last_failure_at,
            last_recovered_at: self.last_recovered_at,
            last_error: self.last_error.clone(),
        }
    }

    pub(crate) fn allow_request(
        &mut self,
        config: &ProviderGovernanceConfig,
        now: Instant,
    ) -> Result<(), Option<Duration>> {
        if !config.is_enabled() {
            return Ok(());
        }

        match self.status {
            ProviderHealthStatus::Healthy => Ok(()),
            ProviderHealthStatus::Open => {
                let Some(opened_at) = self.opened_at_instant else {
                    return Err(Some(config.open_cooldown()));
                };
                let elapsed = now.saturating_duration_since(opened_at);
                if elapsed < config.open_cooldown() {
                    return Err(Some(config.open_cooldown() - elapsed));
                }

                self.status = ProviderHealthStatus::HalfOpen;
                self.half_open_probe_in_flight = true;
                Ok(())
            }
            ProviderHealthStatus::HalfOpen => {
                if self.half_open_probe_in_flight {
                    Err(None)
                } else {
                    self.half_open_probe_in_flight = true;
                    Ok(())
                }
            }
        }
    }

    pub(crate) fn record_success(&mut self, now_ms: i64) {
        let was_unhealthy = self.status != ProviderHealthStatus::Healthy;
        self.status = ProviderHealthStatus::Healthy;
        self.consecutive_failures = 0;
        self.opened_at_instant = None;
        self.opened_at = None;
        self.half_open_probe_in_flight = false;
        if was_unhealthy {
            self.last_recovered_at = Some(now_ms);
        }
        self.last_error = None;
    }

    pub(crate) fn record_failure(
        &mut self,
        config: &ProviderGovernanceConfig,
        now: Instant,
        now_ms: i64,
        error_message: String,
    ) {
        self.last_failure_at = Some(now_ms);
        self.last_error = Some(error_message);
        self.half_open_probe_in_flight = false;

        if !config.is_enabled() {
            self.status = ProviderHealthStatus::Healthy;
            self.consecutive_failures = 0;
            self.opened_at_instant = None;
            self.opened_at = None;
            return;
        }

        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        if self.status == ProviderHealthStatus::HalfOpen
            || self.consecutive_failures >= config.consecutive_failure_threshold
        {
            self.status = ProviderHealthStatus::Open;
            self.opened_at_instant = Some(now);
            self.opened_at = Some(now_ms);
        }
    }
}

#[async_trait]
pub trait ProviderCircuitStore: Send + Sync {
    async fn allow_request(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
    ) -> Result<ProviderHealthSnapshot, Option<Duration>>;

    async fn record_success(&self, provider_id: i64) -> ProviderHealthSnapshot;

    async fn record_failure(
        &self,
        provider_id: i64,
        config: &ProviderGovernanceConfig,
        error_message: String,
    ) -> ProviderHealthSnapshot;

    async fn snapshot(&self, provider_id: i64) -> ProviderHealthSnapshot;
}
