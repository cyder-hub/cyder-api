# Cyder API

Cyder API is a single-admin LLM gateway built with Rust and Vue. It sits between downstream callers and upstream model providers, handling protocol translation, API key governance, runtime operations, request logging, and cost management.

The project is already suitable for:

- self-hosted usage by one administrator
- issuing multiple downstream API keys with governance limits
- proxying multiple upstream providers behind a unified gateway
- operating the system through a management console

It is not yet a fully mature high-availability gateway. The biggest remaining gaps are request-level retry/fallback, replay/debug tooling, proactive alerting, and a few legacy naming/security cleanups.

## Current Product Position

This repository should be understood as:

- a single-admin LLM gateway
- not a multi-tenant SaaS platform
- not a user self-service portal
- not an RBAC/team/workspace product

If you are making roadmap decisions, prioritize:

- gateway stability
- routing and upstream resilience
- observability and debugging
- API key governance
- cost visibility

Do not prioritize multi-tenant account systems unless explicitly required.

## What Already Exists

Current code already provides:

- multi-protocol proxying for OpenAI, Responses, Anthropic, Gemini, and Ollama
- deep request/response transformation, including streaming, tool calls, reasoning, and multimodal content
- provider, model, model route, and API key override management
- API key governance: expiry, RPM, concurrency, daily/monthly quota, daily/monthly budget
- provider circuit governance and runtime status views
- request patch rules with inheritance, conflict detection, and runtime trace
- request log persistence with request/response body bundles
- dashboard, provider runtime, record, API key, route, and cost management pages
- cost catalog/version/component/template/preview management

## Tech Stack

### Backend

- Rust 1.89+
- Axum
- Tokio
- Diesel
- SQLite / PostgreSQL
- Reqwest

### Frontend

- Vue 3
- TypeScript
- Vite
- Pinia
- Vue Router
- Tailwind CSS 4
- `reka-ui` / `radix-vue`

## Repository Layout

### Backend

- `server/src/controller`: management API endpoints
- `server/src/proxy`: gateway request path, routing, auth, proxy execution, logging
- `server/src/service`: app state, cache, storage, transform, request patch resolution
- `server/src/database`: persistence models and DB operations
- `server/src/cost`: rating, normalization, ledger, templates
- `server/migrations`: SQLite and PostgreSQL migrations

### Frontend

- `front/src/pages`: management console pages
- `front/src/components`: shared components and UI primitives
- `front/src/services`: API access and auth helpers
- `front/src/store`: Pinia stores and shared types
- `front/src/router`: route definitions and auth guard

### Workspace Utilities

- `justfile`: optional local shortcuts for dev/build/test
- `task`: internal analysis and planning notes

## Local Development

### Requirements

- Rust toolchain with Cargo
- Node.js 24+
- npm
- `just`

### Configuration

For local development, the default command is zero-config:

```bash
just dev
```

When `CYDER_DATA_DIR` and `CYDER_CONFIG_PATH` are not set, `just dev` and `just dev-backend` run the backend with:

```txt
CYDER_DATA_DIR=<repo>/.cyder/dev
```

The directory is git-ignored and holds generated local state:

- `.cyder/dev/config/config.default.yaml`
- `.cyder/dev/config/config.yaml`, if you create one
- `.cyder/dev/config/config.override.yaml`
- `.cyder/dev/config/config.override.history.jsonl`
- `.cyder/dev/db/cyder.sqlite`
- `.cyder/dev/storage`
- `.cyder/dev/tmp`

Repository-root `config.local.yaml` and `config.yaml` are no longer read automatically for development. To migrate an older local setup, copy the old base config to `.cyder/dev/config/config.yaml`, or run with `CYDER_CONFIG_PATH=/path/to/config.yaml`.

Release and packaged runs also do not use application-root `config.default.yaml`, `config.yaml`, `config.override.yaml`, or `config.override.history.jsonl` as implicit persistence paths. If `CYDER_DATA_DIR` is unset, persistent paths are still derived from `/data/cyder`; use `CYDER_CONFIG_PATH` only when the base config file must live outside that data directory.

Runtime config is loaded in this order, from lowest to highest priority:

1. program defaults compiled into the server
2. bootstrapped `config.default.yaml`
3. base config, normally `${CYDER_DATA_DIR}/config/config.yaml`
4. allowlisted environment variables
5. managed `config.override.yaml`

`config.default.yaml` is generated on first startup to persist random secrets and path-aware defaults. It is runtime state, not a tracked sample that should be hand-maintained.

`config.override.yaml` is a managed override file written by the System Config page in the management console. It is only for the hot-reload allowlist exposed by that page, such as log level, timezone, proxy request timeout settings, routing resilience, provider governance, and diagnostics retention/capture settings.

Do not use `config.override.yaml` as a general replacement for `config.yaml`. Bind settings, manager secrets, database, Redis/cache, storage, deployment mode, runtime state backend, and other non-allowlisted settings must be changed in the base config file and applied with a server restart. If a non-allowlisted path is present in `config.override.yaml`, startup/reload/apply validation rejects it instead of treating it as a restart-only override.

