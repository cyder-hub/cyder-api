use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::HeaderMap, response::Response};
use chrono::Utc;
use serde_json::Value;

use crate::{
    proxy::{
        ProxyError,
        auth::{admit_api_key_request, check_access_control},
        cancellation::ProxyCancellationContext,
        provider_governance::{ProviderGovernanceCheckError, ensure_provider_request_allowed},
        request_context::ProxyRequestContext,
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            capability::{validate_generation_capabilities, validate_utility_capabilities},
            log_writer::{
                RequestLogContextInput, finalize_request_failure_context, new_request_log_context,
                record_completion,
            },
            materializer::{
                apply_gateway_request_identity, materialize_generation_request,
                materialize_utility_request,
            },
            request_patch::load_runtime_request_patch_trace,
            route_resolver::{ExecutionPlan, ExecutionTarget},
            transport::{ReasoningContinuationCaptureContext, send_materialized_request},
        },
        util::get_cost_catalog_version,
        utility::{UtilityOperation, validate_utility_target},
    },
    schema::enum_def::DownstreamProtocol,
    service::{
        app_state::AppState,
        cache::types::CacheApiKey,
        provider_credential::resolve_selected_provider_credential,
        provider_http::normalize_provider_endpoint,
        runtime::{ProviderCircuitProbePermit, ReasoningContinuationScope},
    },
};

#[derive(Debug, Clone)]
pub(in crate::proxy) enum RequestExecutionKind {
    Generation {
        downstream_protocol: DownstreamProtocol,
        is_stream: bool,
        data: Value,
    },
    Utility {
        operation: UtilityOperation,
        data: Value,
    },
}

pub(in crate::proxy) struct RequestExecutionInput {
    pub cancellation: ProxyCancellationContext,
    pub api_key: Arc<CacheApiKey>,
    pub execution_plan: ExecutionPlan,
    pub query_params: HashMap<String, String>,
    pub original_headers: HeaderMap,
    pub client_ip_addr: Option<String>,
    pub request_context: Arc<ProxyRequestContext>,
    pub kind: RequestExecutionKind,
}

async fn fail_before_send(
    app_state: &Arc<AppState>,
    mut context: crate::proxy::logging::RequestLogContext,
    error: ProxyError,
) -> Result<Response<Body>, ProxyError> {
    finalize_request_failure_context(&mut context, &error);
    record_completion(app_state, context).await;
    Err(error)
}

async fn allow_provider(
    app_state: &AppState,
    target: &ExecutionTarget,
    provider_label: &str,
) -> Result<Option<ProviderCircuitProbePermit>, ProxyError> {
    match ensure_provider_request_allowed(app_state, target.provider.id, provider_label).await {
        Ok(permit) => Ok(permit),
        Err(ProviderGovernanceCheckError::Rejected(rejection)) => {
            Err(rejection.to_proxy_error(provider_label))
        }
        Err(ProviderGovernanceCheckError::Backend(error)) => Err(error),
    }
}

