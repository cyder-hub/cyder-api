# Cyder API

Cyder API is a single-admin LLM gateway built with Rust and Vue. It sits between downstream callers and upstream model providers, handling protocol translation, API key governance, runtime operations, request logging, and cost management.

The project is already suitable for:

- self-hosted usage by one administrator
- issuing multiple downstream API keys with governance limits
- proxying multiple upstream providers behind a unified gateway
- operating the system through a management console

It is not yet a fully mature high-availability gateway. Before 1.0, the focus is a small, stable direct-routing core, manager-auth hardening, transform quality, and removal of legacy contracts. Retry/fallback, replay, and proactive alerting require new designs and are intentionally absent from the current baseline.

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
- provider, model, and downstream API key management
- API key governance: expiry, RPM, concurrency, daily/monthly quota, daily/monthly budget
- provider circuit governance and runtime status views
- request patch rules with inheritance, conflict detection, and runtime trace
- request-level log persistence with status, timing, token, and cost summaries
- dashboard, provider runtime, record, API key, and cost management pages
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
- `reka-ui`

## Repository Layout

### Backend

- `server/src/controller`: management API endpoints
- `server/src/proxy`: gateway request path, routing, auth, proxy execution, logging
- `server/src/service`: app state, cache, transform, runtime state, and request patch resolution
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

For local development, the default command prepares the data directory and generated defaults:

```bash
just dev
```

Before the first successful server start, create `.cyder/dev/config/config.yaml` and set the required `secret_encryption.encryption_key`. When `CYDER_DATA_DIR` and `CYDER_CONFIG_PATH` are not set, `just dev` and `just dev-backend` run the backend with:

```txt
CYDER_DATA_DIR=<repo>/.cyder/dev
```

The directory is git-ignored and holds generated local state:

- `.cyder/dev/config/config.default.yaml`
- `.cyder/dev/config/config.yaml`, if you create one
- `.cyder/dev/db/cyder.sqlite`
- `.cyder/dev/tmp`

Repository-root `config.local.yaml` and `config.yaml` are no longer read automatically for development. To migrate an older local setup, copy it to `.cyder/dev/config/config.yaml`, or run with `CYDER_CONFIG_PATH=/path/to/config.yaml`; settings no longer recognized by the current schema are ignored.

Release and packaged runs also do not use application-root `config.default.yaml` or `config.yaml` as implicit persistence paths. If `CYDER_DATA_DIR` is unset, persistent paths are still derived from `/data/cyder`; use `CYDER_CONFIG_PATH` only when the base config file must live outside that data directory.

Runtime config is loaded in this order, from lowest to highest priority:

1. program defaults compiled into the server
2. bootstrapped `config.default.yaml`
3. base config, normally `${CYDER_DATA_DIR}/config/config.yaml`
4. allowlisted environment variables

`config.default.yaml` is generated on first startup to persist random secrets and path-aware defaults. It is runtime state, not a tracked sample that should be hand-maintained.

Configuration is startup-only. Change the base YAML or an allowlisted environment variable, then restart the server. The retired `config.override.yaml` and `config.override.history.jsonl` files are ignored; move any settings that must remain active into `config.yaml`.

Unknown top-level and nested fields in `config.yaml` and the generated `config.default.yaml` are ignored during 1.0 development. This lets older experimental settings remain temporarily while the schema changes. Recognized fields still fail startup when their type, enum value, or validated value is invalid.

Important config areas include:

- server bind settings: `host`, `port`, `base_path`
- manager auth: initialize the manager password in the Web Bootstrap page; `jwt_secret` signs manager tokens
- database: `db_url`
- downstream secret handling: `secret_encryption`
- proxy request behavior: `proxy_request`
- provider governance: `provider_governance`
- cache: `cache`, optional `redis`

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
- `CYDER_CONFIG_PATH`: optional migration hook for an external base config file. It changes only the base config path; generated defaults and SQLite data still belong to the data directory.

Database URLs, secrets, Redis/cache, runtime state, proxy settings, and governance settings are configured through YAML, not environment variables. `CYDER_LOG_THIRD_PARTY_DEBUG` remains a logging diagnostic switch and is not part of `FinalConfig`.

Pre-1.0 supports one running Cyder server instance. Redis remains optional and can preserve short-lived runtime state across process restarts, but it does not enable a supported multi-instance deployment.

### Secret Encryption and Provider Credentials

Every installation must configure one current 32-byte master key, regardless of downstream mode. Generate an independent random value (for example, `openssl rand -hex 32`) and place it in the base YAML before startup:

```yaml
secret_encryption:
  downstream_mode: one_time
  encryption_key: "<exactly 64 hexadecimal characters>"
  previous_encryption_key: null
```

`encryption_key` must contain exactly 64 hexadecimal characters without an encoding prefix. It is required even in the default `one_time` mode because Provider credentials are always stored as XChaCha20-Poly1305 ciphertext. The keys are read only from normal YAML configuration; there is no environment-variable, generated fallback, key-file, or KMS source.

Downstream API keys always authenticate through a stored SHA-256 hash. `one_time` makes Create and Rotate return plaintext once and stores no recoverable copy. Save that response immediately; if it is lost, rotate the downstream API key again. `recoverable` additionally stores downstream ciphertext and enables an explicit manager Reveal action.

