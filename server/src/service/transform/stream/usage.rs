#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::service::transform) enum UsageMergeStrategy {
    Replace,
    FinalOnly,
}
