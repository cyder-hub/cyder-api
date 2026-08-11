mod auth;
mod cancellation;
mod error;
mod gemini;
mod generation;
mod handlers;
pub(crate) mod logging;
mod models;
mod pipeline;
mod request;
mod request_context;
mod requested_model;
mod router;
pub(crate) mod runtime;
mod unified;
mod util;
mod utility;

#[cfg(test)]
mod direct_execution_regression;
#[cfg(test)]
mod error_contract_regression;
#[cfg(test)]
mod log_regression;

pub(crate) use cancellation::ProxyCancellationContext;
use error::classify_request_body_error;
#[cfg(test)]
pub(crate) use error::classify_upstream_status;
pub(crate) use error::{
    ExecutionStage, ProtocolErrorResponseAdapter, ProxyError, ProxyErrorCode, ProxyLogLevel,
    ResponseVisibility, ResponseVisibilityTracker, RouterRejection, TimeoutPhase,
    classify_reqwest_error, classify_transform_failure, classify_upstream_status_captured,
    protocol_transform_error, proxy_error_category,
};
pub(crate) use request_context::ProxyRequestContext;
pub use router::create_proxy_router;
pub(crate) use runtime::request_patch::apply_request_patches;
