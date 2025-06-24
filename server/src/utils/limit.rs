use cyder_tools::log::{debug, error}; // Removed info, not used in this file
use once_cell::sync::Lazy;

// Removed: use crate::controller::proxy::RequestInfo;
use crate::database::access_control::ApiAccessControlPolicy;

pub trait Limiter: Sync + Send {
    fn check_limit_strategy(
        &self,
        policy: &ApiAccessControlPolicy,
        provider_id: i64,
        model_id: i64,
    ) -> Result<(), String>;
}

pub struct MemoryLimiter {}

impl MemoryLimiter {
    pub fn new() -> Self {
        MemoryLimiter {}
    }
}

impl MemoryLimiter {
    fn inner_check(
        &self,
        policy: &ApiAccessControlPolicy,
        provider_id: i64, // Changed parameter
        model_id: i64,    // Changed parameter
    ) -> Result<(), String> {
        // Use parameters directly
        let request_provider_id = provider_id;
        let request_model_id = model_id;

        // Rules in ApiAccessControlPolicy are assumed to be pre-sorted by priority.
        for rule in &policy.rules {
            if !rule.is_enabled {
                continue;
            }

            let mut rule_matches = false;
            match rule.scope.as_str() {
                "MODEL" => {
                    if let Some(rule_model_id) = rule.model_id {
                        if rule_model_id == request_model_id {
                            rule_matches = true;
                        }
                    } else {
                        debug!(
                            "Model-scoped rule ID {} has no model_id, will not match specific model.",
                            rule.id
                        );
                    }
                }
                "PROVIDER" => {
                    if let Some(rule_provider_id) = rule.provider_id {
                        if rule_provider_id == request_provider_id {
                            rule_matches = true;
                        }
                    } else {
                        debug!(
                            "Provider-scoped rule ID {} has no provider_id, will not match specific provider.",
                            rule.id
                        );
                    }
                }
                unknown_scope => {
                    debug!("Unknown rule scope: '{}' for rule ID {}. Rule will not match.", unknown_scope, rule.id);
                }
            }

            if rule_matches {
                match rule.rule_type.as_str() {
                    "ALLOW" => {
                        debug!(
                            "Request allowed by rule ID {} (Priority {}, Scope: {}) for policy '{}'",
                            rule.id, rule.priority, rule.scope, policy.name
                        );
                        return Ok(());
                    }
                    "DENY" => {
                        debug!(
                            "Request denied by rule ID {} (Priority {}, Scope: {}) for policy '{}'",
                            rule.id, rule.priority, rule.scope, policy.name
                        );
                        return Err(format!(
                            "request denied by rule (ID: {}, Policy: '{}', Scope: {}, Type: {})",
                            rule.id, policy.name, rule.scope, rule.rule_type
                        ));
                    }
                    unknown_type => {
                        error!(
                            "Misconfigured rule: unknown rule type '{}' for rule ID {} in policy '{}'. Denying request.",
                            unknown_type, rule.id, policy.name
                        );
                        return Err(format!(
                            "misconfigured rule: unknown rule type '{}' (ID: {}, Policy: '{}')",
                            unknown_type, rule.id, policy.name
                        ));
                    }
                }
            }
        }

        // If no rules matched, apply default_action
        match policy.default_action.as_str() {
            "ALLOW" => {
                debug!("Request allowed by default action of policy '{}'", policy.name);
                Ok(())
            }
            "DENY" => {
                debug!("Request denied by default action of policy '{}'", policy.name);
                Err(format!(
                    "request denied by default policy action from '{}'",
                    policy.name
                ))
            }
            unknown_action => {
                error!(
                    "Unknown default_action '{}' in policy '{}'. Denying request for safety.",
                    unknown_action, policy.name
                );
                Err(format!(
                    "unknown default_action '{}' in policy '{}'",
                    unknown_action, policy.name
                ))
            }
        }
    }
}

impl Limiter for MemoryLimiter {
    fn check_limit_strategy(
        &self,
        policy: &ApiAccessControlPolicy,
        provider_id: i64, // Changed parameter
        model_id: i64,    // Changed parameter
    ) -> Result<(), String> {
        let result = self.inner_check(policy, provider_id, model_id); // Pass new parameters
        result
    }
}

pub static LIMITER: Lazy<Box<dyn Limiter + Sync + Send>> =
    Lazy::new(|| Box::new(MemoryLimiter::new()));
