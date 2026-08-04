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

- four public downstream protocol families: OpenAI, Responses, Anthropic, and Gemini
- five upstream wire families: OpenAI, Responses, Anthropic, Gemini, and Ollama
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
- manager auth: initialize the password in Web Bootstrap; configure a 32-byte-or-longer `jwt_secret` and an exact `manager_auth.browser_origin` for non-loopback browser access
- trusted reverse proxies: `client_identity` (empty trust list by default)
- database: `db_url`
- downstream secret handling: `secret_encryption`
- proxy request behavior: `proxy_request`
- provider governance: `provider_governance`
- cache: `cache`, optional `redis`

Current built-in database backends are SQLite and PostgreSQL; other database URL schemes are not supported.

Default `base_path` is `/ai`.

### Proxy Request Identity

Every request under the four public proxy prefixes—`/ai/openai/*`, `/ai/responses/*`, `/ai/anthropic/*`, and `/ai/gemini/*`—receives a gateway-owned canonical request identity:

- Cyder always generates `X-Request-ID` as a lowercase, hyphenated UUID v4 at the outer proxy boundary.
- A caller-provided `X-Request-ID` is ignored. It cannot become the canonical identity or override the response, upstream request, logs, or Request Record.
- A caller may instead send one `X-Client-Request-ID`. It is accepted only when it is 1–64 ASCII characters from `[A-Za-z0-9._:-]`. Empty, unsafe, overlong, non-UTF-8, or repeated values are ignored without logging the rejected value.
- A valid client ID is echoed in `X-Client-Request-ID` and stored as an optional, non-unique troubleshooting field. It is caller-provided and must never be treated as identity, authentication, authorization, or trusted evidence.

The canonical ID is returned on success, authentication and client-identity errors, proxy-prefix 404/405 responses, and CORS preflight responses. It is sent to the selected upstream as `X-Request-ID`, included in structured request logs, and persisted in the existing Request Record paths. `X-Client-Request-ID` is never sent upstream. Manager, System, the base `/ai` fallback, and unknown `/ai/ollama/*` routes are outside this identity layer.

Request Patch Create and Update reject both `x-request-id` and `x-client-request-id`; upstream response headers with those names also cannot override the gateway response. The Record page displays and copies the gateway ID, shows the optional caller ID separately, and searches both values by exact match. Request IDs are evidence keys, not metric labels or W3C Trace IDs. Protocol error bodies include the same canonical ID only where the downstream contract requires it: Anthropic at `request_id`, and Gemini in `google.rpc.ErrorInfo.metadata.request_id`.

The paired R3.2 SQLite/PostgreSQL development migration is intentionally destructive for Request Records: upgrading clears historical `request_log` rows and their metrics-ingestion cursor before adding the constrained identity fields. It does not backfill legacy IDs, modify Request Patch rows, or delete already aggregated minute rollups. Back up the database first if historical pre-1.0 Request Records are needed outside Cyder.

### Proxy Errors and Upstream Provider Errors

Proxy failures use a stable error fact model with 29 enumerated `code` values, 11 execution stages, and four monotonic response-visibility states: `not_visible`, `headers_committed`, `body_started`, and `unknown`. Before response headers are committed, each public downstream protocol renders that fact in its own final envelope. Stage and visibility are operator facts emitted in structured events; they are not added to the current Request Record schema.

| Downstream protocol | Envelope | Stable Cyder code | Canonical request ID |
| --- | --- | --- | --- |
| OpenAI | `{ "error": { "message", "type", "param": null, "code" } }` | `error.code` | `X-Request-ID` Header |
| Responses | OpenAI-compatible error envelope | `error.code` | `X-Request-ID` Header |
| Anthropic | `{ "type": "error", "error": { "type", "message", "code" }, "request_id" }` | `error.code` | Body `request_id`, `request-id`, and `X-Request-ID` use the same UUID |
| Gemini | Google RPC-style `{ "error": { "code", "message", "status", "details" } }` | `error.details[0].metadata.cyder_code` | `error.details[0].metadata.request_id` and `X-Request-ID` use the same UUID |

