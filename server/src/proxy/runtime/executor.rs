use std::{collections::HashMap, sync::Arc};

use axum::{body::Body, http::HeaderMap, response::Response};
use serde_json::Value;

use crate::{
    proxy::{
        ExecutionStage, ProxyError, ProxyErrorCode, ResponseVisibility,
        auth::{admit_api_key_request, check_access_control},
        cancellation::ProxyCancellationContext,
        request_context::ProxyRequestContext,
        runtime::{
            api_key_lease::ApiKeyRequestLeaseFinalizer,
            log_writer::{
                RequestLogContextInput, finalize_request_failure_context, new_request_log_context,
                record_completion,
            },
            materializer::{
                apply_gateway_request_identity, materialize_generation_request,
                materialize_utility_request,
            },
            request_patch::resolve_runtime_request_patch_trace,
            route_resolver::ExecutionPlan,
            transport::send_materialized_request,
        },
        util::get_cost_catalog_version,
        utility::{UtilityOperation, validate_utility_target},
    },
    schema::enum_def::DownstreamProtocol,
    service::{
        app_state::AppState,
        cache::types::CacheApiKey,
        provider_credential::{ProviderCredentialError, resolve_selected_provider_credential},
        provider_http::normalize_provider_endpoint,
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

fn provider_credential_proxy_error(error: ProviderCredentialError) -> ProxyError {
    let code = match error {
        ProviderCredentialError::RuntimeStateUnavailable => ProxyErrorCode::ServerError,
        ProviderCredentialError::NoEnabledCredential
        | ProviderCredentialError::CredentialUnavailable
        | ProviderCredentialError::VertexTokenUnavailable
        | ProviderCredentialError::ProxyRequiredButNotConfigured
        | ProviderCredentialError::UnsupportedProtocol
        | ProviderCredentialError::InvalidAuthHeader => ProxyErrorCode::ProviderConfigurationError,
    };
    ProxyError::gateway(
        code,
        ExecutionStage::Governance,
        ResponseVisibility::NotVisible,
        None,
        error.to_string(),
    )
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
        } => {
            debug_assert_eq!(*downstream_protocol, target.downstream_protocol);
            target.downstream_protocol
        }
        RequestExecutionKind::Utility { operation, .. } => {
            debug_assert_eq!(operation.downstream_protocol, target.downstream_protocol);
            target.downstream_protocol
        }
    };
    let mut log_context = new_request_log_context(RequestLogContextInput {
        api_key: &api_key,
        target: &target,
        requested_model_name: &execution_plan.requested_name,
        base_requested_model_name: &execution_plan.base_requested_name,
        resolved_patch_suffix: execution_plan.resolved_patch_suffix.as_deref(),
        client_ip_addr: &client_ip_addr,
        request_context: &request_context,
        downstream_protocol,
        selection_reason: target.selection_reason,
    });

    if let Err(error) =
        check_access_control(&api_key, &target.provider, &target.model, &app_state).await
    {
        return fail_before_send(&app_state, log_context, error).await;
    }

    if let RequestExecutionKind::Utility { operation, .. } = &kind {
        if execution_plan.resolved_patch_suffix.is_some() {
            let message = format!(
                "Patch suffixes are only supported for generation requests; '{}' is a utility operation.",
                operation.name
            );
            return fail_before_send(
                &app_state,
                log_context,
                ProxyError::gateway(
                    ProxyErrorCode::UnsupportedCapabilityError,
                    ExecutionStage::Capability,
                    ResponseVisibility::NotVisible,
                    Some(message.clone()),
                    message,
                ),
            )
            .await;
        }
        if let Err(error) = validate_utility_target(operation, target.upstream_protocol) {
            return fail_before_send(&app_state, log_context, error).await;
        }
    }

    let request_patch_trace = resolve_runtime_request_patch_trace(
        &target.upstream_source,
        Some(target.model.id),
        target.requested_patch_suffix.clone(),
        execution_plan.request_patch_variants.as_slice(),
    );
    debug_assert_eq!(request_patch_trace.source_id, target.upstream_source.id);
    debug_assert_eq!(request_patch_trace.model_id, Some(target.model.id));
    debug_assert_eq!(
        request_patch_trace.profile_type,
        target.upstream_source.profile_type
    );
    debug_assert_eq!(
        request_patch_trace.suffix.as_deref(),
        target.requested_patch_suffix.as_deref()
    );
    debug_assert_eq!(
        request_patch_trace.layers.len(),
        if target.requested_patch_suffix.is_some() {
            4
        } else {
            2
        }
    );
    debug_assert!(
        request_patch_trace.explain.len() >= request_patch_trace.applied_rules.len(),
        "frozen Request Patch explain snapshot must cover applied Rules"
    );
    if let Some(error) =
        request_patch_trace.execution_error(&target.provider.provider_key, &target.model.model_name)
    {
        return fail_before_send(&app_state, log_context, error).await;
    }

    let mut normalized_source = (*target.upstream_source).clone();
    normalized_source.endpoint = match normalize_provider_endpoint(&target.upstream_source.endpoint)
    {
        Ok(endpoint) => endpoint,
        Err(error) => {
            return fail_before_send(
                &app_state,
                log_context,
                ProxyError::gateway(
                    ProxyErrorCode::ProviderConfigurationError,
                    ExecutionStage::Materialize,
                    ResponseVisibility::NotVisible,
                    None,
                    format!(
                        "Upstream source endpoint is invalid and must be repaired before use: {error}"
                    ),
                ),
            )
            .await;
        }
    };
    target.upstream_source = Arc::new(normalized_source);
    log_context.set_source_endpoint_snapshot(&target.upstream_source.endpoint);
    if let Err(error) = app_state
        .infra
        .provider_client(target.upstream_source.use_proxy)
        .await
    {
        return fail_before_send(
            &app_state,
            log_context,
            ProxyError::gateway(
                ProxyErrorCode::ProviderConfigurationError,
                ExecutionStage::Materialize,
                ResponseVisibility::NotVisible,
                None,
                error.to_string(),
            ),
        )
        .await;
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

    let provider_credential = match resolve_selected_provider_credential(
        &target.provider,
        &target.upstream_source,
        &app_state,
    )
    .await
    {
        Ok(credential) => credential,
        Err(error) => {
            request_lease.release().await;
            return fail_before_send(
                &app_state,
                log_context,
                provider_credential_proxy_error(error),
            )
            .await;
        }
    };
    log_context.provider_api_key_id = Some(provider_credential.key_id());

    let mut materialized = match kind {
        RequestExecutionKind::Generation {
            is_stream, data, ..
        } => {
            match materialize_generation_request(
                &target,
                data,
                target.downstream_protocol,
                is_stream,
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

    match send_materialized_request(
        Arc::clone(&app_state),
        cancellation,
        log_context,
        materialized.final_url,
        materialized.final_body,
        materialized.final_headers,
        materialized.model_str,
        target.upstream_source.use_proxy,
        cost_catalog_version,
        request_lease,
        materialized.response_mode,
        request_context.response_visibility.clone(),
    )
    .await
    {
        Ok(outcome) => {
            if !outcome.log_context.is_stream && !outcome.log_context.completion_deferred {
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

#[cfg(test)]
mod tests {
    use super::provider_credential_proxy_error;
    use crate::{
        proxy::{ExecutionStage, ProxyErrorCode},
        service::provider_credential::ProviderCredentialError,
    };

    #[test]
    fn credential_runtime_state_outage_is_a_server_error() {
        let error =
            provider_credential_proxy_error(ProviderCredentialError::RuntimeStateUnavailable);

        assert_eq!(error.code(), ProxyErrorCode::ServerError);
        assert_eq!(error.stage(), ExecutionStage::Governance);
    }

    #[test]
    fn unusable_provider_credentials_remain_configuration_errors() {
        for credential_error in [
            ProviderCredentialError::NoEnabledCredential,
            ProviderCredentialError::CredentialUnavailable,
            ProviderCredentialError::VertexTokenUnavailable,
            ProviderCredentialError::ProxyRequiredButNotConfigured,
            ProviderCredentialError::UnsupportedProtocol,
            ProviderCredentialError::InvalidAuthHeader,
        ] {
            let error = provider_credential_proxy_error(credential_error);
            assert_eq!(error.code(), ProxyErrorCode::ProviderConfigurationError);
            assert_eq!(error.stage(), ExecutionStage::Governance);
        }
    }
}
