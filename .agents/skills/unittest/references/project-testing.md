# Project Testing Reference

This reference only covers test patterns that already exist and are stable in the repository today. When adding new tests, fit into these patterns first instead of building a new harness.

## 1. Current Test Map

- The main backend suite lives under `server/src` and currently contains about `578` Rust tests.
- Frontend tests live under `front/tests/*.test.mjs` and are run by `front/package.json` via `node --test tests/**/*.test.mjs`.
- The backend has effectively settled into three main testing layers:
  - database / migration / repository
  - service / app state / logging / cache
  - proxy integration

## 2. Which Layer Should Own The Test

### A. Database And Migration Tests

Use this layer for:

- schema and migration constraints
- repository CRUD behavior
- SQL-level invariants
- upgrade and backfill paths for older data

Main entry points:

- `server/src/database/mod.rs`
- the specific repository file, for example `server/src/database/request_attempt.rs`

Preferred helpers:

- `open_test_sqlite_connection("foo.sqlite")`
- `open_test_sqlite_connection_with_migrations("foo.sqlite")`

Short template:

```rust
#[test]
fn sqlite_rule_is_enforced() {
    let (_temp_dir, mut connection) =
        crate::database::open_test_sqlite_connection_with_migrations("rule.sqlite");

    // prepare data
    // execute repository code or direct SQL
    // assert final DB state
}
```

Rules:

- Database unit tests should use one database per test.
- If you need to validate migrations, prefer constructing the old sqlite state directly and then running the migration path.
- Do not promote a repository test into `AppState` or proxy integration unless the behavior truly depends on those layers.

### B. Service / AppState / Logging Tests

Use this layer for:

- cache hit or invalidation behavior
- `AppState` reload, lookup, and runtime state behavior
- instance-owned log workers
- cases that must hit the current test-private database through `get_connection()`

Key entry points:

- `server/src/database/mod.rs`: `TestDbContext`
- `server/src/service/app_state.rs`: `create_test_app_state(...)`
- `server/src/proxy/logging.rs`: instance-owned `LogManager`

Short template:

```rust
#[tokio::test]
async fn app_state_reads_only_its_scoped_db() {
    let test_db_context = TestDbContext::new_sqlite("app-state.sqlite");

    test_db_context
        .run_async(async {
            let app_state = create_test_app_state(test_db_context.clone()).await;

            // prepare data
            // call app_state behavior
            // flush before reading async log results
            app_state.flush_proxy_logs().await;
        })
        .await;
}
```

Rules:

- `TestDbContext` is the correct entry point. Do not create a new "set a global env var and then test" pattern.
- A test's `AppState`, log worker, and database context must belong to the same scoped test runtime.
- If you need to assert `request_log` or `request_attempt`, flush logs first with `flush_proxy_logs()`.

### C. Proxy Integration Tests

Use this layer for:

- mock upstream behavior
- real provider/model/api key wiring
- request forwarding, fallback, governance, log persistence, and bundle persistence
- parallel isolation verification

Key entry points:

- `server/src/proxy/integration.rs`
- `run_integration_test(...)`
- `spawn_test_upstream_or_skip(...)`
- `TestFixture::new(...)`

Short template:

```rust
#[test]
fn my_proxy_flow() {
    run_integration_test(|test_db_context| async move {
        let Some(upstream) = spawn_test_upstream_or_skip(|request| {
            // assert the upstream method/path/body
            UpstreamReply::json(StatusCode::OK, serde_json::json!({ "ok": true }))
        })
        .await
        else {
            return;
        };

        let fixture = TestFixture::new(
            test_db_context.clone(),
            ProviderType::Openai,
            format!("{}/v1", upstream.base_url),
            None,
            Some("test-model".to_string()),
        )
        .await;

        let response = fixture.send(request).await;
        assert_eq!(response.status(), StatusCode::OK);

        let log = fixture.latest_log().await;
        let attempts = fixture.attempts_for_log(log.id).await;
        assert_eq!(attempts.len(), 1);

        fixture.cleanup().await;
    });
}
```

Rules:

- One integration case should own one runtime, one `TestDbContext`, and one `AppState`.
- Do not share sqlite files between cases.
- Do not rely on a global `DB_LOCK` or single-threaded test execution to "make it pass".
- If a failure is status-sensitive, print the response body when the status is unexpected. That makes diagnosis much faster.
- If a change touches concurrent writes, background logging, fallback queues, or request replay, prefer adding a parallel isolation regression. Existing reference: `integration_fixtures_run_in_parallel_without_cross_db_visibility`.

## 3. Frontend Test Pattern

The current frontend suite is intentionally biased toward pure functions and pure state:

- page state shaping
- bundle parsing
- route and navigation constraints
- i18n cleanup after migrations

Rules:

- Keep these tests in `front/tests/*.test.mjs`.
- Continue using Node's built-in `test(...)` style.
- Prefer extracting pure helpers or state builders before writing tests instead of defaulting to a browser-rendering framework.
- If the change is only about derived manager UI state or mapping logic, the test should focus on inputs and outputs instead of constructing a full Vue runtime.

## 4. Best Practices Already Fixed In This Repository

- SQLite test paths already use `WAL` and `busy_timeout`, but those are stability enhancements, not a justification for shared databases.
- In test builds, object storage is forced to a local temporary directory. Without a real S3 environment, do not claim S3 coverage.
- For async background logging, tests must flush explicitly before reading database results.
- For proxy behavior, prefer assertions on what the operator actually cares about:
  - response status
  - response body
  - `request_log`
  - `request_attempt`
  - artifact or bundle contents
  - fallback and governance decisions

## 5. Anti-Patterns

- Reusing a shared sqlite file or reviving a global override for convenience.
- Adding tests that only pass under `--test-threads=1`.
- Adding heavy end-to-end UI tests for frontend changes that only affect pure state logic.
- Changing backend governance, routing, or logging paths but only adding DTO snapshots.
- Writing vague test names that do not describe the behavioral contract.

## 6. Common Verification Commands

Focused backend runs:

- `rtk cargo test -p cyder-api database::tests:: -- --nocapture`
- `rtk cargo test -p cyder-api service::app_state::tests -- --nocapture`
- `rtk cargo test -p cyder-api proxy::logging --lib -- --nocapture`
- `rtk cargo test -p cyder-api proxy::integration:: -- --nocapture`

Wider backend runs:

- `rtk cargo test -p cyder-api -- --nocapture`
- `rtk cargo xtask test`

Frontend:

- `rtk npm --prefix front test`

If a test depends on mock upstream listeners or external services, say so explicitly in the final result. Do not present a skip as a pass.
