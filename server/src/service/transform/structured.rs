use serde_json::Value;
use sha2::{Digest, Sha256};

use super::unified::UnifiedStructuredOutput;

const SYNTHETIC_SCHEMA_NAME_PREFIX: &str = "cyder_schema_";

pub(crate) fn stable_schema_name(schema: &Value) -> String {
    let encoded = serde_json::to_vec(schema)
        .expect("serde_json::Value serialization is structurally infallible");
    let digest = format!("{:x}", Sha256::digest(encoded));
    format!("{SYNTHETIC_SCHEMA_NAME_PREFIX}{}", &digest[..16])
}

pub(crate) fn is_valid_schema_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub(crate) fn schema_contains_property_ordering(schema: &Value) -> bool {
    match schema {
        Value::Array(items) => items.iter().any(schema_contains_property_ordering),
        Value::Object(object) => {
            object.contains_key("propertyOrdering")
                || object.values().any(schema_contains_property_ordering)
        }
        _ => false,
    }
}

pub(crate) fn without_property_ordering(mut schema: Value) -> Value {
    fn visit(value: &mut Value) {
        match value {
            Value::Array(items) => items.iter_mut().for_each(visit),
            Value::Object(object) => {
                object.remove("propertyOrdering");
                object.values_mut().for_each(visit);
            }
            _ => {}
        }
    }

    visit(&mut schema);
    schema
}

pub(crate) fn openai_response_format(output: UnifiedStructuredOutput) -> Value {
    match output {
        UnifiedStructuredOutput::JsonObject => serde_json::json!({"type":"json_object"}),
        UnifiedStructuredOutput::JsonSchema {
            name,
            description,
            schema,
            strict,
        } => {
            let mut definition = serde_json::Map::from_iter([
                ("name".to_string(), Value::String(name)),
                ("schema".to_string(), schema),
                ("strict".to_string(), Value::Bool(strict)),
            ]);
            if let Some(description) = description {
                definition.insert("description".to_string(), Value::String(description));
            }
            serde_json::json!({
                "type": "json_schema",
                "json_schema": definition,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn synthesized_schema_names_are_stable_and_openai_safe() {
        let schema = json!({"type":"object","properties":{"answer":{"type":"string"}}});

        let first = stable_schema_name(&schema);
        let second = stable_schema_name(&schema);

        assert_eq!(first, second);
        assert!(first.starts_with(SYNTHETIC_SCHEMA_NAME_PREFIX));
        assert!(is_valid_schema_name(&first));
    }

    #[test]
    fn property_ordering_is_removed_recursively_without_changing_constraints() {
        let schema = json!({
            "type":"object",
            "propertyOrdering":["answer"],
            "properties":{
                "answer":{
                    "type":"object",
                    "propertyOrdering":["value"],
                    "properties":{"value":{"type":"string","minLength":2}}
                }
            }
        });

        assert!(schema_contains_property_ordering(&schema));
        let normalized = without_property_ordering(schema);
        assert!(!schema_contains_property_ordering(&normalized));
        assert_eq!(
            normalized.pointer("/properties/answer/properties/value/minLength"),
            Some(&json!(2))
        );
    }
}