All four forms omit the old top-level `code` and `message`. The Gemini `details` array contains one `google.rpc.ErrorInfo` with domain `cyder.gateway`: its `reason` is the AIP-193-compatible uppercase form (for example, `RATE_LIMIT_ERROR`), while `metadata.cyder_code` preserves the exact lowercase Cyder stable code. Protocol-prefix 404 and 405 responses use `route_not_found_error` and `method_not_allowed_error`; 405 preserves the route's `Allow` Header. Anthropic HTTP 413 responses use the official `request_too_large` error type.

When an upstream Provider explicitly returns a non-2xx HTTP response, Cyder preserves that Provider response for the current downstream caller inside the long-lived top-level `upstream_error` extension. For example, an upstream JSON 429 currently produces:

```json
{
  "error": {
    "message": "Upstream provider rate limited the request.",
    "type": "rate_limit_error",
    "param": null,
    "code": "upstream_rate_limit_error"
  },
  "upstream_error": {
    "status": 429,
    "content_type": "application/json",
    "body": {
      "error": {
        "message": "quota exceeded",
        "type": "provider_quota"
      }
    },
    "truncated": false,
    "captured_bytes": 62,
    "limit_bytes": 65536
  }
}
```

The Provider body has one reversible representation:

- valid complete JSON uses `body`
- other complete UTF-8 uses `body_text`
- non-UTF-8 uses `body_base64` plus `"body_encoding": "base64"`
- an empty body uses an empty `body_text`
- an over-limit body uses `body_text` or `body_base64` for the captured prefix, sets `truncated: true`, and adds `"notice": "Upstream error body was truncated by the gateway."`

`captured_bytes` is the number of exposed prefix bytes; Cyder does not claim to know the Provider's original total body length. Missing or invalid Provider `Content-Type` is represented as `null`. Other upstream headers, request URLs, queries, credentials, and gateway configuration are never attached to this extension.

This is intentional pass-through diagnostics, not a claim that arbitrary Provider error bodies are safe. A gateway cannot reliably infer which Provider-owned fields are sensitive without destroying useful error evidence. Provider owners remain responsible for their error payloads, and downstream callers authorized to make the request receive the captured payload. Cyder wraps it, selects a reversible representation, and applies a configured disclosure limit; it does not silently rewrite or redact an explicit Provider error.

Gateway-owned failures—such as invalid Provider configuration, connect/request failures without an HTTP response, response-read failures, and downstream response construction failures—do not carry `upstream_error`. Their public message remains fixed while bounded operator diagnostics stay in structured logs and the existing Request Record summary fields.

Every protocol error returns JSON, `X-Request-ID`, `Cache-Control: no-store`, and `X-Content-Type-Options: nosniff`. OpenAI, Responses, and Anthropic 401 responses also return `WWW-Authenticate: Bearer`; Gemini does not. Anthropic additionally returns `request-id`. `Retry-After` is emitted only from an exact local producer fact: resettable API Key limits use their next UTC bucket, and Provider Circuit uses its supplied remaining cooldown. Concurrency, ACL, half-open probe, and Provider HTTP errors without a local recovery fact omit it. Provider response Headers are not passed through by this contract.

These envelopes apply only before response Headers are committed. R3.7 owns generic post-commit SSE resource termination, while protocol-specific stream completion remains with R3.14–R3.20; Cyder does not replace an in-progress stream with a new HTTP envelope. Ollama remains upstream-only, so unknown `/ai/ollama/*` paths use the ordinary application 404 without Proxy Request ID, CORS, security Headers, authentication, or Request Records.

