use serde_json::Value;

use crate::{
    database::reasoning_config::ReasoningPreset,
    proxy::{ProxyError, runtime::route_resolver::ExecutionTarget},
};

pub(in crate::proxy) fn validate_generation_capabilities(
    target: &ExecutionTarget,
    data: &Value,
    is_stream: bool,
    reasoning_preset: Option<ReasoningPreset>,
) -> Result<(), ProxyError> {
    let model = target.model.as_ref();
    let missing = [
        (is_stream && !model.supports_streaming, "streaming"),
        (request_uses_tools(data) && !model.supports_tools, "tools"),
        (
            (request_uses_reasoning(data)
                || reasoning_preset.is_some_and(ReasoningPreset::requires_reasoning))
                && !model.supports_reasoning,
            "reasoning",
        ),
        (
            request_uses_image_input(data) && !model.supports_image_input,
            "image_input",
        ),
    ]
    .into_iter()
    .filter_map(|(is_missing, name)| is_missing.then_some(name))
    .collect::<Vec<_>>();

    validate_missing_capabilities(target, &missing)
}

pub(in crate::proxy) fn validate_utility_capabilities(
    target: &ExecutionTarget,
    operation_name: &str,
    data: &Value,
) -> Result<(), ProxyError> {
    let model = target.model.as_ref();
    let operation_name = operation_name.to_ascii_lowercase();
    let missing = [
        (
            request_uses_image_input(data) && !model.supports_image_input,
            "image_input",
        ),
        (
            operation_name == "embeddings" && !model.supports_embeddings,
            "embeddings",
        ),
        (
            operation_name == "rerank" && !model.supports_rerank,
            "rerank",
        ),
    ]
    .into_iter()
    .filter_map(|(is_missing, name)| is_missing.then_some(name))
    .collect::<Vec<_>>();

    validate_missing_capabilities(target, &missing)
}

fn validate_missing_capabilities(
    target: &ExecutionTarget,
    missing: &[&str],
) -> Result<(), ProxyError> {
    if missing.is_empty() {
        return Ok(());
    }

    Err(ProxyError::BadRequest(format!(
        "Model '{}' does not support the required capabilities: {}",
        target.model.model_name,
        missing.join(", ")
    )))
}

fn request_uses_tools(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            if matches!(key.as_str(), "tools" | "functions") {
                return match value {
                    Value::Array(items) => !items.is_empty(),
                    Value::Object(items) => !items.is_empty(),
                    Value::Null => false,
                    _ => true,
                };
            }
            if matches!(key.as_str(), "tool_choice" | "function_call") {
                return !matches!(value, Value::Null)
                    && value.as_str().is_none_or(|choice| choice != "none");
            }
            request_uses_tools(value)
        }),
        Value::Array(items) => items.iter().any(request_uses_tools),
        _ => false,
    }
}

fn request_uses_reasoning(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            if matches!(
                key.as_str(),
                "reasoning"
                    | "reasoning_effort"
                    | "thinking"
                    | "thinking_config"
                    | "thinkingConfig"
                    | "enable_thinking"
                    | "enableThinking"
                    | "include_reasoning"
                    | "includeReasoning"
            ) {
                return !matches!(value, Value::Null | Value::Bool(false));
            }
            request_uses_reasoning(value)
        }),
        Value::Array(items) => items.iter().any(request_uses_reasoning),
        _ => false,
    }
}

fn request_uses_image_input(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "image" | "image_url" | "input_image"))
                || map.contains_key("image_url")
            {
                return true;
            }

            if map.iter().any(|(key, value)| {
                matches!(key.as_str(), "mime_type" | "mimeType")
                    && value
                        .as_str()
                        .is_some_and(|mime_type| mime_type.starts_with("image/"))
            }) {
                return true;
            }

            map.values().any(request_uses_image_input)
        }
        Value::Array(items) => items.iter().any(request_uses_image_input),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{request_uses_image_input, request_uses_reasoning, request_uses_tools};

    #[test]
    fn detects_nested_generation_requirements() {
        let request = json!({
            "tools": [{"type": "function"}],
            "reasoning_effort": "medium",
            "messages": [{
                "role": "user",
                "content": [{"type": "image_url", "image_url": {"url": "data:image/png;base64,abc"}}]
            }]
        });

        assert!(request_uses_tools(&request));
        assert!(request_uses_reasoning(&request));
        assert!(request_uses_image_input(&request));
    }

    #[test]
    fn disabled_optional_features_are_not_requirements() {
        let request = json!({
            "tools": [],
            "tool_choice": "none",
            "enable_thinking": false
        });

        assert!(!request_uses_tools(&request));
        assert!(!request_uses_reasoning(&request));
        assert!(!request_uses_image_input(&request));
    }
}
