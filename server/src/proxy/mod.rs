mod auth;
mod cancellation;
mod error;
mod gemini;
mod generation;
mod handlers;
pub(crate) mod logging;
mod models;
mod pipeline;
mod provider_governance;
pub(crate) mod reasoning_suffix;
mod request;
mod requested_model;
#[allow(dead_code)]
mod retry_policy;
mod router;
pub(crate) mod runtime;
mod unified;
mod util;
mod utility;

#[cfg(test)]
mod integration;
#[cfg(test)]
mod log_regression;

pub(crate) use cancellation::ProxyCancellationContext;
use error::classify_request_body_error;
pub(crate) use error::{
    ProxyError, classify_reqwest_error, classify_upstream_status, protocol_transform_error,
};
pub use router::create_proxy_router;
pub(crate) use runtime::credential::apply_provider_request_auth_header;
pub(crate) use runtime::facade::{execute_gateway_replay_request, preview_gateway_replay_request};
pub(crate) use runtime::request_patch::{apply_request_patches, load_runtime_request_patch_trace};
pub(crate) use runtime::route_resolver::{
    candidate_supports_reasoning_preset, resolve_effective_reasoning_config,
    resolve_route_runtime_candidates,
};
pub(crate) use runtime::transport::{process_success_response_body, send_with_first_byte_timeout};
pub(crate) use utility::{UtilityOperation, UtilityProtocol};
