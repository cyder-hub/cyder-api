use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Violation {
    path: PathBuf,
    line: usize,
    reason: &'static str,
    excerpt: String,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    println!("Running persistence boundary lint...");
    let root = workspace_root();
    let violations = collect_violations(&root)?;
    if violations.is_empty() {
        println!("Persistence boundary lint passed.");
        return Ok(());
    }

    eprintln!(
        "Persistence boundary lint failed with {} violation(s):",
        violations.len()
    );
    for violation in violations {
        let path = violation
            .path
            .strip_prefix(&root)
            .unwrap_or(&violation.path)
            .display();
        eprintln!(
            "  {path}:{}: {} :: {}",
            violation.line, violation.reason, violation.excerpt
        );
    }
    Err("persistence boundary lint failed".to_string())
}

fn collect_violations(root: &Path) -> Result<Vec<Violation>, String> {
    let server_src = root.join("server/src");
    let database_dir = server_src.join("database");
    let lint_path = server_src.join("bin/persistence_boundary_lint.rs");
    let mut files = Vec::new();
    collect_rust_files(&server_src, &mut files)?;

    let mut violations = Vec::new();
    for path in files {
        if path == lint_path {
            continue;
        }
        let contents = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        violations.extend(scan_source(
            &path,
            &contents,
            path.starts_with(&database_dir),
        ));
    }

    let manifest = root.join("server/Cargo.toml");
    let manifest_contents = fs::read_to_string(&manifest)
        .map_err(|error| format!("read {}: {error}", manifest.display()))?;
    violations.extend(find_patterns(
        &manifest,
        &manifest_contents,
        &[("r2d2", "database r2d2 dependency or feature is forbidden")],
    ));

    violations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.reason.cmp(right.reason))
    });
    Ok(violations)
}

fn scan_source(path: &Path, contents: &str, is_database_file: bool) -> Vec<Violation> {
    let mut violations = find_patterns(
        path,
        contents,
        &[
            (
                "get_connection",
                "implicit database connection lookup is forbidden",
            ),
            ("DbPool", "legacy synchronous database pool is forbidden"),
            (
                "DbConnection",
                "legacy pooled database connection is forbidden",
            ),
            ("r2d2", "runtime r2d2 usage is forbidden"),
            (
                "LegacyTestDbContext",
                "implicit test database context is forbidden",
            ),
            (
                "TestDbContext",
                "implicit test database context is forbidden",
            ),
            (
                "ACTIVE_TEST_DB",
                "task-local test database selector is forbidden",
            ),
            (
                "TEST_DB_SCOPE",
                "thread-local test database selector is forbidden",
            ),
            (
                "CURRENT_DATABASE",
                "implicit current database selector is forbidden",
            ),
        ],
    );

    if !is_database_file {
        violations.extend(find_patterns(
            path,
            contents,
            &[
                (
                    "RunQueryDsl",
                    "raw Diesel query execution must stay in database modules",
                ),
                (
                    "diesel_async::",
                    "raw Diesel query execution must stay in database modules",
                ),
                ("diesel::sql_query", "raw SQL must stay in database modules"),
                ("sql_query(", "raw SQL must stay in database modules"),
                (
                    "PgConnection::establish",
                    "raw database connection establishment is forbidden",
                ),
                (
                    "SqliteConnection::establish",
                    "raw database connection establishment is forbidden",
                ),
                (
                    "diesel::Connection::establish",
                    "raw database connection establishment is forbidden",
                ),
            ],
        ));
    }

    violations.extend(scan_spawn_blocking(path, contents));
    violations
}

fn scan_spawn_blocking(path: &Path, contents: &str) -> Vec<Violation> {
    let mut violations = Vec::new();
    for spawn_call in ["tokio::task::spawn_blocking", "tokio::spawn_blocking"] {
        let mut offset = 0;
        while let Some(relative) = contents[offset..].find(spawn_call) {
            let index = offset + relative;
            let end = index.saturating_add(1_200).min(contents.len());
            let snippet = &contents[index..end];
            if [
                "diesel::",
                "diesel_async::",
                "RunQueryDsl",
                "sql_query(",
                "SqliteConnection",
            ]
            .iter()
            .any(|needle| snippet.contains(needle))
            {
                violations.push(Violation {
                    path: path.to_path_buf(),
                    line: line_number(contents, index),
                    reason: "bare spawn_blocking database execution is forbidden",
                    excerpt: compact_excerpt(snippet),
                });
            }
            offset = index + spawn_call.len();
        }
    }
    violations
}

fn find_patterns(
    path: &Path,
    contents: &str,
    patterns: &[(&'static str, &'static str)],
) -> Vec<Violation> {
    let mut violations = Vec::new();
    for (needle, reason) in patterns {
        let mut offset = 0;
        while let Some(relative) = contents[offset..].find(needle) {
            let index = offset + relative;
            violations.push(Violation {
                path: path.to_path_buf(),
                line: line_number(contents, index),
                reason,
                excerpt: compact_excerpt(contents[index..].lines().next().unwrap_or_default()),
            });
            offset = index + needle.len();
        }
    }
    violations
}

fn collect_rust_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("read directory {}: {error}", directory.display()))?
    {
        let entry =
            entry.map_err(|error| format!("iterate directory {}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, files)?;
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn line_number(contents: &str, index: usize) -> usize {
    contents[..index]
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count()
        + 1
}

fn compact_excerpt(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 160;
    if compact.len() <= LIMIT {
        compact
    } else {
        format!("{}...", &compact[..LIMIT])
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
    use super::scan_source;
    use std::path::Path;

    #[test]
    fn rejects_implicit_and_business_raw_database_access() {
        let source = "get_connection(); diesel::sql_query(\"SELECT 1\");";
        let violations = scan_source(Path::new("service/sample.rs"), source, false);
        assert_eq!(violations.len(), 3);
    }

    #[test]
    fn allows_raw_execution_inside_database_modules() {
        let source = "diesel::sql_query(\"SELECT 1\"); RunQueryDsl::execute(query, conn);";
        assert!(scan_source(Path::new("database/startup.rs"), source, true).is_empty());
    }

    #[test]
    fn rejects_database_work_inside_spawn_blocking() {
        let source = "tokio::task::spawn_blocking(|| diesel::sql_query(\"SELECT 1\"));";
        assert!(
            scan_source(Path::new("service/sample.rs"), source, false)
                .iter()
                .any(|violation| violation.reason.contains("spawn_blocking"))
        );
    }

    #[test]
    fn allows_sync_connection_wrapper_managed_blocking_execution() {
        let source = "connection.spawn_blocking(|conn| conn.register_noarg_sql_function(\"probe\", false, || 1));";
        assert!(scan_source(Path::new("database/runtime.rs"), source, true).is_empty());
    }
}
