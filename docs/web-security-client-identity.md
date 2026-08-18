# Web Security and Client Identity Living Contract

This document defines the browser boundary and client-IP trust contract for the single-admin Cyder gateway. It applies to the default `base_path: /ai`; substitute a configured base path without changing the relative route semantics.

## Route ownership

| Surface | Routes | Browser contract | Client identity |
| --- | --- | --- | --- |
| Manager | `/ai/manager/ui`, `/ai/manager/api/*` | Same-origin only; no CORS headers | Required before auth/UI handling |
| Public AI proxy | `/ai/openai/*`, `/ai/responses/*`, `/ai/anthropic/*`, `/ai/gemini/*` | Fixed public CORS | Required before proxy auth/routing |
| System | `/ai/health`, `/ai/ready` | No CORS added by this contract | Not resolved |
| Base fallback | other `/ai/*` paths | No CORS added by this contract | Not resolved |

Manager remains same-origin and now enforces the R2.11 browser Auth boundary described below. This does not make Manager cross-origin.

## Manager Auth browser boundary

The six state-changing or Cookie-consuming Manager Auth commands are:

- `POST /ai/manager/api/auth/bootstrap`
- `POST /ai/manager/api/auth/login`
- `POST /ai/manager/api/auth/access`
- `POST /ai/manager/api/auth/password/rotate`
- `POST /ai/manager/api/auth/logout`
- `POST /ai/manager/api/auth/logout_all`

Before body parsing, authentication, rate-limit accounting, or database mutation, every command requires:

```http
Origin: <exact configured browser origin>
Sec-Fetch-Site: same-origin
Content-Type: application/json
X-Cyder-Manager-Auth: 1
```

The Origin comparison uses only the request `Origin`. It never derives trust from `Host`, `Forwarded`, `X-Forwarded-Host`, or `X-Forwarded-Proto`. A boundary failure returns `403` with Manager error code `1461`. `GET /auth/bootstrap/status` is a public, read-only probe and does not consume the Mediator Cookie.

Configure one exact origin:

```yaml
manager_auth:
  browser_origin: https://cyder-admin.example.com
```

`CYDER_MANAGER_AUTH_BROWSER_ORIGIN` is the allowlisted environment override. The value cannot contain a path, query, fragment, userinfo, wildcard, or list. Non-loopback values must use HTTPS. When omitted, Auth commands are allowed only if the TCP peer and the request Origin are both explicitly loopback; forwarding metadata cannot create this exception.

The production Cookie is `__Secure-cyder_manager_session`; explicit loopback development uses `cyder_manager_session_dev`. Both are `HttpOnly`, `SameSite=Strict`, omit `Domain`, and use `Path=<base_path>/manager/api/auth`; production additionally uses `Secure`. The Cookie represents only a purpose-limited Mediator JWT. It contains no Access/Refresh JTI, password, permission, or administrator data. Refresh JWTs stay entirely inside the server and Access JWTs stay only in page memory.

Current logout is Cookie-only and idempotent. Its `503` response still deletes the calling browser Cookie because local logout has completed but persistent revocation is unconfirmed. Password rotation and logout-all require a matching Access Bearer plus Mediator Cookie; their `503` responses preserve the Cookie and current page state because the transaction outcome was not confirmed.

## Manager response security

Every Manager HTML, static, API, error, and Manager-local 404 response overrides these headers:

```http
X-Content-Type-Options: nosniff
X-Frame-Options: DENY
Referrer-Policy: no-referrer
Permissions-Policy: camera=(), microphone=(), geolocation=(), payment=(), usb=()
Content-Security-Policy: default-src 'none'; base-uri 'none'; connect-src 'self'; font-src 'self' data:; form-action 'self'; frame-ancestors 'none'; frame-src 'none'; img-src 'self' data:; manifest-src 'self'; media-src 'none'; object-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; style-src-elem 'self'; style-src-attr 'unsafe-inline'; worker-src 'none'
```

`unsafe-inline` is limited to style directives for the current Vue runtime. Scripts must be same-origin external files; inline scripts and `unsafe-eval` are forbidden.

Manager cache behavior is status and content aware:

| Response | Cache-Control |
| --- | --- |
| `/manager/api/*`, every status | `no-store` |
| HTML and SPA fallback | `no-cache, no-store, must-revalidate` |
| Successful `/manager/ui/assets/*` | `public, max-age=31536000, immutable` |
| Other successful static files | `no-cache` |
| Any unsuccessful static/Manager fallback | `no-store` |

Manager never returns `Access-Control-Allow-Origin` or `Access-Control-Allow-Credentials` from this contract.

## Public proxy CORS

The four AI protocol prefixes share one fixed public policy. Their exact
version aliases, methods, and endpoint ownership are defined by the generated
[Protocol Compatibility Matrix](protocol-compatibility.md):

- origin: `*`
- browser methods: `GET`, `POST`
- allowed request headers: mirror `Access-Control-Request-Headers`
- exposed response headers: `*`
- credentials: disabled; `Access-Control-Allow-Credentials` is absent
- preflight cache: `Access-Control-Max-Age: 600`
- normal `Vary` protection for Origin, requested method, and requested headers

This browser policy does not narrow the existing non-browser Axum method routers. A preflight requesting PUT, PATCH, or DELETE does not receive that method in `Access-Control-Allow-Methods`, so the browser rejects it.

Ordinary success, authentication/governance rejection, parameter error, and protocol-prefix 404 responses receive the public CORS policy. Every Proxy response also overrides:

```http
Cache-Control: no-store
X-Content-Type-Options: nosniff
```

Client-identity rejection happens outside CORS. Its 400/500 response still receives `no-store` and `nosniff`, but callers must not depend on CORS headers for invalid forwarding metadata.

