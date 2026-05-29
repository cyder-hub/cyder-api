#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeExecutionPolicy {
    Normal,
    ReplayDryRun,
    ReplayLive,
}

impl RuntimeExecutionPolicy {
    pub(crate) fn sends_upstream_request(self) -> bool {
        matches!(self, Self::Normal | Self::ReplayLive)
    }

    pub(crate) fn records_request_log(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn records_provider_runtime(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn captures_reasoning_continuations(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn admits_api_key_requests(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn uses_mutating_provider_governance(self) -> bool {
        matches!(self, Self::Normal)
    }

    pub(crate) fn uses_read_only_provider_governance(self) -> bool {
        matches!(self, Self::ReplayDryRun | Self::ReplayLive)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeLogMode {
    RecordAll,
    DeferNonStreaming,
}

impl RuntimeLogMode {
    pub(crate) fn should_record_immediate(self) -> bool {
        matches!(self, Self::RecordAll)
    }

    pub(crate) fn should_record_streaming(self) -> bool {
        matches!(self, Self::RecordAll | Self::DeferNonStreaming)
    }

    pub(crate) fn should_record_attempt_failure(self) -> bool {
        matches!(self, Self::RecordAll)
    }

    pub(crate) fn proxy_log_mode(self) -> Self {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::{RuntimeExecutionPolicy, RuntimeLogMode};

    #[test]
    fn normal_policy_allows_live_runtime_side_effects() {
        let policy = RuntimeExecutionPolicy::Normal;

        assert!(policy.sends_upstream_request());
        assert!(policy.records_request_log());
        assert!(policy.records_provider_runtime());
        assert!(policy.captures_reasoning_continuations());
        assert!(policy.admits_api_key_requests());
        assert!(policy.uses_mutating_provider_governance());
        assert!(!policy.uses_read_only_provider_governance());
    }

    #[test]
    fn replay_dry_run_policy_disables_upstream_and_runtime_side_effects() {
        let policy = RuntimeExecutionPolicy::ReplayDryRun;

        assert!(!policy.sends_upstream_request());
        assert!(!policy.records_request_log());
        assert!(!policy.records_provider_runtime());
        assert!(!policy.captures_reasoning_continuations());
        assert!(!policy.admits_api_key_requests());
        assert!(!policy.uses_mutating_provider_governance());
        assert!(policy.uses_read_only_provider_governance());
    }

    #[test]
    fn replay_live_policy_sends_upstream_without_live_runtime_side_effects() {
        let policy = RuntimeExecutionPolicy::ReplayLive;

        assert!(policy.sends_upstream_request());
        assert!(!policy.records_request_log());
        assert!(!policy.records_provider_runtime());
        assert!(!policy.captures_reasoning_continuations());
        assert!(!policy.admits_api_key_requests());
        assert!(!policy.uses_mutating_provider_governance());
        assert!(policy.uses_read_only_provider_governance());
    }

    #[test]
    fn record_all_log_mode_records_immediate_and_streaming_paths() {
        let mode = RuntimeLogMode::RecordAll;

        assert!(mode.should_record_immediate());
        assert!(mode.should_record_streaming());
        assert!(mode.should_record_attempt_failure());
        assert_eq!(mode.proxy_log_mode(), mode);
    }

    #[test]
    fn defer_non_streaming_log_mode_keeps_streaming_finalizers_active() {
        let mode = RuntimeLogMode::DeferNonStreaming;

        assert!(!mode.should_record_immediate());
        assert!(mode.should_record_streaming());
        assert!(!mode.should_record_attempt_failure());
        assert_eq!(mode.proxy_log_mode(), mode);
    }
}
