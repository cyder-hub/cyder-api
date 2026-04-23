use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{body::Body, extract::Request, response::Response};

use super::{
    ProxyError,
    pipeline::{AuthenticationStrategy, OperationAdapter},
    utility::{UtilityOperation, UtilityProtocol},
};
use crate::{schema::enum_def::LlmApiType, service::app_state::AppState};

const GEMINI_GENERATION_ACTIONS: [&str; 2] = ["generateContent", "streamGenerateContent"];
const GEMINI_UTILITY_ACTIONS: [&str; 3] = ["countMessageTokens", "countTextTokens", "countTokens"];

pub async fn handle_gemini_request(
    app_state: Arc<AppState>,
    addr: SocketAddr,
    path_segment: String,
    query_params: HashMap<String, String>,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let (model_name, action) = parse_gemini_model_action(&path_segment)?;

    let adapter = if GEMINI_UTILITY_ACTIONS.contains(&action) {
        OperationAdapter::fixed_model_utility(
            AuthenticationStrategy::Gemini,
            UtilityOperation {
                name: action.to_string(),
                api_type: LlmApiType::Gemini,
                protocol: UtilityProtocol::GeminiCompatible,
                downstream_path: action.to_string(),
            },
            model_name.to_string(),
        )
    } else if GEMINI_GENERATION_ACTIONS.contains(&action) {
        OperationAdapter::fixed_generation(
            AuthenticationStrategy::Gemini,
            LlmApiType::Gemini,
            model_name.to_string(),
            action == "streamGenerateContent",
        )
    } else {
        let err_msg = format!(
            "Invalid action: '{}'. Must be one of 'generateContent', 'streamGenerateContent', 'countMessageTokens', 'countTextTokens', or 'countTokens'.",
            action
        );
        crate::debug_event!(
            "proxy.gemini_bad_request",
            reason = "invalid_action",
            path_segment = &path_segment,
        );
        return Err(ProxyError::BadRequest(err_msg));
    };

    adapter
        .execute(app_state, Some(addr), query_params, request)
        .await
}

fn parse_gemini_model_action(path_segment: &str) -> Result<(&str, &str), ProxyError> {
    let parts: Vec<&str> = path_segment.rsplitn(2, ':').collect();
    if parts.len() != 2 {
        let err_msg = format!(
            "Invalid model_action_segment format: '{}'. Expected 'model_name:action'.",
            path_segment
        );
        crate::debug_event!(
            "proxy.gemini_bad_request",
            reason = "invalid_model_action_segment",
            path_segment = path_segment,
        );
        return Err(ProxyError::BadRequest(err_msg));
    }

    Ok((parts[1], parts[0]))
}

#[cfg(test)]
mod tests {
    use super::parse_gemini_model_action;
    use crate::proxy::ProxyError;

    #[test]
    fn parses_gemini_model_action_segment() {
        let (model, action) =
            parse_gemini_model_action("models/gemini-2.5-pro:countTokens").unwrap();
        assert_eq!(model, "models/gemini-2.5-pro");
        assert_eq!(action, "countTokens");
    }

    #[test]
    fn rejects_invalid_gemini_model_action_segment() {
        assert!(matches!(
            parse_gemini_model_action("models/gemini-2.5-pro"),
            Err(ProxyError::BadRequest(_))
        ));
    }
}
