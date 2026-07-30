# AGENTS Guidelines

This repository is a single-admin LLM gateway. It is not a multi-tenant SaaS control plane.

Use that assumption when making product and engineering decisions:

- optimize for one administrator operating the whole system
- downstream users call the proxy with issued API keys
- prioritize gateway stability, observability, and governance
- do not introduce RBAC, teams, workspaces, or end-user portals unless explicitly requested

## Current Product Status

The codebase already has:

- four public downstream protocol families: OpenAI, Responses, Anthropic, and Gemini
- five upstream wire families: OpenAI, Responses, Anthropic, Gemini, and Ollama
- deep protocol transformation, including streaming/tool/reasoning/multimodal paths
- provider/model/api-key management
- API key governance with expiry, RPM, concurrency, quota, and budget
- provider runtime aggregation and dashboard operational signals
- request patch rules with explain/conflict/runtime trace
- request-level log persistence with status, timing, token, and cost summaries
- cost catalog/version/component/template/preview flows

Model routes, request attempts, retry/fallback, request replay/bundles,
alert/notification delivery, and the generic System Config control plane were
deliberately removed before 1.0. Do not restore their old DTOs, tables,
configuration layers, or disabled skeletons. Future designs must follow the
records under `task/refactor/` and begin from explicit domain contracts.

The most important remaining capability work is manager auth hardening and
careful productization of transform diagnostics without recreating the retired
request bundle contract.

When deciding what to build next, bias toward those areas.

## Tech Stack

### Frontend

- Vue 3
- TypeScript
- Vite
- Pinia
- Vue Router
- Tailwind CSS 4
- `reka-ui`
- `class-variance-authority` for variants

### Backend

- Rust 1.89+
- Axum
- Tokio
- Serde
- Diesel
- SQLite / PostgreSQL

## Project Structure

- Server-side code lives under `/server`.
- Frontend code lives under `/front`.

### Frontend

- `front/src/pages`: page-level management console views
- `front/src/components`: shared components, including `ui` primitives
- `front/src/services`: auth and HTTP request helpers
- `front/src/store`: Pinia stores and shared frontend types
- `front/src/router`: frontend routes and auth guards
- `front/src/layouts`: management UI layouts

### Backend

- `server/migrations`: SQL migrations for SQLite and PostgreSQL
- `server/src/database`: DB models and persistence logic
- `server/src/schema`: Diesel-generated schema
- `server/src/controller`: management API handlers
- `server/src/proxy`: gateway runtime path, auth, routing, execution, and logging
- `server/src/service`: app state, transform, cache, redis, runtime state, and request patch logic
- `server/src/cost`: cost normalization, ledger, pricing engine, templates
- `server/src/utils`: shared support utilities

## Routing And Product Guidance

Treat the product as a gateway first, not a generic admin app.

Good investments:

- direct provider/model execution stability
- observability and runtime operations
- API key governance
- cost visibility
- manager authentication hardening
- transform diagnostics with explicit evidence contracts

Retry/fallback, routing candidates, replay, and proactive alerts may return only
after their domain contracts are redesigned. Do not rebuild them by restoring
the deleted tables, DTOs, configuration, or UI.

Poor default investments:

- multi-user admin systems
- tenant models
- team/project/workspace hierarchies
- user self-service dashboards
- end-user billing portals

## Backend Logging Guide

Use `cyder_tools::log` for backend logging:

```rust
use cyder_tools::log::{debug, info, warn, error};
```

### Log Levels

- `debug`: development-only detail or targeted production debugging
- `info`: normal operational milestones
- `warn`: degraded behavior that does not stop the system
- `error`: fatal or near-fatal failures that need attention

Prefer logs that help answer:

- which provider/model was selected
- why a request failed
- whether governance rejected the request
- whether runtime state or request logging degraded

## Frontend Guide

The frontend is a Vue admin console, not a Solid app.

### UI Conventions

- Reuse `front/src/components/ui` primitives before creating ad hoc controls.
- Follow the existing light admin visual system already used in `Dashboard.vue`, `ProviderRuntime.vue`, `ApiKey.vue`, and `Record.vue`.
- Keep layouts practical and operations-focused; this UI exists for one administrator.
- Prefer Tailwind utility classes and existing helper utilities such as `cn`.
- Use `class-variance-authority` when a component genuinely has reusable variants.

