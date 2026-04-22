use std::time::Duration;

use super::ProxyError;
pub(super) use super::error::REQUEST_PATCH_CONFLICT_ERROR;
use crate::{config::RoutingResilienceConfig, schema::enum_def::SchedulerAction};

pub(super) const PROVIDER_OPEN_SKIPPED_ERROR: &str = "provider_open_skipped";
pub(super) const PROVIDER_HALF_OPEN_SKIPPED_ERROR: &str = "provider_half_open_skipped";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProviderGovernanceRejection {
    Open,
    HalfOpenProbeInFlight,
}

impl ProviderGovernanceRejection {
    pub(super) fn error_code(self) -> &'static str {
        match self {
            ProviderGovernanceRejection::Open => PROVIDER_OPEN_SKIPPED_ERROR,
            ProviderGovernanceRejection::HalfOpenProbeInFlight => PROVIDER_HALF_OPEN_SKIPPED_ERROR,
        }
    }

    pub(super) fn message(self, provider_label: &str) -> String {
        match self {
            ProviderGovernanceRejection::Open => format!(
                "Provider '{provider_label}' is temporarily unavailable due to recent upstream failures."
            ),
            ProviderGovernanceRejection::HalfOpenProbeInFlight => format!(
                "Provider '{provider_label}' is temporarily unavailable because another half-open probe is already in flight."
            ),
        }
    }

    pub(super) fn to_proxy_error(self, provider_label: &str) -> ProxyError {
        let message = self.message(provider_label);
        match self {
            ProviderGovernanceRejection::Open => ProxyError::ProviderOpenSkipped(message),
            ProviderGovernanceRejection::HalfOpenProbeInFlight => {
                ProxyError::ProviderHalfOpenProbeInFlight(message)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum RetryFailureKind<'a> {
    ProxyError(&'a ProxyError),
    ProviderGovernance(ProviderGovernanceRejection),
    CapabilityMismatch,
    RequestPatchConflict,
    NoCandidateAvailable,
}

impl RetryFailureKind<'_> {
    pub(super) fn error_code(self) -> &'static str {
        match self {
            RetryFailureKind::ProxyError(error) => error.error_code(),
            RetryFailureKind::ProviderGovernance(rejection) => rejection.error_code(),
            RetryFailureKind::CapabilityMismatch => {
                super::orchestrator::CAPABILITY_MISMATCH_SKIPPED_ERROR
            }
            RetryFailureKind::RequestPatchConflict => REQUEST_PATCH_CONFLICT_ERROR,
            RetryFailureKind::NoCandidateAvailable => {
                super::orchestrator::NO_CANDIDATE_AVAILABLE_ERROR
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RetryPolicyContext<'a> {
    pub failure: RetryFailureKind<'a>,
    pub same_candidate_retry_count: u32,
    pub attempted_candidate_count: u32,
    pub next_candidate_available: bool,
    pub response_started_to_client: bool,
    pub retry_after: Option<Duration>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetryDecision {
    FailFast,
    RetrySameCandidate { backoff_ms: u64 },
    FallbackNextCandidate,
}

impl RetryDecision {
    pub(super) fn scheduler_action(self) -> SchedulerAction {
        match self {
            RetryDecision::FailFast => SchedulerAction::FailFast,
            RetryDecision::RetrySameCandidate { .. } => SchedulerAction::RetrySameCandidate,
            RetryDecision::FallbackNextCandidate => SchedulerAction::FallbackNextCandidate,
        }
    }
}

pub(super) fn decide_retry(
    config: &RoutingResilienceConfig,
    context: RetryPolicyContext<'_>,
) -> RetryDecision {
    if context.response_started_to_client {
        return RetryDecision::FailFast;
    }

    match context.failure {
        RetryFailureKind::NoCandidateAvailable | RetryFailureKind::RequestPatchConflict => {
            RetryDecision::FailFast
        }
        RetryFailureKind::CapabilityMismatch | RetryFailureKind::ProviderGovernance(_) => {
            fallback_or_fail_fast(config, &context)
        }
        RetryFailureKind::ProxyError(error) => match proxy_error_retry_class(error) {
            ProxyErrorRetryClass::FailFast => RetryDecision::FailFast,
            ProxyErrorRetryClass::FallbackOnly => fallback_or_fail_fast(config, &context),
            ProxyErrorRetryClass::RetrySameCandidateThenFallback => {
                if context.same_candidate_retry_count < config.same_candidate_max_retries {
                    RetryDecision::RetrySameCandidate {
                        backoff_ms: calculate_backoff_ms(config, context),
                    }
                } else {
                    fallback_or_fail_fast(config, &context)
                }
            }
        },
    }
}

fn fallback_or_fail_fast(
    config: &RoutingResilienceConfig,
    context: &RetryPolicyContext<'_>,
) -> RetryDecision {
    if context.next_candidate_available
        && context.attempted_candidate_count < config.max_candidates_per_request
    {
        RetryDecision::FallbackNextCandidate
    } else {
        RetryDecision::FailFast
    }
}

fn calculate_backoff_ms(config: &RoutingResilienceConfig, context: RetryPolicyContext<'_>) -> u64 {
    if let Some(retry_after) = context.retry_after {
        let max_retry_after = Duration::from_secs(config.respect_retry_after_up_to_seconds);
        if retry_after <= max_retry_after {
            return duration_to_millis(retry_after).min(config.max_backoff_ms);
        }
    }

    let multiplier = if context.same_candidate_retry_count >= u64::BITS {
        u64::MAX
    } else {
        1_u64 << context.same_candidate_retry_count
    };
    config
        .base_backoff_ms
        .saturating_mul(multiplier)
        .min(config.max_backoff_ms)
}

fn duration_to_millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProxyErrorRetryClass {
    FailFast,
    FallbackOnly,
    RetrySameCandidateThenFallback,
}

fn proxy_error_retry_class(error: &ProxyError) -> ProxyErrorRetryClass {
    match error {
        ProxyError::UpstreamRateLimited(_)
        | ProxyError::BadGateway(_)
        | ProxyError::UpstreamService(_)
        | ProxyError::UpstreamTimeout(_) => ProxyErrorRetryClass::RetrySameCandidateThenFallback,
        ProxyError::UpstreamAuthentication(_) => ProxyErrorRetryClass::FallbackOnly,
        ProxyError::Unauthorized(_)
        | ProxyError::KeyDisabled(_)
        | ProxyError::KeyExpired(_)
        | ProxyError::BadRequest(_)
        | ProxyError::Forbidden(_)
        | ProxyError::RateLimited(_)
        | ProxyError::ConcurrencyLimited(_)
        | ProxyError::QuotaExhausted(_)
        | ProxyError::BudgetExhausted(_)
        | ProxyError::ProviderOpenSkipped(_)
        | ProxyError::ProviderHalfOpenProbeInFlight(_)
        | ProxyError::PayloadTooLarge(_)
        | ProxyError::ClientCancelled(_)
        | ProxyError::RequestPatchConflict(_)
        | ProxyError::InternalError(_)
        | ProxyError::ProtocolTransformError(_)
        | ProxyError::UpstreamBadRequest(_) => ProxyErrorRetryClass::FailFast,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> RoutingResilienceConfig {
        RoutingResilienceConfig::default()
    }

    fn retryable_error() -> ProxyError {
        ProxyError::UpstreamTimeout("timeout".to_string())
    }

    fn context<'a>(failure: RetryFailureKind<'a>) -> RetryPolicyContext<'a> {
        RetryPolicyContext {
            failure,
            same_candidate_retry_count: 0,
            attempted_candidate_count: 1,
            next_candidate_available: true,
            response_started_to_client: false,
            retry_after: None,
        }
    }

    #[test]
    fn response_started_forces_fail_fast_even_for_retryable_errors() {
        let error = retryable_error();
        let decision = decide_retry(
            &default_config(),
            RetryPolicyContext {
                response_started_to_client: true,
                ..context(RetryFailureKind::ProxyError(&error))
            },
        );

        assert_eq!(decision, RetryDecision::FailFast);
        assert_eq!(decision.scheduler_action(), SchedulerAction::FailFast);
    }

    #[test]
    fn retryable_upstream_errors_retry_same_candidate_before_fallback() {
        let error = retryable_error();
        let config = default_config();
        let first_decision = decide_retry(&config, context(RetryFailureKind::ProxyError(&error)));

        assert_eq!(
            first_decision,
            RetryDecision::RetrySameCandidate { backoff_ms: 250 }
        );
        assert_eq!(
            first_decision.scheduler_action(),
            SchedulerAction::RetrySameCandidate
        );

        let second_decision = decide_retry(
            &config,
            RetryPolicyContext {
                same_candidate_retry_count: 1,
                ..context(RetryFailureKind::ProxyError(&error))
            },
        );

        assert_eq!(second_decision, RetryDecision::FallbackNextCandidate);
    }

    #[test]
    fn fallback_is_limited_by_candidate_budget() {
        let error = retryable_error();
        let decision = decide_retry(
            &default_config(),
            RetryPolicyContext {
                same_candidate_retry_count: 1,
                attempted_candidate_count: 2,
                next_candidate_available: true,
                ..context(RetryFailureKind::ProxyError(&error))
            },
        );

        assert_eq!(decision, RetryDecision::FailFast);
    }

    #[test]
    fn provider_governance_and_capability_events_fallback_when_possible() {
        let config = default_config();
        let provider_open = decide_retry(
            &config,
            context(RetryFailureKind::ProviderGovernance(
                ProviderGovernanceRejection::Open,
            )),
        );
        let provider_half_open = decide_retry(
            &config,
            context(RetryFailureKind::ProviderGovernance(
                ProviderGovernanceRejection::HalfOpenProbeInFlight,
            )),
        );
        let capability_mismatch =
            decide_retry(&config, context(RetryFailureKind::CapabilityMismatch));

        assert_eq!(provider_open, RetryDecision::FallbackNextCandidate);
        assert_eq!(provider_half_open, RetryDecision::FallbackNextCandidate);
        assert_eq!(capability_mismatch, RetryDecision::FallbackNextCandidate);
        assert_eq!(
            ProviderGovernanceRejection::Open.error_code(),
            PROVIDER_OPEN_SKIPPED_ERROR
        );
        assert_eq!(
            ProviderGovernanceRejection::HalfOpenProbeInFlight.error_code(),
            PROVIDER_HALF_OPEN_SKIPPED_ERROR
        );
    }

    #[test]
    fn retry_after_is_respected_only_within_configured_cap() {
        let error = retryable_error();
        let config = default_config();
        let short_retry_after = decide_retry(
            &config,
            RetryPolicyContext {
                retry_after: Some(Duration::from_secs(2)),
                ..context(RetryFailureKind::ProxyError(&error))
            },
        );
        let long_retry_after = decide_retry(
            &config,
            RetryPolicyContext {
                retry_after: Some(Duration::from_secs(4)),
                ..context(RetryFailureKind::ProxyError(&error))
            },
        );

        assert_eq!(
            short_retry_after,
            RetryDecision::RetrySameCandidate { backoff_ms: 1500 }
        );
        assert_eq!(
            long_retry_after,
            RetryDecision::RetrySameCandidate { backoff_ms: 250 }
        );
    }

    #[test]
    fn fail_fast_errors_never_retry_or_fallback() {
        let errors = vec![
            ProxyError::Unauthorized("x".to_string()),
            ProxyError::KeyDisabled("x".to_string()),
            ProxyError::KeyExpired("x".to_string()),
            ProxyError::BadRequest("x".to_string()),
            ProxyError::Forbidden("x".to_string()),
            ProxyError::RateLimited("x".to_string()),
            ProxyError::ConcurrencyLimited("x".to_string()),
            ProxyError::QuotaExhausted("x".to_string()),
            ProxyError::BudgetExhausted("x".to_string()),
            ProxyError::ProviderOpenSkipped("x".to_string()),
            ProxyError::ProviderHalfOpenProbeInFlight("x".to_string()),
            ProxyError::PayloadTooLarge("x".to_string()),
            ProxyError::ClientCancelled("x".to_string()),
            ProxyError::RequestPatchConflict("x".to_string()),
            ProxyError::InternalError("x".to_string()),
            ProxyError::ProtocolTransformError("x".to_string()),
            ProxyError::UpstreamBadRequest("x".to_string()),
        ];

        for error in &errors {
            assert_eq!(
                decide_retry(
                    &default_config(),
                    context(RetryFailureKind::ProxyError(error))
                ),
                RetryDecision::FailFast,
                "{} should fail fast",
                error.error_code()
            );
        }

        assert_eq!(
            decide_retry(
                &default_config(),
                context(RetryFailureKind::RequestPatchConflict)
            ),
            RetryDecision::FailFast
        );
        assert_eq!(
            RetryFailureKind::RequestPatchConflict.error_code(),
            REQUEST_PATCH_CONFLICT_ERROR
        );
        assert_eq!(
            decide_retry(
                &default_config(),
                context(RetryFailureKind::NoCandidateAvailable)
            ),
            RetryDecision::FailFast
        );
    }

    #[test]
    fn fallback_only_errors_do_not_retry_same_candidate() {
        let error = ProxyError::UpstreamAuthentication("bad provider key".to_string());
        let decision = decide_retry(
            &default_config(),
            context(RetryFailureKind::ProxyError(&error)),
        );

        assert_eq!(decision, RetryDecision::FallbackNextCandidate);
    }
}
