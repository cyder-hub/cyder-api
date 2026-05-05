use serde_json::Value;

use super::adapter::adapter_for;
use super::diagnostics::{capture_transform_diagnostics, json_value_log_summary};
use super::unified::{UnifiedResponse, UnifiedTransformDiagnostic};
use crate::cost::UsageNormalization;
use crate::schema::enum_def::LlmApiType;
use crate::utils::usage::UsageInfo;

pub(in crate::service::transform) fn transform_result(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (Value, Option<UsageInfo>) {
    let output = transform_result_with_cost_and_diagnostics(data, api_type, target_api_type);
    (output.value, output.usage_info)
}

pub(in crate::service::transform) fn transform_result_with_cost(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (Value, Option<UsageInfo>, Option<UsageNormalization>) {
    let output = transform_result_with_cost_and_diagnostics(data, api_type, target_api_type);
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
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> ResponseTransformOutput {
    let ((value, usage_info, usage_normalization), diagnostics) =
        capture_transform_diagnostics(|| {
            transform_result_with_cost_inner(data, api_type, target_api_type)
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
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (Value, Option<UsageInfo>, Option<UsageNormalization>) {
    // Step 1: Deserialize to UnifiedResponse. This is now UNCONDITIONAL.
    // This allows us to get usage info from a typed struct.
    let source_adapter = adapter_for(api_type);
    let target_adapter = adapter_for(target_api_type);
    let unified_response_result = (source_adapter.response.decode)(data.clone());

    let unified_response: UnifiedResponse = match unified_response_result {
        Ok(ur) => ur,
        Err(e) => {
            crate::error_event!(
                "transform.response_decode_failed",
                source_api = format!("{:?}", source_adapter.api_type),
                target_api = format!("{target_api_type:?}"),
                error = e,
            );
            return (data, None, None);
        }
    };

    let usage_info: Option<UsageInfo> = unified_response.usage.clone().map(Into::into);
    let usage_normalization: Option<UsageNormalization> =
        unified_response.usage.as_ref().map(Into::into);

    if api_type == target_api_type {
        // No transformation needed, return original data and parsed usage.
        return (data, usage_info, usage_normalization);
    }

    let (response_body_bytes, response_body_sha256, json_top_level_fields) =
        json_value_log_summary(&data);
    crate::debug_event!(
        "transform.response_reencode_started",
        source_api = format!("{api_type:?}"),
        target_api = format!("{target_api_type:?}"),
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
                source_api = format!("{:?}", source_adapter.api_type),
                target_api = format!("{:?}", target_adapter.api_type),
                response_body_bytes = response_body_bytes,
                response_body_sha256 = response_body_sha256,
                json_top_level_fields = json_top_level_fields,
            );
            (value, usage_info, usage_normalization)
        }
        Err(e) => {
            crate::error_event!(
                "transform.response_encode_failed",
                source_api = format!("{:?}", source_adapter.api_type),
                target_api = format!("{:?}", target_adapter.api_type),
                error = e,
            );
            (data, usage_info, usage_normalization)
        }
    }
}
