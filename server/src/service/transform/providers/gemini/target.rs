use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::service::transform::media::{is_valid_base64, portable_gemini_inline_mime};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GeminiTargetValidationError {
    pub(crate) path: &'static str,
    pub(crate) reason: &'static str,
}

fn invalid(path: &'static str, reason: &'static str) -> Result<(), GeminiTargetValidationError> {
    Err(GeminiTargetValidationError { path, reason })
}

fn validate_inline_data(value: &Value) -> Result<(), GeminiTargetValidationError> {
    let Some(data) = value.as_object() else {
        return invalid("/contents/*/parts/*/inlineData", "must be an object");
    };
    if data.len() != 2 || !data.contains_key("mimeType") || !data.contains_key("data") {
        return invalid(
            "/contents/*/parts/*/inlineData",
            "must contain exactly mimeType and data",
        );
    }
    if !data
        .get("mimeType")
        .and_then(Value::as_str)
        .is_some_and(|value| portable_gemini_inline_mime(value) == Some(value))
    {
        return invalid(
            "/contents/*/parts/*/inlineData/mimeType",
            "must be a canonical portable inline MIME type",
        );
    }
    if !data
        .get("data")
        .and_then(Value::as_str)
        .is_some_and(is_valid_base64)
    {
        return invalid(
            "/contents/*/parts/*/inlineData/data",
            "must be non-empty valid base64",
        );
    }
    Ok(())
}

fn validate_function_value(
    part: &Map<String, Value>,
    field: &'static str,
) -> Result<(), GeminiTargetValidationError> {
    let path = if field == "functionCall" {
        "/contents/*/parts/*/functionCall"
    } else {
        "/contents/*/parts/*/functionResponse"
    };
    let Some(function) = part.get(field).and_then(Value::as_object) else {
        return invalid(path, "must be an object");
    };
    if !function
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty())
    {
        return invalid(path, "must contain a non-empty function name");
    }
    let value_field = if field == "functionCall" {
        "args"
    } else {
        "response"
    };
    if !function.get(value_field).is_some_and(Value::is_object) {
        return invalid(path, "must contain an object value");
    }
    if !function
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(|id| !id.trim().is_empty())
    {
        return invalid(path, "function id must be a non-empty string");
    }
    Ok(())
}

