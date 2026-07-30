use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, extract::Request, response::Response};

use super::{
    ProxyError,
    pipeline::{AuthenticationStrategy, OperationAdapter},
    utility::{UtilityOperation, UtilityProtocol},
};
use crate::{schema::enum_def::DownstreamProtocol, service::app_state::AppState};

pub async fn openai_utility_handler(
    app_state: Arc<AppState>,
    params: HashMap<String, String>,
    request: Request<Body>,
    downstream_path: &'static str,
) -> Result<Response<Body>, ProxyError> {
    OperationAdapter::utility(
        AuthenticationStrategy::OpenaiCompatible,
        UtilityOperation {
            name: downstream_path.to_string(),
            downstream_protocol: DownstreamProtocol::Openai,
            protocol: UtilityProtocol::OpenaiCompatible,
            downstream_path: downstream_path.to_string(),
        },
    )
    .execute(app_state, params, request)
    .await
}

pub async fn list_models_handler(
    app_state: Arc<AppState>,
    params: HashMap<String, String>,
    request: Request<Body>,
    downstream_protocol: DownstreamProtocol,
) -> Result<Response<Body>, ProxyError> {
    OperationAdapter::list_models(downstream_protocol)
        .execute(app_state, params, request)
        .await
}
