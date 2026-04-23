use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    body::Body,
    extract::{OriginalUri, Request},
    http::HeaderMap,
    response::Response,
};
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
    prepare::build_execution_plan,
    request::parse_json_request,
    util::build_request_snapshot,
    utility::{UtilityExecutionInput, UtilityOperation, execute_utility_proxy},
};
use crate::{
    schema::enum_def::LlmApiType,
    service::{app_state::AppState, cache::types::CacheApiKey},
    utils::storage::RequestLogBundleRequestSnapshot,
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
    pub api_key: Arc<CacheApiKey>,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub request_snapshot: RequestLogBundleRequestSnapshot,
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
        let request_uri = request
            .extensions()
            .get::<OriginalUri>()
            .map(|uri| &uri.0)
            .unwrap_or_else(|| request.uri());
        let request_uri_path = request_uri.path().to_string();
        let request_uri_query = request_uri.query().map(str::to_string);
        let original_headers = request.headers().clone();
        let operation_kind = derive_request_operation_kind(&request_uri_path);
        let request_snapshot = build_request_snapshot(
            &request_uri_path,
            &operation_kind,
            request_uri_query.as_deref(),
            &original_headers,
        );
        crate::debug_event!(
            "proxy.request_received",
            request_path = &request_uri_path,
            operation_kind = &operation_kind,
            query_param_count = query_params.len(),
        );

        let api_key = self
            .authenticate(&app_state, &original_headers, &query_params)
            .await?;
        let context = ProxyPipelineContext {
            app_state,
            api_key,
            query_params,
            original_headers,
            request_snapshot,
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
                execute_models_listing(context.app_state, context.api_key, operation.api_type).await
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
    let execution_plan =
        build_execution_plan(&context.app_state, context.api_key.id, &requested_model)
            .await
            .map_err(|e| {
                crate::debug_event!(
                    "proxy.execution_plan_build_failed",
                    requested_model = &requested_model,
                    error = &e,
                );
                ProxyError::BadRequest(e)
            })?;
    debug!(
        "Built execution plan for '{}': {}",
        requested_model,
        execution_plan.candidate_summary_for_log()
    );

    execute_generation_proxy(
        context.app_state,
        GenerationExecutionInput {
            cancellation,
            api_key: context.api_key,
            api_type: operation.api_type,
            execution_plan,
            is_stream,
            query_params: context.query_params,
            original_headers: context.original_headers,
            request_snapshot: context.request_snapshot,
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
    let execution_plan =
        build_execution_plan(&context.app_state, context.api_key.id, &requested_model)
            .await
            .map_err(|e| {
                crate::debug_event!(
                    "proxy.execution_plan_build_failed",
                    requested_model = &requested_model,
                    error = &e,
                );
                ProxyError::BadRequest(e)
            })?;
    debug!(
        "Built utility execution plan for '{}': {}",
        requested_model,
        execution_plan.candidate_summary_for_log()
    );

    execute_utility_proxy(
        context.app_state,
        UtilityExecutionInput {
            cancellation,
            api_key: context.api_key,
            operation: operation.operation,
            execution_plan,
            query_params: context.query_params,
            original_headers: context.original_headers,
            request_snapshot: context.request_snapshot,
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

fn derive_request_operation_kind(request_path: &str) -> String {
    let normalized_path = request_path.trim_end_matches('/');
    if normalized_path.ends_with("/chat/completions") {
        "chat_completions_create".to_string()
    } else if normalized_path.ends_with("/responses") {
        "responses_create".to_string()
    } else if normalized_path.ends_with("/messages") {
        "messages_create".to_string()
    } else if normalized_path.ends_with("/embeddings") {
        "embeddings".to_string()
    } else if normalized_path.ends_with("/rerank") {
        "rerank".to_string()
    } else if normalized_path.ends_with("/models") || normalized_path.ends_with("/api/tags") {
        "models_list".to_string()
    } else if normalized_path.ends_with("/api/chat") {
        "chat".to_string()
    } else if normalized_path.ends_with("/api/generate") {
        "generate".to_string()
    } else {
        normalized_path
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .map(path_segment_to_operation_kind)
            .unwrap_or_else(|| "request".to_string())
    }
}

fn path_segment_to_operation_kind(segment: &str) -> String {
    if let Some((_, action)) = segment.split_once(':') {
        camel_case_to_snake_case(action)
    } else {
        segment
            .chars()
            .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
            .collect()
    }
}

fn camel_case_to_snake_case(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut prev_is_lower_or_digit = false;

    for ch in value.chars() {
        if ch.is_ascii_uppercase() {
            if prev_is_lower_or_digit && !normalized.ends_with('_') {
                normalized.push('_');
            }
            normalized.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = false;
        } else if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        } else if !normalized.ends_with('_') {
            normalized.push('_');
            prev_is_lower_or_digit = false;
        }
    }

    normalized.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::{
        ModelSource, StreamMode, derive_request_operation_kind, resolve_model_source,
        resolve_stream_mode,
    };
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

    #[test]
    fn derive_request_operation_kind_covers_common_routes() {
        assert_eq!(
            derive_request_operation_kind("/openai/v1/chat/completions"),
            "chat_completions_create"
        );
        assert_eq!(
            derive_request_operation_kind("/responses/v1/responses"),
            "responses_create"
        );
        assert_eq!(
            derive_request_operation_kind("/gemini/v1beta/models/foo:streamGenerateContent"),
            "stream_generate_content"
        );
        assert_eq!(
            derive_request_operation_kind("/ollama/api/tags"),
            "models_list"
        );
    }
}
