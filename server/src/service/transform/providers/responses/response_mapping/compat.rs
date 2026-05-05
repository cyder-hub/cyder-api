use serde_json::Value;

use crate::service::transform::unified::*;

use super::super::payload::*;

pub(in crate::service::transform::providers::responses) fn convert_openai_tool_choice_to_responses(
    value: Value,
) -> Option<ToolChoice> {
    match value {
        Value::String(value) => match value.as_str() {
            "none" => Some(ToolChoice::Value(ToolChoiceValue::None)),
            "auto" => Some(ToolChoice::Value(ToolChoiceValue::Auto)),
            "required" => Some(ToolChoice::Value(ToolChoiceValue::Required)),
            _ => None,
        },
        other => serde_json::from_value(other).ok(),
    }
}

pub(in crate::service::transform::providers::responses) fn convert_openai_response_format_to_responses(
    value: Value,
) -> Option<TextResponseFormat> {
    match value {
        Value::Object(map) => match map.get("type").and_then(Value::as_str) {
            Some("json_object") => Some(TextResponseFormat::JsonObject),
            Some("json_schema") => {
                let schema = map.get("json_schema")?;
                Some(TextResponseFormat::JsonSchema {
                    name: schema.get("name")?.as_str()?.to_string(),
                    description: schema
                        .get("description")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    schema: schema.get("schema").cloned(),
                    strict: schema
                        .get("strict")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            }
            Some("text") => Some(TextResponseFormat::Text),
            _ => serde_json::from_value(Value::Object(map)).ok(),
        },
        other => serde_json::from_value(other).ok(),
    }
}

pub(in crate::service::transform::providers::responses) fn convert_openai_passthrough_to_responses_reasoning(
    value: &Value,
) -> Option<Reasoning> {
    let effort = value.get("reasoning_effort")?;
    Some(Reasoning {
        effort: serde_json::from_value(effort.clone()).ok(),
        summary: None,
    })
}

pub(in crate::service::transform::providers::responses) fn parse_function_arguments(
    arguments: &str,
) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

pub(in crate::service::transform::providers::responses) fn stringify_function_arguments(
    arguments: Value,
) -> String {
    match arguments {
        Value::String(value) => value,
        other => serde_json::to_string(&other).unwrap_or_default(),
    }
}

pub(in crate::service::transform::providers::responses) fn function_output_payload_to_unified(
    output: FunctionCallOutputPayload,
) -> UnifiedToolResultOutput {
    match output {
        FunctionCallOutputPayload::Text(text) => UnifiedToolResultOutput::Text { text },
        FunctionCallOutputPayload::Content(parts) => UnifiedToolResultOutput::Content {
            parts: parts
                .into_iter()
                .map(|part| match part {
                    FunctionCallOutputContent::Text { text } => {
                        UnifiedToolResultPart::Text { text }
                    }
                    FunctionCallOutputContent::File { filename, file_url } => {
                        UnifiedToolResultPart::File { filename, file_url }
                    }
                    FunctionCallOutputContent::Image {
                        image_url,
                        file_url,
                    } => UnifiedToolResultPart::Image {
                        image_url,
                        file_url,
                    },
                    FunctionCallOutputContent::Unknown(value) => {
                        UnifiedToolResultPart::Json { value }
                    }
                })
                .collect(),
        },
        FunctionCallOutputPayload::Unknown(value) => unified_tool_result_output_from_value(value),
    }
}

pub(in crate::service::transform::providers::responses) fn unified_tool_result_to_function_output_payload(
    output: UnifiedToolResultOutput,
) -> FunctionCallOutputPayload {
    match output {
        UnifiedToolResultOutput::Text { text } => FunctionCallOutputPayload::Text(text),
        UnifiedToolResultOutput::Content { parts } => FunctionCallOutputPayload::Content(
            parts
                .into_iter()
                .map(|part| match part {
                    UnifiedToolResultPart::Text { text } => {
                        FunctionCallOutputContent::Text { text }
                    }
                    UnifiedToolResultPart::File { filename, file_url } => {
                        FunctionCallOutputContent::File { filename, file_url }
                    }
                    UnifiedToolResultPart::Image {
                        image_url,
                        file_url,
                    } => FunctionCallOutputContent::Image {
                        image_url,
                        file_url,
                    },
                    UnifiedToolResultPart::Json { value } => {
                        FunctionCallOutputContent::Unknown(value)
                    }
                })
                .collect(),
        ),
        UnifiedToolResultOutput::File { filename, file_url } => {
            FunctionCallOutputPayload::Content(vec![FunctionCallOutputContent::File {
                filename,
                file_url,
            }])
        }
        UnifiedToolResultOutput::Image {
            image_url,
            file_url,
        } => FunctionCallOutputPayload::Content(vec![FunctionCallOutputContent::Image {
            image_url,
            file_url,
        }]),
        UnifiedToolResultOutput::Json { value } => FunctionCallOutputPayload::Unknown(value),
    }
}

pub(in crate::service::transform::providers::responses) fn build_data_url(
    mime_type: &str,
    data: &str,
) -> String {
    format!("data:{mime_type};base64,{data}")
}

pub(in crate::service::transform::providers::responses) fn render_executable_code_text(
    language: &str,
    code: &str,
) -> String {
    format!("```{language}\n{code}\n```")
}

pub(in crate::service::transform::providers::responses) fn render_responses_file_reference_text(
    url: &str,
    mime_type: Option<&str>,
    filename: Option<&str>,
) -> String {
    let mut lines = vec![format!("file_url: {url}")];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    if let Some(mime_type) = mime_type.filter(|value| !value.is_empty()) {
        lines.push(format!("mime_type: {mime_type}"));
    }
    lines.join("\n")
}

pub(in crate::service::transform::providers::responses) fn render_responses_inline_file_data_text(
    data: &str,
    mime_type: &str,
    filename: Option<&str>,
) -> String {
    let mut lines = vec![
        format!("file_data: {data}"),
        format!("mime_type: {mime_type}"),
    ];
    if let Some(filename) = filename.filter(|value| !value.is_empty()) {
        lines.push(format!("filename: {filename}"));
    }
    lines.join("\n")
}

pub(in crate::service::transform::providers::responses) fn parse_responses_input_file_data(
    file_data: &str,
    filename: Option<String>,
) -> UnifiedContentPart {
    if let Some(rest) = file_data.strip_prefix("data:") {
        let mut split = rest.splitn(2, ';');
        let mime_type = split.next().unwrap_or("application/octet-stream");
        if let Some(payload) = split.next().and_then(|value| value.strip_prefix("base64,")) {
            return UnifiedContentPart::FileData {
                data: payload.to_string(),
                mime_type: mime_type.to_string(),
                filename,
            };
        }
    }

    UnifiedContentPart::FileData {
        data: file_data.to_string(),
        mime_type: "application/octet-stream".to_string(),
        filename,
    }
}

pub(in crate::service::transform::providers::responses) fn render_responses_instruction_part(
    part: UnifiedContentPart,
) -> Option<String> {
    match part {
        UnifiedContentPart::Text { text }
        | UnifiedContentPart::Refusal { text }
        | UnifiedContentPart::Reasoning { text } => Some(text),
        UnifiedContentPart::ImageUrl { url, detail } => Some(match detail {
            Some(detail) if !detail.is_empty() => format!("image_url: {url}\ndetail: {detail}"),
            _ => format!("image_url: {url}"),
        }),
        UnifiedContentPart::ImageData { mime_type, data } => {
            Some(build_data_url(&mime_type, &data))
        }
        UnifiedContentPart::FileUrl {
            url,
            mime_type,
            filename,
        } => Some(render_responses_file_reference_text(
            &url,
            mime_type.as_deref(),
            filename.as_deref(),
        )),
        UnifiedContentPart::FileData {
            data,
            mime_type,
            filename,
        } => Some(render_responses_inline_file_data_text(
            &data,
            &mime_type,
            filename.as_deref(),
        )),
        UnifiedContentPart::ExecutableCode { language, code } => {
            Some(render_executable_code_text(&language, &code))
        }
        UnifiedContentPart::ToolCall(call) => Some(format!(
            "tool_call: {}\narguments: {}",
            call.name,
            serde_json::to_string(&call.arguments).unwrap_or_default()
        )),
        UnifiedContentPart::ToolResult(result) => Some(match result.name {
            Some(ref name) if !name.is_empty() => format!(
                "tool_result: {name}\ntool_call_id: {}\ncontent: {}",
                result.tool_call_id,
                result.legacy_content()
            ),
            _ => format!(
                "tool_result_id: {}\ncontent: {}",
                result.tool_call_id,
                result.legacy_content()
            ),
        }),
    }
}
