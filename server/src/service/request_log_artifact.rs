use crate::{controller::BaseError, service::diagnostics::DiagnosticsService};

pub use crate::service::diagnostics::artifact_read_model::{
    CandidateManifestItemResponse, CandidateManifestResponse, NameValueResponse,
    PayloadAttemptManifestResponse, PayloadManifestResponse, PayloadRequestManifestResponse,
    RequestLogArtifactResponse, RequestSnapshotResponse, TransformDiagnosticItemResponse,
    TransformDiagnosticsSummaryBodyResponse, TransformDiagnosticsSummaryResponse,
};
pub use crate::service::diagnostics::capability::{
    ReplayCapabilitySummary, ReplayKindCapabilitySummary,
};

pub async fn get_request_log_artifacts(
    request_log_id: i64,
) -> Result<RequestLogArtifactResponse, BaseError> {
    DiagnosticsService::new()
        .get_request_log_artifacts(request_log_id)
        .await
}
