use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{body::Body, extract::Request, response::Response};

use super::{
    ProxyError,
    pipeline::{AuthenticationStrategy, OperationAdapter},
    utility::{UtilityOperation, UtilityProtocol},
};
use crate::{schema::enum_def::LlmApiType, service::app_state::AppState};

pub async fn openai_utility_handler(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    params: HashMap<String, String>,
    request: Request<Body>,
    downstream_path: &'static str,
) -> Result<Response<Body>, ProxyError> {
    OperationAdapter::utility(
        AuthenticationStrategy::OpenaiCompatible,
        UtilityOperation {
            name: downstream_path.to_string(),
            api_type: LlmApiType::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: downstream_path.to_string(),
        },
    )
    .execute(app_state, Some(addr), params, request)
    .await
}

pub async fn list_models_handler(
    app_state: Arc<AppState>,
    params: HashMap<String, String>,
    request: Request<Body>,
    api_type: LlmApiType,
) -> Result<Response<Body>, ProxyError> {
    OperationAdapter::list_models(api_type)
        .execute(app_state, None, params, request)
        .await
}
