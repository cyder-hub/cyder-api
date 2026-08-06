<!-- GENERATED FILE. DO NOT EDIT. Canonical source: `docs/protocol-compatibility.yaml`. -->
# Protocol Compatibility

This document reports current, evidence-backed runtime behavior. It is generated deterministically by `cargo run -p cyder-api --bin compatibility_matrix -- --write` from `docs/protocol-compatibility.yaml`.

Unversioned downstream routes are compatibility aliases for the current `/v1` semantics. They are not a protocol-version claim; a future `/v2` may become the alias target without removing the unversioned route.

## Protocol boundaries

- Matrix schema: v2
- Downstream: OpenAI, Responses, Anthropic, Gemini
- Upstream: OpenAI, Responses, Anthropic, Gemini, Ollama
- Ollama is an upstream-only protocol and has no public downstream router.

## Downstream error contracts

- Scope: HTTP error envelopes apply before response headers are committed: `true`.
- After headers are committed, stream/error ownership remains with R3.7, R3.8, R3.14-R3.20.
- Ollama downstream contract: `absent`; Provider error extension location: `top_level`.
- Every pre-commit error is JSON with `X-Request-ID`, `Cache-Control: no-store`, and `X-Content-Type-Options: nosniff`; Anthropic also returns `request-id`.
- OpenAI, Responses, and Anthropic 401 responses use `WWW-Authenticate: Bearer`; Gemini does not. `Retry-After` appears only when an exact producer fact exists.

### Protocol envelopes

| Protocol | Envelope | Stable code path | Request ID body path | Request ID headers | Upstream error path | Evidence |
| --- | --- | --- | --- | --- | --- | --- |
| OpenAI | `openai_error` | `error.code` | `absent` | `x-request-id` | `upstream_error` | error-contract-unit, error-contract-golden, error-contract-router, response-limit-raw-four-protocol, response-limit-decoded-four-protocol, response-encoding-four-protocol, stream-resource-postcommit-four-protocol, provider-error-hard-limit-four-protocol |
| Responses | `openai_error` | `error.code` | `absent` | `x-request-id` | `upstream_error` | error-contract-unit, error-contract-golden, error-contract-router, response-limit-raw-four-protocol, response-limit-decoded-four-protocol, response-encoding-four-protocol, stream-resource-postcommit-four-protocol, provider-error-hard-limit-four-protocol |
| Anthropic | `anthropic_error` | `error.code` | `request_id` | `x-request-id`, `request-id` | `upstream_error` | error-contract-unit, error-contract-golden, error-contract-router, response-limit-raw-four-protocol, response-limit-decoded-four-protocol, response-encoding-four-protocol, stream-resource-postcommit-four-protocol, provider-error-hard-limit-four-protocol |
| Gemini | `google_rpc_error` | `error.details.google_rpc_error_info.metadata.cyder_code` | `error.details.google_rpc_error_info.metadata.request_id` | `x-request-id` | `upstream_error` | error-contract-unit, error-contract-golden, error-contract-router, response-limit-raw-four-protocol, response-limit-decoded-four-protocol, response-encoding-four-protocol, stream-resource-postcommit-four-protocol, provider-error-hard-limit-four-protocol |

### Router rejections

| Stable code | HTTP status | Required header |
| --- | --- | --- |
| `route_not_found_error` | 404 | — |
| `method_not_allowed_error` | 405 | Allow |

## Upstream Source contract

- Owner: `R3.9`; every active Logical Provider has exactly one implicitly enabled and implicitly default Source with `source_key=primary`.
- Credentials remain `provider` scoped and are applied according to the selected Source Profile.
- Evidence: r3-9-primary-source-aggregate, r3-9-source-runtime-evidence.

## Upstream Source profiles