The management console can reload `config.override.yaml` after a manual edit, but routine edits should be made through the UI so preview, validation, and audit history stay consistent. `config.override.history.jsonl` records apply/reset/reload history for audit display only; it is not part of configuration loading.

In the first version of this feature, multi-instance deployments are read-only for System Config writes. This prevents multiple instances from diverging through separate local override files.

Important config areas include:

- server bind settings: `host`, `port`, `base_path`
- manager auth: `secret_key`, `jwt_secret`
- downstream API key JWT: `api_key_jwt_secret`
- database: `db_url`
- proxy request behavior: `proxy_request`
- provider governance: `provider_governance`
- cache: `cache`, optional `redis`
- storage: local filesystem or S3-compatible object storage

Current built-in database backends are SQLite and PostgreSQL; other database URL schemes are not supported.

Default `base_path` is `/ai`.

The only environment variables that can override final config fields are:

- `CYDER_HOST`
- `CYDER_PORT`
- `CYDER_BASE_PATH`
- `CYDER_LOG_LEVEL`
- `CYDER_TIMEZONE`

Startup path environment variables are separate:

- `CYDER_DATA_DIR`: data directory root. Docker images set this to `/data/cyder`.
- `CYDER_CONFIG_PATH`: optional migration hook for an external base config file. It changes only the base config path; default config, System Config override/history, SQLite defaults, and local storage still belong to the data directory.

Database URLs, secrets, Redis/cache, S3, local storage roots, deployment mode, runtime state, proxy settings, and governance settings are configured through YAML, not environment variables. `CYDER_LOG_THIRD_PARTY_DEBUG` remains a logging diagnostic switch and is not part of `FinalConfig`.

## Common Commands

Human local shortcuts are available through `just` from the repository root:

| Command | Purpose |
| --- | --- |
| `just --list` | Show available shortcuts |
| `just dev` | Run backend and frontend dev servers together |
| `just dev-backend` | Run backend dev server |
| `just dev-front` | Ensure frontend deps and run Vite dev server |
| `just install-front-deps` | Ensure frontend dependencies for development |
| `just front-ci-deps` | Install locked frontend dependencies |
| `just build` | Build backend and frontend |
| `just build-backend` | Build backend |
| `just build-front` | Build frontend |
| `just test` | Run backend and frontend tests |
| `just test-backend` | Run backend tests |
| `just test-front` | Run frontend tests |
| `just check` | Run local aggregate checks |
| `just fmt` | Format Rust sources |
| `just fmt-check` | Check Rust formatting |
| `just log-lint` | Run backend log lint |
| `just i18n-check` | Check frontend i18n coverage |
| `just transform-gate` | Run transform quality gate |
| `just transform-gate-report` | Run transform quality gate and write a JSON report |

## Portable Config Export And Import

Portable Config is the v1 migration and backup path for gateway configuration. It exports a `.cyd` bundle from the manager API and imports it into a fresh or existing environment through a required preview step.

Manager API endpoints:

- `GET /ai/manager/api/system/portable/modules`
- `POST /ai/manager/api/system/portable/export`
- `POST /ai/manager/api/system/portable/import/preview`
- `POST /ai/manager/api/system/portable/import/apply`

Supported modules:

- `provider_profile`: providers, provider API keys, provider models, request patch rules, and reasoning config.
- `api_keys`: downstream API keys, ACL rules, and model override references to routes that already exist in the target environment.
- `cost_catalogs`: cost catalogs, versions, and components.
- `cost_bindings`: model-to-cost-catalog bindings through `model.cost_catalog_id`.

Export files can be plaintext JSON or password-encrypted armored `.cyd` files. Use password encryption when the bundle contains provider keys or downstream API keys. The encrypted format hides the whole JSON bundle; plaintext export intentionally contains raw secrets so the target environment can preserve existing downstream keys.

Import always starts with preview. Preview validates the schema, password and integrity status, module versions, dependencies, conflicts, missing provider/model/route/cost references, and dangerous request patch targets. Apply must submit the same bundle digest returned by preview.

For existing downstream API keys, `overwrite_existing` updates API key metadata and governance limits only. Bundle ACL rules and model overrides for an already-existing raw API key are counted as skipped and are not appended, upserted, replaced, or used to delete target-environment child rows. ACL rules and model overrides are imported only when the API key itself is newly created.

Portable Config intentionally does not migrate these runtime, history, audit, or deployment records:

