use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{body::Body, extract::Request, response::Response};

use super::{ProxyError, pipeline::OperationAdapter};
use crate::{schema::enum_def::LlmApiType, service::app_state::AppState};

/// Unified proxy handler for OpenAI, Anthropic, Responses, and Ollama generation requests.
pub async fn unified_proxy_handler(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    query_params: HashMap<String, String>,
    api_type: LlmApiType,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    OperationAdapter::openai_generation(api_type)
        .execute(app_state, Some(addr), query_params, request)
        .await
}