| Profile type | Upstream protocol | Dialect | Auth | Endpoint |
| --- | --- | --- | --- | --- |
| OpenAI | OpenAI | `standard` | `bearer_api_key` | `base_url` |
| Gemini | Gemini | `standard` | `gemini_api_key` | `base_url` |
| Vertex | Gemini | `standard` | `vertex_oauth` | `vertex_gemini` |
| VertexOpenAI | OpenAI | `gemini_openai_compatibility` | `vertex_oauth` | `vertex_openai` |
| Ollama | Ollama | `standard` | `bearer_api_key` | `base_url` |
| Anthropic | Anthropic | `standard` | `anthropic_api_key` | `base_url` |
| Responses | Responses | `standard` | `bearer_api_key` | `base_url` |
| GeminiOpenAI | OpenAI | `gemini_openai_compatibility` | `bearer_api_key` | `base_url` |

## Public downstream routes

| Protocol | Prefix | Versions | Endpoint | Method | Kind |
| --- | --- | --- | --- | --- | --- |
| OpenAI | `/ai/openai` | unversioned, v1 | `/chat/completions` | POST | generation |
| OpenAI | `/ai/openai` | unversioned, v1 | `/embeddings` | POST | utility |
| OpenAI | `/ai/openai` | unversioned, v1 | `/rerank` | POST | utility |
| OpenAI | `/ai/openai` | unversioned, v1 | `/models` | GET | local |
| Responses | `/ai/responses` | unversioned, v1 | `/responses` | POST | generation |
| Responses | `/ai/responses` | unversioned, v1 | `/models` | GET | local |
| Anthropic | `/ai/anthropic` | unversioned, v1 | `/messages` | POST | generation |
| Anthropic | `/ai/anthropic` | unversioned, v1 | `/models` | GET | local |
| Gemini | `/ai/gemini` | unversioned, v1, v1beta | `/models/{model}:generateContent` | POST | generation |
| Gemini | `/ai/gemini` | unversioned, v1, v1beta | `/models/{model}:streamGenerateContent` | POST | generation |
| Gemini | `/ai/gemini` | unversioned, v1, v1beta | `/models/{model}:countTokens` | POST | utility |
| Gemini | `/ai/gemini` | unversioned, v1, v1beta | `/models` | GET | local |

## Generation base dimensions

| Downstream | Upstream | Non-stream text | Stream text | Usage | Normal termination | Upstream error | Cancellation | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| OpenAI | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.13 | native-materializer-unavailable |
| OpenAI | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.14 | native-materializer-unavailable |
| OpenAI | Gemini | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.15 | reachable-materializers-without-cell-regression |
| OpenAI | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.16 | ollama-stream-incomplete, ollama-upstream-incomplete |
| Responses | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Responses | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.13 | native-materializer-unavailable |
| Responses | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.14 | native-materializer-unavailable |
| Responses | Gemini | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.15 | reachable-materializers-without-cell-regression |
| Responses | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.16 | ollama-stream-incomplete, ollama-upstream-incomplete |
| Anthropic | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Anthropic | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.13 | native-materializer-unavailable |
| Anthropic | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.14 | native-materializer-unavailable |
| Anthropic | Gemini | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.15 | reachable-materializers-without-cell-regression |
| Anthropic | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.16 | ollama-stream-incomplete, ollama-upstream-incomplete |
| Gemini | OpenAI | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.12 | reachable-materializers-without-cell-regression |
| Gemini | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.13 | native-materializer-unavailable |
| Gemini | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.14 | native-materializer-unavailable |
| Gemini | Gemini | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Gemini | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.16 | ollama-stream-incomplete, ollama-upstream-incomplete |

## Generation advanced dimensions

