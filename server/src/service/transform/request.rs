use serde_json::Value;

use super::adapter::{adapter_for, noop_finalize_request};
use super::capability::TransformValueKind;
use super::diagnostics::{capture_transform_diagnostics, json_value_log_summary};
use super::unified::{UnifiedRequest, UnifiedTransformDiagnostic};
use super::{TransformProtocol, apply_transform_policy};
use crate::schema::enum_def::{LlmApiType, ProviderType};

pub(in crate::service::transform) fn apply_stream_options(data: &mut Value) {
    let is_stream = data.get("stream").and_then(Value::as_bool).unwrap_or(false);
    if !is_stream {
        return;
    }

    if let Some(stream_options) = data.get_mut("stream_options") {
        if let Some(include_usage) = stream_options.get_mut("include_usage") {
            *include_usage = Value::Bool(true);
        } else {
            stream_options["include_usage"] = Value::Bool(true);
        }
    } else {
        data["stream_options"] = serde_json::json!({ "include_usage": true });
    }
}

pub(in crate::service::transform) fn finalize_request_data(
    data: Value,
    target_api_type: LlmApiType,
    provider_type: &ProviderType,
    downstream_path: &str,
) -> Value {
    let adapter = adapter_for(target_api_type);
    let finalize = adapter.request.finalize.unwrap_or(noop_finalize_request);
    finalize(data, provider_type, downstream_path)
}

pub(in crate::service::transform) fn transform_request_data(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Value {
    transform_request_data_with_diagnostics(data, api_type, target_api_type, is_stream).value
}

#[derive(Debug, Clone)]
pub struct RequestTransformOutput {
    pub value: Value,
    pub diagnostics: Vec<UnifiedTransformDiagnostic>,
}

pub(in crate::service::transform) fn transform_request_data_with_diagnostics(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> RequestTransformOutput {
    let (value, diagnostics) = capture_transform_diagnostics(|| {
        transform_request_data_inner(data, api_type, target_api_type, is_stream)
    });
    RequestTransformOutput { value, diagnostics }
}

fn transform_request_data_inner(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Value {
    if api_type == target_api_type {
        return data;
    }

    let (request_body_bytes, request_body_sha256, json_top_level_fields) =
        json_value_log_summary(&data);
    crate::debug_event!(
        "transform.request_reencode_started",
        source_api = format!("{api_type:?}"),
        target_api = format!("{target_api_type:?}"),
        request_body_bytes = request_body_bytes,
        request_body_sha256 = request_body_sha256,
        json_top_level_fields = json_top_level_fields,
    );

    let source_adapter = adapter_for(api_type);
    let target_adapter = adapter_for(target_api_type);

    let mut unified_request: UnifiedRequest = match (source_adapter.request.decode)(data.clone()) {
        Ok(payload) => payload,
        Err(e) => {
            crate::error_event!(
                "transform.request_decode_failed",
                source_api = source_adapter.name,
                target_api = target_adapter.name,
                error = e,
            );
            return data;
        }
    };

    // The `is_stream` from the request URL is the source of truth.
    unified_request.stream = is_stream;

    // Warn if top_k is used with non-Anthropic targets
    if unified_request.top_k().is_some() && target_api_type != LlmApiType::Anthropic {
        apply_transform_policy(
            TransformProtocol::Api(api_type),
            TransformProtocol::Api(target_api_type),
            TransformValueKind::TopKParameter,
            "Dropping unsupported request field during UnifiedRequest serialization.",
        );
    }

    // Warn if tools are used with Ollama
    if unified_request.tools.is_some() && target_api_type == LlmApiType::Ollama {
        apply_transform_policy(
            TransformProtocol::Api(api_type),
            TransformProtocol::Api(target_api_type),
            TransformValueKind::ToolDefinitions,
            "Dropping unsupported tool definitions during UnifiedRequest serialization.",
        );
    }

    let target_payload_result = (target_adapter.request.encode)(unified_request);

    match target_payload_result {
        Ok(value) => {
            let (request_body_bytes, request_body_sha256, json_top_level_fields) =
                json_value_log_summary(&value);
            crate::debug_event!(
                "transform.request_reencode_completed",
                source_api = source_adapter.name,
                target_api = target_adapter.name,
                request_body_bytes = request_body_bytes,
                request_body_sha256 = request_body_sha256,
                json_top_level_fields = json_top_level_fields,
            );
            value
        }
        Err(e) => {
            crate::error_event!(
                "transform.request_encode_failed",
                source_api = source_adapter.name,
                target_api = target_adapter.name,
                error = e,
            );
            data
        }
    }
}