Provider credentials use the same master key with a separate authenticated-encryption domain. Manager list/detail responses, runtime caches, and normal logs contain only identifiers, masks, or ciphertext—not usable plaintext. Create and Replace accept a new secret, Reveal is an explicit POST action, and disabled or deleted Provider keys are not selected. Vertex and Vertex OpenAI credentials must be complete Google service-account JSON; the service account is decrypted only long enough to obtain an OAuth token.

To replace a configured master key, set the new value as `encryption_key` and the existing value as `previous_encryption_key`, then restart. Before Axum begins serving requests, Cyder processes downstream and Provider ciphertext in one database transaction. Matching Previous ciphertext is re-encrypted with Current; every non-deleted Provider credential must also authenticate, decrypt, and match its keyed fingerprint. Remove `previous_encryption_key` only after a successful startup. Unknown or damaged downstream ciphertext is preserved and becomes unavailable to Reveal, but an unknown, incomplete, damaged, or mismatched Provider credential rolls back the whole rotation and prevents startup.

Losing the master key does not invalidate downstream callers because their authentication remains hash-based, but it does make encrypted downstream plaintext unavailable and prevents Provider credentials from being used. Restore the correct key configuration; after startup, rotate affected downstream keys and Replace affected Provider credentials as necessary.

### R2.7 Development Upgrade Boundary

R2.7 is intentionally destructive for pre-1.0 Provider credentials. Its paired SQLite/PostgreSQL migration deletes every historical `provider_api_key` row, clears `request_log.provider_api_key_id`, and preserves providers, models, and request logs. Before upgrading:

1. Back up the database if you need a rollback snapshot.
2. Configure the required current `secret_encryption.encryption_key`.
3. Apply/start the upgraded application.
4. Re-enter each Provider credential in the Provider management page and verify connectivity.

Historical Provider secrets are not migrated or recoverable after this migration. The final pre-1.0-to-1.0 upgrade will require a clean database as described in the roadmap. The previous Portable Config import/export implementation has also been removed; no current endpoint, UI, file format, or compatibility path can be used to preserve or transfer these credentials. Portable Config will be redesigned from a new 1.0 domain contract in R7.9.

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

Current test coverage is strong across transform, proxy, cost, governance, and runtime logic.

## Current Priorities

Based on the current codebase, the most valuable next steps are:

1. stabilize and verify the direct `provider/model` execution path
2. tighten manager authentication and secret ownership
3. productize transform diagnostics without recreating the retired request-bundle contract
4. finish the remaining `api_key` naming, documentation, and test convergence
5. redesign retry/fallback, replay, and proactive alerts as separate domains before implementation

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

- generated defaults and base configuration: `/data/cyder/config`
- SQLite database files: `/data/cyder/db`

The image sets `CYDER_DATA_DIR=/data/cyder`, declares `/data/cyder` as the only volume, and runs the service process as the non-root `cyder` user. Temporary process files use `/tmp/cyder-api` and are not persisted.

Advanced deployments can override `CYDER_DATA_DIR`, but the default and recommended path remains `/data/cyder`. When overriding it, mount the replacement directory explicitly:

```bash
docker run --rm -p 8000:8000 \
  -e CYDER_DATA_DIR=/var/lib/cyder \
  -v ./cyder-data:/var/lib/cyder \
  cyder-api:latest
```

PostgreSQL and Redis are external state. Configure those services in `/data/cyder/config/config.yaml` and restart the container; do not pass database, secret, or Redis settings through environment variables.

For an existing deployment, mount the old config file into the container and point `CYDER_CONFIG_PATH` at it while keeping `/data/cyder` as the persistent data root:

```bash
docker run --rm -p 8000:8000 \
  -v ./cyder-data:/data/cyder \
  -v /path/to/config.yaml:/etc/cyder/config.yaml:ro \
  -e CYDER_CONFIG_PATH=/etc/cyder/config.yaml \
  cyder-api:latest
```

This migration hook only changes the base config file path. Generated defaults and the default SQLite path remain under `/data/cyder`.

## Release Workflow

Release publishing is tag-driven. Push a semver tag that matches `server/Cargo.toml`, for example `v0.7.0`, and the `Release` workflow will:

1. verify the tag is on `master` and that CI passed for the tagged commit
2. run the transform quality gate
3. build and push GHCR/Docker Hub `X.Y.Z` images
4. create or update a draft GitHub Release with a release manifest and checksum
5. after the `release` environment gate, promote `X.Y` and `latest` image tags and publish the GitHub Release

Do not publish releases directly from the GitHub Release UI. Use the same tag with `workflow_dispatch` only to recover a failed release run. Recovery reruns reuse existing `X.Y.Z` images only when both registries already contain them; if only one registry has the version image, the workflow fails instead of overwriting an immutable tag.

## Summary

Cyder API is already beyond the "CRUD plus proxy" stage. It has the core shape of a single-admin LLM gateway with strong transformation logic and an operations console. The next stage is to make that core smaller, safer, and easier to reason about before selectively rebuilding advanced resilience and debugging capabilities.