| Downstream | Upstream | Tools | Reasoning | Multimodal | Structured output | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.12 | advanced-transform-only |
| OpenAI | Responses | not_verified | not_verified | not_verified | not_verified | R3.13 | advanced-transform-only |
| OpenAI | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.14 | advanced-transform-only |
| OpenAI | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| OpenAI | Ollama | not_verified | not_verified | not_verified | not_verified | R3.16 | advanced-transform-only |
| Responses | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.12 | advanced-transform-only |
| Responses | Responses | not_verified | not_verified | not_verified | not_verified | R3.13 | advanced-transform-only |
| Responses | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.14 | advanced-transform-only |
| Responses | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Responses | Ollama | not_verified | not_verified | not_verified | not_verified | R3.16 | advanced-transform-only |
| Anthropic | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.12 | advanced-transform-only |
| Anthropic | Responses | not_verified | not_verified | not_verified | not_verified | R3.13 | advanced-transform-only |
| Anthropic | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.14 | advanced-transform-only |
| Anthropic | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Anthropic | Ollama | not_verified | not_verified | not_verified | not_verified | R3.16 | advanced-transform-only |
| Gemini | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.12 | advanced-transform-only |
| Gemini | Responses | not_verified | not_verified | not_verified | not_verified | R3.13 | advanced-transform-only |
| Gemini | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.14 | advanced-transform-only |
| Gemini | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Gemini | Ollama | not_verified | not_verified | not_verified | not_verified | R3.16 | advanced-transform-only |

## Utility contracts

| Utility | Route suffix | Method | Exposed on | Not exposed on | Execution | Allowed upstreams | Incompatible upstream | Verification | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| models | `/models` | GET | OpenAI, Responses, Anthropic, Gemini | — | local | — | not_applicable | verified | — | router-contract, router-method-contract |
| embeddings | `/embeddings` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | partial | R3.17 | router-contract, router-method-contract, utility-pre-send-reject |
| rerank | `/rerank` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | partial | R3.17 | router-contract, router-method-contract, utility-pre-send-reject |
| countTokens | `/models/{model}:countTokens` | POST | Gemini | OpenAI, Responses, Anthropic | upstream | Gemini | pre_send_reject | partial | R3.17 | gemini-utility-exposure, router-method-contract, utility-pre-send-reject |

## Evidence registry

