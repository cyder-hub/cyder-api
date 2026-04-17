mod auth;
mod cancellation;
mod core;
mod error;
mod gemini;
mod generation;
mod handlers;
pub(super) mod logging;
mod models;
mod pipeline;
mod prepare;
mod provider_governance;
mod request;
mod router;
mod unified;
mod util;
mod utility;

#[cfg(test)]
mod integration;

use error::{
    ProxyError, classify_request_body_error, classify_reqwest_error, classify_upstream_status,
    protocol_transform_error,
};
pub use router::create_proxy_router;

pub async fn flush_proxy_logs() {
    logging::get_log_manager().flush().await;
}
