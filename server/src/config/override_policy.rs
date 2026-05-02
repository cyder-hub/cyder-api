use serde_yaml::Value;

pub const OVERRIDE_ALLOWED_PATHS: &[&str] = &[
    "log_level",
    "timezone",
    "max_body_size",
    "proxy",
    "proxy_request.connect_timeout_seconds",
    "proxy_request.first_byte_timeout_seconds",
    "proxy_request.total_timeout_seconds",
    "provider_governance.enabled",
    "provider_governance.consecutive_failure_threshold",
    "provider_governance.open_cooldown_seconds",
    "routing_resilience.same_candidate_max_retries",
    "routing_resilience.max_candidates_per_request",
    "routing_resilience.base_backoff_ms",
    "routing_resilience.max_backoff_ms",
    "routing_resilience.respect_retry_after_up_to_seconds",
    "diagnostics.replay_preview_confirmation_ttl_seconds",
    "diagnostics.replay_preview_confirmation_clock_skew_seconds",
    "diagnostics.response_capture_max_bytes",
    "diagnostics.raw_bundle_download_enabled",
    "diagnostics.retention.enabled",
    "diagnostics.retention.request_log_bundle_retention_days",
    "diagnostics.retention.replay_artifact_retention_days",
    "diagnostics.retention.delete_batch_size",
];

pub fn validate_override_document(value: &Value) -> Result<(), Vec<String>> {
    let mut paths = Vec::new();
    flatten_yaml_paths(value, "", &mut paths);

    let invalid_paths = paths
        .into_iter()
        .filter(|path| !OVERRIDE_ALLOWED_PATHS.contains(&path.as_str()))
        .collect::<Vec<_>>();

    if invalid_paths.is_empty() {
        Ok(())
    } else {
        Err(invalid_paths)
    }
}

fn flatten_yaml_paths(value: &Value, prefix: &str, paths: &mut Vec<String>) {
    match value {
        Value::Null => {
            if !prefix.is_empty() {
                paths.push(prefix.to_string());
            }
        }
        Value::Mapping(mapping) => {
            if mapping.is_empty() && !prefix.is_empty() {
                paths.push(prefix.to_string());
                return;
            }

            for (key, value) in mapping {
                let Some(key) = key.as_str() else {
                    paths.push(if prefix.is_empty() {
                        "<non_string_key>".to_string()
                    } else {
                        format!("{prefix}.<non_string_key>")
                    });
                    continue;
                };
                let next_prefix = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_yaml_paths(value, &next_prefix, paths);
            }
        }
        Value::Sequence(_) => {
            paths.push(if prefix.is_empty() {
                "<root>".to_string()
            } else {
                prefix.to_string()
            });
        }
        _ => {
            paths.push(if prefix.is_empty() {
                "<root>".to_string()
            } else {
                prefix.to_string()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::validate_override_document;

    #[test]
    fn override_policy_accepts_nested_whitelisted_paths() {
        let value = serde_yaml::from_str(
            r#"
log_level: debug
proxy: null
proxy_request:
  first_byte_timeout_seconds: 120
diagnostics:
  retention:
    enabled: true
"#,
        )
        .expect("override YAML should parse");

        assert!(validate_override_document(&value).is_ok());
    }

    #[test]
    fn override_policy_reports_non_whitelisted_paths() {
        let value = serde_yaml::from_str(
            r#"
db_url: postgres://example
storage:
  driver: s3
proxy_request:
  first_byte_timeout_seconds: 120
"#,
        )
        .expect("override YAML should parse");

        let err = validate_override_document(&value).expect_err("override should be rejected");

        assert_eq!(
            err,
            vec!["db_url".to_string(), "storage.driver".to_string()]
        );
    }
}
