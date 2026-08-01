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
mod log_regression;

use error::classify_request_body_error;
pub(crate) use error::{
    ExecutionStage, ProxyError, ProxyErrorCode, ProxyLogLevel, ResponseVisibility,
    ResponseVisibilityTracker, classify_reqwest_error, classify_upstream_status,
    protocol_transform_error,
};
pub use router::create_proxy_router;
pub(crate) use runtime::request_patch::{apply_request_patches, load_runtime_request_patch_trace};
