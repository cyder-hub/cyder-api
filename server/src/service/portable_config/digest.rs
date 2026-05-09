use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PortableDigestError {
    #[error("failed to parse portable bundle JSON for digest: {0}")]
    Json(#[from] serde_json::Error),
    #[error("portable bundle digest mismatch: expected `{expected}`, actual `{actual}`")]
    DigestMismatch { expected: String, actual: String },
}

pub fn canonical_json_digest_str(input: &str) -> Result<String, PortableDigestError> {
    let value = serde_json::from_str::<Value>(input)?;
    Ok(canonical_json_digest_value(&value))
}

pub fn canonical_json_digest<T>(value: &T) -> Result<String, PortableDigestError>
where
    T: Serialize,
{
    let value = serde_json::to_value(value)?;
    Ok(canonical_json_digest_value(&value))
}

pub fn canonical_json_digest_value(value: &Value) -> String {
    let mut canonical = Vec::new();
    write_canonical_json(value, &mut canonical);
    format!("sha256:{}", sha256_hex(&canonical))
}

pub fn verify_canonical_json_digest_str(
    input: &str,
    expected: &str,
) -> Result<(), PortableDigestError> {
    let actual = canonical_json_digest_str(input)?;
    if actual != expected {
        return Err(PortableDigestError::DigestMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

pub fn sha256_hex(bytes: impl AsRef<[u8]>) -> String {
    hex_lower(&Sha256::digest(bytes))
}

fn write_canonical_json(value: &Value, output: &mut Vec<u8>) {
    match value {
        Value::Null => output.extend_from_slice(b"null"),
        Value::Bool(true) => output.extend_from_slice(b"true"),
        Value::Bool(false) => output.extend_from_slice(b"false"),
        Value::Number(number) => output.extend_from_slice(number.to_string().as_bytes()),
        Value::String(value) => {
            let encoded = serde_json::to_string(value)
                .expect("serializing a JSON string into canonical JSON cannot fail");
            output.extend_from_slice(encoded.as_bytes());
        }
        Value::Array(values) => {
            output.push(b'[');
            for (index, child) in values.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                write_canonical_json(child, output);
            }
            output.push(b']');
        }
        Value::Object(map) => {
            output.push(b'{');
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    output.push(b',');
                }
                let encoded_key = serde_json::to_string(key)
                    .expect("serializing a JSON object key into canonical JSON cannot fail");
                output.extend_from_slice(encoded_key.as_bytes());
                output.push(b':');
                write_canonical_json(&map[*key], output);
            }
            output.push(b'}');
        }
    }
}

fn hex_lower(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::{PortableDigestError, canonical_json_digest_str, verify_canonical_json_digest_str};

    #[test]
    fn canonical_json_digest_is_stable_for_object_key_order() {
        let left = r#"{
            "schema_version": "cyder.portable.v1",
            "exported_at": 1778236800000,
            "cyder_version": "1.0.0",
            "modules": [
                {
                    "module_id": "provider_profile",
                    "module_version": 1,
                    "summary": {"skip": 0, "total": 1},
                    "items": {"providers": [{"provider_key": "openai", "name": "OpenAI"}]}
                }
            ]
        }"#;
        let right = r#"{
            "modules": [
                {
                    "items": {"providers": [{"name": "OpenAI", "provider_key": "openai"}]},
                    "summary": {"total": 1, "skip": 0},
                    "module_version": 1,
                    "module_id": "provider_profile"
                }
            ],
            "cyder_version": "1.0.0",
            "exported_at": 1778236800000,
            "schema_version": "cyder.portable.v1"
        }"#;

        assert_eq!(
            canonical_json_digest_str(left).expect("left digest"),
            canonical_json_digest_str(right).expect("right digest")
        );
    }

    #[test]
    fn verify_canonical_json_digest_rejects_mismatch() {
        let raw = r#"{"schema_version":"cyder.portable.v1","modules":[]}"#;
        let err = verify_canonical_json_digest_str(raw, "sha256:wrong")
            .expect_err("digest mismatch should be rejected");

        assert!(matches!(
            err,
            PortableDigestError::DigestMismatch { actual, .. }
                if actual.starts_with("sha256:")
        ));
    }
}
