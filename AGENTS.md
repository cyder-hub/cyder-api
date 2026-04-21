# AGENTS Guidelines

This repository is a single-admin LLM gateway. It is not a multi-tenant SaaS control plane.

Use that assumption when making product and engineering decisions:

- optimize for one administrator operating the whole system
- downstream users call the proxy with issued API keys
- prioritize gateway stability, observability, and governance
- do not introduce RBAC, teams, workspaces, or end-user portals unless explicitly requested

## Current Product Status

The codebase already has:

- multi-protocol proxying for OpenAI, Responses, Anthropic, Gemini, and Ollama
- deep protocol transformation, including streaming/tool/reasoning/multimodal paths
- provider/model/model-route/api-key management
- API key governance with expiry, RPM, concurrency, quota, and budget
- provider runtime aggregation and dashboard alerts
- request patch rules with explain/conflict/runtime trace
- request log persistence with object-storage bundles
- cost catalog/version/component/template/preview flows

The most important missing capabilities today are:

- request-level retry / fallback
- execution-time use of full route candidate queues
- replay/debug tooling
- proactive alert channels
- fuller transform diagnostics productization
- manager auth hardening

When deciding what to build next, bias toward those areas.

## Tech Stack

### Frontend

- Vue 3
- TypeScript
- Vite
- Pinia
- Vue Router
- Tailwind CSS 4
- `reka-ui` / `radix-vue`
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
- `server/src/service`: app state, transform, cache, storage, redis, request patch logic
- `server/src/cost`: cost normalization, ledger, pricing engine, templates
- `server/src/utils`: shared support utilities

## Routing And Product Guidance

Treat the product as a gateway first, not a generic admin app.

Good investments:

- route candidate execution and resilience
- retry / fallback
- observability and runtime operations
- replay and debugging
- API key governance
- cost visibility

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

- which provider/model/route was selected
- why a request failed
- whether governance rejected the request
- whether runtime state or storage/logging paths degraded

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

- `config.default.yaml`
- `config.local.yaml` in development when present
- otherwise `config.yaml`
- environment variables override file values

Default `base_path` is `/ai`.

Manager routes are under:

- `/ai/manager/ui`
- `/ai/manager/api/*`

Proxy routes are under:

- `/ai/openai/*`
- `/ai/responses/*`
- `/ai/anthropic/*`
- `/ai/gemini/*`
- `/ai/ollama/*`

## Use Cargo Xtask To Manage The Project

This repository uses `cargo xtask` for common workflows.

| Command | Purpose |
| --- | --- |
| `cargo xtask dev` | Start backend and frontend dev servers |
| `cargo xtask dev-backend` | Start backend dev server |
| `cargo xtask dev-front` | Install frontend deps and start frontend dev server |
| `cargo xtask build` | Build backend and frontend |
| `cargo xtask build-backend` | Build backend only |
| `cargo xtask build-front` | Build frontend only |
| `cargo xtask install-front-deps` | Install frontend dependencies |
| `cargo xtask test` | Run backend tests |

## Testing And Verification

- Add tests for new backend functionality.
- Prefer integration coverage for routing/governance/logging behavior changes.
- If you touch transform logic, add or update transform tests.
- If you touch pricing or governance, add assertions at the domain level, not just controller level.

Current note:

- backend tests are broadly healthy, but S3 storage integration tests may require a valid S3-compatible environment to verify object-storage paths fully

Do not claim storage-related verification unless you actually ran it in a valid environment.

## Best Practices

- Be conservative with existing behavior, especially proxy and logging paths.
- Prefer best-practice end-state design over preserving already-known weak abstractions.
- Do not keep expanding legacy `system_api_key` semantics for new governance work; prefer the newer `api_key` aggregate direction.
- Outside explicit database compatibility boundaries, business-layer naming should use `api_key` and must not expose new `system_api_key` DTO or UI fields.
- Use clear error handling and preserve operator-facing diagnostic value.
- When in doubt, improve debuggability.
