use std::sync::Arc;

use bytes::Bytes;

use crate::{
    controller::BaseError,
    database::{
        request_attempt::RequestAttempt,
        request_log::RequestLog,
        request_replay_run::{RequestReplayRun, RequestReplayRunRecord},
    },
    service::{
        app_state::AppState,
        diagnostics::{
            artifact_read_model::{
                RequestLogArtifactResponse, build_request_log_artifact_response,
            },
            bundle,
            policy::{DiagnosticsPolicy, DiagnosticsPolicyManager},
            replay::artifact_store,
            replay::executor,
            replay::types::{
                AttemptReplayExecuteParams, AttemptReplayPreviewParams,
                AttemptReplayPreviewResponse, GatewayReplayExecuteParams,
                GatewayReplayPreviewParams, GatewayReplayPreviewResponse, RequestReplayArtifact,
            },
            retention::{self, DiagnosticsRetentionParams, DiagnosticsRetentionResponse},
            storage_inventory::{
                self, DiagnosticsStorageInventoryParams, DiagnosticsStorageInventoryResponse,
            },
        },
    },
};

#[derive(Debug, Clone)]
pub struct DiagnosticsService {
    policy_manager: Arc<DiagnosticsPolicyManager>,
}

impl DiagnosticsService {
    pub fn new(policy_manager: Arc<DiagnosticsPolicyManager>) -> Self {
        Self { policy_manager }
    }

    pub fn new_with_default_policy() -> Self {
        Self::new(Arc::new(DiagnosticsPolicyManager::new(
            DiagnosticsPolicy::default(),
        )))
    }

    pub fn policy_manager(&self) -> Arc<DiagnosticsPolicyManager> {
        Arc::clone(&self.policy_manager)
    }

    pub async fn policy(&self) -> DiagnosticsPolicy {
        self.policy_manager.current().await
    }

    pub async fn get_request_log_artifacts(
        &self,
        request_log_id: i64,
    ) -> Result<RequestLogArtifactResponse, BaseError> {
        let record = RequestLog::get_by_id(request_log_id)?;
        let attempts = RequestAttempt::list_by_request_log_id(request_log_id)?;
        let bundle = bundle::load_request_log_bundle(&record).await?;

        Ok(build_request_log_artifact_response(
            &record,
            &attempts,
            bundle.as_ref(),
        ))
    }

    pub async fn get_request_log_bundle_content(
        &self,
        request_log_id: i64,
    ) -> Result<Bytes, BaseError> {
        let policy = self.policy().await;
        bundle::load_request_log_bundle_content(request_log_id, &policy).await
    }

    pub async fn preview_attempt_replay(
        &self,
        app_state: &Arc<AppState>,
        request_log_id: i64,
        attempt_id: i64,
        params: AttemptReplayPreviewParams,
    ) -> Result<AttemptReplayPreviewResponse, BaseError> {
        executor::preview_attempt_replay(app_state, request_log_id, attempt_id, params).await
    }

    pub async fn execute_attempt_replay(
        &self,
        app_state: &Arc<AppState>,
        request_log_id: i64,
        attempt_id: i64,
        params: AttemptReplayExecuteParams,
    ) -> Result<RequestReplayRunRecord, BaseError> {
        executor::execute_attempt_replay(app_state, request_log_id, attempt_id, params).await
    }

    pub async fn preview_gateway_replay(
        &self,
        app_state: &Arc<AppState>,
        request_log_id: i64,
        params: GatewayReplayPreviewParams,
    ) -> Result<GatewayReplayPreviewResponse, BaseError> {
        executor::preview_gateway_replay(app_state, request_log_id, params).await
    }

    pub async fn execute_gateway_replay(
        &self,
        app_state: &Arc<AppState>,
        request_log_id: i64,
        params: GatewayReplayExecuteParams,
    ) -> Result<RequestReplayRunRecord, BaseError> {
        executor::execute_gateway_replay(app_state, request_log_id, params).await
    }

    pub fn list_replay_runs(
        &self,
        request_log_id: i64,
    ) -> Result<Vec<RequestReplayRunRecord>, BaseError> {
        RequestReplayRun::list_by_source_request_log_id(request_log_id)
    }

    pub fn get_replay_run(
        &self,
        request_log_id: i64,
        replay_run_id: i64,
    ) -> Result<RequestReplayRunRecord, BaseError> {
        RequestReplayRun::get_by_source_and_id(request_log_id, replay_run_id)
    }

    pub async fn get_replay_artifact(
        &self,
        request_log_id: i64,
        replay_run_id: i64,
    ) -> Result<RequestReplayArtifact, BaseError> {
        let run = self.get_replay_run(request_log_id, replay_run_id)?;
        artifact_store::load_replay_artifact_for_run(&run).await
    }

    pub async fn retention_preview(
        &self,
        params: DiagnosticsRetentionParams,
    ) -> Result<DiagnosticsRetentionResponse, BaseError> {
        let policy = self.policy().await;
        retention::preview_retention(params, &policy)
    }

    pub async fn retention_execute(
        &self,
        params: DiagnosticsRetentionParams,
    ) -> Result<DiagnosticsRetentionResponse, BaseError> {
        let policy = self.policy().await;
        retention::execute_retention(params, &policy).await
    }

    pub async fn storage_inventory_preview(
        &self,
        params: DiagnosticsStorageInventoryParams,
    ) -> Result<DiagnosticsStorageInventoryResponse, BaseError> {
        storage_inventory::preview_storage_inventory(params).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_service_exposes_policy_manager() {
        let service = DiagnosticsService::new_with_default_policy();

        assert_eq!(Arc::strong_count(&service.policy_manager()), 2);
    }

    #[test]
    fn diagnostics_service_wires_replay_run_repository_methods() {
        let _list_runs: fn(
            &DiagnosticsService,
            i64,
        ) -> Result<Vec<RequestReplayRunRecord>, BaseError> = DiagnosticsService::list_replay_runs;
        let _get_run: fn(
            &DiagnosticsService,
            i64,
            i64,
        ) -> Result<RequestReplayRunRecord, BaseError> = DiagnosticsService::get_replay_run;
    }

    #[test]
    fn diagnostics_service_exposes_existing_async_use_cases() {
        let _get_artifacts = DiagnosticsService::get_request_log_artifacts;
        let _get_bundle_content = DiagnosticsService::get_request_log_bundle_content;
        let _preview_attempt = DiagnosticsService::preview_attempt_replay;
        let _execute_attempt = DiagnosticsService::execute_attempt_replay;
        let _preview_gateway = DiagnosticsService::preview_gateway_replay;
        let _execute_gateway = DiagnosticsService::execute_gateway_replay;
        let _get_replay_artifact = DiagnosticsService::get_replay_artifact;
        let _retention_preview = DiagnosticsService::retention_preview;
        let _retention_execute = DiagnosticsService::retention_execute;
        let _storage_inventory_preview = DiagnosticsService::storage_inventory_preview;
    }
}
