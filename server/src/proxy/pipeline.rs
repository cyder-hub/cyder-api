use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, extract::Request, http::HeaderMap, response::Response};

use super::{
    ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
    auth::{
        authenticate_anthropic_request, authenticate_gemini_request, authenticate_openai_request,
    },
    cancellation::ProxyCancellationContext,
    generation::{GenerationExecutionInput, execute_generation_proxy, extract_model_from_request},
    models::execute_models_listing,
    request::parse_json_request,
    request_context::ProxyRequestContext,
    runtime::route_resolver::{ExecutionPlan, ExecutionPlanBuildError, build_execution_plan},
    utility::{UtilityExecutionInput, UtilityOperation, execute_utility_proxy},
};
use crate::{
    ingress::client_identity::ClientIdentity,
    schema::enum_def::DownstreamProtocol,
    service::{app_state::AppState, cache::types::CacheApiKey},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AuthenticationStrategy {
    OpenaiCompatible,
    Anthropic,
    Gemini,
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
    pub downstream_protocol: DownstreamProtocol,
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
    pub downstream_protocol: DownstreamProtocol,
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
    pub client_ip_addr: Option<String>,
    pub request_context: Arc<ProxyRequestContext>,
}

pub(super) struct OperationAdapter {
    auth: AuthenticationStrategy,
    operation: ProxyOperation,
}

impl OperationAdapter {
    pub(super) fn new(auth: AuthenticationStrategy, operation: ProxyOperation) -> Self {
        Self { auth, operation }
    }

    pub(super) fn openai_generation(downstream_protocol: DownstreamProtocol) -> Self {
        Self::new(
            AuthenticationStrategy::for_downstream_protocol(downstream_protocol),
            ProxyOperation::Generation(GenerationOperation {
                downstream_protocol,
                model_source: ModelSource::RequestBodyField,
                stream_mode: StreamMode::RequestBodyField,
            }),
        )
    }

    pub(super) fn fixed_generation(
        auth: AuthenticationStrategy,
        downstream_protocol: DownstreamProtocol,
        model_name: String,
        is_stream: bool,
    ) -> Self {
        Self::new(
            auth,
            ProxyOperation::Generation(GenerationOperation {
                downstream_protocol,
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

    pub(super) fn list_models(downstream_protocol: DownstreamProtocol) -> Self {
        Self::new(
            AuthenticationStrategy::for_downstream_protocol(downstream_protocol),
            ProxyOperation::Models(ModelsOperation {
                downstream_protocol,
            }),
        )
    }

    pub(super) async fn execute(
        self,
        app_state: Arc<AppState>,
        query_params: HashMap<String, String>,
        request: Request<Body>,
    ) -> Result<Response<Body>, ProxyError> {
        let request_context = request
            .extensions()
            .get::<Arc<ProxyRequestContext>>()
            .cloned()
            .ok_or_else(|| {
                ProxyError::gateway(
                    ProxyErrorCode::ServerError,
                    ExecutionStage::Receive,
                    ResponseVisibility::NotVisible,
                    None,
                    "proxy request context unavailable",
                )
            })?;
        let client_ip_addr = request
            .extensions()
            .get::<ClientIdentity>()
            .map(|identity| identity.client_ip.to_string())
            .ok_or_else(|| {
                ProxyError::gateway(
                    ProxyErrorCode::ServerError,
                    ExecutionStage::Receive,
                    ResponseVisibility::NotVisible,
                    None,
                    "client identity unavailable",
                )
            })?;
        let original_headers = request.headers().clone();

        let api_key = self
            .authenticate(&app_state, &original_headers, &query_params)
            .await?;
        let context = ProxyPipelineContext {
            app_state,
            api_key,
            query_params,
            original_headers,
            client_ip_addr: Some(client_ip_addr),
            request_context,
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
                    context.api_key,
                    operation.downstream_protocol,
                    context.request_context,
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
        }?;

        Ok(result.api_key)
    }
}

impl AuthenticationStrategy {
    fn for_downstream_protocol(downstream_protocol: DownstreamProtocol) -> Self {
        match downstream_protocol {
            DownstreamProtocol::Openai | DownstreamProtocol::Responses => Self::OpenaiCompatible,
            DownstreamProtocol::Anthropic => Self::Anthropic,
            DownstreamProtocol::Gemini => Self::Gemini,
        }
    }
}

async fn execute_generation_operation(
    context: ProxyPipelineContext,
    cancellation: ProxyCancellationContext,
    operation: GenerationOperation,
    request: Request<Body>,
) -> Result<Response<Body>, ProxyError> {
    let max_body_size = context.app_state.max_body_size;
    let parsed_request = parse_json_request(request, max_body_size).await?;
    let requested_model = resolve_model_source(&operation.model_source, &parsed_request.data)?;
    let is_stream = resolve_stream_mode(operation.stream_mode, &parsed_request.data);
    let execution_plan = build_execution_plan(
        &context.app_state,
        &requested_model,
        operation.downstream_protocol,
    )
    .await
    .map_err(|error| {
        crate::debug_event!(
            "proxy.execution_plan_build_failed",
            requested_model = &requested_model,
            error = &error,
        );
        execution_plan_build_error(error)
    })?;
    log_execution_plan(&requested_model, &execution_plan);

    execute_generation_proxy(
        context.app_state,
        GenerationExecutionInput {
            cancellation,
            api_key: context.api_key,
            downstream_protocol: operation.downstream_protocol,
            execution_plan,
            is_stream,
            query_params: context.query_params,
            original_headers: context.original_headers,
            client_ip_addr: context.client_ip_addr,
            request_context: context.request_context,
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
    let max_body_size = context.app_state.max_body_size;
    let parsed_request = parse_json_request(request, max_body_size).await?;
    let requested_model = resolve_model_source(&operation.model_source, &parsed_request.data)?;
    let execution_plan = build_execution_plan(
        &context.app_state,
        &requested_model,
        operation.operation.downstream_protocol,
    )
    .await
    .map_err(|error| {
        crate::debug_event!(
            "proxy.execution_plan_build_failed",
            requested_model = &requested_model,
            error = &error,
        );
        execution_plan_build_error(error)
    })?;
    log_execution_plan(&requested_model, &execution_plan);

    execute_utility_proxy(
        context.app_state,
        UtilityExecutionInput {
            cancellation,
            api_key: context.api_key,
            operation: operation.operation,
            execution_plan,
            query_params: context.query_params,
            original_headers: context.original_headers,
            client_ip_addr: context.client_ip_addr,
            request_context: context.request_context,
            parsed_request,
        },
    )
    .await
}

fn log_execution_plan(requested_model: &str, execution_plan: &ExecutionPlan) {
    let target = &execution_plan.target;
    crate::debug_event!(
        "proxy.execution_plan_selected",
        requested_model = requested_model,
        provider_id = target.provider.id,
        provider_key = &target.provider.provider_key,
        model_id = target.model.id,
        source_id = target.upstream_source.id,
        source_profile_type = format!("{:?}", target.upstream_source.profile_type),
        downstream_protocol = format!("{:?}", target.downstream_protocol),
        upstream_protocol = format!("{:?}", target.upstream_protocol),
        selection_reason = target.selection_reason.as_key(),
    );
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

fn execution_plan_build_error(error: ExecutionPlanBuildError) -> ProxyError {
    let message = error.to_string();
    let (code, stage, client_diagnostic) = match error {
        ExecutionPlanBuildError::InvalidModelFormat(_) => (
            ProxyErrorCode::InvalidRequestError,
            ExecutionStage::Parse,
            Some(message.clone()),
        ),
        ExecutionPlanBuildError::TargetNotFound(_)
        | ExecutionPlanBuildError::UnsupportedCapability(_) => (
            ProxyErrorCode::UnsupportedCapabilityError,
            ExecutionStage::Capability,
            Some(message.clone()),
        ),
        ExecutionPlanBuildError::ProviderConfiguration(_) => (
            ProxyErrorCode::ProviderConfigurationError,
            ExecutionStage::Capability,
            Some(message.clone()),
        ),
        ExecutionPlanBuildError::CatalogUnavailable(_) => (
            ProxyErrorCode::ServerError,
            ExecutionStage::Capability,
            None,
        ),
    };
    ProxyError::gateway(
        code,
        stage,
        ResponseVisibility::NotVisible,
        client_diagnostic,
        message,
    )
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
    use super::{
        ExecutionPlanBuildError, ModelSource, StreamMode, execution_plan_build_error,
        resolve_model_source, resolve_stream_mode,
    };
    use crate::proxy::{ExecutionStage, ProxyErrorCode};
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
        let error = resolve_model_source(&ModelSource::RequestBodyField, &data).unwrap_err();
        assert_eq!(error.code(), ProxyErrorCode::InvalidRequestError);
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
    fn execution_plan_errors_preserve_stable_error_classification() {
        for (resolver_error, expected_code, expected_stage) in [
            (
                ExecutionPlanBuildError::InvalidModelFormat("invalid model".to_string()),
                ProxyErrorCode::InvalidRequestError,
                ExecutionStage::Parse,
            ),
            (
                ExecutionPlanBuildError::TargetNotFound("missing target".to_string()),
                ProxyErrorCode::UnsupportedCapabilityError,
                ExecutionStage::Capability,
            ),
            (
                ExecutionPlanBuildError::UnsupportedCapability("unsupported reasoning".to_string()),
                ProxyErrorCode::UnsupportedCapabilityError,
                ExecutionStage::Capability,
            ),
            (
                ExecutionPlanBuildError::ProviderConfiguration("invalid source".to_string()),
                ProxyErrorCode::ProviderConfigurationError,
                ExecutionStage::Capability,
            ),
            (
                ExecutionPlanBuildError::CatalogUnavailable("catalog offline".to_string()),
                ProxyErrorCode::ServerError,
                ExecutionStage::Capability,
            ),
        ] {
            let error = execution_plan_build_error(resolver_error);
            assert_eq!(error.code(), expected_code);
            assert_eq!(error.stage(), expected_stage);
        }
    }
}
