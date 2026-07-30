use serde_json::Value;

use super::adapter::{downstream_adapter_for, upstream_adapter_for};
use super::diagnostics::{capture_transform_diagnostics, json_value_log_summary};
use super::unified::{UnifiedResponse, UnifiedTransformDiagnostic};
use crate::cost::UsageNormalization;
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProtocol};
use crate::utils::usage::UsageInfo;

pub(in crate::service::transform) fn transform_result(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> (Value, Option<UsageInfo>) {
    let output =
        transform_result_with_cost_and_diagnostics(data, upstream_protocol, downstream_protocol);
    (output.value, output.usage_info)
}

pub(in crate::service::transform) fn transform_result_with_cost(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> (Value, Option<UsageInfo>, Option<UsageNormalization>) {
    let output =
        transform_result_with_cost_and_diagnostics(data, upstream_protocol, downstream_protocol);
    (output.value, output.usage_info, output.usage_normalization)
}

#[derive(Debug, Clone)]
pub struct ResponseTransformOutput {
    pub value: Value,
    pub usage_info: Option<UsageInfo>,
    pub usage_normalization: Option<UsageNormalization>,
    pub diagnostics: Vec<UnifiedTransformDiagnostic>,
}

pub(in crate::service::transform) fn transform_result_with_cost_and_diagnostics(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> ResponseTransformOutput {
    let ((value, usage_info, usage_normalization), diagnostics) =
        capture_transform_diagnostics(|| {
            transform_result_with_cost_inner(data, upstream_protocol, downstream_protocol)
        });

    ResponseTransformOutput {
        value,
        usage_info,
        usage_normalization,
        diagnostics,
    }
}

fn transform_result_with_cost_inner(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> (Value, Option<UsageInfo>, Option<UsageNormalization>) {
    // Step 1: Deserialize to UnifiedResponse. This is now UNCONDITIONAL.
    // This allows us to get usage info from a typed struct.
    let source_adapter = upstream_adapter_for(upstream_protocol);
    let target_adapter = downstream_adapter_for(downstream_protocol);
    let unified_response_result = (source_adapter.response.decode)(data.clone());

    let unified_response: UnifiedResponse = match unified_response_result {
        Ok(ur) => ur,
        Err(e) => {
            crate::error_event!(
                "transform.response_decode_failed",
                source_api = format!("{:?}", source_adapter.protocol),
                target_api = format!("{downstream_protocol:?}"),
                error = e,
            );
            return (data, None, None);
        }
    };

    let usage_info: Option<UsageInfo> = unified_response.usage.clone().map(Into::into);
    let usage_normalization: Option<UsageNormalization> =
        unified_response.usage.as_ref().map(Into::into);

    if matches!(
        (upstream_protocol, downstream_protocol),
        (UpstreamProtocol::Openai, DownstreamProtocol::Openai)
            | (UpstreamProtocol::Responses, DownstreamProtocol::Responses)
            | (UpstreamProtocol::Anthropic, DownstreamProtocol::Anthropic)
            | (UpstreamProtocol::Gemini, DownstreamProtocol::Gemini)
    ) {
        // No transformation needed, return original data and parsed usage.
        return (data, usage_info, usage_normalization);
    }

    let (response_body_bytes, response_body_sha256, json_top_level_fields) =
        json_value_log_summary(&data);
    crate::debug_event!(
        "transform.response_reencode_started",
        source_api = format!("{upstream_protocol:?}"),
        target_api = format!("{downstream_protocol:?}"),
        response_body_bytes = response_body_bytes,
        response_body_sha256 = response_body_sha256,
        json_top_level_fields = json_top_level_fields,
    );

    // Step 2: Serialize from UnifiedResponse to target format
    let target_payload_result = (target_adapter.response.encode)(unified_response);

    match target_payload_result {
        Ok(value) => {
            let (response_body_bytes, response_body_sha256, json_top_level_fields) =
                json_value_log_summary(&value);
            crate::debug_event!(
                "transform.response_reencode_completed",
                source_api = format!("{:?}", source_adapter.protocol),
                target_api = format!("{:?}", target_adapter.protocol),
                response_body_bytes = response_body_bytes,
                response_body_sha256 = response_body_sha256,
                json_top_level_fields = json_top_level_fields,
            );
            (value, usage_info, usage_normalization)
        }
        Err(e) => {
            crate::error_event!(
                "transform.response_encode_failed",
                source_api = format!("{:?}", source_adapter.protocol),
                target_api = format!("{:?}", target_adapter.protocol),
                error = e,
            );
            (data, usage_info, usage_normalization)
        }
    }
}
