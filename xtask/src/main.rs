use anyhow::{bail, Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;

fn main() -> Result<()> {
    let mut args = pico_args::Arguments::from_env();

    // Help flags
    if args.contains(["-h", "--help"]) {
        print_help();
        return Ok(());
    }

    let subcommand = args.subcommand()?.unwrap_or_else(|| "default".into());

    // Set working directory to project root before running commands that need it
    let root = project_root();
    println!("Running xtask in: {}", root.display());
    // Note: Setting global CWD might not be ideal if commands need different CWDs.
    // We'll handle CWD within specific run_* functions where needed.
    // env::set_current_dir(&root).context("Failed to set working directory to project root")?;

    match subcommand.as_str() {
        "dev" => cmd_dev()?,
        "build" => cmd_build()?,
        "dev-backend" => cmd_dev_backend()?,
        "build-backend" => cmd_build_backend()?,
        "dev-front" => cmd_dev_front()?,
        "build-front" => cmd_build_front()?,
        "install-front-deps" => cmd_install_front_deps()?, // Add this line
        "log-lint" => cmd_log_lint()?,
        "test" => {
            cmd_test(args)?;
            return Ok(());
        }
        "default" => {
            // Optional: Define a default behavior, e.g., print help or run combined dev
            println!("Default task: Running combined dev server (backend + frontend).");
            cmd_dev()?;
        }
        _ => {
            eprintln!("Error: Unknown command '{}'", subcommand);
            print_help();
            std::process::exit(1); // Use exit code 1 for errors
        }
    }

    // Ensure all remaining arguments are processed or fail if unexpected ones are found
    let remaining = args.finish();
    if !remaining.is_empty() {
        eprintln!("Error: Unexpected arguments: {:?}", remaining);
        print_help();
        std::process::exit(1); // Use exit code 1 for errors
    }

    Ok(())
}

fn print_help() {
    println!(
        r#"
Usage: cargo xtask <COMMAND>

Commands:
  dev             Runs the backend and frontend development servers concurrently.
  build           Builds the backend and frontend projects in release mode.
  dev-backend     Runs the backend development server using 'cargo run'.
  build-backend        Builds the backend project in release mode using 'cargo build --release'.
  dev-front            Installs deps and runs the frontend development server using 'npm run dev' in './front'.
  build-front          Installs deps and builds the frontend project using 'npm run build' in './front'.
  install-front-deps   Installs frontend dependencies using 'npm install' in './front'.
  log-lint             Scans server runtime code for forbidden logging patterns.
  test                 Runs backend tests, with optional test name and arguments.
  default              Runs the 'dev' command.
"#
    );
}

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

fn cmd_log_lint() -> Result<()> {
    println!("🔎 Running log regression lint...");
    let root = project_root();
    let violations = collect_log_lint_violations(&root)?;

    if violations.is_empty() {
        println!("✅ Log lint passed.");
        return Ok(());
    }

    eprintln!("❌ Log lint failed with {} violation(s):", violations.len());
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

    bail!("log lint failed");
}

fn collect_log_lint_violations(root: &Path) -> Result<Vec<LintViolation>> {
    let server_src = root.join("server").join("src");
    let mut files = Vec::new();
    collect_rust_files(&server_src, &mut files)?;

    let mut violations = Vec::new();
    for path in files {
        let contents = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read source file {}", path.display()))?;
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

fn collect_rust_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in
        fs::read_dir(dir).with_context(|| format!("Failed to read directory {}", dir.display()))?
    {
        let entry =
            entry.with_context(|| format!("Failed to iterate directory {}", dir.display()))?;
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

// New combined dev command
fn cmd_dev() -> Result<()> {
    println!("🚀 Starting backend and frontend development servers...");

    let backend_handle = thread::spawn(|| {
        println!("▶️ Starting backend dev server...");
        // Call the dedicated backend dev function
        if let Err(e) = cmd_dev_backend() {
            // Change this line
            eprintln!("Backend dev server failed: {}", e);
        }
    });

    let frontend_handle = thread::spawn(|| {
        println!("▶️ Starting frontend dev server (will install deps if needed)..."); // Update print statement slightly
                                                                                      // Call the dedicated frontend dev function (which includes dep install)
        if let Err(e) = cmd_dev_front() {
            // Change this line
            eprintln!("Frontend dev server failed: {}", e);
        }
    });

    // Wait for both threads to complete.
    // Note: Dev servers usually run indefinitely, so join() might block forever
    // unless the servers exit or error out. This setup assumes you'll manually
    // stop the combined process (Ctrl+C), which should terminate the child processes.
    let backend_res = backend_handle.join();
    let frontend_res = frontend_handle.join();

    if backend_res.is_err() {
        eprintln!("Error joining backend thread.");
    }
    if frontend_res.is_err() {
        eprintln!("Error joining frontend thread.");
    }

    // Check if either thread panicked or the underlying command failed (if error handling inside thread was more robust)
    // Depending on the exact behavior desired, you might want to bail out here.
    // For now, just print that they were launched.
    println!("✅ Development servers launched (running concurrently). Press Ctrl+C to stop.");

    Ok(())
}

// New combined build command
fn cmd_build() -> Result<()> {
    println!("🏗️ Building backend and frontend projects...");

    // Build backend first
    cmd_build_backend()?;

    // Then build frontend
    cmd_build_front()?;

    println!("✅ Combined build complete.");
    Ok(())
}

// Renamed from cmd_dev
fn cmd_dev_backend() -> Result<()> {
    println!("🚀 Starting backend development server...");
    let server_dir = project_root().join("server");
    // Run 'cargo run' within the server directory
    run_cargo("run", &[], &server_dir)?; // Remove -p flag, update directory
    Ok(())
}

// Renamed from cmd_build
fn cmd_build_backend() -> Result<()> {
    println!("🏗️ Building backend project in release mode...");
    let server_dir = project_root().join("server");
    // Run 'cargo build --release' within the server directory
    run_cargo("build", &["--release"], &server_dir)?; // Remove -p flag, update directory
    println!("✅ Backend build complete.");
    Ok(())
}

fn cmd_test(args: pico_args::Arguments) -> Result<()> {
    println!("🧪 Running backend tests...");
    let server_dir = project_root().join("server");

    let mut cargo_args: Vec<String> = vec!["--package".to_string(), "cyder-api".to_string()];

    // All remaining arguments are for cargo test.
    let test_args: Vec<std::ffi::OsString> = args.finish();
    cargo_args.extend(
        test_args
            .into_iter()
            .map(|s| s.to_string_lossy().into_owned()),
    );

    let cargo_args_str: Vec<&str> = cargo_args.iter().map(|s| s.as_str()).collect();
    run_cargo("test", &cargo_args_str, &server_dir)?;
    println!("✅ Backend tests complete.");
    Ok(())
}

fn cmd_dev_front() -> Result<()> {
    // Install dependencies first
    cmd_install_front_deps()?; // Add this line

    println!("🚀 Starting frontend development server...");
    run_npm("run", &["dev"], &project_root().join("front"))?;
    Ok(())
}

fn cmd_build_front() -> Result<()> {
    // Install dependencies first
    cmd_install_front_deps()?; // Add this line

    println!("🏗️ Building frontend project...");
    let front_dir = project_root().join("front");
    // Remove the following two lines:
    // println!("▶️ Running: npm install (in ./front)");
    // run_npm("install", &[], &front_dir)?;
    println!("▶️ Running: npm run build (in ./front)");
    run_npm("run", &["build"], &front_dir)?;
    println!("✅ Frontend build complete.");
    Ok(())
}

fn cmd_install_front_deps() -> Result<()> {
    println!("📦 Installing frontend dependencies...");
    let front_dir = project_root().join("front");
    run_npm("install", &[], &front_dir)?;
    println!("✅ Frontend dependencies installed.");
    Ok(())
}

fn project_root() -> PathBuf {
    // Assumes xtask is directly inside the workspace root
    Path::new(&env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(1)
        .unwrap()
        .to_path_buf()
}

fn run_cargo(command: &str, args: &[&str], directory: &Path) -> Result<ExitStatus> {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let mut cmd = Command::new(cargo);
    cmd.arg(command);
    cmd.args(args);
    cmd.current_dir(directory); // Set the working directory

    println!("▶️ Running: {:?} in {:?}", cmd, directory.display());

    // Inherit stdio to see output/errors directly, useful for dev servers
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute: {:?} in {:?}", cmd, directory.display()))?;

    if !status.success() {
        bail!("Command `{:?}` failed with status {}", cmd, status);
    }
    Ok(status)
}

fn run_npm(npm_command: &str, args: &[&str], directory: &Path) -> Result<ExitStatus> {
    let npm_executable = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut cmd = Command::new(npm_executable);
    cmd.arg(npm_command);
    cmd.args(args);
    cmd.current_dir(directory); // Set the working directory

    println!("▶️ Running: {:?} in {:?}", cmd, directory.display());

    // Inherit stdio to see output/errors directly
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute: {:?} in {:?}", cmd, directory.display()))?;

    if !status.success() {
        bail!("npm command `{:?}` failed with status {}", cmd, status);
    }

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::{
        collect_log_lint_violations, compact_excerpt, extract_log_invocations,
        logs_auth_header_value, project_root,
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
        assert!(logs_auth_header_value(
            r#"debug!("bad auth {:?}", headers.get("x-api-key"));"#
        ));
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
    fn repository_log_lint_passes() {
        let root = project_root();
        let violations = collect_log_lint_violations(&root).expect("log lint should scan repo");
        assert!(
            violations.is_empty(),
            "unexpected violations: {violations:#?}"
        );
    }
}
