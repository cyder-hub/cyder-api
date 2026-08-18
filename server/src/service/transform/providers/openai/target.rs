use serde_json::{Map, Value};

use crate::schema::enum_def::UpstreamProfileType;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenAiTargetValidationError {
    pub path: &'static str,
    pub reason: &'static str,
}

fn invalid(path: &'static str, reason: &'static str) -> Result<(), OpenAiTargetValidationError> {
    Err(OpenAiTargetValidationError { path, reason })
}

fn require_object<'a>(
    value: &'a Value,
    path: &'static str,
) -> Result<&'a Map<String, Value>, OpenAiTargetValidationError> {
    value.as_object().ok_or(OpenAiTargetValidationError {
        path,
        reason: "must be an object",
    })
}

fn require_closed_object<'a>(
    value: &'a Value,
    path: &'static str,
    allowed_fields: &[&str],
) -> Result<&'a Map<String, Value>, OpenAiTargetValidationError> {
    let object = require_object(value, path)?;
    if object
        .keys()
        .any(|field| !allowed_fields.contains(&field.as_str()))
    {
        return Err(OpenAiTargetValidationError {
            path,
            reason: "contains an unregistered field for GEMINI_OPENAI",
        });
    }
    Ok(object)
}

fn require_non_empty_string(
    value: Option<&Value>,
    path: &'static str,
) -> Result<(), OpenAiTargetValidationError> {
    if value
        .and_then(Value::as_str)
        .is_some_and(|value| !value.is_empty())
    {
        Ok(())
    } else {
        invalid(path, "must be a non-empty string")
    }
}

fn require_string_value(
    value: Option<&Value>,
    path: &'static str,
    allowed: &[&str],
) -> Result<(), OpenAiTargetValidationError> {
    if value
        .and_then(Value::as_str)
        .is_some_and(|value| allowed.contains(&value))
    {
        Ok(())
    } else {
        invalid(path, "must be a registered value")
    }
}

fn require_optional(
    object: &Map<String, Value>,
    field: &str,
    path: &'static str,
    predicate: fn(&Value) -> bool,
    reason: &'static str,
) -> Result<(), OpenAiTargetValidationError> {
    if object.get(field).is_none_or(predicate) {
        Ok(())
    } else {
        invalid(path, reason)
    }
}

fn require_nullable_optional(
    object: &Map<String, Value>,
    field: &str,
    path: &'static str,
    predicate: fn(&Value) -> bool,
    reason: &'static str,
) -> Result<(), OpenAiTargetValidationError> {
    if object
        .get(field)
        .is_none_or(|value| value.is_null() || predicate(value))
    {
        Ok(())
    } else {
        invalid(path, reason)
    }
}

fn is_integer(value: &Value) -> bool {
    value.as_i64().is_some() || value.as_u64().is_some()
}

fn is_number(value: &Value) -> bool {
    value.is_number()
}

fn is_string(value: &Value) -> bool {
    value.is_string()
}

fn is_boolean(value: &Value) -> bool {
    value.is_boolean()
}

fn is_object(value: &Value) -> bool {
    value.is_object()
}

fn is_array(value: &Value) -> bool {
    value.is_array()
}

fn is_string_or_object(value: &Value) -> bool {
    value.is_string() || value.is_object()
}

fn is_string_or_string_array_or_null(value: &Value) -> bool {
    value.is_null()
        || value.is_string()
        || value
            .as_array()
            .is_some_and(|values| values.iter().all(Value::is_string))
}

fn validate_openai_core(data: &Value) -> Result<&Map<String, Value>, OpenAiTargetValidationError> {
    let object = require_object(data, "$")?;
    require_non_empty_string(object.get("model"), "/model")?;
    if !object.get("messages").is_some_and(Value::is_array) {
        return Err(OpenAiTargetValidationError {
            path: "/messages",
            reason: "must be an array",
        });
    }
    validate_stream_fields(object)?;
    Ok(object)
}

fn validate_stream_fields(object: &Map<String, Value>) -> Result<(), OpenAiTargetValidationError> {
    require_nullable_optional(object, "stream", "/stream", is_boolean, "must be a boolean")?;
    let Some(stream_options) = object
        .get("stream_options")
        .filter(|value| !value.is_null())
    else {
        return Ok(());
    };
    let stream_options = require_object(stream_options, "/stream_options")?;
    require_nullable_optional(
        stream_options,
        "include_usage",
        "/stream_options/include_usage",
        is_boolean,
        "must be a boolean",
    )
}