fn validate_part(part: &Value, same_wire: bool) -> Result<(), GeminiTargetValidationError> {
    let Some(part) = part.as_object() else {
        return invalid("/contents/*/parts/*", "must be an object");
    };
    if same_wire {
        return if part.is_empty() {
            invalid("/contents/*/parts/*", "must be a non-empty object")
        } else {
            Ok(())
        };
    }

    let tags = [
        "text",
        "inlineData",
        "functionCall",
        "functionResponse",
        "executableCode",
    ];
    let present = tags
        .into_iter()
        .filter(|field| part.contains_key(*field))
        .collect::<Vec<_>>();
    if present.len() != 1 {
        return invalid(
            "/contents/*/parts/*",
            "must contain exactly one supported semantic Part tag",
        );
    }
    match present[0] {
        "text" => {
            if !part.get("text").is_some_and(Value::is_string) {
                return invalid("/contents/*/parts/*/text", "must be a string");
            }
            if part.get("thought").is_some_and(|value| !value.is_boolean()) {
                return invalid(
                    "/contents/*/parts/*/thought",
                    "must be a boolean when present",
                );
            }
            if part
                .get("thoughtSignature")
                .is_some_and(|value| !value.is_string())
            {
                return invalid(
                    "/contents/*/parts/*/thoughtSignature",
                    "must be a string when present",
                );
            }
        }
        "inlineData" => validate_inline_data(&part["inlineData"])?,
        "functionCall" | "functionResponse" => validate_function_value(part, present[0])?,
        "executableCode" => {
            if !part.get("executableCode").is_some_and(Value::is_object) {
                return invalid("/contents/*/parts/*/executableCode", "must be an object");
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn validate_tool_history(data: &Value) -> Result<(), GeminiTargetValidationError> {
    let mut calls = HashMap::<&str, (&str, bool)>::new();
    for content in data
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        for part in content
            .get("parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(call) = part.get("functionCall") {
                let id = call
                    .get("id")
                    .and_then(Value::as_str)
                    .expect("cross-wire functionCall id was validated");
                let name = call
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("cross-wire functionCall name was validated");
                let has_signature = part
                    .get("thoughtSignature")
                    .and_then(Value::as_str)
                    .is_some_and(|signature| !signature.is_empty());
                if part.get("thoughtSignature").is_some() && !has_signature {
                    return invalid(
                        "/contents/*/parts/*/thoughtSignature",
                        "must be a non-empty string when present",
                    );
                }
                if calls.insert(id, (name, has_signature)).is_some() {
                    return invalid(
                        "/contents/*/parts/*/functionCall/id",
                        "function call ids must be unique",
                    );
                }
            }
            if let Some(result) = part.get("functionResponse") {
                let id = result
                    .get("id")
                    .and_then(Value::as_str)
                    .expect("cross-wire functionResponse id was validated");
                let name = result
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("cross-wire functionResponse name was validated");
                let Some((call_name, has_signature)) = calls.get(id) else {
                    return invalid(
                        "/contents/*/parts/*/functionResponse/id",
                        "must reference a prior function call",
                    );
                };
                if call_name != &name {
                    return invalid(
                        "/contents/*/parts/*/functionResponse/name",
                        "must match the referenced function call name",
                    );
                }
                if !has_signature {
                    return invalid(
                        "/contents/*/parts/*/thoughtSignature",
                        "cross-wire tool history requires a thought signature",
                    );
                }
            }
        }
    }
    Ok(())
}

fn validate_tools(data: &Value, same_wire: bool) -> Result<(), GeminiTargetValidationError> {
    let Some(tools) = data.get("tools") else {
        return Ok(());
    };
    let Some(tools) = tools.as_array() else {
        return invalid("/tools", "must be an array when present");
    };
    if same_wire {
        return Ok(());
    }
    for tool in tools {
        let Some(tool) = tool.as_object() else {
            return invalid("/tools/*", "must be an object");
        };
        let Some(declarations) = tool.get("functionDeclarations").and_then(Value::as_array) else {
            return invalid(
                "/tools/*/functionDeclarations",
                "cross-wire tools must contain function declarations",
            );
        };
        if declarations.is_empty() {
            return invalid("/tools/*/functionDeclarations", "must be a non-empty array");
        }
        for declaration in declarations {
            let Some(declaration) = declaration.as_object() else {
                return invalid("/tools/*/functionDeclarations/*", "must be an object");
            };
            if !declaration
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|name| !name.trim().is_empty())
            {
                return invalid(
                    "/tools/*/functionDeclarations/*/name",
                    "must be a non-empty string",
                );
            }
            if !declaration.get("parameters").is_some_and(Value::is_object) {
                return invalid(
                    "/tools/*/functionDeclarations/*/parameters",
                    "must be an object",
                );
            }
            if declaration
                .get("description")
                .is_some_and(|value| !value.is_string())
            {
                return invalid(
                    "/tools/*/functionDeclarations/*/description",
                    "must be a string when present",
                );
            }
        }
    }
    Ok(())
}

fn validate_tool_config(data: &Value, same_wire: bool) -> Result<(), GeminiTargetValidationError> {
    let Some(tool_config) = data.get("toolConfig") else {
        return Ok(());
    };
    let Some(tool_config) = tool_config.as_object() else {
        return invalid("/toolConfig", "must be an object");
    };
    if same_wire {
        return Ok(());
    }
    let Some(function_config) = tool_config
        .get("functionCallingConfig")
        .and_then(Value::as_object)
    else {
        return invalid("/toolConfig/functionCallingConfig", "must be an object");
    };
    if !function_config
        .get("mode")
        .and_then(Value::as_str)
        .is_some_and(|mode| matches!(mode, "AUTO" | "ANY" | "NONE" | "VALIDATED"))
    {
        return invalid(
            "/toolConfig/functionCallingConfig/mode",
            "must be a supported function calling mode",
        );
    }
    if let Some(names) = function_config.get("allowedFunctionNames") {
        let Some(names) = names.as_array() else {
            return invalid(
                "/toolConfig/functionCallingConfig/allowedFunctionNames",
                "must be an array",
            );
        };
        if names
            .iter()
            .any(|name| !name.as_str().is_some_and(|name| !name.trim().is_empty()))
        {
            return invalid(
                "/toolConfig/functionCallingConfig/allowedFunctionNames",
                "must contain only non-empty strings",
            );
        }
    }
    Ok(())
}

fn validate_content(value: &Value, same_wire: bool) -> Result<(), GeminiTargetValidationError> {
    let Some(content) = value.as_object() else {
        return invalid("/contents/*", "must be an object");
    };
    if content.get("role").is_some_and(|role| {
        !role
            .as_str()
            .is_some_and(|role| same_wire || matches!(role, "user" | "model"))
    }) {
        return invalid("/contents/*/role", "must be user or model");
    }
    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
        return invalid("/contents/*/parts", "must be a non-empty array");
    };
    if parts.is_empty() {
        return invalid("/contents/*/parts", "must be a non-empty array");
    }
    for part in parts {
        if !same_wire
            && content.get("role").and_then(Value::as_str) != Some("user")
            && part.get("inlineData").is_some()
        {
            return invalid(
                "/contents/*/role",
                "cross-wire inline media is allowed only in user content",
            );
        }
        validate_part(part, same_wire)?;
    }
    Ok(())
}

