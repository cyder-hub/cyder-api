use serde_json::Value;

use super::TransformResult;
pub use super::response::ResponseTransformValue;
use super::{request, response};
use crate::schema::enum_def::{DownstreamProtocol, UpstreamProfileType, UpstreamProtocol};
use crate::utils::usage::UsageInfo;

pub fn finalize_request_data(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    profile_type: &UpstreamProfileType,
    downstream_path: &str,
) -> Value {
    request::finalize_request_data(data, upstream_protocol, profile_type, downstream_path)
}

pub fn transform_request_data(
    data: Value,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> TransformResult<Value> {
    request::transform_request_data(data, downstream_protocol, upstream_protocol, is_stream)
}

pub(crate) fn validate_final_generation_request(
    data: &Value,
    upstream_protocol: UpstreamProtocol,
    profile_type: &UpstreamProfileType,
) -> Result<(), request::FinalRequestValidationError> {
    request::validate_final_generation_request(data, upstream_protocol, profile_type)
}

pub fn transform_result(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> TransformResult<(Value, Option<UsageInfo>)> {
    response::transform_result(data, upstream_protocol, downstream_protocol)
}

pub fn transform_result_with_cost(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> TransformResult<ResponseTransformValue> {
    response::transform_result_with_cost(data, upstream_protocol, downstream_protocol)
}

#[cfg(test)]
mod tests;
