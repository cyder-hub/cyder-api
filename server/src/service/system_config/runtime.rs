use serde::{Deserialize, Serialize};

use crate::config::{
    DiagnosticsConfig, FinalConfig, ProviderGovernanceConfig, ProxyRequestConfig,
    RoutingResilienceConfig,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeConfigSnapshot {
    pub version: u64,
    pub log_level: String,
    pub timezone: Option<String>,
    pub max_body_size: usize,
    pub proxy: Option<String>,
    pub proxy_request: ProxyRequestConfig,
    pub provider_governance: ProviderGovernanceConfig,
    pub routing_resilience: RoutingResilienceConfig,
    pub diagnostics: DiagnosticsConfig,
}

impl RuntimeConfigSnapshot {
    pub fn from_config(version: u64, config: &FinalConfig) -> Self {
        Self {
            version,
            log_level: config.log_level.clone(),
            timezone: config.timezone.clone(),
            max_body_size: config.max_body_size,
            proxy: config.proxy.clone(),
            proxy_request: config.proxy_request.clone(),
            provider_governance: config.provider_governance.clone(),
            routing_resilience: config.routing_resilience.clone(),
            diagnostics: config.diagnostics.clone(),
        }
    }
}