| ID | Kind | Reference | Summary |
| --- | --- | --- | --- |
| `error-contract-unit` | test | `proxy::error::response::tests::protocol_error_contracts_cover_all_116_proxy_and_8_router_combinations` | Exhaustively pins all 116 ProxyError and 8 Router Rejection protocol envelopes, code paths, headers, and upstream extensions. |
| `error-contract-golden` | test | `proxy::error_contract_regression::four_downstream_error_contracts_match_golden_fixtures` | Exercises authentic Provider 429 responses through all four real downstream routers with complete protocol bodies and one upstream call. |
| `error-contract-router` | test | `proxy::error_contract_regression::router_and_ingress_rejections_use_protocol_contracts` | Exercises four-protocol ingress, extractor, utility, 404, 405, CORS, and Ollama boundaries through the real router. |
| `response-limit-raw-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_use_existing_envelopes_for_non_stream_response_limit` | Exercises raw response hard-limit failures through all four real downstream routers with stable pre-commit envelopes, one request log, and one upstream call. |
| `response-limit-decoded-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_use_existing_envelopes_for_decoded_response_limit` | Exercises gzip decoded-body hard-limit failures through all four real downstream routers without exposing an upstream_error extension. |
| `response-encoding-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_reject_sse_encoding_before_headers` | Exercises unsupported SSE response encoding before header commit through all four protocol adapters, including request identity and lifecycle assertions. |
| `stream-resource-postcommit-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_terminate_body_on_sse_parser_failure` | Exercises post-commit SSE parser failure through all four real downstream routers, proving one Body error, no second envelope, and one terminal lifecycle. |
| `provider-error-hard-limit-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_preserve_bounded_provider_error_when_body_reaches_hard_limit` | Exercises bounded Provider 429 payloads at the overall response hard limit through all four protocol adapters without replacing the authentic status or extension. |
| `direct-non-stream` | test | `proxy::direct_execution_regression::direct_execution_regression_non_stream_request_response_usage_and_log_golden` | Exercises routing, request materialization, non-stream response conversion, usage, logging, and one upstream call for the four representative cells. |
| `direct-stream` | test | `proxy::direct_execution_regression::direct_execution_regression_stream_events_usage_and_single_call_golden` | Exercises streaming request materialization, ordered downstream events, text, usage, and one upstream call for the four representative cells. |
| `direct-upstream-error` | test | `proxy::direct_execution_regression::direct_execution_regression_upstream_429_is_authentic_logged_and_never_retried` | Exercises authentic upstream error conversion, complete protocol bodies, safe logging, and the no-retry contract for the four representative cells. |
| `direct-cancellation` | test | `proxy::direct_execution_regression::direct_execution_regression_client_cancellation_closes_upstream_and_logs_cancelled` | Exercises client cancellation, upstream connection closure, cancelled logging, and the no-retry contract for the four representative cells. |
| `direct-call-count` | test | `proxy::direct_execution_regression::four_public_downstream_generation_paths_call_upstream_at_most_once` | Pins each representative public generation path to exactly one real upstream request. |
| `r3-9-primary-source-aggregate` | test | `database::provider::tests::provider_aggregate_reads_fail_closed_for_missing_or_ambiguous_source` | Pins every readable Logical Provider to exactly one active primary Upstream Source and fails closed for zero or multiple Sources. |
| `r3-9-source-runtime-evidence` | test | `controller::provider_runtime::tests::snapshot_builds_summary_and_filtered_items_from_one_provider_set` | Pins Provider Runtime aggregation to the selected Source identity, Profile, safe endpoint snapshot, and Source-scoped circuit health. |
| `representative-fixture-scope` | test | `proxy::direct_execution_regression::direct_execution_regression_fixtures_define_four_complete_protocols` | Proves that the direct-execution suite contains exactly the four representative cells and no downstream Ollama fixture. |
| `reachable-materializers-without-cell-regression` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | OpenAI and Gemini upstream materializers exist, but cells outside the representative suite lack complete direct-execution evidence. |
| `native-materializer-unavailable` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | Responses and Anthropic upstream generation protocols are rejected before HTTP materialization. |
| `ollama-upstream-incomplete` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | An Ollama /api/chat materialization path exists, but no complete direct-execution cell proves native behavior. |
| `ollama-stream-incomplete` | code | `server/src/proxy/runtime/transport/stream.rs` | The transport does not provide a complete native Ollama NDJSON streaming contract. |
| `advanced-transform-only` | test | `cargo run -p cyder-api --bin transform_quality_gate -- --quick` | Transform quality evidence exists, but it does not satisfy the router-to-upstream direct-execution bar for advanced dimensions. |
| `router-contract` | test | `proxy::router::tests::four_protocol_version_aliases_are_direct_and_equivalent` | Pins four public protocol prefixes, direct unversioned aliases, version variants, and local model routes. |
| `router-method-contract` | test | `proxy::router::tests::route_methods_fail_with_405_before_authentication` | Pins strict route methods before authentication or upstream work. |
| `gemini-utility-exposure` | test | `proxy::router::tests::gemini_exposes_only_count_tokens_utility_action` | Pins countTokens as the only exposed Gemini utility action. |
| `utility-pre-send-reject` | test | `proxy::direct_execution_regression::incompatible_utility_targets_are_rejected_before_any_upstream_call` | Proves incompatible embeddings, rerank, and countTokens targets fail before any HTTP upstream request. |

## Status semantics

- Base: `verified`, `partial`, `unavailable`, `not_verified`.
- Advanced: `full`, `controlled_loss`, `explicit_reject`, `not_verified`.
- `verified` requires direct execution through routing, materialization, transport, downstream output, and call-count assertions. Transform-only evidence is insufficient.