### State And Data

- Use Pinia stores for shared management state.
- Use `front/src/services/request.ts` for API access instead of ad hoc fetch wrappers.
- Keep route-aware UI in `front/src/pages` and reusable logic in components/composables.

### UX Priorities

- Optimize for troubleshooting speed, not marketing polish.
- Prefer surfacing runtime state, cost, and failure context over decorative UI.
- When adding new screens, think "operator console" first.

## Configuration Notes

The app loads configuration from:

- program defaults
- `config.default.yaml` under the resolved data directory config path
- base `config.yaml` under the resolved data directory config path, or `CYDER_CONFIG_PATH` when explicitly set
- environment variables

Repository/application-root `config.default.yaml`, `config.local.yaml`, and `config.yaml` are not implicit persistence paths. Debug defaults derive from `.cyder/dev`; release defaults derive from `/data/cyder` unless `CYDER_DATA_DIR` is explicitly set.

Configuration is startup-only. Change the base config or an allowlisted environment variable and restart the server. Retired `config.override.yaml` and `config.override.history.jsonl` files are ignored.

Unknown top-level and nested fields in the base YAML and generated `config.default.yaml` are ignored during 1.0 development. Recognized fields still fail startup when their type, enum value, or validated value is invalid.

Default `base_path` is `/ai`.

Manager routes are under:

- `/ai/manager/ui`
- `/ai/manager/api/*`

Proxy routes are under:

- `/ai/openai/*`
- `/ai/responses/*`
- `/ai/anthropic/*`
- `/ai/gemini/*`

Unversioned routes are direct compatibility aliases for current `/v1`
semantics; Gemini also exposes `/v1beta`. Ollama is upstream-only. The
generated [Protocol Compatibility Matrix](docs/protocol-compatibility.md)
defines the exact routes, provider profiles, current generation cells,
utilities, evidence, and follow-up owners.

## Command Entry Points

AI agent / vibe coding work must use native Cargo, npm, and Docker commands directly. Do not use `just` or `rtk just ...` unless the user explicitly allows `just` for the current task.

For Codex/AI agent execution, keep `rtk` as the outer command runner and do not write it into project files. Default to commands such as `rtk cargo ...`, `rtk npm --prefix front ...`, and `rtk docker ...`.

| Command | Purpose |
| --- | --- |
| `cargo run -p cyder-api` | Run backend dev server |
| `cargo build -p cyder-api --release` | Build backend release binary |
| `cargo fmt --check` | Check Rust formatting |
| `cargo run -p cyder-api --bin log_lint` | Run backend log lint |
| `cargo test -p cyder-api` | Run backend tests |
| `cargo run -p cyder-api --bin transform_quality_gate -- --quick` | Run transform quality gate |
| `npm --prefix front install` | Install frontend dependencies for development |
| `npm --prefix front ci` | Install locked frontend dependencies for CI/release builds |
| `npm --prefix front run dev` | Start frontend dev server |
| `npm --prefix front run i18n:check` | Check frontend i18n coverage |
| `npm --prefix front test` | Run frontend tests |
| `npm --prefix front run build` | Build frontend assets |

## Testing And Verification

- Add tests for new backend functionality.
- Prefer integration coverage for routing/governance/logging behavior changes.
- If you touch transform logic, add or update transform tests.
- If you touch pricing or governance, add assertions at the domain level, not just controller level.

Current backend tests are broadly healthy.

## Best Practices

- Be conservative with existing behavior, especially proxy and logging paths.
- Prefer best-practice end-state design over preserving already-known weak abstractions.
- Do not keep expanding legacy API key compatibility semantics for new governance work; prefer the newer `api_key` aggregate direction.
- Outside explicit database compatibility boundaries, business-layer naming should use `api_key` and must not expose new legacy API key DTO or UI fields.
- Use clear error handling and preserve operator-facing diagnostic value.
- When in doubt, improve debuggability.