Complete non-stream responses accept only absent/`identity` or `gzip` Content-Encoding and are bounded independently on raw and decoded bytes. SSE requests advertise `Accept-Encoding: identity`; an SSE response with any other encoding is rejected before downstream Headers, while malformed UTF-8 or line/event/buffer/frame overflow after commit terminates the Body without writing a second protocol error. Successful responses inherit only a normalized `Content-Type`, never arbitrary Provider response Headers.

Configure the disclosure limit in the generated or base YAML and restart:

```yaml
outbound_http:
  connect_timeout_seconds: 10
  auxiliary_total_timeout_seconds: 60
proxy_request:
  timeouts:
    request_send_seconds: 7200
    first_byte_seconds: 7200
    response_idle_seconds: 7200
    total_seconds: 7200
  upstream_error_body_limit_bytes: 65536
  non_stream_response:
    raw_body_limit_bytes: 33554432
    decoded_body_limit_bytes: 67108864
  sse_response:
    line_limit_bytes: 4194304
    event_limit_bytes: 8388608
    buffer_limit_bytes: 16777216
    frame_count_limit: 1000000
```

Proxy timeout settings are finite and startup-only. The defaults are 2 hours for request send, first response Body, active response idle, and the request total; the accepted phase range is 60 through 86400 seconds and the total range is 300 through 86400 seconds. Auxiliary HTTP operations use a separate 60-second total, with an accepted range of 10 through 300 seconds. `outbound_http.connect_timeout_seconds` accepts 1 through 120 seconds and must not exceed the auxiliary total. The retired `proxy_request.connect_timeout_seconds`, `first_byte_timeout_seconds`, and `total_timeout_seconds` paths fail startup with their replacement paths; timeout values cannot be disabled by null, zero, or omission.

The Provider error disclosure default is 65536 bytes and its accepted startup range is 1024 through 1048576. Complete non-stream responses default to 33554432 raw bytes and 67108864 decoded bytes; each accepts 1048576 through 536870912 bytes. The disclosure limit must not exceed the decoded-body limit. SSE defaults are 4194304 bytes per line, 8388608 bytes per event, 16777216 retained bytes, and 1000000 blank-line frames. SSE byte limits accept 1024 through 536870912, the frame limit accepts 1 through 10000000, and startup enforces `line <= event <= buffer`.

All of these settings are startup-only. Invalid recognized values fail startup, and there is no environment-variable override or runtime write API for them. Change the base YAML and restart the server.

The only environment variables that can override final config fields are:

- `CYDER_HOST`
- `CYDER_PORT`
- `CYDER_BASE_PATH`
- `CYDER_LOG_LEVEL`
- `CYDER_TIMEZONE`
- `CYDER_MANAGER_AUTH_BROWSER_ORIGIN`

Startup path environment variables are separate:

- `CYDER_DATA_DIR`: data directory root. Docker images set this to `/data/cyder`.
- `CYDER_CONFIG_PATH`: optional migration hook for an external base config file. It changes only the base config path; generated defaults and SQLite data still belong to the data directory.

Database URLs, secrets, Redis/cache, runtime state, proxy settings, and governance settings are configured through YAML, not environment variables. `CYDER_LOG_THIRD_PARTY_DEBUG` remains a logging diagnostic switch and is not part of `FinalConfig`.

Before 1.0, Cyder supports exactly one running server instance and does not consider multi-instance compatibility. Redis remains optional and can preserve short-lived runtime state across process restarts, but it does not enable a supported multi-instance deployment. Do not add shared Manager sessions, distributed locks, cross-node singleflight, Pub/Sub invalidation, sticky-session requirements, or other multi-instance scaffolding.

### Manager Browser Authentication

Manager authentication uses three purpose-isolated JWT domains:

- a 10-minute Access JWT, held only in the current page's JavaScript memory and sent as a Bearer credential to ordinary Manager APIs
- a single-use Refresh JWT Family held only inside the server, with a 30-day absolute lifetime and a 7-day idle lifetime
- a Mediator Session JWT sent only as an `HttpOnly`, `SameSite=Strict` Cookie on `<base_path>/manager/api/auth`

