mod auth;
mod cancellation;
mod core;
mod error;
mod gemini;
mod generation;
mod handlers;
pub(super) mod logging;
mod models;
mod orchestrator;
mod pipeline;
mod prepare;
mod provider_governance;
mod request;
#[allow(dead_code)]
mod retry_policy;
mod router;
mod unified;
mod util;
mod utility;

#[cfg(test)]
mod integration;
#[cfg(test)]
mod log_regression;

pub(crate) use cancellation::ProxyCancellationContext;
pub(crate) use core::{process_success_response_body, send_with_first_byte_timeout};
use error::classify_request_body_error;
pub(crate) use error::{
    ProxyError, classify_reqwest_error, classify_upstream_status, protocol_transform_error,
};
pub(crate) use orchestrator::{
    GatewayReplayAttemptKind, GatewayReplayCandidateDecision, GatewayReplayExecutionFailure,
    GatewayReplayExecutionMetadata, GatewayReplayFinalAttempt, GatewayReplayInput,
    GatewayReplayPreparedRequest, execute_gateway_replay_request, preview_gateway_replay_request,
};
pub(crate) use prepare::{
    apply_provider_request_auth_header, apply_request_patches, load_runtime_request_patch_trace,
};
pub use router::create_proxy_router;
pub(crate) use utility::{UtilityOperation, UtilityProtocol};

pub async fn flush_proxy_logs() {
    logging::get_log_manager().flush().await;
}
