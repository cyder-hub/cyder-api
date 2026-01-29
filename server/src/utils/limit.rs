use cyder_tools::log::debug; // Removed info, not used in this file
use once_cell::sync::Lazy;

// Removed: use crate::controller::proxy::RequestInfo;
use crate::service::cache::types::CacheAccessControl;
use crate::schema::enum_def::{Action, RuleScope};

pub trait Limiter: Sync + Send {
    fn check_limit_strategy(
        &self,
        policy: &CacheAccessControl,
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
        policy: &CacheAccessControl,
        provider_id: i64, // Changed parameter
        model_id: i64,    // Changed parameter
    ) -> Result<(), String> {
        // Use parameters directly
        let request_provider_id = provider_id;
        let request_model_id = model_id;

        // Rules in ApiAccessControlPolicy are assumed to be pre-sorted by priority.
        for rule in &policy.rules {
            let mut rule_matches = false;
            match rule.scope {
                RuleScope::Model => {
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
                RuleScope::Provider => {
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
            }

            if rule_matches {
                match rule.rule_type {
                    Action::Allow => {
                        debug!(
                            "Request allowed by rule ID {} (Priority {}, Scope: {:?}) for policy '{:?}'",
                            rule.id, rule.priority, rule.scope, policy.name
                        );
                        return Ok(());
                    }
                    Action::Deny => {
                        debug!(
                            "Request denied by rule ID {} (Priority {}, Scope: {:?}) for policy '{:?}'",
                            rule.id, rule.priority, rule.scope, policy.name
                        );
                        return Err(format!(
                            "request denied by rule (ID: {}, Policy: '{}', Scope: {:?}, Type: {:?})",
                            rule.id, policy.name, rule.scope, rule.rule_type
                        ));
                    }
                }
            }
        }

        // If no rules matched, apply default_action
        match policy.default_action {
            Action::Allow => {
                debug!("Request allowed by default action of policy '{}'", policy.name);
                Ok(())
            }
            Action::Deny => {
                debug!("Request denied by default action of policy '{}'", policy.name);
                Err(format!(
                    "request denied by default policy action from '{}'",
                    policy.name
                ))
            }
        }
    }
}

impl Limiter for MemoryLimiter {
    fn check_limit_strategy(
        &self,
        policy: &CacheAccessControl,
        provider_id: i64, // Changed parameter
        model_id: i64,    // Changed parameter
    ) -> Result<(), String> {
        let result = self.inner_check(policy, provider_id, model_id); // Pass new parameters
        result
    }
}

pub static LIMITER: Lazy<Box<dyn Limiter + Sync + Send>> =
    Lazy::new(|| Box::new(MemoryLimiter::new()));