pub(in crate::proxy) async fn execute_request(
    app_state: Arc<AppState>,
    input: RequestExecutionInput,
) -> Result<Response<Body>, ProxyError> {
    let RequestExecutionInput {
        cancellation,
        api_key,
        execution_plan,
        query_params,
        original_headers,
        client_ip_addr,
        request_context,
        kind,
    } = input;
    let mut target = execution_plan.target.clone();
    let downstream_protocol = match &kind {
        RequestExecutionKind::Generation {
            downstream_protocol,
            ..
        } => *downstream_protocol,
        RequestExecutionKind::Utility { operation, .. } => operation.downstream_protocol,
    };
    let mut log_context = new_request_log_context(RequestLogContextInput {
        api_key: &api_key,
        target: &target,
        requested_model_name: &execution_plan.requested_name,
        base_requested_model_name: &execution_plan.base_requested_name,
        resolved_reasoning_suffix: execution_plan.resolved_reasoning_suffix.as_deref(),
        resolved_reasoning_preset: execution_plan
            .resolved_reasoning_preset
            .map(|preset| preset.as_key()),
        client_ip_addr: &client_ip_addr,
        request_context: &request_context,
        downstream_protocol,
    });

    let capability_result = match &kind {
        RequestExecutionKind::Generation {
            is_stream, data, ..
        } => validate_generation_capabilities(
            &target,
            data,
            *is_stream,
            execution_plan.resolved_reasoning_preset,
        ),
        RequestExecutionKind::Utility { operation, data } => {
            if execution_plan.resolved_reasoning_preset.is_some() {
                Err(ProxyError::BadRequest(format!(
                    "Reasoning suffixes are only supported for generation requests; '{}' is a utility operation.",
                    operation.name
                )))
            } else {
                validate_utility_target(operation, target.upstream_protocol)
                    .and_then(|()| validate_utility_capabilities(&target, &operation.name, data))
            }
        }
    };
    if let Err(error) = capability_result {
        return fail_before_send(&app_state, log_context, error).await;
    }

    if let Err(error) =
        check_access_control(&api_key, &target.provider, &target.model, &app_state).await
    {
        return fail_before_send(&app_state, log_context, error).await;
    }

    let mut normalized_provider = (*target.provider).clone();
    normalized_provider.endpoint = match normalize_provider_endpoint(&target.provider.endpoint) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            return fail_before_send(
                &app_state,
                log_context,
                ProxyError::BadGateway(format!(
                    "Provider endpoint is invalid and must be repaired before use: {error}"
                )),
            )
            .await;
        }
    };
    target.provider = Arc::new(normalized_provider);
    if let Err(error) = app_state
        .infra
        .provider_client(target.provider.use_proxy)
        .await
    {
        return fail_before_send(
            &app_state,
            log_context,
            ProxyError::BadGateway(error.to_string()),
        )
        .await;
    }

    let request_patch_trace = match load_runtime_request_patch_trace(
        &target.provider,
        Some(&target.model),
        Some(&target),
        &app_state,
    )
    .await
    {
        Ok(trace) => trace,
        Err(error) => return fail_before_send(&app_state, log_context, error).await,
    };
    if let Some(error) = request_patch_trace.conflict_error(&target.model.model_name) {
        return fail_before_send(&app_state, log_context, error).await;
    }

    let cost_catalog_version = get_cost_catalog_version(&target.model, &app_state).await;
    let request_lease = match admit_api_key_request(&app_state, &api_key).await {
        Ok(lease) => lease,
        Err(error) => return fail_before_send(&app_state, log_context, error).await,
    };
    let mut request_lease = ApiKeyRequestLeaseFinalizer::new(
        &app_state,
        request_lease,
        request_context.request_id.clone(),
    );

    let provider_credential =
        match resolve_selected_provider_credential(&target.provider, &app_state).await {
            Ok(credential) => credential,
            Err(error) => {
                request_lease.release().await;
                return fail_before_send(
                    &app_state,
                    log_context,
                    ProxyError::InternalError(error.to_string()),
                )
                .await;
            }
        };
    log_context.provider_api_key_id = Some(provider_credential.key_id());

    let mut materialized = match kind {
        RequestExecutionKind::Generation {
            downstream_protocol,
            is_stream,
            data,
        } => {
            match materialize_generation_request(
                &target,
                data,
                downstream_protocol,
                is_stream,
                &original_headers,
                &query_params,
                &request_patch_trace.applied_rules,
                &provider_credential,
                api_key.id,
                app_state.reasoning_continuation_store.as_ref(),
            )
            .await
            {
                Ok(request) => request,
                Err(error) => {
                    request_lease.release().await;
                    return fail_before_send(&app_state, log_context, error).await;
                }
            }
        }
        RequestExecutionKind::Utility { operation, data } => match materialize_utility_request(
            &target,
            &operation,
            data,
            &original_headers,
            &query_params,
            &request_patch_trace.applied_rules,
            &provider_credential,
        )
        .await
        {
            Ok(request) => request,
            Err(error) => {
                request_lease.release().await;
                return fail_before_send(&app_state, log_context, error).await;
            }
        },
    };
    apply_gateway_request_identity(&mut materialized.final_headers, &request_context);

    log_context.request_url = Some(materialized.final_url.clone());
    log_context.llm_request_sent_at = Some(Utc::now().timestamp_millis());

    let provider_permit = match allow_provider(&app_state, &target, &materialized.model_str).await {
        Ok(permit) => permit,
        Err(error) => {
            request_lease.release().await;
            return fail_before_send(&app_state, log_context, error).await;
        }
    };
    let reasoning_capture = Some(ReasoningContinuationCaptureContext {
        scope: ReasoningContinuationScope {
            api_key_id: api_key.id,
            provider_id: target.provider.id,
            model_id: target.model.id,
        },
        feature_enabled: target
            .runtime_features
            .openai_reasoning_content_repair_enabled,
    });

    match send_materialized_request(
        Arc::clone(&app_state),
        cancellation,
        log_context,
        materialized.final_url,
        materialized.final_body,
        materialized.final_headers,
        materialized.model_str,
        target.provider.use_proxy,
        cost_catalog_version,
        request_lease,
        provider_permit,
        materialized.response_mode,
        reasoning_capture,
    )
    .await
    {
        Ok(outcome) => {
            if !outcome.log_context.is_stream {
                record_completion(&app_state, outcome.log_context).await;
            }
            Ok(outcome.response)
        }
        Err(mut failure) => {
            finalize_request_failure_context(&mut failure.log_context, &failure.error);
            record_completion(&app_state, failure.log_context).await;
            Err(failure.error)
        }
    }
}
