use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, extract::Request, response::Response};

use super::{ProxyError, pipeline::OperationAdapter};
use crate::{schema::enum_def::DownstreamProtocol, service::app_state::AppState};

/// Unified proxy handler for the four public downstream generation protocols.
pub async fn unified_proxy_handler(
    app_state: Arc<AppState>,
    query_params: HashMap<String, String>,
    downstream_protocol: DownstreamProtocol,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    OperationAdapter::openai_generation(downstream_protocol)
        .execute(app_state, query_params, request)
        .await
}
