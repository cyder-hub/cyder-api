use axum::body::Bytes;
use chrono::Utc;
use cyder_tools::log::{debug, error};
use serde_json::{json, Value};

use crate::utils::ID_GENERATOR;

// Transforms an OpenAI-compatible request body to a Gemini-compatible one.
pub fn transform_request_data_openai_to_gemini(data: Value) -> Value {
    debug!("[transform] Starting OpenAI to Gemini transformation.");

    let mut openai_request = match data {
        Value::Object(map) => map,
        _ => {
            debug!("[transform] Data is not a JSON object, returning as is.");
            return data;
        }
    };

    // 1. Extract messages
    let messages_val = match openai_request.remove("messages") {
        Some(val) => val,
        None => {
            debug!("[transform] 'messages' field not found, returning data as is.");
            return Value::Object(openai_request);
        }
    };

    let messages: Vec<Value> = match serde_json::from_value(messages_val) {
        Ok(m) => m,
        Err(e) => {
            debug!("[transform] 'messages' is not a valid array: {}. Returning data as is.", e);
            // Put it back so the original request can be seen if it fails later
            openai_request.insert("messages".to_string(), json!([]));
            return Value::Object(openai_request);
        }
    };

    let mut gemini_contents: Vec<Value> = Vec::new();
    let mut system_instructions = Vec::new();

    // 2. Process messages into Gemini format
    for msg in messages {
        // For now, we only handle simple string content.
        // TODO: Handle complex content (e.g., image parts).
        if let (Some(role), Some(content)) = (
            msg.get("role").and_then(Value::as_str),
            msg.get("content").and_then(Value::as_str),
        ) {
            if content.is_empty() {
                continue;
            }

            if role == "system" {
                system_instructions.push(content.to_string());
            } else {
                let gemini_role = if role == "assistant" { "model" } else { "user" };

                // Gemini requires alternating user/model roles. Merge consecutive messages of the same role.
                let should_merge = if let Some(last_content) = gemini_contents.last() {
                    last_content.get("role").and_then(Value::as_str) == Some(gemini_role)
                } else {
                    false
                };

                if should_merge {
                    if let Some(last_content) = gemini_contents.last_mut() {
                        if let Some(parts) = last_content.get_mut("parts").and_then(Value::as_array_mut) {
                            if let Some(first_part) = parts.get_mut(0) {
                                if let Some(text_val) = first_part.get_mut("text") {
                                    if let Some(text_str) = text_val.as_str() {
                                        let new_text = format!("{}\n\n{}", text_str, content);
                                        *text_val = Value::String(new_text);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    gemini_contents.push(json!({
                        "role": gemini_role,
                        "parts": [{ "text": content }]
                    }));
                }
            }
        }
    }

    // 3. Create the new Gemini request body
    let mut gemini_request = serde_json::Map::new();
    gemini_request.insert("contents".to_string(), json!(gemini_contents));

    // 4. Handle system instructions
    if !system_instructions.is_empty() {
        let combined_instructions = system_instructions.join("\n\n");
        gemini_request.insert("system_instruction".to_string(), json!({
            "parts": [{ "text": combined_instructions }]
        }));
    }

    // 5. Map generation config from remaining OpenAI parameters
    let mut generation_config = serde_json::Map::new();
    if let Some(temp) = openai_request.remove("temperature") {
        generation_config.insert("temperature".to_string(), temp);
    }
    if let Some(max_tokens) = openai_request.remove("max_tokens") {
        generation_config.insert("maxOutputTokens".to_string(), max_tokens);
    }
    if let Some(top_p) = openai_request.remove("top_p") {
        generation_config.insert("topP".to_string(), top_p);
    }
    if let Some(stop) = openai_request.remove("stop") {
        // `stop` can be a string or an array of strings in OpenAI. Gemini wants an array of strings.
        if stop.is_string() {
            generation_config.insert("stopSequences".to_string(), json!([stop]));
        } else if stop.is_array() {
            generation_config.insert("stopSequences".to_string(), stop);
        }
    }
    // Note: other fields like 'n', 'presence_penalty', etc., are ignored as they don't have direct Gemini equivalents.

    if !generation_config.is_empty() {
        gemini_request.insert("generationConfig".to_string(), Value::Object(generation_config));
    }

    let final_request = Value::Object(gemini_request);
    debug!("[transform] OpenAI to Gemini transformation complete. Result: {}", serde_json::to_string(&final_request).unwrap_or_default());
    final_request
}

// Transforms a Gemini-compatible request body to an OpenAI-compatible one.
pub fn transform_request_data_gemini_to_openai(data: Value, is_stream: bool) -> Value {
    debug!("[transform] Starting Gemini to OpenAI transformation.");

    let mut gemini_request = match data {
        Value::Object(map) => map,
        _ => {
            debug!("[transform] Data is not a JSON object, returning as is.");
            return data;
        }
    };

    let mut openai_messages = Vec::new();

    // 1. Handle system instruction first, if it exists.
    if let Some(system_instruction) = gemini_request.remove("system_instruction") {
        if let Some(parts) = system_instruction.get("parts").and_then(Value::as_array) {
            let content = parts
                .iter()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
                .collect::<Vec<&str>>()
                .join("\n");
            if !content.is_empty() {
                openai_messages.push(json!({
                    "role": "system",
                    "content": content
                }));
            }
        }
    }

    // 2. Handle contents
    if let Some(contents_val) = gemini_request.remove("contents") {
        if let Some(contents) = contents_val.as_array() {
            for content_item in contents {
                // The role is optional and defaults to "user" in the Gemini API.
                let role = content_item
                    .get("role")
                    .and_then(Value::as_str)
                    .unwrap_or("user");

                if let Some(parts) = content_item.get("parts").and_then(Value::as_array) {
                    let openai_role = if role == "model" { "assistant" } else { "user" };
                    // For now, we just concatenate text parts.
                    // TODO: Handle multi-modal content if needed.
                    let combined_text = parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<Vec<&str>>()
                        .join("\n");

                    if !combined_text.is_empty() {
                        openai_messages.push(json!({
                            "role": openai_role,
                            "content": combined_text
                        }));
                    }
                }
            }
        }
    }

    // 3. Create the new OpenAI request body
    let mut openai_request = serde_json::Map::new();
    openai_request.insert("messages".to_string(), json!(openai_messages));

    // 4. Map generation config
    if let Some(Value::Object(gen_config)) = gemini_request.remove("generationConfig") {
        if let Some(temp) = gen_config.get("temperature") {
            openai_request.insert("temperature".to_string(), temp.clone());
        }
        if let Some(max_tokens) = gen_config.get("maxOutputTokens") {
            openai_request.insert("max_tokens".to_string(), max_tokens.clone());
        }
        if let Some(top_p) = gen_config.get("topP") {
            openai_request.insert("top_p".to_string(), top_p.clone());
        }
        if let Some(stop) = gen_config.get("stopSequences") {
            // OpenAI's `stop` can be a string or an array. Gemini's is an array.
            // We'll just pass it as-is, assuming the target can handle an array.
            openai_request.insert("stop".to_string(), stop.clone());
        }
    }

    // Note: other fields from the original gemini_request are ignored.

    openai_request.insert("stream".to_string(), json!(is_stream));

    let final_request = Value::Object(openai_request);
    debug!("[transform] Gemini to OpenAI transformation complete. Result: {}", serde_json::to_string(&final_request).unwrap_or_default());
    final_request
}

// Transforms an OpenAI-compatible non-streaming result to a Gemini-compatible one.
pub fn transform_result_openai_to_gemini(data: Value) -> Value {
    debug!("[transform_result] Starting OpenAI to Gemini transformation.");

    let mut openai_response = match data {
        Value::Object(map) => map,
        _ => {
            debug!("[transform_result] Data is not a JSON object, returning as is.");
            return data;
        }
    };

    let mut gemini_response = serde_json::Map::new();

    // 1. Transform choices to candidates
    let gemini_candidates = if let Some(choices_val) = openai_response.remove("choices") {
        if let Some(choices) = choices_val.as_array() {
            choices.iter().map(|choice| {
                let mut candidate = serde_json::Map::new();
                let mut content = serde_json::Map::new();
                let mut parts = Vec::new();

                // message -> content
                if let Some(message) = choice.get("message") {
                    // role
                    let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
                    content.insert("role".to_string(), json!(if role == "assistant" { "model" } else { "user" }));

                    // content string -> parts
                    if let Some(content_str) = message.get("content").and_then(Value::as_str) {
                        parts.push(json!({ "text": content_str }));
                    }
                }
                content.insert("parts".to_string(), json!(parts));
                candidate.insert("content".to_string(), Value::Object(content));

                // index
                candidate.insert("index".to_string(), choice.get("index").cloned().unwrap_or(json!(0)));

                // finish_reason -> finishReason
                let finish_reason = match choice.get("finish_reason").and_then(Value::as_str) {
                    Some("stop") => "STOP",
                    Some("length") => "MAX_TOKENS",
                    Some("content_filter") => "SAFETY",
                    _ => "FINISH_REASON_UNSPECIFIED",
                };
                candidate.insert("finishReason".to_string(), json!(finish_reason));

                // Add placeholder safety ratings as they are expected by some clients
                candidate.insert("safetyRatings".to_string(), json!([
                    { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" },
                    { "category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE" },
                    { "category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE" },
                    { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }
                ]));

                Value::Object(candidate)
            }).collect::<Vec<Value>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    gemini_response.insert("candidates".to_string(), json!(gemini_candidates));

    // 2. Transform usage to usageMetadata
    if let Some(usage_val) = openai_response.remove("usage") {
        if let Some(usage) = usage_val.as_object() {
            let mut usage_metadata = serde_json::Map::new();
            if let Some(prompt_tokens) = usage.get("prompt_tokens") {
                usage_metadata.insert("promptTokenCount".to_string(), prompt_tokens.clone());
            }
            if let Some(completion_tokens) = usage.get("completion_tokens") {
                usage_metadata.insert("candidatesTokenCount".to_string(), completion_tokens.clone());
            }
            if let Some(total_tokens) = usage.get("total_tokens") {
                usage_metadata.insert("totalTokenCount".to_string(), total_tokens.clone());
            }
            gemini_response.insert("usageMetadata".to_string(), Value::Object(usage_metadata));
        }
    }

    let final_response = Value::Object(gemini_response);
    debug!("[transform_result] OpenAI to Gemini transformation complete. Result: {}", serde_json::to_string(&final_response).unwrap_or_default());
    final_response
}

// Transforms a Gemini-compatible non-streaming result to an OpenAI-compatible one.
pub fn transform_result_gemini_to_openai(data: Value) -> Value {
    debug!("[transform_result] Starting Gemini to OpenAI transformation.");

    let mut gemini_response = match data {
        Value::Object(map) => map,
        _ => {
            debug!("[transform_result] Data is not a JSON object, returning as is.");
            return data;
        }
    };

    let mut openai_response = serde_json::Map::new();

    // 1. Add static and simple fields
    openai_response.insert("id".to_string(), json!(format!("chatcmpl-{}", ID_GENERATOR.generate_id())));
    openai_response.insert("object".to_string(), json!("chat.completion"));
    openai_response.insert("created".to_string(), json!(Utc::now().timestamp()));
    // The model name isn't available here, so we use a placeholder.
    // This could be improved by passing the model name down.
    openai_response.insert("model".to_string(), json!("gemini-transformed-model"));

    // 2. Transform candidates to choices
    let openai_choices = if let Some(candidates_val) = gemini_response.remove("candidates") {
        if let Some(candidates) = candidates_val.as_array() {
            candidates.iter().map(|candidate| {
                let mut choice = serde_json::Map::new();
                let mut message = serde_json::Map::new();

                // Content -> message
                if let Some(content) = candidate.get("content") {
                    // Role
                    let role = content.get("role").and_then(Value::as_str).unwrap_or("user");
                    message.insert("role".to_string(), json!(if role == "model" { "assistant" } else { "user" }));

                    // Parts -> content string
                    if let Some(parts) = content.get("parts").and_then(Value::as_array) {
                        let content_str = parts.iter()
                            .filter_map(|part| part.get("text").and_then(Value::as_str))
                            .collect::<Vec<&str>>()
                            .join("");
                        message.insert("content".to_string(), json!(content_str));
                    } else {
                        message.insert("content".to_string(), Value::Null);
                    }
                }
                choice.insert("message".to_string(), Value::Object(message));

                // Index
                choice.insert("index".to_string(), candidate.get("index").cloned().unwrap_or(json!(0)));

                // Finish reason
                let finish_reason = match candidate.get("finishReason").and_then(Value::as_str) {
                    Some("STOP") => "stop",
                    Some("MAX_TOKENS") => "length",
                    Some("SAFETY") | Some("RECITATION") => "content_filter",
                    _ => "stop", // Default to stop
                };
                choice.insert("finish_reason".to_string(), json!(finish_reason));

                Value::Object(choice)
            }).collect::<Vec<Value>>()
        } else {
            vec![]
        }
    } else {
        vec![]
    };
    openai_response.insert("choices".to_string(), json!(openai_choices));

    // 3. Transform usageMetadata to usage
    if let Some(usage_metadata) = gemini_response.remove("usageMetadata") {
        let mut usage = serde_json::Map::new();
        if let Some(prompt_tokens) = usage_metadata.get("promptTokenCount") {
            usage.insert("prompt_tokens".to_string(), prompt_tokens.clone());
        }
        if let Some(completion_tokens) = usage_metadata.get("candidatesTokenCount") {
            usage.insert("completion_tokens".to_string(), completion_tokens.clone());
        }
        if let Some(total_tokens) = usage_metadata.get("totalTokenCount") {
            usage.insert("total_tokens".to_string(), total_tokens.clone());
        }
        openai_response.insert("usage".to_string(), Value::Object(usage));
    }

    let final_response = Value::Object(openai_response);
    debug!("[transform_result] Gemini to OpenAI transformation complete. Result: {}", serde_json::to_string(&final_response).unwrap_or_default());
    final_response
}

// Transforms an OpenAI-compatible streaming chunk to a Gemini-compatible one.
pub fn transform_result_chunk_openai_to_gemini(chunk: Bytes) -> Option<Bytes> {
    debug!("[transform_result_chunk] Starting OpenAI to Gemini transformation for chunk: {:?}", String::from_utf8_lossy(&chunk));

    let line_str = String::from_utf8_lossy(&chunk);

    // Handle the [DONE] marker
    if line_str.trim() == "data: [DONE]" {
        // Gemini stream just ends, so we return None to not send anything.
        return None;
    }

    if !line_str.starts_with("data:") {
        // Not a data line (e.g., empty keep-alive), pass it through.
        return Some(chunk);
    }

    let json_str = line_str.strip_prefix("data:").unwrap().trim();
    if json_str.is_empty() {
        return Some(chunk); // empty data line
    }

    let openai_value: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            error!("[transform_result_chunk] Failed to parse OpenAI chunk JSON: {}. Chunk: '{}'", e, json_str);
            // Return an empty data chunk to avoid breaking client-side parsers.
            return Some(Bytes::from("data: {}"));
        }
    };

    // Handle if the chunk is an OpenAI error response
    if openai_value.get("error").is_some() {
        let gemini_error = json!({
            "error": {
                "code": 500, // Default error code
                "message": openai_value.get("error").and_then(|e| e.get("message")).cloned().unwrap_or(json!("Unknown OpenAI Error")),
                "status": "INTERNAL" // Default status
            }
        });
        let gemini_json_str = serde_json::to_string(&gemini_error).unwrap();
        return Some(Bytes::from(format!("data: {}", gemini_json_str)));
    }

    let mut gemini_response = serde_json::Map::new();
    let mut candidates = Vec::new();

    if let Some(choices) = openai_value.get("choices").and_then(Value::as_array) {
        for choice in choices {
            let mut candidate = serde_json::Map::new();
            let mut content = serde_json::Map::new();
            let mut parts = Vec::new();
            let mut has_content_data = false;

            if let Some(delta) = choice.get("delta") {
                // Role
                if let Some(role) = delta.get("role").and_then(Value::as_str) {
                    content.insert("role".to_string(), json!(if role == "assistant" { "model" } else { "user" }));
                    has_content_data = true;
                }

                // Content
                if let Some(content_str) = delta.get("content").and_then(Value::as_str) {
                    parts.push(json!({ "text": content_str }));
                    has_content_data = true;
                }
            }

            if has_content_data {
                content.insert("parts".to_string(), json!(parts));
                candidate.insert("content".to_string(), Value::Object(content));
            }

            // Index
            candidate.insert("index".to_string(), choice.get("index").cloned().unwrap_or(json!(0)));

            // Finish reason
            if let Some(finish_reason_val) = choice.get("finish_reason") {
                if !finish_reason_val.is_null() {
                    let finish_reason = match finish_reason_val.as_str() {
                        Some("stop") => "STOP",
                        Some("length") => "MAX_TOKENS",
                        Some("content_filter") => "SAFETY",
                        _ => "FINISH_REASON_UNSPECIFIED",
                    };
                    candidate.insert("finishReason".to_string(), json!(finish_reason));

                    // Add placeholder safety ratings as they are expected by some clients
                    candidate.insert("safetyRatings".to_string(), json!([
                        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_HATE_SPEECH", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_HARASSMENT", "probability": "NEGLIGIBLE" },
                        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "probability": "NEGLIGIBLE" }
                    ]));
                }
            }

            // Only add the candidate if it has some content or a finish reason
            if candidate.contains_key("content") || candidate.contains_key("finishReason") {
                candidates.push(Value::Object(candidate));
            }
        }

        // If choices were present but resulted in no valid candidates, ignore the chunk.
        if candidates.is_empty() {
            return None;
        }
    }

    if !candidates.is_empty() {
        gemini_response.insert("candidates".to_string(), json!(candidates));
    }

    // Usage (from the last chunk in some OpenAI implementations)
    if let Some(usage_val) = openai_value.get("usage") {
        if let Some(usage) = usage_val.as_object() {
            let mut usage_metadata = serde_json::Map::new();
            if let Some(prompt_tokens) = usage.get("prompt_tokens") {
                usage_metadata.insert("promptTokenCount".to_string(), prompt_tokens.clone());
            }
            if let Some(completion_tokens) = usage.get("completion_tokens") {
                usage_metadata.insert("candidatesTokenCount".to_string(), completion_tokens.clone());
            }
            if let Some(total_tokens) = usage.get("total_tokens") {
                usage_metadata.insert("totalTokenCount".to_string(), total_tokens.clone());
            }
            if !usage_metadata.is_empty() {
                gemini_response.insert("usageMetadata".to_string(), Value::Object(usage_metadata));
            }
        }
    }

    // If after all processing, the response is empty, don't send anything.
    // This can happen for chunks that only contain a role but no content.
    if gemini_response.is_empty() {
        return None;
    }

    let gemini_json_str = serde_json::to_string(&gemini_response).unwrap();
    Some(Bytes::from(format!("data: {}", gemini_json_str)))
}

// Transforms a Gemini-compatible streaming chunk to an OpenAI-compatible one.
pub fn transform_result_chunk_gemini_to_openai(chunk: Bytes) -> Option<Bytes> {
    debug!("[transform_result_chunk] Starting Gemini to OpenAI transformation for chunk: {:?}", String::from_utf8_lossy(&chunk));

    let line_str = String::from_utf8_lossy(&chunk);
    if !line_str.starts_with("data:") {
        // Not a data line (e.g., empty keep-alive), pass it through.
        return Some(chunk);
    }

    let json_str = line_str.strip_prefix("data:").unwrap().trim();
    if json_str.is_empty() {
        return Some(chunk); // empty data line
    }

    let gemini_value: Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(e) => {
            error!("[transform_result_chunk] Failed to parse Gemini chunk JSON: {}. Chunk: '{}'", e, json_str);
            // Return an empty data chunk to avoid breaking client-side parsers.
            return Some(Bytes::from("data: {}"));
        }
    };

    // Handle if the chunk is a Gemini error response
    if gemini_value.get("error").is_some() {
        let openai_error = json!({
            "error": {
                "message": gemini_value.get("error").and_then(|e| e.get("message")).cloned().unwrap_or(json!("Unknown Gemini Error")),
                "type": "upstream_error",
                "param": null,
                "code": gemini_value.get("error").and_then(|e| e.get("status")).cloned().unwrap_or(json!(null))
            }
        });
        let openai_json_str = serde_json::to_string(&openai_error).unwrap();
        return Some(Bytes::from(format!("data: {}", openai_json_str)));
    }

    let mut openai_chunk = serde_json::Map::new();
    openai_chunk.insert("id".to_string(), json!(format!("chatcmpl-{}", ID_GENERATOR.generate_id())));
    openai_chunk.insert("object".to_string(), json!("chat.completion.chunk"));
    openai_chunk.insert("created".to_string(), json!(Utc::now().timestamp()));
    openai_chunk.insert("model".to_string(), json!("gemini-transformed-model")); // Placeholder

    let mut choices = Vec::new();

    if let Some(candidates) = gemini_value.get("candidates").and_then(Value::as_array) {
        for candidate in candidates {
            let mut choice = serde_json::Map::new();
            let mut delta = serde_json::Map::new();
            let mut has_text_content = false;

            // Content -> delta
            if let Some(content) = candidate.get("content") {
                if let Some(role) = content.get("role").and_then(Value::as_str) {
                    delta.insert("role".to_string(), json!(if role == "model" { "assistant" } else { "user" }));
                }
                if let Some(parts) = content.get("parts").and_then(Value::as_array) {
                    let has_thought_part = parts
                        .iter()
                        .any(|p| p.get("thought").and_then(Value::as_bool).unwrap_or(false));

                    let content_str = parts
                        .iter()
                        .filter_map(|part| part.get("text").and_then(Value::as_str))
                        .collect::<Vec<&str>>()
                        .join("");

                    if !content_str.is_empty() {
                        let key = if has_thought_part {
                            "reasoning_content"
                        } else {
                            "content"
                        };
                        delta.insert(key.to_string(), json!(content_str));
                        has_text_content = true;
                    }
                }
            }
            choice.insert("delta".to_string(), Value::Object(delta));

            // Index
            choice.insert("index".to_string(), candidate.get("index").cloned().unwrap_or(json!(0)));

            // Finish reason
            let finish_reason = match candidate.get("finishReason").and_then(Value::as_str) {
                Some("STOP") => Some("stop"),
                Some("MAX_TOKENS") => Some("length"),
                Some("SAFETY") | Some("RECITATION") => Some("content_filter"),
                _ => None,
            };
            choice.insert("finish_reason".to_string(), json!(finish_reason));

            if has_text_content || finish_reason.is_some() {
                choices.push(Value::Object(choice));
            }
        }
    }

    // If there are no choices with content or finish_reason, and no usage data, skip the chunk.
    if choices.is_empty() && gemini_value.get("usageMetadata").is_none() {
        return None;
    }

    openai_chunk.insert("choices".to_string(), json!(choices));

    // Usage metadata (usually in the last chunk from Gemini)
    if let Some(usage_metadata) = gemini_value.get("usageMetadata") {
        let mut usage = serde_json::Map::new();
        let prompt_tokens_opt = usage_metadata.get("promptTokenCount").and_then(Value::as_i64);
        let total_tokens_opt = usage_metadata.get("totalTokenCount").and_then(Value::as_i64);

        if let Some(prompt_tokens) = prompt_tokens_opt {
            usage.insert("prompt_tokens".to_string(), json!(prompt_tokens));
        }
        if let (Some(prompt_tokens), Some(total_tokens)) = (prompt_tokens_opt, total_tokens_opt) {
            usage.insert("completion_tokens".to_string(), json!(total_tokens - prompt_tokens));
        }
        if let Some(total_tokens) = total_tokens_opt {
            usage.insert("total_tokens".to_string(), json!(total_tokens));
        }

        if !usage.is_empty() {
            openai_chunk.insert("usage".to_string(), Value::Object(usage));
        }
    }

    let openai_json_str = serde_json::to_string(&openai_chunk).unwrap();
    Some(Bytes::from(format!("data: {}", openai_json_str)))
}