The browser never receives the Refresh JWT. Login, bootstrap, password rotation, and `POST <base_path>/manager/api/auth/access` return JSON containing only `access_token`. Access is never written to `localStorage`, `sessionStorage`, IndexedDB, a Cookie, or a URL. On first load, the frontend removes only the legacy `auth_token` key from local and session storage, then asks `/auth/access` whether the HttpOnly session is usable.

Set an independent, random root secret of at least 32 bytes and, outside loopback development, configure the exact HTTPS origin that serves both the Manager UI and API:

```yaml
jwt_secret: "<independent random value of at least 32 bytes>"
manager_auth:
  browser_origin: "https://cyder-admin.example.com"
```

`CYDER_MANAGER_AUTH_BROWSER_ORIGIN` is the only environment override for this field. The value must be one exact `http(s)://host[:port]` origin with no path, query, fragment, userinfo, wildcard, or list. A non-loopback origin must use HTTPS. If the field is omitted, browser Auth commands are accepted only when both the TCP peer and request Origin are loopback.

Production uses `__Secure-cyder_manager_session` with `Secure`; explicit loopback development uses `cyder_manager_session_dev` without the `__Secure-` prefix. Both are `HttpOnly`, `SameSite=Strict`, omit `Domain`, and use `Path=<base_path>/manager/api/auth`. A reverse proxy must preserve the browser's real `Origin`, terminate TLS for production, and must not rely on `Host`, `Forwarded`, or `X-Forwarded-Proto` to infer the Manager origin.

Changing `jwt_secret` is an intentional hard cutover: all Manager sessions become invalid and every browser must log in again. There is no previous-secret overlap or gradual JWT rotation. This does not invalidate downstream Proxy API keys, whose authentication is independent.

The R2.11 pre-1.0 migration clears historical Manager session rows while preserving the Manager password verifier. After upgrading, remove any stale `auth_token` storage through the current UI load and log in again. Do not attempt to preserve or import old Access/Refresh Token Pair data.

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

- OpenAI-compatible: `/ai/openai/*` and `/ai/openai/v1/*`
- Responses-compatible: `/ai/responses/*` and `/ai/responses/v1/*`
- Anthropic-compatible: `/ai/anthropic/*` and `/ai/anthropic/v1/*`
- Gemini-compatible: `/ai/gemini/*`, `/ai/gemini/v1/*`, and `/ai/gemini/v1beta/*`

The unversioned routes are direct compatibility aliases for the current `/v1`
semantics, not redirects or a permanent protocol-version claim. Ollama is
upstream-only and has no public downstream route. See the generated
[Protocol Compatibility Matrix](docs/protocol-compatibility.md) for exact
routes, provider profiles, current generation cells, utility boundaries, and
evidence.

### System Endpoints

- health: `/ai/health`
- readiness: `/ai/ready`

## Web Security and Reverse Proxies

The Manager UI/API is same-origin and does not expose CORS. Its responses carry a strict script CSP, anti-embedding and content-type protections, a minimal permissions policy, and status-aware cache rules.

The four public AI protocol prefixes allow browser calls from any Origin with GET/POST, mirrored request headers, exposed response headers, no credentials, and a 600-second preflight cache. Manager, System, and base fallback routes do not inherit this public CORS policy.

Client IP defaults to the TCP peer. To trust a reverse proxy, add only its canonical CIDR to `client_identity.trusted_proxy_cidrs`; the proxy must clear untrusted forwarding headers before generating `Forwarded` or `X-Forwarded-For`. Invalid metadata from a trusted peer is rejected rather than silently falling back.

`client_identity` is YAML-only startup configuration. `max_forwarded_hops` defaults to 8 and accepts 1 through 32; restart the server after changing either field.

See [Web Security and Client Identity](docs/web-security-client-identity.md) for the exact headers, cache/CORS matrix, configuration validation, resolution algorithm, Nginx/Caddy patterns, privacy rules, and troubleshooting steps.

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
