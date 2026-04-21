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

- `xtask`: project management commands for dev/build/test
- `task`: internal analysis and planning notes

## Local Development

### Requirements

- Rust toolchain with Cargo
- Node.js 24+
- npm

### Configuration

Runtime config is loaded from:

- `config.default.yaml`
- `config.local.yaml` in development if present
- otherwise `config.yaml`
- environment variables override file values

Important config areas include:

- server bind settings: `host`, `port`, `base_path`
- manager auth: `secret_key`, `jwt_secret`
- downstream API key JWT: `api_key_jwt_secret`
- database: `db_url`
- proxy request behavior: `proxy_request`
- provider governance: `provider_governance`
- cache: `cache`, optional `redis`
- storage: local filesystem or S3-compatible object storage

Default `base_path` is `/ai`.

## Common Commands

Use `cargo xtask` from the repository root.

| Command | Purpose |
| --- | --- |
| `cargo xtask dev` | Run backend and frontend dev servers together |
| `cargo xtask dev-backend` | Run backend dev server |
| `cargo xtask dev-front` | Install frontend deps and run Vite dev server |
| `cargo xtask build` | Build backend and frontend |
| `cargo xtask build-backend` | Build backend only |
| `cargo xtask build-front` | Install frontend deps and build frontend |
| `cargo xtask install-front-deps` | Install frontend dependencies |
| `cargo xtask test` | Run backend test suite |

Useful direct frontend commands:

- `cd front && npm run dev`
- `cd front && npm run build`
- `cd front && npm test`

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

- `cargo xtask test`

Frontend verification:

- `cd front && npm test`
- `cd front && npm run build`

Current test coverage is strong across transform, proxy, cost, governance, and runtime logic. However, storage integration around S3 may require a working S3-compatible environment when configured. Do not assume object-storage paths are verified unless you have run the relevant tests in a valid environment.

## Current Priorities

Based on the current codebase, the most valuable next steps are:

1. make `model_route` candidates participate in real execution
2. add request-level retry/fallback and attempt trace
3. add replay/debug tooling for request logs
4. productize transform diagnostics for operations/debugging
5. add proactive alert channels
6. tighten manager auth and clean up legacy `system_api_key` naming

## Docker

Build the image from the repository root:

```bash
docker build -t cyder-api:latest .
```

## Summary

Cyder API is already beyond the "CRUD plus proxy" stage. It now has the core shape of a serious single-admin LLM gateway, with strong transformation logic and a growing operations console. The next stage is not more platform surface area; it is resilience, replay, alerting, and tighter operational feedback loops.
