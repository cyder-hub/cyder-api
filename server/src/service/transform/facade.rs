use serde_json::Value;

pub use super::request::RequestTransformOutput;
pub use super::response::ResponseTransformOutput;
use super::{request, response};
use crate::cost::UsageNormalization;
use crate::schema::enum_def::{LlmApiType, ProviderType};
use crate::utils::usage::UsageInfo;

pub fn finalize_request_data(
    data: Value,
    target_api_type: LlmApiType,
    provider_type: &ProviderType,
    downstream_path: &str,
) -> Value {
    request::finalize_request_data(data, target_api_type, provider_type, downstream_path)
}

pub fn transform_request_data(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> Value {
    request::transform_request_data(data, api_type, target_api_type, is_stream)
}

pub fn transform_request_data_with_diagnostics(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
    is_stream: bool,
) -> RequestTransformOutput {
    request::transform_request_data_with_diagnostics(data, api_type, target_api_type, is_stream)
}

pub fn transform_result(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (Value, Option<UsageInfo>) {
    response::transform_result(data, api_type, target_api_type)
}

pub fn transform_result_with_cost(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> (Value, Option<UsageInfo>, Option<UsageNormalization>) {
    response::transform_result_with_cost(data, api_type, target_api_type)
}

pub fn transform_result_with_cost_and_diagnostics(
    data: Value,
    api_type: LlmApiType,
    target_api_type: LlmApiType,
) -> ResponseTransformOutput {
    response::transform_result_with_cost_and_diagnostics(data, api_type, target_api_type)
}

#[cfg(test)]
mod tests;
