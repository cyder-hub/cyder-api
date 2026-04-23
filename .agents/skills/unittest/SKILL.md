---
name: unittest
description: Add or update repository-specific unit, async, database, service, proxy integration, or frontend state tests for this repository. Use when changing `server` or `front` code and you need to choose the right test layer, follow the current `TestDbContext` / `AppState` / proxy fixture patterns, and run the right verification commands.
---

# Unit Test

When adding tests in this repository, adapt to the existing test architecture first. Do not invent a new fixture stack unless the current one is clearly insufficient.

The current testing shape is clear:

- The backend under `server/src` already has about `578` Rust `#[test]` / `#[tokio::test]` cases and is the most mature test surface.
- Frontend tests are concentrated in `front/tests/*.test.mjs` and mostly cover pure state, pure view-model, and route/config behavior.
- Backend test infrastructure has already converged on the final ownership model: `TestDbContext`, instance-owned `LogManager`, `create_test_app_state(...)`, test-local storage, and the one-test-one-runtime model in `proxy::integration`.

## When To Use This Skill

- You changed Rust backend logic and need to add or update unit tests, async tests, database tests, or proxy integration tests.
- You changed Vue admin-console state, mapping, routing, i18n, or bundle parsing logic and need to add or update `front/tests`.
- You are unsure which layer should own the test: repository, service, or proxy integration.

## Pick The Layer First

- Pure frontend helpers, view-models, or page state belong in `front/tests/*.test.mjs`. Keep using the current `node --test` style. Do not introduce Vitest or Jest by default.
- Pure database semantics, migrations, and repository invariants should usually live in the relevant Rust module or in `server/src/database/mod.rs`, using `open_test_sqlite_connection*()`.
- Tests that need `AppState`, cache, log workers, or request persistence should use `TestDbContext::new_sqlite(...)` and `create_test_app_state(...)`.
- Tests that need a real proxy request path, mock upstream, and persisted request logs, attempts, or bundles should follow `server/src/proxy/integration.rs` with `run_integration_test(...)`, `spawn_test_upstream_or_skip(...)`, and `TestFixture`.

See [references/project-testing.md](references/project-testing.md) for the concrete patterns and short templates.

## Backend Hard Rules

- Do not reintroduce shared sqlite files, global test DB overrides, or the old "global lock guarantees correctness" model.
- Integration tests should be designed for normal parallel `cargo test` execution. Do not treat `--test-threads=1` as a routine fix.
- Before asserting request logs, attempts, or artifacts, call `app_state.flush_proxy_logs().await`.
- In test builds, storage is forced to a local temporary directory. If you do not have a real S3 environment, do not claim S3 verification.
- If a change touches background workers, log persistence, request replay, or route fallback/concurrency, prefer adding isolation and parallel regression coverage.
- Prefer assertions on operator-visible gateway behavior: status code, response body, `request_log`, `request_attempt`, bundle contents, and governance results. Do not overfit to private implementation details.

## Align With Repository Priorities

- This product is a single-admin gateway, not a multi-tenant SaaS control plane. Tests should bias toward stability, observability, governance, routing, and replay behavior.
- If you change transform logic, add or update transform-related tests.
- If you change pricing or governance, add domain-level assertions instead of only controller DTO tests.
- If you change proxy or logging paths, prefer integration coverage over isolated mock-only tests.

## Verification Order

1. Run the narrowest target test first.
2. Then run the related module or subsystem tests.
3. If the change affects runtime behavior, logging, or routing, run a wider backend suite.
4. If the frontend change is limited to pure state logic, `front/tests` is usually enough. If the API contract changed, add backend verification too.

Common commands:

- `rtk cargo test -p cyder-api <test-or-module> -- --nocapture`
- `rtk cargo test -p cyder-api proxy::integration:: -- --nocapture`
- `rtk cargo test -p cyder-api -- --nocapture`
- `rtk cargo xtask test`
- `rtk npm --prefix front test`

## Output Expectations

- Reuse the nearest existing test file and helper pattern whenever possible.
- New test names should be self-explanatory and describe the business contract directly.
- If failures are easier to diagnose with the response body or persisted payload, print the relevant context inside the test instead of forcing a second debugging round.