fn validate_generation_config(
    data: &Value,
    same_wire: bool,
) -> Result<(), GeminiTargetValidationError> {
    let Some(config) = data.get("generationConfig") else {
        if same_wire {
            return Ok(());
        }
        return invalid(
            "/generationConfig/candidateCount",
            "cross-wire requests must target exactly one candidate",
        );
    };
    let Some(config) = config.as_object() else {
        return invalid("/generationConfig", "must be an object");
    };
    if same_wire {
        return Ok(());
    }
    if config.get("candidateCount").and_then(Value::as_u64) != Some(1) {
        return invalid(
            "/generationConfig/candidateCount",
            "cross-wire requests must target exactly one candidate",
        );
    }
    for (field, path, minimum, maximum) in [
        ("temperature", "/generationConfig/temperature", 0.0, 2.0),
        ("topP", "/generationConfig/topP", 0.0, 1.0),
        (
            "presencePenalty",
            "/generationConfig/presencePenalty",
            -2.0,
            2.0,
        ),
        (
            "frequencyPenalty",
            "/generationConfig/frequencyPenalty",
            -2.0,
            2.0,
        ),
    ] {
        if let Some(value) = config.get(field).filter(|value| !value.is_null())
            && !value
                .as_f64()
                .is_some_and(|value| value >= minimum && value <= maximum)
        {
            return invalid(path, "must be a number within the supported range");
        }
    }
    if let Some(value) = config
        .get("maxOutputTokens")
        .filter(|value| !value.is_null())
        && !value
            .as_u64()
            .is_some_and(|value| value > 0 && u32::try_from(value).is_ok())
    {
        return invalid(
            "/generationConfig/maxOutputTokens",
            "must be a positive 32-bit integer",
        );
    }
    if let Some(value) = config.get("topK").filter(|value| !value.is_null())
        && !value
            .as_u64()
            .is_some_and(|value| value > 0 && u32::try_from(value).is_ok())
    {
        return invalid(
            "/generationConfig/topK",
            "must be a positive 32-bit integer",
        );
    }
    if let Some(value) = config.get("seed").filter(|value| !value.is_null())
        && !value
            .as_i64()
            .is_some_and(|value| i32::try_from(value).is_ok())
    {
        return invalid("/generationConfig/seed", "must be a signed 32-bit integer");
    }
    if let Some(stops) = config.get("stopSequences").filter(|value| !value.is_null()) {
        let Some(stops) = stops.as_array() else {
            return invalid("/generationConfig/stopSequences", "must be an array");
        };
        if stops.is_empty()
            || stops.len() > 5
            || stops
                .iter()
                .any(|stop| !stop.as_str().is_some_and(|stop| !stop.is_empty()))
        {
            return invalid(
                "/generationConfig/stopSequences",
                "must contain one to five non-empty strings",
            );
        }
    }
    if let Some(thinking) = config.get("thinkingConfig") {
        let Some(thinking) = thinking.as_object() else {
            return invalid("/generationConfig/thinkingConfig", "must be an object");
        };
        if thinking.keys().any(|field| {
            !matches!(
                field.as_str(),
                "thinkingBudget" | "thinkingLevel" | "includeThoughts"
            )
        }) {
            return invalid(
                "/generationConfig/thinkingConfig",
                "contains a field not owned by the cross-wire encoder",
            );
        }
        let budget = thinking.get("thinkingBudget");
        let level = thinking.get("thinkingLevel");
        if budget.is_some() && level.is_some() {
            return invalid(
                "/generationConfig/thinkingConfig",
                "thinkingBudget and thinkingLevel are mutually exclusive",
            );
        }
        if budget.is_none() && level.is_none() {
            return invalid(
                "/generationConfig/thinkingConfig",
                "must contain exactly one reasoning strength form",
            );
        }
        if budget.is_some_and(|budget| {
            !budget
                .as_u64()
                .is_some_and(|budget| u32::try_from(budget).is_ok())
        }) {
            return invalid(
                "/generationConfig/thinkingConfig/thinkingBudget",
                "must be a non-negative 32-bit integer",
            );
        }
        if level.is_some_and(|level| {
            !matches!(level.as_str(), Some("minimal" | "low" | "medium" | "high"))
        }) {
            return invalid(
                "/generationConfig/thinkingConfig/thinkingLevel",
                "must be a registered Gemini reasoning level",
            );
        }
        let include_thoughts = thinking.get("includeThoughts");
        if include_thoughts.is_some_and(|value| !value.is_boolean()) {
            return invalid(
                "/generationConfig/thinkingConfig/includeThoughts",
                "must be a boolean",
            );
        }
        let reasoning_is_enabled = level.is_some()
            || budget
                .and_then(Value::as_u64)
                .is_some_and(|budget| budget > 0);
        if reasoning_is_enabled && include_thoughts.and_then(Value::as_bool) != Some(true) {
            return invalid(
                "/generationConfig/thinkingConfig/includeThoughts",
                "must be true for an explicit non-none reasoning request",
            );
        }
    }
    if config.contains_key("responseSchema") || config.contains_key("responseFormat") {
        return invalid(
            "/generationConfig",
            "cross-wire target must use only responseJsonSchema",
        );
    }
    let response_mime_type = config.get("responseMimeType");
    if response_mime_type.is_some()
        && response_mime_type.and_then(Value::as_str) != Some("application/json")
    {
        return invalid(
            "/generationConfig/responseMimeType",
            "must be application/json when present",
        );
    }
    if let Some(schema) = config.get("responseJsonSchema") {
        if !schema.is_object() {
            return invalid(
                "/generationConfig/responseJsonSchema",
                "must be a JSON object",
            );
        }
        if response_mime_type.and_then(Value::as_str) != Some("application/json") {
            return invalid(
                "/generationConfig/responseMimeType",
                "must be application/json when responseJsonSchema is present",
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_gemini_target_request(
    data: &Value,
    same_wire: bool,
) -> Result<(), GeminiTargetValidationError> {
    if !data.is_object() {
        return invalid("$", "must be a JSON object");
    }
    for owned_target in ["model", "stream"] {
        if data.get(owned_target).is_some() {
            return invalid(
                if owned_target == "model" {
                    "/model"
                } else {
                    "/stream"
                },
                "target is owned by the request path",
            );
        }
    }
    let Some(contents) = data.get("contents").and_then(Value::as_array) else {
        return invalid("/contents", "must be a non-empty array");
    };
    if contents.is_empty() {
        return invalid("/contents", "must be a non-empty array");
    }
    for content in contents {
        validate_content(content, same_wire)?;
    }
    if !same_wire {
        validate_tool_history(data)?;
    }
    if let Some(system) = data.get("systemInstruction") {
        if !system.is_string() {
            validate_content(system, same_wire).map_err(|_| GeminiTargetValidationError {
                path: "/systemInstruction",
                reason: "must be a string or Content object with non-empty Parts",
            })?;
        }
    }
    validate_tools(data, same_wire)?;
    validate_tool_config(data, same_wire)?;
    validate_generation_config(data, same_wire)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn same_wire_preserves_future_fields_and_parts_while_enforcing_core_shape() {
        let request = json!({
            "contents":[{"role":"future-role","parts":[{"futurePart":{"preserve":true}}]}],
            "generationConfig":{"candidateCount":0,"temperature":"provider-owned","futureConfig":true,"thinkingConfig":{"thinkingBudget":-1,"includeThoughts":true,"futureThinking":true}},
            "cachedContent":"cachedContents/native",
            "futureRoot":true
        });
        validate_gemini_target_request(&request, true).expect("same-wire future request");
        for invalid_request in [
            json!({"contents":[]}),
            json!({"contents":[{"parts":[]}]}),
            json!({"contents":[{"parts":["text"]}]}),
            json!({"contents":[{"parts":[{}]}]}),
        ] {
            assert!(validate_gemini_target_request(&invalid_request, true).is_err());
        }
    }

    #[test]
    fn cross_wire_requires_single_candidate_known_parts_and_valid_registered_shapes() {
        let request = json!({
            "contents":[
                {"role":"model","parts":[
                    {"functionCall":{"id":"call-1","name":"weather","args":{}},"thoughtSignature":"opaque-signature"}
                ]},
                {"role":"user","parts":[
                    {"text":"hi"},
                    {"inlineData":{"mimeType":"image/png","data":"aGVsbG8="}},
                    {"functionResponse":{"id":"call-1","name":"weather","response":{"ok":true}}}
                ]}
            ],
            "generationConfig":{
                "candidateCount":1,
                "temperature":2.0,
                "maxOutputTokens":1,
                "topP":1.0,
                "topK":1,
                "seed":2147483647,
                "presencePenalty":-2.0,
                "frequencyPenalty":2.0,
                "stopSequences":["STOP"],
                "thinkingConfig":{"thinkingBudget":1024,"includeThoughts":true}
            }
        });
        validate_gemini_target_request(&request, false).expect("portable target");

        for (request, path) in [
            (
                json!({"contents":[{"role":"tool","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/role",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"futurePart":true}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x","functionCall":{"name":"f","args":{}}}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*",
            ),
            (
                json!({"contents":[{"role":"model","parts":[{"functionCall":{"name":"f","args":{}}}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*/functionCall",
            ),
            (
                json!({"contents":[{"role":"model","parts":[{"functionCall":{"id":"call-1","name":"f","args":{}}}]},{"role":"user","parts":[{"functionResponse":{"id":"call-1","name":"f","response":{}}}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*/thoughtSignature",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"functionResponse":{"id":"orphan","name":"f","response":{}}}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*/functionResponse/id",
            ),
            (
                json!({"contents":[{"role":"model","parts":[{"functionCall":{"id":"duplicate","name":"f","args":{}},"thoughtSignature":"sig"},{"functionCall":{"id":"duplicate","name":"f","args":{}},"thoughtSignature":"sig"}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*/functionCall/id",
            ),
            (
                json!({"contents":[{"role":"model","parts":[{"functionCall":{"id":"call-1","name":"f","args":{}},"thoughtSignature":"sig"}]},{"role":"user","parts":[{"functionResponse":{"id":"call-1","name":"other","response":{}}}]}],"generationConfig":{"candidateCount":1}}),
                "/contents/*/parts/*/functionResponse/name",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}]}),
                "/generationConfig/candidateCount",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":2}}),
                "/generationConfig/candidateCount",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"temperature":2.1}}),
                "/generationConfig/temperature",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"maxOutputTokens":0}}),
                "/generationConfig/maxOutputTokens",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"topP":-0.1}}),
                "/generationConfig/topP",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"topK":0}}),
                "/generationConfig/topK",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"seed":2147483648_i64}}),
                "/generationConfig/seed",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"presencePenalty":-2.1}}),
                "/generationConfig/presencePenalty",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"frequencyPenalty":"invalid"}}),
                "/generationConfig/frequencyPenalty",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"stopSequences":[]}}),
                "/generationConfig/stopSequences",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"responseSchema":{"type":"object"},"responseMimeType":"application/json"}}),
                "/generationConfig",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"responseJsonSchema":[],"responseMimeType":"application/json"}}),
                "/generationConfig/responseJsonSchema",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"responseJsonSchema":{"type":"object"}}}),
                "/generationConfig/responseMimeType",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingBudget":1,"thinkingLevel":"low"}}}),
                "/generationConfig/thinkingConfig",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingBudget":-1}}}),
                "/generationConfig/thinkingConfig/thinkingBudget",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingBudget":4294967296_u64}}}),
                "/generationConfig/thinkingConfig/thinkingBudget",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingLevel":"high"}}}),
                "/generationConfig/thinkingConfig/includeThoughts",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingBudget":1,"includeThoughts":false}}}),
                "/generationConfig/thinkingConfig/includeThoughts",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingLevel":"future"}}}),
                "/generationConfig/thinkingConfig/thinkingLevel",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1,"thinkingConfig":{"thinkingLevel":"high","includeThoughts":true,"futureThinking":true}}}),
                "/generationConfig/thinkingConfig",
            ),
            (
                json!({"model":"path-owned","contents":[{"role":"user","parts":[{"text":"x"}]}],"generationConfig":{"candidateCount":1}}),
                "/model",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"tools":[{"googleSearch":{}}],"generationConfig":{"candidateCount":1}}),
                "/tools/*/functionDeclarations",
            ),
            (
                json!({"contents":[{"role":"user","parts":[{"text":"x"}]}],"tools":[{"functionDeclarations":[{"name":"lookup","parameters":{}}]}],"toolConfig":{"functionCallingConfig":{"mode":"FUTURE"}},"generationConfig":{"candidateCount":1}}),
                "/toolConfig/functionCallingConfig/mode",
            ),
        ] {
            assert_eq!(
                validate_gemini_target_request(&request, false)
                    .expect_err("invalid cross-wire target")
                    .path,
                path
            );
        }
    }
}