fn validate_known_message_fields(messages: &Value) -> Result<(), OpenAiTargetValidationError> {
    for message in messages
        .as_array()
        .expect("OpenAI core validation requires messages to be an array")
    {
        let message = require_object(message, "/messages/*")?;
        require_string_value(
            message.get("role"),
            "/messages/*/role",
            &[
                "developer",
                "system",
                "user",
                "assistant",
                "tool",
                "function",
            ],
        )?;
        if let Some(content) = message.get("content") {
            validate_known_content(content)?;
        }
        require_nullable_optional(
            message,
            "name",
            "/messages/*/name",
            is_string,
            "must be a string",
        )?;
        require_nullable_optional(
            message,
            "tool_call_id",
            "/messages/*/tool_call_id",
            is_string,
            "must be a string",
        )?;
        require_nullable_optional(
            message,
            "tool_calls",
            "/messages/*/tool_calls",
            is_array,
            "must be an array",
        )?;
    }
    Ok(())
}

fn validate_known_content(content: &Value) -> Result<(), OpenAiTargetValidationError> {
    if content.is_string() || content.is_null() {
        return Ok(());
    }
    let Some(parts) = content.as_array() else {
        return invalid("/messages/*/content", "must be a string, array, or null");
    };
    for part in parts {
        let part = require_object(part, "/messages/*/content/*")?;
        let Some(kind) = part.get("type").and_then(Value::as_str) else {
            return invalid("/messages/*/content/*/type", "must be a non-empty string");
        };
        match kind {
            "text" => require_non_empty_string(part.get("text"), "/messages/*/content/*/text")?,
            "image_url" => {
                let image_url = part.get("image_url").and_then(Value::as_object);
                if !image_url
                    .and_then(|image| image.get("url"))
                    .and_then(Value::as_str)
                    .is_some_and(crate::service::transform::media::is_valid_image_reference)
                {
                    return invalid("/messages/*/content/*/image_url", "must be an object");
                }
            }
            "input_audio" => {
                let input_audio = part.get("input_audio").and_then(Value::as_object);
                if !input_audio
                    .and_then(|audio| audio.get("data"))
                    .and_then(Value::as_str)
                    .is_some_and(crate::service::transform::media::is_valid_base64)
                    || !matches!(
                        input_audio
                            .and_then(|audio| audio.get("format"))
                            .and_then(Value::as_str),
                        Some("wav" | "mp3")
                    )
                {
                    return invalid("/messages/*/content/*/input_audio", "must be an object");
                }
            }
            "file" => {
                let Some(file) = part.get("file").and_then(Value::as_object) else {
                    return invalid("/messages/*/content/*/file", "must be an object");
                };
                let file_data = file.get("file_data").filter(|value| !value.is_null());
                let file_id = file.get("file_id").filter(|value| !value.is_null());
                if file_data.is_some() == file_id.is_some()
                    || file_data.is_some_and(|data| {
                        !data
                            .as_str()
                            .is_some_and(crate::service::transform::media::is_valid_base64)
                            || !file
                                .get("filename")
                                .and_then(Value::as_str)
                                .is_some_and(|filename| !filename.trim().is_empty())
                    })
                    || file_id
                        .is_some_and(|id| !id.as_str().is_some_and(|id| !id.trim().is_empty()))
                {
                    return invalid(
                        "/messages/*/content/*/file",
                        "must contain exactly one valid file_data or file_id",
                    );
                }
            }
            "refusal" => {
                require_non_empty_string(part.get("refusal"), "/messages/*/content/*/refusal")?
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_official_openai_fields(
    object: &Map<String, Value>,
) -> Result<(), OpenAiTargetValidationError> {
    validate_known_message_fields(
        object
            .get("messages")
            .expect("OpenAI core validation requires messages"),
    )?;

    for (field, path) in [
        ("frequency_penalty", "/frequency_penalty"),
        ("presence_penalty", "/presence_penalty"),
        ("temperature", "/temperature"),
        ("top_p", "/top_p"),
    ] {
        require_nullable_optional(object, field, path, is_number, "must be a number")?;
    }
    for (field, path) in [
        ("max_completion_tokens", "/max_completion_tokens"),
        ("max_tokens", "/max_tokens"),
        ("n", "/n"),
        ("seed", "/seed"),
        ("top_logprobs", "/top_logprobs"),
    ] {
        require_nullable_optional(object, field, path, is_integer, "must be an integer")?;
    }
    for (field, path) in [
        ("logprobs", "/logprobs"),
        ("parallel_tool_calls", "/parallel_tool_calls"),
        ("store", "/store"),
    ] {
        require_nullable_optional(object, field, path, is_boolean, "must be a boolean")?;
    }
    for (field, path) in [
        ("reasoning_effort", "/reasoning_effort"),
        ("service_tier", "/service_tier"),
        ("user", "/user"),
        ("verbosity", "/verbosity"),
    ] {
        require_nullable_optional(object, field, path, is_string, "must be a string")?;
    }
    for (field, path) in [
        ("audio", "/audio"),
        ("logit_bias", "/logit_bias"),
        ("metadata", "/metadata"),
        ("prediction", "/prediction"),
        ("response_format", "/response_format"),
        ("web_search_options", "/web_search_options"),
    ] {
        require_nullable_optional(object, field, path, is_object, "must be an object")?;
    }
    for (field, path) in [
        ("functions", "/functions"),
        ("modalities", "/modalities"),
        ("tools", "/tools"),
    ] {
        require_nullable_optional(object, field, path, is_array, "must be an array")?;
    }
    require_nullable_optional(
        object,
        "function_call",
        "/function_call",
        is_string_or_object,
        "must be a string or object",
    )?;
    require_nullable_optional(
        object,
        "tool_choice",
        "/tool_choice",
        is_string_or_object,
        "must be a string or object",
    )?;
    require_nullable_optional(
        object,
        "stop",
        "/stop",
        is_string_or_string_array_or_null,
        "must be a string, string array, or null",
    )?;
    if let Some(response_format) = object
        .get("response_format")
        .filter(|value| !value.is_null())
    {
        validate_official_openai_response_format(response_format)?;
    }
    Ok(())
}

fn validate_official_openai_response_format(
    value: &Value,
) -> Result<(), OpenAiTargetValidationError> {
    let response_format = require_object(value, "/response_format")?;
    let Some(kind) = response_format.get("type").and_then(Value::as_str) else {
        return invalid(
            "/response_format/type",
            "must be a registered response format",
        );
    };
    if !matches!(kind, "text" | "json_object" | "json_schema") {
        return invalid(
            "/response_format/type",
            "must be a registered response format",
        );
    }
    match (kind, response_format.get("json_schema")) {
        ("json_schema", Some(json_schema)) => {
            let json_schema = require_object(json_schema, "/response_format/json_schema")?;
            require_non_empty_string(json_schema.get("name"), "/response_format/json_schema/name")?;
            if !json_schema
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(crate::service::transform::structured::is_valid_schema_name)
            {
                return invalid(
                    "/response_format/json_schema/name",
                    "must be a portable schema name",
                );
            }
            require_nullable_optional(
                json_schema,
                "description",
                "/response_format/json_schema/description",
                is_string,
                "must be a string",
            )?;
            require_optional(
                json_schema,
                "schema",
                "/response_format/json_schema/schema",
                is_object,
                "must be an object",
            )?;
            if !json_schema.contains_key("schema") {
                return invalid("/response_format/json_schema/schema", "must be an object");
            }
            require_nullable_optional(
                json_schema,
                "strict",
                "/response_format/json_schema/strict",
                is_boolean,
                "must be a boolean",
            )
        }
        ("json_schema", None) => invalid(
            "/response_format/json_schema",
            "must be an object for json_schema",
        ),
        (_, Some(_)) => invalid(
            "/response_format/json_schema",
            "is only valid for json_schema",
        ),
        (_, None) => Ok(()),
    }
}

const GEMINI_CHAT_FIELDS: &[&str] = &[
    "model",
    "messages",
    "max_completion_tokens",
    "max_tokens",
    "n",
    "frequency_penalty",
    "presence_penalty",
    "reasoning_effort",
    "response_format",
    "seed",
    "service_tier",
    "stop",
    "stream",
    "stream_options",
    "temperature",
    "top_p",
    "tools",
    "tool_choice",
    "function_call",
    "functions",
    "extra_body",
];

fn validate_gemini_message_content(content: &Value) -> Result<(), OpenAiTargetValidationError> {
    if content.is_string() || content.is_null() {
        return Ok(());
    }
    let Some(parts) = content.as_array() else {
        return invalid("/messages/*/content", "must be a string, array, or null");
    };
    for part in parts {
        let part_object = require_object(part, "/messages/*/content/*")?;
        let Some(kind) = part_object.get("type").and_then(Value::as_str) else {
            return invalid(
                "/messages/*/content/*/type",
                "must be a registered content type",
            );
        };
        match kind {
            "text" => {
                let part_object =
                    require_closed_object(part, "/messages/*/content/*", &["type", "text"])?;
                require_non_empty_string(part_object.get("text"), "/messages/*/content/*/text")?;
            }
            "image_url" => {
                let part_object =
                    require_closed_object(part, "/messages/*/content/*", &["type", "image_url"])?;
                let image_url = require_closed_object(
                    part_object.get("image_url").unwrap_or(&Value::Null),
                    "/messages/*/content/*/image_url",
                    &["url", "detail"],
                )?;
                require_non_empty_string(
                    image_url.get("url"),
                    "/messages/*/content/*/image_url/url",
                )?;
                if !image_url
                    .get("url")
                    .and_then(Value::as_str)
                    .is_some_and(crate::service::transform::media::is_valid_image_reference)
                {
                    return invalid(
                        "/messages/*/content/*/image_url/url",
                        "must be an HTTP(S) URL or image data URL",
                    );
                }
                require_optional(
                    image_url,
                    "detail",
                    "/messages/*/content/*/image_url/detail",
                    is_string,
                    "must be a string",
                )?;
            }
            "input_audio" => {
                let part_object =
                    require_closed_object(part, "/messages/*/content/*", &["type", "input_audio"])?;
                let input_audio = require_closed_object(
                    part_object.get("input_audio").unwrap_or(&Value::Null),
                    "/messages/*/content/*/input_audio",
                    &["data", "format"],
                )?;
                require_non_empty_string(
                    input_audio.get("data"),
                    "/messages/*/content/*/input_audio/data",
                )?;
                if !input_audio
                    .get("data")
                    .and_then(Value::as_str)
                    .is_some_and(crate::service::transform::media::is_valid_base64)
                {
                    return invalid(
                        "/messages/*/content/*/input_audio/data",
                        "must be valid base64",
                    );
                }
                require_non_empty_string(
                    input_audio.get("format"),
                    "/messages/*/content/*/input_audio/format",
                )?;
                require_string_value(
                    input_audio.get("format"),
                    "/messages/*/content/*/input_audio/format",
                    &["wav", "mp3"],
                )?;
            }
            _ => {
                return invalid(
                    "/messages/*/content/*/type",
                    "must be a registered GEMINI_OPENAI Chat content type",
                );
            }
        }
    }
    Ok(())
}

fn validate_gemini_tool_calls(value: &Value) -> Result<(), OpenAiTargetValidationError> {
    let Some(tool_calls) = value.as_array() else {
        return invalid("/messages/*/tool_calls", "must be an array");
    };
    for tool_call in tool_calls {
        let tool_call = require_closed_object(
            tool_call,
            "/messages/*/tool_calls/*",
            &["id", "type", "function"],
        )?;
        require_non_empty_string(tool_call.get("id"), "/messages/*/tool_calls/*/id")?;
        require_string_value(
            tool_call.get("type"),
            "/messages/*/tool_calls/*/type",
            &["function"],
        )?;
        let function = require_closed_object(
            tool_call.get("function").unwrap_or(&Value::Null),
            "/messages/*/tool_calls/*/function",
            &["name", "arguments"],
        )?;
        require_non_empty_string(
            function.get("name"),
            "/messages/*/tool_calls/*/function/name",
        )?;
        require_non_empty_string(
            function.get("arguments"),
            "/messages/*/tool_calls/*/function/arguments",
        )?;
    }
    Ok(())
}

fn validate_gemini_messages(messages: &Value) -> Result<(), OpenAiTargetValidationError> {
    for message in messages
        .as_array()
        .expect("OpenAI core validation requires messages to be an array")
    {
        let message = require_closed_object(
            message,
            "/messages/*",
            &[
                "role",
                "content",
                "name",
                "tool_calls",
                "tool_call_id",
                "function_call",
                "refusal",
                "audio",
            ],
        )?;
        require_string_value(
            message.get("role"),
            "/messages/*/role",
            &[
                "developer",
                "system",
                "user",
                "assistant",
                "tool",
                "function",
            ],
        )?;
        if let Some(content) = message.get("content") {
            validate_gemini_message_content(content)?;
        }
        require_optional(
            message,
            "name",
            "/messages/*/name",
            is_string,
            "must be a string",
        )?;
        require_optional(
            message,
            "tool_call_id",
            "/messages/*/tool_call_id",
            is_string,
            "must be a string",
        )?;
        require_optional(
            message,
            "refusal",
            "/messages/*/refusal",
            |value| value.is_string() || value.is_null(),
            "must be a string or null",
        )?;
        if let Some(tool_calls) = message.get("tool_calls") {
            validate_gemini_tool_calls(tool_calls)?;
        }
        if let Some(function_call) = message.get("function_call") {
            let function_call = require_closed_object(
                function_call,
                "/messages/*/function_call",
                &["name", "arguments"],
            )?;
            require_non_empty_string(function_call.get("name"), "/messages/*/function_call/name")?;
            require_non_empty_string(
                function_call.get("arguments"),
                "/messages/*/function_call/arguments",
            )?;
        }
        if let Some(audio) = message.get("audio") {
            if !audio.is_null() {
                let audio = require_closed_object(audio, "/messages/*/audio", &["id"])?;
                require_non_empty_string(audio.get("id"), "/messages/*/audio/id")?;
            }
        }
    }
    Ok(())
}

fn validate_gemini_tools(value: &Value) -> Result<(), OpenAiTargetValidationError> {
    let Some(tools) = value.as_array() else {
        return invalid("/tools", "must be an array");
    };
    for tool in tools {
        let tool = require_closed_object(tool, "/tools/*", &["type", "function"])?;
        require_string_value(tool.get("type"), "/tools/*/type", &["function"])?;
        let function = require_closed_object(
            tool.get("function").unwrap_or(&Value::Null),
            "/tools/*/function",
            &["name", "description", "parameters", "strict"],
        )?;
        require_non_empty_string(function.get("name"), "/tools/*/function/name")?;
        require_optional(
            function,
            "description",
            "/tools/*/function/description",
            is_string,
            "must be a string",
        )?;
        require_optional(
            function,
            "parameters",
            "/tools/*/function/parameters",
            is_object,
            "must be an object",
        )?;
        require_optional(
            function,
            "strict",
            "/tools/*/function/strict",
            is_boolean,
            "must be a boolean",
        )?;
    }
    Ok(())
}

fn validate_gemini_functions(value: &Value) -> Result<(), OpenAiTargetValidationError> {
    let Some(functions) = value.as_array() else {
        return invalid("/functions", "must be an array");
    };
    for function in functions {
        let function = require_closed_object(
            function,
            "/functions/*",
            &["name", "description", "parameters", "strict"],
        )?;
        require_non_empty_string(function.get("name"), "/functions/*/name")?;
        require_optional(
            function,
            "description",
            "/functions/*/description",
            is_string,
            "must be a string",
        )?;
        require_optional(
            function,
            "parameters",
            "/functions/*/parameters",
            is_object,
            "must be an object",
        )?;
        require_optional(
            function,
            "strict",
            "/functions/*/strict",
            is_boolean,
            "must be a boolean",
        )?;
    }
    Ok(())
}

fn validate_gemini_function_selection(
    value: &Value,
    path: &'static str,
    function_path: &'static str,
    name_path: &'static str,
) -> Result<(), OpenAiTargetValidationError> {
    if value.is_string() {
        return Ok(());
    }
    let selection = require_closed_object(value, path, &["type", "function", "name"])?;
    if let Some(kind) = selection.get("type") {
        require_string_value(Some(kind), path, &["function"])?;
    }
    if let Some(function) = selection.get("function") {
        let function = require_closed_object(function, function_path, &["name"])?;
        require_non_empty_string(function.get("name"), name_path)?;
    }
    require_optional(selection, "name", name_path, is_string, "must be a string")?;
    if !selection.contains_key("function") && !selection.contains_key("name") {
        return invalid(path, "must select a function by name");
    }
    Ok(())
}

fn validate_gemini_response_format(value: &Value) -> Result<(), OpenAiTargetValidationError> {
    let response_format =
        require_closed_object(value, "/response_format", &["type", "json_schema"])?;
    require_string_value(
        response_format.get("type"),
        "/response_format/type",
        &["text", "json_object", "json_schema"],
    )?;
    let kind = response_format
        .get("type")
        .and_then(Value::as_str)
        .expect("response format type is validated above");
    if kind == "json_schema" && !response_format.contains_key("json_schema") {
        return invalid(
            "/response_format/json_schema",
            "must be an object for json_schema",
        );
    }
    if kind != "json_schema" && response_format.contains_key("json_schema") {
        return invalid(
            "/response_format/json_schema",
            "is only valid for json_schema",
        );
    }
    if let Some(json_schema) = response_format.get("json_schema") {
        let json_schema = require_closed_object(
            json_schema,
            "/response_format/json_schema",
            &["name", "description", "schema", "strict"],
        )?;
        require_non_empty_string(json_schema.get("name"), "/response_format/json_schema/name")?;
        if !json_schema
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(crate::service::transform::structured::is_valid_schema_name)
        {
            return invalid(
                "/response_format/json_schema/name",
                "must be a portable schema name",
            );
        }
        require_optional(
            json_schema,
            "description",
            "/response_format/json_schema/description",
            is_string,
            "must be a string",
        )?;
        require_optional(
            json_schema,
            "schema",
            "/response_format/json_schema/schema",
            is_object,
            "must be an object",
        )?;
        if !json_schema.contains_key("schema") {
            return invalid("/response_format/json_schema/schema", "must be an object");
        }
        require_optional(
            json_schema,
            "strict",
            "/response_format/json_schema/strict",
            is_boolean,
            "must be a boolean",
        )?;
    }
    Ok(())
}

fn validate_gemini_extra_body(value: &Value) -> Result<bool, OpenAiTargetValidationError> {
    const NON_CHAT_FIELDS: &[&str] = &[
        "aspect_ratio",
        "generation_config",
        "safety_settings",
        "tools",
        "resolution",
        "duration_seconds",
        "frame_rate",
        "input_reference",
        "extend_video_id",
        "negative_prompt",
        "seed",
        "style",
        "person_generation",
        "reference_images",
        "image",
        "last_frame",
    ];

    let extra_body = require_object(value, "/extra_body")?;
    if extra_body
        .keys()
        .any(|field| NON_CHAT_FIELDS.contains(&field.as_str()))
    {
        return Err(OpenAiTargetValidationError {
            path: "/extra_body",
            reason: "image and video generation fields are not valid for Chat",
        });
    }
    if extra_body.keys().any(|field| field != "google") {
        return Err(OpenAiTargetValidationError {
            path: "/extra_body",
            reason: "contains an unregistered field for GEMINI_OPENAI",
        });
    }
    let Some(google) = extra_body.get("google") else {
        return Ok(false);
    };
    let google = require_object(google, "/extra_body/google")?;
    if google
        .keys()
        .any(|field| NON_CHAT_FIELDS.contains(&field.as_str()))
    {
        return Err(OpenAiTargetValidationError {
            path: "/extra_body/google",
            reason: "image and video generation fields are not valid for Chat",
        });
    }
    if google
        .keys()
        .any(|field| !["cached_content", "thinking_config"].contains(&field.as_str()))
    {
        return Err(OpenAiTargetValidationError {
            path: "/extra_body/google",
            reason: "contains an unregistered field for GEMINI_OPENAI",
        });
    }
    require_optional(
        google,
        "cached_content",
        "/extra_body/google/cached_content",
        is_string,
        "must be a string",
    )?;
    let Some(thinking_config) = google.get("thinking_config") else {
        return Ok(false);
    };
    let thinking_config = require_closed_object(
        thinking_config,
        "/extra_body/google/thinking_config",
        &["thinking_level", "thinking_budget", "include_thoughts"],
    )?;
    require_optional(
        thinking_config,
        "thinking_level",
        "/extra_body/google/thinking_config/thinking_level",
        is_string,
        "must be a string",
    )?;
    require_optional(
        thinking_config,
        "thinking_budget",
        "/extra_body/google/thinking_config/thinking_budget",
        is_integer,
        "must be an integer",
    )?;
    require_optional(
        thinking_config,
        "include_thoughts",
        "/extra_body/google/thinking_config/include_thoughts",
        is_boolean,
        "must be a boolean",
    )?;
    Ok(true)
}

fn validate_gemini_openai(data: &Value) -> Result<(), OpenAiTargetValidationError> {
    let object = require_closed_object(data, "$", GEMINI_CHAT_FIELDS)?;
    require_non_empty_string(object.get("model"), "/model")?;
    let Some(messages) = object.get("messages").filter(|value| value.is_array()) else {
        return invalid("/messages", "must be an array");
    };
    validate_stream_fields(object)?;
    if let Some(stream_options) = object.get("stream_options") {
        require_closed_object(stream_options, "/stream_options", &["include_usage"])?;
    }
    validate_gemini_messages(messages)?;

    for (field, path) in [
        ("frequency_penalty", "/frequency_penalty"),
        ("presence_penalty", "/presence_penalty"),
        ("temperature", "/temperature"),
        ("top_p", "/top_p"),
    ] {
        require_optional(object, field, path, is_number, "must be a number")?;
    }
    for (field, path) in [
        ("max_completion_tokens", "/max_completion_tokens"),
        ("max_tokens", "/max_tokens"),
        ("n", "/n"),
        ("seed", "/seed"),
    ] {
        require_optional(object, field, path, is_integer, "must be an integer")?;
    }
    if let Some(reasoning_effort) = object.get("reasoning_effort") {
        require_string_value(
            Some(reasoning_effort),
            "/reasoning_effort",
            &["none", "minimal", "low", "medium", "high"],
        )?;
    }
    if let Some(service_tier) = object.get("service_tier") {
        require_string_value(
            Some(service_tier),
            "/service_tier",
            &["auto", "default", "standard", "flex", "priority"],
        )?;
    }
    require_optional(
        object,
        "stop",
        "/stop",
        is_string_or_string_array_or_null,
        "must be a string, string array, or null",
    )?;
    if let Some(tools) = object.get("tools") {
        validate_gemini_tools(tools)?;
    }
    if let Some(response_format) = object.get("response_format") {
        validate_gemini_response_format(response_format)?;
    }
    if let Some(tool_choice) = object.get("tool_choice") {
        validate_gemini_function_selection(
            tool_choice,
            "/tool_choice",
            "/tool_choice/function",
            "/tool_choice/function/name",
        )?;
    }
    if let Some(function_call) = object.get("function_call") {
        validate_gemini_function_selection(
            function_call,
            "/function_call",
            "/function_call/function",
            "/function_call/name",
        )?;
    }
    if let Some(functions) = object.get("functions") {
        validate_gemini_functions(functions)?;
    }

    let has_thinking_config = object
        .get("extra_body")
        .map(validate_gemini_extra_body)
        .transpose()?
        .unwrap_or(false);
    if object.contains_key("reasoning_effort") && has_thinking_config {
        return invalid(
            "/extra_body/google/thinking_config",
            "conflicts with /reasoning_effort",
        );
    }
    Ok(())
}

/// Final, borrowed validation boundary for every OpenAI-wire Chat request.
///
/// JSON Schema and function parameter payloads are intentionally opaque. They
/// carry administrator/client-defined structures and are not vendor extension
/// surfaces. No request clone or second Serde decode is performed here.
pub(crate) fn validate_openai_target_request(
    data: &Value,
    profile_type: &UpstreamProfileType,
) -> Result<(), OpenAiTargetValidationError> {
    match profile_type {
        UpstreamProfileType::Openai => {
            let object = validate_openai_core(data)?;
            validate_official_openai_fields(object)
        }
        UpstreamProfileType::OpenaiCompatible => {
            validate_openai_core(data)?;
            Ok(())
        }
        UpstreamProfileType::GeminiOpenai => validate_gemini_openai(data),
        _ => invalid("$", "Profile is not valid for the OpenAI wire protocol"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn base_request() -> Value {
        json!({
            "model": "model",
            "messages": [{"role": "user", "content": "hello"}]
        })
    }

    #[test]
    fn profiles_apply_distinct_unknown_field_policies() {
        let mut request = base_request();
        request["vendor_extension"] = json!({"enabled": true});

        assert!(validate_openai_target_request(&request, &UpstreamProfileType::Openai).is_ok());
        assert!(
            validate_openai_target_request(&request, &UpstreamProfileType::OpenaiCompatible)
                .is_ok()
        );
        let error = validate_openai_target_request(&request, &UpstreamProfileType::GeminiOpenai)
            .expect_err("Gemini compatibility Profile is recursively closed");
        assert_eq!(error.path, "$");
        assert!(!error.reason.contains("vendor_extension"));
    }

    #[test]
    fn official_profile_validates_known_fields_but_passes_unknown_fields() {
        let mut request = base_request();
        request["temperature"] = json!("secret-value");
        request["future_extension"] = json!({"arbitrary": true});

        let error = validate_openai_target_request(&request, &UpstreamProfileType::Openai)
            .expect_err("known official fields retain their type contract");
        assert_eq!(error.path, "/temperature");
        assert!(!format!("{error:?}").contains("secret-value"));
    }

    #[test]
    fn official_profile_treats_null_optional_controls_as_omitted() {
        let mut request = base_request();
        request["messages"][0]["name"] = Value::Null;
        request["temperature"] = Value::Null;
        request["max_completion_tokens"] = Value::Null;
        request["logprobs"] = Value::Null;
        request["reasoning_effort"] = Value::Null;
        request["response_format"] = Value::Null;
        request["tools"] = Value::Null;
        request["tool_choice"] = Value::Null;
        request["stream"] = Value::Null;
        request["stream_options"] = Value::Null;

        assert!(validate_openai_target_request(&request, &UpstreamProfileType::Openai).is_ok());

        request["temperature"] = json!("not-a-number");
        let error = validate_openai_target_request(&request, &UpstreamProfileType::Openai)
            .expect_err("non-null controls must retain their type validation");
        assert_eq!(error.path, "/temperature");
    }

    #[test]
    fn official_profile_rejects_unregistered_or_incomplete_response_formats() {
        let mut grammar = base_request();
        grammar["response_format"] =
            json!({"type":"grammar","grammar":"structured-private-marker"});
        let error = validate_openai_target_request(&grammar, &UpstreamProfileType::Openai)
            .expect_err("custom grammar is not an OpenAI Chat response format");
        assert_eq!(error.path, "/response_format/type");
        assert!(!format!("{error:?}").contains("structured-private-marker"));

        let mut incomplete = base_request();
        incomplete["response_format"] = json!({
            "type":"json_schema",
            "json_schema":{"name":"answer"}
        });
        let error = validate_openai_target_request(&incomplete, &UpstreamProfileType::Openai)
            .expect_err("json_schema requires the schema object");
        assert_eq!(error.path, "/response_format/json_schema/schema");
    }

    #[test]
    fn gemini_profile_accepts_registered_chat_capabilities_and_opaque_schemas() {
        let request = json!({
            "model": "gemini",
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "describe"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AA==", "detail": "auto"}},
                    {"type": "input_audio", "input_audio": {"data": "AA==", "format": "wav"}}
                ]
            }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "lookup",
                    "description": "lookup",
                    "parameters": {"type": "object", "vendor_schema_keyword": {"anything": true}},
                    "strict": true
                }
            }],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "answer",
                    "schema": {"type": "object", "vendor_schema_keyword": true},
                    "strict": true
                }
            },
            "service_tier": "priority",
            "extra_body": {
                "google": {
                    "cached_content": "cachedContents/example",
                    "thinking_config": {"thinking_level": "low", "include_thoughts": true}
                }
            }
        });

        assert!(
            validate_openai_target_request(&request, &UpstreamProfileType::GeminiOpenai).is_ok()
        );
    }

    #[test]
    fn gemini_profile_rejects_recursive_unknown_conflicts_and_generation_fields() {
        let mut unknown = base_request();
        unknown["messages"][0]["content"] =
            json!([{"type": "text", "text": "hello", "payload-secret-marker": true}]);
        let error = validate_openai_target_request(&unknown, &UpstreamProfileType::GeminiOpenai)
            .expect_err("recursive unknown fields must be rejected");
        assert_eq!(error.path, "/messages/*/content/*");
        assert!(!format!("{error:?}").contains("payload-secret-marker"));

        let conflict = json!({
            "model": "gemini",
            "messages": [{"role": "user", "content": "hello"}],
            "reasoning_effort": "low",
            "extra_body": {"google": {"thinking_config": {"thinking_budget": 10}}}
        });
        let error = validate_openai_target_request(&conflict, &UpstreamProfileType::GeminiOpenai)
            .expect_err("overlapping reasoning controls must be rejected");
        assert_eq!(error.path, "/extra_body/google/thinking_config");

        let generation = json!({
            "model": "gemini",
            "messages": [{"role": "user", "content": "hello"}],
            "extra_body": {"google": {"generation_config": {"responseModalities": ["IMAGE"]}}}
        });
        let error = validate_openai_target_request(&generation, &UpstreamProfileType::GeminiOpenai)
            .expect_err("image generation settings are outside the Chat contract");
        assert_eq!(error.path, "/extra_body/google");
    }
}
