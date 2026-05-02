use reqwest::Url;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const SAFE_KEYWORD_PATHS: &[&str] = &["runtime_state.redis.api_key_concurrency_lease_ttl_seconds"];

pub fn redact_config_value(path: &str, value: &Value) -> Value {
    match path {
        "db_url" => redact_database_url(value),
        "redis.url" => redact_redis_url(value),
        "proxy" => redact_proxy_url(value),
        _ if is_sensitive_config_path(path) => redact_secret_value(value),
        _ => value.clone(),
    }
}

pub fn redact_config_tree_value(path: &str, value: &Value) -> Value {
    let Some(map) = value.as_object() else {
        return redact_config_value(path, value);
    };

    let redacted = map
        .iter()
        .map(|(key, value)| {
            let child_path = if path.is_empty() {
                key.to_string()
            } else {
                format!("{path}.{key}")
            };
            (key.clone(), redact_config_tree_value(&child_path, value))
        })
        .collect();

    Value::Object(redacted)
}

pub fn is_sensitive_config_path(path: &str) -> bool {
    if SAFE_KEYWORD_PATHS.contains(&path) {
        return false;
    }

    let lower = path.to_ascii_lowercase();
    lower.contains("secret")
        || lower.contains("password")
        || lower.contains("token")
        || lower.contains("access_key")
        || lower.contains("api_key")
}

fn redact_secret_value(value: &Value) -> Value {
    let Some(raw) = value.as_str() else {
        return json!({
            "redacted": true,
            "configured": !value.is_null(),
            "length": null,
            "sha256_prefix": null,
        });
    };

    let configured = !raw.trim().is_empty();
    json!({
        "redacted": true,
        "configured": configured,
        "length": raw.len(),
        "sha256_prefix": configured.then(|| sha256_prefix(raw)),
    })
}

fn redact_database_url(value: &Value) -> Value {
    let Some(raw) = value.as_str() else {
        return value.clone();
    };

    if raw.trim().is_empty() {
        return json!({
            "configured": false,
            "redacted": true,
            "kind": "database_url",
        });
    }

    if let Ok(url) = Url::parse(raw) {
        return json!({
            "configured": true,
            "redacted": url_has_userinfo(&url),
            "kind": "database_url",
            "scheme": url.scheme(),
            "host": url.host_str(),
            "port": url.port(),
            "database": url.path().trim_start_matches('/'),
            "display": redact_url_userinfo(url).to_string(),
        });
    }

    json!({
        "configured": true,
        "redacted": false,
        "kind": "database_path",
        "path": raw,
    })
}

fn redact_redis_url(value: &Value) -> Value {
    let Some(raw) = value.as_str() else {
        return value.clone();
    };

    if raw.trim().is_empty() {
        return json!({
            "configured": false,
            "redacted": true,
            "kind": "redis_url",
        });
    }

    if let Ok(url) = Url::parse(raw) {
        return json!({
            "configured": true,
            "redacted": url_has_userinfo(&url),
            "kind": "redis_url",
            "scheme": url.scheme(),
            "host": url.host_str(),
            "port": url.port(),
            "db_index": url.path().trim_start_matches('/'),
            "display": redact_url_userinfo(url).to_string(),
        });
    }

    json!({
        "configured": true,
        "redacted": true,
        "kind": "redis_url",
        "display": "<invalid-url>",
    })
}

fn redact_proxy_url(value: &Value) -> Value {
    let Some(raw) = value.as_str() else {
        return value.clone();
    };

    if raw.trim().is_empty() {
        return json!({
            "configured": false,
            "redacted": true,
            "kind": "proxy_url",
        });
    }

    if let Ok(url) = Url::parse(raw) {
        return json!({
            "configured": true,
            "redacted": url_has_userinfo(&url),
            "kind": "proxy_url",
            "display": redact_url_userinfo(url).to_string(),
        });
    }

    json!({
        "configured": true,
        "redacted": true,
        "kind": "proxy_url",
        "display": "<invalid-url>",
    })
}

fn url_has_userinfo(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

fn redact_url_userinfo(mut url: Url) -> Url {
    if url_has_userinfo(&url) {
        let _ = url.set_username("***");
        let _ = url.set_password(None);
    }
    url
}

fn sha256_prefix(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::redact_config_value;

    #[test]
    fn proxy_userinfo_is_redacted() {
        let value = redact_config_value(
            "proxy",
            &json!("http://proxy-user:proxy-pass@127.0.0.1:8080/path"),
        );
        let serialized = serde_json::to_string(&value).expect("redacted proxy should serialize");

        assert!(serialized.contains("***"));
        assert!(!serialized.contains("proxy-user"));
        assert!(!serialized.contains("proxy-pass"));
    }
}
