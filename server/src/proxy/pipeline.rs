use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{body::Body, extract::Request, http::HeaderMap, response::Response};
use chrono::Utc;
use cyder_tools::log::debug;

use super::{
    ProxyError,
    auth::{
        authenticate_anthropic_request, authenticate_gemini_request, authenticate_ollama_request,
        authenticate_openai_request,
    },
    cancellation::ProxyCancellationContext,
    generation::{GenerationExecutionInput, execute_generation_proxy, extract_model_from_request},
    models::execute_models_listing,
    request::parse_json_request,
    utility::{UtilityExecutionInput, UtilityOperation, execute_utility_proxy},
};
use crate::{
    schema::enum_def::LlmApiType,
    service::{app_state::AppState, cache::types::CacheApiKey},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuthenticationStrategy {
    OpenaiCompatible,
    Anthropic,
    Gemini,
    Ollama,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ModelSource {
    RequestBodyField,
    Fixed(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum StreamMode {
    RequestBodyField,
    Fixed(bool),
}

#[derive(Clone, Debug)]
pub(super) struct GenerationOperation {
    pub api_type: LlmApiType,
    pub model_source: ModelSource,
    pub stream_mode: StreamMode,
}

#[derive(Clone, Debug)]
pub(super) struct UtilityPipelineOperation {
    pub operation: UtilityOperation,
    pub model_source: ModelSource,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ModelsOperation {
    pub api_type: LlmApiType,
}

#[derive(Clone, Debug)]
pub(super) enum ProxyOperation {
    Generation(GenerationOperation),
    Utility(UtilityPipelineOperation),
    Models(ModelsOperation),
}

pub(super) struct ProxyPipelineContext {
    pub app_state: Arc<AppState>,
    pub system_api_key: Arc<CacheApiKey>,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub start_time: i64,
}

pub(super) struct OperationAdapter {
    auth: AuthenticationStrategy,
    operation: ProxyOperation,
}

impl OperationAdapter {
    pub(super) fn new(auth: AuthenticationStrategy, operation: ProxyOperation) -> Self {
        Self { auth, operation }
    }

    pub(super) fn openai_generation(api_type: LlmApiType) -> Self {
        Self::new(
            AuthenticationStrategy::for_api_type(api_type),
            ProxyOperation::Generation(GenerationOperation {
                api_type,
                model_source: ModelSource::RequestBodyField,
                stream_mode: StreamMode::RequestBodyField,
            }),
        )
    }

    pub(super) fn fixed_generation(
        auth: AuthenticationStrategy,
        api_type: LlmApiType,
        model_name: String,
        is_stream: bool,
    ) -> Self {
        Self::new(
            auth,
            ProxyOperation::Generation(GenerationOperation {
                api_type,
                model_source: ModelSource::Fixed(model_name),
                stream_mode: StreamMode::Fixed(is_stream),
            }),
        )
    }

    pub(super) fn utility(auth: AuthenticationStrategy, operation: UtilityOperation) -> Self {
        Self::new(
            auth,
            ProxyOperation::Utility(UtilityPipelineOperation {
                operation,
                model_source: ModelSource::RequestBodyField,
            }),
        )
    }

    pub(super) fn fixed_model_utility(
        auth: AuthenticationStrategy,
        operation: UtilityOperation,
        model_name: String,
    ) -> Self {
        Self::new(
            auth,
            ProxyOperation::Utility(UtilityPipelineOperation {
                operation,
                model_source: ModelSource::Fixed(model_name),
            }),
        )
    }

    pub(super) fn list_models(api_type: LlmApiType) -> Self {
        Self::new(
            AuthenticationStrategy::for_api_type(api_type),
            ProxyOperation::Models(ModelsOperation { api_type }),
        )
    }

    pub(super) async fn execute(
        self,
        app_state: Arc<AppState>,
        addr: Option<SocketAddr>,
        query_params: HashMap<String, String>,
        request: Request<Body>,
    ) -> Result<Response<Body>, ProxyError> {
        let start_time = Utc::now().timestamp_millis();
        let request_uri_path = request.uri().path().to_string();
        let original_headers = request.headers().clone();
        debug!("{} --- {:?}", &request_uri_path, &query_params);

        let system_api_key = self
            .authenticate(&app_state, &original_headers, &query_params)
            .await?;
        let context = ProxyPipelineContext {
            app_state,
            system_api_key,
            query_params,
            original_headers,
            client_ip_addr: addr.map(|addr| addr.ip().to_string()),
            start_time,
        };
        let cancellation = ProxyCancellationContext::new();

        match self.operation {
            ProxyOperation::Generation(operation) => {
                execute_generation_operation(context, cancellation, operation, request).await
            }
            ProxyOperation::Utility(operation) => {
                execute_utility_operation(context, cancellation, operation, request).await
            }
            ProxyOperation::Models(operation) => {
                execute_models_listing(
                    context.app_state,
                    context.system_api_key,
                    operation.api_type,
                )
                .await
            }
        }
    }

    async fn authenticate(
        &self,
        app_state: &Arc<AppState>,
        headers: &HeaderMap,
        query_params: &HashMap<String, String>,
    ) -> Result<Arc<CacheApiKey>, ProxyError> {
        let result = match self.auth {
            AuthenticationStrategy::OpenaiCompatible => {
                authenticate_openai_request(headers, query_params, app_state).await
            }
            AuthenticationStrategy::Anthropic => {
                authenticate_anthropic_request(headers, app_state).await
            }
            AuthenticationStrategy::Gemini => {
                authenticate_gemini_request(headers, query_params, app_state).await
            }
            AuthenticationStrategy::Ollama => {
                authenticate_ollama_request(headers, query_params, app_state).await
            }
        }?;

        Ok(result.api_key)
    }
}

impl AuthenticationStrategy {
    fn for_api_type(api_type: LlmApiType) -> Self {
        match api_type {
            LlmApiType::Openai | LlmApiType::Responses | LlmApiType::GeminiOpenai => {
                Self::OpenaiCompatible
            }
            LlmApiType::Anthropic => Self::Anthropic,
            LlmApiType::Gemini => Self::Gemini,
            LlmApiType::Ollama => Self::Ollama,
        }
    }
}

async fn execute_generation_operation(
    context: ProxyPipelineContext,
    cancellation: ProxyCancellationContext,
    operation: GenerationOperation,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let parsed_request = parse_json_request(request).await?;
    let requested_model = resolve_model_source(&operation.model_source, &parsed_request.data)?;
    let is_stream = resolve_stream_mode(operation.stream_mode, &parsed_request.data);

    execute_generation_proxy(
        context.app_state,
        GenerationExecutionInput {
            cancellation,
            system_api_key: context.system_api_key,
            api_type: operation.api_type,
            requested_model,
            is_stream,
            query_params: context.query_params,
            original_headers: context.original_headers,
            client_ip_addr: context.client_ip_addr,
            start_time: context.start_time,
            parsed_request,
        },
    )
    .await
}

async fn execute_utility_operation(
    context: ProxyPipelineContext,
    cancellation: ProxyCancellationContext,
    operation: UtilityPipelineOperation,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let parsed_request = parse_json_request(request).await?;
    let requested_model = resolve_model_source(&operation.model_source, &parsed_request.data)?;

    execute_utility_proxy(
        context.app_state,
        UtilityExecutionInput {
            cancellation,
            system_api_key: context.system_api_key,
            operation: operation.operation,
            requested_model,
            query_params: context.query_params,
            original_headers: context.original_headers,
            client_ip_addr: context.client_ip_addr,
            start_time: context.start_time,
            parsed_request,
        },
    )
    .await
}

fn resolve_model_source(
    model_source: &ModelSource,
    request_data: &serde_json::Value,
) -> Result<String, ProxyError> {
    match model_source {
        ModelSource::RequestBodyField => Ok(extract_model_from_request(request_data)?.to_string()),
        ModelSource::Fixed(model_name) => Ok(model_name.clone()),
    }
}

fn resolve_stream_mode(stream_mode: StreamMode, request_data: &serde_json::Value) -> bool {
    match stream_mode {
        StreamMode::RequestBodyField => request_data
            .get("stream")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        StreamMode::Fixed(is_stream) => is_stream,
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelSource, StreamMode, resolve_model_source, resolve_stream_mode};
    use crate::proxy::ProxyError;
    use serde_json::json;

    #[test]
    fn resolve_model_source_reads_request_body_field() {
        let data = json!({ "model": "provider/model" });
        assert_eq!(
            resolve_model_source(&ModelSource::RequestBodyField, &data).unwrap(),
            "provider/model"
        );
    }

    #[test]
    fn resolve_model_source_supports_fixed_model() {
        let data = json!({});
        assert_eq!(
            resolve_model_source(&ModelSource::Fixed("gemini-2.5-pro".to_string()), &data).unwrap(),
            "gemini-2.5-pro"
        );
    }

    #[test]
    fn resolve_model_source_rejects_missing_model_field() {
        let data = json!({});
        assert!(matches!(
            resolve_model_source(&ModelSource::RequestBodyField, &data),
            Err(ProxyError::BadRequest(_))
        ));
    }

    #[test]
    fn resolve_stream_mode_supports_request_body_and_fixed_values() {
        let streaming = json!({ "stream": true });
        let non_streaming = json!({});

        assert!(resolve_stream_mode(
            StreamMode::RequestBodyField,
            &streaming
        ));
        assert!(!resolve_stream_mode(
            StreamMode::RequestBodyField,
            &non_streaming
        ));
        assert!(resolve_stream_mode(StreamMode::Fixed(true), &non_streaming));
        assert!(!resolve_stream_mode(StreamMode::Fixed(false), &streaming));
    }
}
