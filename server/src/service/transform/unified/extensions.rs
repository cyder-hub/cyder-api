use cyder_tools::log::warn;
use serde_json::{Map, Value};

pub const REGISTERED_PASSTHROUGH_KEYS: &[&str] = &[
    "logprobs",
    "top_logprobs",
    "parallel_tool_calls",
    "reasoning_effort",
];

pub fn is_registered_passthrough_key(key: &str) -> bool {
    REGISTERED_PASSTHROUGH_KEYS.contains(&key)
}

pub fn build_registered_passthrough(
    entries: impl IntoIterator<Item = (String, Value)>,
    context: &str,
) -> Option<Value> {
    let mut passthrough = Map::new();

    for (key, value) in entries {
        if is_registered_passthrough_key(&key) {
            passthrough.insert(key, value);
        } else {
            warn!(
                "[transform][passthrough] rejected_unregistered_key key={} context={} registered_keys={:?}",
                key, context, REGISTERED_PASSTHROUGH_KEYS
            );
        }
    }

    if passthrough.is_empty() {
        None
    } else {
        Some(Value::Object(passthrough))
    }
}

pub fn audit_passthrough_keys(passthrough: &Value, context: &str) {
    let Some(object) = passthrough.as_object() else {
        warn!(
            "[transform][passthrough] non_object_passthrough context={} value_type={}",
            context,
            match passthrough {
                Value::Null => "null",
                Value::Bool(_) => "bool",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            }
        );
        return;
    };

    for key in object.keys() {
        if !is_registered_passthrough_key(key) {
            warn!(
                "[transform][passthrough] encountered_unregistered_key key={} context={} registered_keys={:?}",
                key, context, REGISTERED_PASSTHROUGH_KEYS
            );
        }
    }
}