- `request_log`, `request_attempt`, `request_replay_run`, replay artifacts, object-storage bundles, and object-storage artifacts.
- `metric_ingested_request_log`, `metric_request_rollup_minute`, `metric_attempt_rollup_minute`, `metric_http_status_rollup_minute`, and `metric_cost_rollup_minute`.
- `alert_event`, `alert_rule_state`, `notification_channel`, `notification_channel_state`, `notification_delivery`, and notification test results.
- `api_key_rollup_daily`, `api_key_rollup_monthly`, `manager_auth_instance`, refresh sessions, and manager login rate-limit runtime.
- Provider circuit runtime state, provider key cursors, API key concurrency windows, API key RPM windows, and Redis-backed runtime state.
- `config.default.yaml`, `config.yaml`, `config.override.yaml`, and `config.override.history.jsonl`.

Those are runtime facts or deployment configuration, not portable gateway configuration.

Release verification for this feature must include:

- plaintext core bundle export/import into fresh SQLite
- password-encrypted full bundle export/import into fresh SQLite
- provider/provider key/model/API key/cost catalog/model cost binding lookups after import
- frontend export/import state tests

## Main Routes

Assuming `base_path: /ai`:

### Management Console

- UI: `/ai/manager/ui`
- API: `/ai/manager/api/*`

### Gateway Endpoints

- OpenAI-compatible: `/ai/openai/v1/*`
- Responses-compatible: `/ai/responses/v1/*`
- Anthropic-compatible: `/ai/anthropic/v1/*`
- Gemini-compatible: `/ai/gemini/v1/*`
- Ollama-compatible: `/ai/ollama/api/*`

### System Endpoints

- health: `/ai/health`
- readiness: `/ai/ready`

## Testing Notes

Primary backend verification:

- `just check`

Frontend verification:

- `just i18n-check`
- `just test-front`
- `just build-front`

Current test coverage is strong across transform, proxy, cost, governance, and runtime logic. However, storage integration around S3 may require a working S3-compatible environment when configured. Do not assume object-storage paths are verified unless you have run the relevant tests in a valid environment.

## Current Priorities

Based on the current codebase, the most valuable next steps are:

1. make `model_route` candidates participate in real execution
2. add request-level retry/fallback and attempt trace
3. add replay/debug tooling for request logs
4. productize transform diagnostics for operations/debugging
5. add proactive alert channels
6. tighten manager auth and finish the remaining `api_key` convergence/docs/test cleanup

## Docker

Build the image from the repository root:

```bash
docker build -t cyder-api:latest .
```

Run it with the built-in zero-config defaults:

```bash
docker run --rm -p 8000:8000 cyder-api:latest
```

For persistent local state, mount one host directory to `/data/cyder`:

```bash
mkdir -p cyder-data
docker run --rm -p 8000:8000 -v ./cyder-data:/data/cyder cyder-api:latest
```

The runtime image keeps application artifacts under `/opt/cyder`:

- binary: `/opt/cyder/bin/cyder-api`
- management UI assets: `/opt/cyder/public`

Mutable local state is under `/data/cyder`:

- config files and System Config override/history: `/data/cyder/config`
- SQLite database files: `/data/cyder/db`
- local request log and replay object storage: `/data/cyder/storage`

The image sets `CYDER_DATA_DIR=/data/cyder`, declares `/data/cyder` as the only volume, and runs the service process as the non-root `cyder` user. Temporary request-log spool files use `/tmp/cyder-api` and are not persisted.

Advanced deployments can override `CYDER_DATA_DIR`, but the default and recommended path remains `/data/cyder`. When overriding it, mount the replacement directory explicitly:

```bash
docker run --rm -p 8000:8000 \
  -e CYDER_DATA_DIR=/var/lib/cyder \
  -v ./cyder-data:/var/lib/cyder \
  cyder-api:latest
```

Maintainer verification on 2026-05-06 built `cyder-api:task10` from this Dockerfile and confirmed zero-config startup, empty `/data/cyder` volume startup, container recreation with persisted `config.default.yaml`/SQLite/override/history, request-log bundle writes under `/data/cyder/storage`, read-only `/opt/cyder` behavior for the service user, and management UI asset loading from `/opt/cyder/public`. S3 persistence was not verified in this smoke test.

PostgreSQL, S3-compatible object storage, and Redis are external state. Configure those services in `/data/cyder/config/config.yaml` and restart the container; do not pass database, secret, Redis, or S3 settings through environment variables.

For an existing deployment, mount the old config file into the container and point `CYDER_CONFIG_PATH` at it while keeping `/data/cyder` as the persistent data root:

```bash
docker run --rm -p 8000:8000 \
  -v ./cyder-data:/data/cyder \
  -v /path/to/config.yaml:/etc/cyder/config.yaml:ro \
  -e CYDER_CONFIG_PATH=/etc/cyder/config.yaml \
  cyder-api:latest
```

This migration hook only changes the base config file path. Managed override/history files and default local SQLite/storage paths remain under `/data/cyder`.

## Summary

Cyder API is already beyond the "CRUD plus proxy" stage. It now has the core shape of a serious single-admin LLM gateway, with strong transformation logic and a growing operations console. The next stage is not more platform surface area; it is resilience, replay, alerting, and tighter operational feedback loops.