## Startup configuration

```yaml
manager_auth:
  browser_origin: https://cyder-admin.example.com
client_identity:
  trusted_proxy_cidrs: []
  max_forwarded_hops: 8
```

- `trusted_proxy_cidrs` defaults to empty: direct TCP peers are the client identity and all forwarding headers are ignored.
- Entries must be explicit canonical IPv4/IPv6 networks such as `10.0.0.0/8` or `2001:db8::/32`. Bare IPs, host bits, invalid text, and duplicates prevent startup. Write a single host as `/32` or `/128`.
- Overlapping but different networks are allowed.
- `max_forwarded_hops` accepts `1..=32` and defaults to `8`.
- Configuration is startup-only. Restart after editing the base YAML.
- Manager browser origin is an independent trust input. Trusted proxy CIDRs affect normalized client IP only and never authorize a browser Origin.

## Resolution algorithm

1. Normalize IPv4-mapped IPv6 addresses to IPv4.
2. If the TCP peer is outside every trusted CIDR, return that peer immediately without reading `Forwarded` or `X-Forwarded-For`.
3. A trusted peer with no supported forwarding header also resolves to the TCP peer.
4. Parse every `Forwarded` header line in arrival order with `rfc7239`; every element must contain exactly one IP-valued `for` identifier. `unknown`, obfuscated identifiers, missing/duplicate `for`, malformed ports, invalid encoding, and malformed syntax are rejected.
5. Parse every XFF line in arrival order. Bare IPv4/IPv6, IPv4 with port, and bracketed IPv6 with an optional port are accepted. Empty or non-IP nodes are rejected.
6. When both headers exist, their full normalized client-to-nearest-proxy chains must be identical.
7. Reject a chain longer than `max_forwarded_hops`.
8. Starting at the TCP peer, walk the chain from right to left while the current node is trusted. The first untrusted node, or the leftmost node when all intermediaries are trusted, is the client.
9. Store only the normalized client IP, TCP peer, source type, and stripped trusted-hop count in request context. Do not retain the raw chain.

`X-Real-IP` is always ignored.

## Failure and privacy contract

Trusted forwarding metadata fails closed:

| Surface | Missing ConnectInfo | Invalid trusted forwarding metadata |
| --- | --- | --- |
| Manager | 500, Manager JSON code `0` | 400, Manager JSON code `1001` |
| Proxy | 500, `server_error` | 400, `invalid_request_error` |

Client responses use generic messages. Structured logs may contain the surface, normalized TCP peer, Header kind/line counts, optional observed hop count, and one of these stable reasons:

- `missing_connect_info`
- `invalid_header_encoding`
- `malformed_forwarded`
- `malformed_x_forwarded_for`
- `unsupported_identifier`
- `duplicate_for`
- `chain_too_long`
- `conflicting_forwarded_chains`

Never log or persist raw `Forwarded`, XFF, `X-Real-IP`, Authorization/API-key values, or the complete proxy chain. Manager authentication uses the normalized IP only for the existing login-source limiter. Proxy request logs store the same normalized IP in the existing `request_log.client_ip`; there is no new schema or session/JWT binding.

## Reverse-proxy deployment

Only add a reverse proxy CIDR after the network path is controlled and the application cannot be reached through an untrusted address that falls inside that CIDR.

At the trusted edge:

1. Remove client-supplied `Forwarded`, `X-Forwarded-For`, and `X-Real-IP`.
2. Generate one authoritative `Forwarded` or XFF chain. If both are sent, their complete normalized IP chains must match.
3. Do not use `$proxy_add_x_forwarded_for` until untrusted incoming XFF has already been cleared.
4. Keep the chain at or below `max_forwarded_hops`.
5. Terminate TLS for non-loopback Manager deployments and proxy the browser's `Origin` unchanged. Do not synthesize Manager Origin from forwarded scheme/host headers.

Minimal Nginx pattern:

```nginx
location /ai/ {
    proxy_set_header Forwarded "";
    proxy_set_header X-Forwarded-For $remote_addr;
    proxy_set_header X-Real-IP "";
    proxy_pass http://cyder;
}
```

Minimal Caddy pattern:

```caddyfile
reverse_proxy cyder:8000 {
    header_up -Forwarded
    header_up -X-Forwarded-For
    header_up -X-Real-IP
    header_up X-Forwarded-For {remote_host}
}
```

For multiple trusted hops, each controlled proxy must append only after its inbound untrusted value has been removed or replaced at the edge.

## Verification and troubleshooting

- Generate the frontend first, then run `npm --prefix front run security:check`.
- A Manager CSP failure usually means the build introduced inline script/style elements, `eval`, or an asset outside `/ai/manager/ui/`; do not weaken `script-src`.
- A 400 at a trusted proxy boundary means malformed, conflicting, unsupported, or overlong forwarding metadata. Inspect proxy configuration and stable server reason fields; do not log raw headers.
- If every request resolves to the TCP proxy address, confirm the exact proxy CIDR is configured canonically and the server restarted.
- If requests from an untrusted direct client appear to honor forwarding headers, treat it as a security defect.
- A Manager Auth `403/1461` means Origin, Fetch Metadata, JSON Content-Type, or `X-Cyder-Manager-Auth` failed before authentication. Compare the browser-visible origin with the exact startup configuration; do not relax CORS or trust forwarded host/proto.
- A production response setting `cyder_manager_session_dev`, omitting `Secure`, or using a Cookie Path outside `<base_path>/manager/api/auth` is a security defect.

Automated Router and parser matrices are the acceptance mechanism; no real Nginx/Caddy or browser E2E dependency is required for this contract.
