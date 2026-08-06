use serde_json::Value;

pub use super::request::RequestTransformOutput;
pub use super::response::ResponseTransformOutput;
use super::{request, response};
use crate::cost::UsageNormalization;
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
) -> Value {
    request::transform_request_data(data, downstream_protocol, upstream_protocol, is_stream)
}

pub fn transform_request_data_with_diagnostics(
    data: Value,
    downstream_protocol: DownstreamProtocol,
    upstream_protocol: UpstreamProtocol,
    is_stream: bool,
) -> RequestTransformOutput {
    request::transform_request_data_with_diagnostics(
        data,
        downstream_protocol,
        upstream_protocol,
        is_stream,
    )
}

pub fn transform_result(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> (Value, Option<UsageInfo>) {
    response::transform_result(data, upstream_protocol, downstream_protocol)
}

pub fn transform_result_with_cost(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> (Value, Option<UsageInfo>, Option<UsageNormalization>) {
    response::transform_result_with_cost(data, upstream_protocol, downstream_protocol)
}

pub fn transform_result_with_cost_and_diagnostics(
    data: Value,
    upstream_protocol: UpstreamProtocol,
    downstream_protocol: DownstreamProtocol,
) -> ResponseTransformOutput {
    response::transform_result_with_cost_and_diagnostics(
        data,
        upstream_protocol,
        downstream_protocol,
    )
}

#[cfg(test)]
mod tests;
