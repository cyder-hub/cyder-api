use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone, PartialEq, Eq)]
struct LintViolation {
    path: PathBuf,
    line: usize,
    reason: &'static str,
    excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MacroInvocation {
    line: usize,
    snippet: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    println!("Running log regression lint...");
    let root = workspace_root();
    let violations = collect_log_lint_violations(&root)?;

    if violations.is_empty() {
        println!("Log lint passed.");
        return Ok(());
    }

    eprintln!("Log lint failed with {} violation(s):", violations.len());
    for violation in &violations {
        let display_path = violation
            .path
            .strip_prefix(&root)
            .unwrap_or(&violation.path)
            .display();
        eprintln!(
            "  {}:{}: {} :: {}",
            display_path, violation.line, violation.reason, violation.excerpt
        );
    }

    Err("log lint failed".to_string())
}

fn collect_log_lint_violations(root: &Path) -> Result<Vec<LintViolation>, String> {
    let server_src = root.join("server").join("src");
    let mut files = Vec::new();
    collect_rust_files(&server_src, &mut files)?;

    let mut violations = Vec::new();
    for path in files {
        let contents = fs::read_to_string(&path)
            .map_err(|err| format!("read source file {}: {err}", path.display()))?;
        violations.extend(scan_builder_usage(root, &path, &contents));
        violations.extend(scan_log_invocations(&path, &contents));
    }

    violations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.reason.cmp(right.reason))
    });

    Ok(violations)
}

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in
        fs::read_dir(dir).map_err(|err| format!("read directory {}: {err}", dir.display()))?
    {
        let entry = entry.map_err(|err| format!("iterate directory {}: {err}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn is_business_logging_file(root: &Path, path: &Path) -> bool {
    let controller = root.join("server").join("src").join("controller");
    let proxy = root.join("server").join("src").join("proxy");
    let service = root.join("server").join("src").join("service");
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    (path.starts_with(controller) || path.starts_with(proxy) || path.starts_with(service))
        && file_name != "integration.rs"
        && file_name != "log_regression.rs"
}

fn scan_builder_usage(root: &Path, path: &Path, contents: &str) -> Vec<LintViolation> {
    if !is_business_logging_file(root, path)
        || path == root.join("server").join("src").join("logging.rs")
    {
        return Vec::new();
    }

    let mut violations = Vec::new();
    for (needle, reason) in [
        (
            "event_message(",
            "business code must not call event_message directly",
        ),
        (
            ".field(",
            "business code must not use chained .field(...) logging",
        ),
    ] {
        violations.extend(find_substring_violations(path, contents, needle, reason));
    }

    violations
}

fn scan_log_invocations(path: &Path, contents: &str) -> Vec<LintViolation> {
    let mut violations = Vec::new();
    for invocation in extract_log_invocations(contents) {
        if contains_any(
            &invocation.snippet,
            &[
                "original request data",
                "request data:",
                "Body:",
                "Data: '",
                "unified request:",
                "Transformation complete. Result",
                "[transform][diagnostic]",
            ],
        ) {
            violations.push(LintViolation {
                path: path.to_path_buf(),
                line: invocation.line,
                reason: "raw payload logging pattern is forbidden",
                excerpt: compact_excerpt(&invocation.snippet),
            });
        }

        let query_param_normalized = invocation.snippet.replace("query_params.len()", "");
        if query_param_normalized.contains("query_params") {
            violations.push(LintViolation {
                path: path.to_path_buf(),
                line: invocation.line,
                reason: "direct query_params logging is forbidden",
                excerpt: compact_excerpt(&invocation.snippet),
            });
        }

        if logs_auth_header_value(&invocation.snippet) {
            violations.push(LintViolation {
                path: path.to_path_buf(),
                line: invocation.line,
                reason: "auth header/value logging is forbidden",
                excerpt: compact_excerpt(&invocation.snippet),
            });
        }
    }

    violations
}

fn find_substring_violations(
    path: &Path,
    contents: &str,
    needle: &str,
    reason: &'static str,
) -> Vec<LintViolation> {
    let mut violations = Vec::new();
    let mut offset = 0usize;
    while let Some(found) = contents[offset..].find(needle) {
        let index = offset + found;
        violations.push(LintViolation {
            path: path.to_path_buf(),
            line: line_number(contents, index),
            reason,
            excerpt: line_excerpt(contents, index),
        });
        offset = index + needle.len();
    }
    violations
}

fn logs_auth_header_value(snippet: &str) -> bool {
    let has_auth_literal = contains_any(
        snippet,
        &[
            "Authorization",
            "AUTHORIZATION",
            "\"x-api-key\"",
            "\"x-goog-api-key\"",
        ],
    );
    let has_value_signal = contains_any(
        snippet,
        &[
            "headers",
            "header_value",
            ".get(",
            "request_headers",
            "original_headers",
            "value =",
        ],
    );
    has_auth_literal && has_value_signal
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn extract_log_invocations(contents: &str) -> Vec<MacroInvocation> {
    const LOG_MACROS: [&str; 8] = [
        "debug!",
        "info!",
        "warn!",
        "error!",
        "debug_event!",
        "info_event!",
        "warn_event!",
        "error_event!",
    ];

    let bytes = contents.as_bytes();
    let mut invocations = Vec::new();
    let mut index = 0usize;

    while index < bytes.len() {
        let matched = LOG_MACROS
            .iter()
            .find(|candidate| starts_with_macro(contents, index, candidate));
        let Some(macro_name) = matched else {
            index += 1;
            continue;
        };

        let mut cursor = index + macro_name.len();
        while cursor < bytes.len() && bytes[cursor].is_ascii_whitespace() {
            cursor += 1;
        }

        if cursor >= bytes.len() || bytes[cursor] != b'(' {
            index += 1;
            continue;
        }

        if let Some(end) = find_invocation_end(contents, cursor) {
            invocations.push(MacroInvocation {
                line: line_number(contents, index),
                snippet: contents[index..end].to_string(),
            });
            index = end;
        } else {
            index += 1;
        }
    }

    invocations
}

fn starts_with_macro(contents: &str, index: usize, macro_name: &str) -> bool {
    contents.as_bytes()[index..].starts_with(macro_name.as_bytes())
        && (index == 0
            || (!contents.as_bytes()[index - 1].is_ascii_alphanumeric()
                && contents.as_bytes()[index - 1] != b'_'))
}

fn find_invocation_end(contents: &str, open_paren_index: usize) -> Option<usize> {
    let bytes = contents.as_bytes();
    let mut depth = 0usize;
    let mut index = open_paren_index;
    let mut in_string = false;
    let mut string_delimiter = b'"';
    let mut escaped = false;
    let mut line_comment = false;
    let mut block_comment_depth = 0usize;

    while index < bytes.len() {
        let current = bytes[index];

        if line_comment {
            if current == b'\n' {
                line_comment = false;
            }
            index += 1;
            continue;
        }

        if block_comment_depth > 0 {
            if current == b'/' && bytes.get(index + 1) == Some(&b'*') {
                block_comment_depth += 1;
                index += 2;
                continue;
            }
            if current == b'*' && bytes.get(index + 1) == Some(&b'/') {
                block_comment_depth -= 1;
                index += 2;
                continue;
            }
            index += 1;
            continue;
        }

        if in_string {
            if escaped {
                escaped = false;
            } else if current == b'\\' {
                escaped = true;
            } else if current == string_delimiter {
                in_string = false;
            }
            index += 1;
            continue;
        }

        if current == b'/' && bytes.get(index + 1) == Some(&b'/') {
            line_comment = true;
            index += 2;
            continue;
        }
        if current == b'/' && bytes.get(index + 1) == Some(&b'*') {
            block_comment_depth += 1;
            index += 2;
            continue;
        }

        if current == b'"' || current == b'\'' {
            in_string = true;
            string_delimiter = current;
            index += 1;
            continue;
        }

        if current == b'(' {
            depth += 1;
        } else if current == b')' {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(index + 1);
            }
        }

        index += 1;
    }

    None
}

fn line_number(contents: &str, index: usize) -> usize {
    contents[..index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn line_excerpt(contents: &str, index: usize) -> String {
    let line = contents[index..].lines().next().unwrap_or_default();
    compact_excerpt(line)
}

fn compact_excerpt(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const MAX_LEN: usize = 160;
    if compact.len() <= MAX_LEN {
        compact
    } else {
        format!("{}...", &compact[..MAX_LEN])
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("server crate should have a workspace root parent")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        collect_log_lint_violations, compact_excerpt, extract_log_invocations,
        logs_auth_header_value, scan_builder_usage, scan_log_invocations, workspace_root,
    };

    #[test]
    fn extract_log_invocations_handles_multiline_event_macros() {
        let invocations = extract_log_invocations(
            r#"
            fn sample() {
                crate::warn_event!(
                    "proxy.request_failed",
                    log_id = 42,
                );
            }
            "#,
        );

        assert_eq!(invocations.len(), 1);
        assert!(invocations[0].snippet.contains("proxy.request_failed"));
    }

    #[test]
    fn auth_header_detection_requires_value_signals() {
        assert!(logs_auth_header_value(&format!(
            "{}{}",
            "debug", r#"!("bad auth {:?}", headers.get("x-api-key"));"#
        )));
        assert!(!logs_auth_header_value(
            r#"crate::debug_event!("auth.request_rejected", source = "x-goog-api-key");"#
        ));
    }

    #[test]
    fn compact_excerpt_collapses_whitespace() {
        assert_eq!(
            compact_excerpt("  alpha\n beta\tgamma "),
            "alpha beta gamma"
        );
    }

    #[test]
    fn scan_builder_usage_flags_business_event_builder_calls() {
        let root = PathBuf::from("/repo");
        let path = root
            .join("server")
            .join("src")
            .join("service")
            .join("sample.rs");
        let violations = scan_builder_usage(
            &root,
            &path,
            r#"
            fn sample() {
                event_message("proxy.request_failed");
                logger.field("provider", "openai");
            }
            "#,
        );

        assert_eq!(violations.len(), 2);
        assert_eq!(
            violations[0].reason,
            "business code must not call event_message directly"
        );
        assert_eq!(
            violations[1].reason,
            "business code must not use chained .field(...) logging"
        );
    }

    #[test]
    fn scan_log_invocations_flags_raw_payload_query_params_and_auth_values() {
        let path = PathBuf::from("sample.rs");
        let contents = format!(
            "{}\n{}\n{}",
            concat!("debug", r#"!("request data: {:?}", body);"#),
            concat!("info", r#"!("query params: {:?}", query_params);"#),
            concat!("warn", r#"!("bad auth {:?}", headers.get("x-api-key"));"#)
        );

        let violations = scan_log_invocations(&path, &contents);

        assert_eq!(violations.len(), 3);
        assert!(
            violations
                .iter()
                .any(|violation| violation.reason == "raw payload logging pattern is forbidden")
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.reason == "direct query_params logging is forbidden")
        );
        assert!(
            violations
                .iter()
                .any(|violation| violation.reason == "auth header/value logging is forbidden")
        );
    }

    #[test]
    fn scan_log_invocations_allows_query_params_len() {
        let path = PathBuf::from("sample.rs");
        let contents = concat!("info", r#"!("query param count: {}", query_params.len());"#);

        let violations = scan_log_invocations(&path, contents);

        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:#?}"
        );
    }

    #[test]
    fn repository_log_lint_passes() {
        let root = workspace_root();
        let violations = collect_log_lint_violations(&root).expect("log lint should scan repo");
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:#?}"
        );
    }
}
