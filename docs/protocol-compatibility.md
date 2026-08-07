<!-- GENERATED FILE. DO NOT EDIT. Canonical source: `docs/protocol-compatibility.yaml`. -->
# Protocol Compatibility

This document reports current, evidence-backed runtime behavior. It is generated deterministically by `cargo run -p cyder-api --bin compatibility_matrix -- --write` from `docs/protocol-compatibility.yaml`.

Unversioned downstream routes are compatibility aliases for the current `/v1` semantics. They are not a protocol-version claim; a future `/v2` may become the alias target without removing the unversioned route.

## Protocol boundaries

- Matrix schema: v3
- Downstream: OpenAI, Responses, Anthropic, Gemini
- Upstream: OpenAI, Responses, Anthropic, Gemini, Ollama
- Ollama is an upstream-only protocol and has no public downstream router.

## Downstream error contracts

- Scope: HTTP error envelopes apply before response headers are committed: `true`.
- After headers are committed, stream/error ownership remains with R3.7, R3.8, R3.15-R3.21.
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

- Owner: `R3.11`; aggregate field `upstream_sources` has `zero_to_many` cardinality and `source_id_only` identity. Source state is `explicit_enabled_and_default`, default semantics are `optional`, and Profile mutability is `create_only`.
- Family uniqueness is `one_active_source_per_wire_family`; disabled Sources reserve family capacity: `true`; deleted Sources release it: `true`.
- Selection order: `protocol_match -> inherit_all_provider_default_transform -> explicit_model_default_transform`; no fallback after selection: `true`. Credentials remain `provider` scoped and `opaque` representation; models use `model_source_selection_mode_and_visible_bindings`.
- Model owner: `R3.11`; Request Patch scope: `provider_global`; Reasoning scope: `provider_global`; configuration owner: `R3.11`.
- Evidence: r3-10-source-aggregate, r3-10-source-repository, r3-10-source-selector, r3-10-manager-source-contract, r3-10-runtime-source-contract, r3-10-migration-contract, r3-11-source-selector, r3-11-model-source-config, r3-11-source-impact-and-check, r3-11-request-log-reason.

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
| OpenAI | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.17 | native-materializer-unavailable |
| OpenAI | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.18 | native-materializer-unavailable |
| OpenAI | Gemini | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.19 | reachable-materializers-without-cell-regression |
| OpenAI | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.20 | ollama-stream-incomplete, ollama-upstream-incomplete |
| Responses | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Responses | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.17 | native-materializer-unavailable |
| Responses | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.18 | native-materializer-unavailable |
| Responses | Gemini | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.19 | reachable-materializers-without-cell-regression |
| Responses | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.20 | ollama-stream-incomplete, ollama-upstream-incomplete |
| Anthropic | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Anthropic | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.17 | native-materializer-unavailable |
| Anthropic | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.18 | native-materializer-unavailable |
| Anthropic | Gemini | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.19 | reachable-materializers-without-cell-regression |
| Anthropic | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.20 | ollama-stream-incomplete, ollama-upstream-incomplete |
| Gemini | OpenAI | not_verified | not_verified | not_verified | not_verified | not_verified | not_verified | R3.16 | reachable-materializers-without-cell-regression |
| Gemini | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.17 | native-materializer-unavailable |
| Gemini | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.18 | native-materializer-unavailable |
| Gemini | Gemini | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Gemini | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.20 | ollama-stream-incomplete, ollama-upstream-incomplete |

## Generation advanced dimensions

| Downstream | Upstream | Tools | Reasoning | Multimodal | Structured output | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| OpenAI | Responses | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| OpenAI | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| OpenAI | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| OpenAI | Ollama | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Responses | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Responses | Responses | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Responses | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Responses | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Responses | Ollama | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Anthropic | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Anthropic | Responses | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Anthropic | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Anthropic | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Anthropic | Ollama | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Gemini | OpenAI | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Gemini | Responses | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Gemini | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Gemini | Gemini | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |
| Gemini | Ollama | not_verified | not_verified | not_verified | not_verified | R3.15 | advanced-transform-only |

## Utility contracts

| Utility | Route suffix | Method | Exposed on | Not exposed on | Execution | Allowed upstreams | Incompatible upstream | Verification | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| models | `/models` | GET | OpenAI, Responses, Anthropic, Gemini | — | local | — | not_applicable | verified | — | router-contract, router-method-contract, r3-11-model-catalog-selector, r3-11-ollama-discovery-boundary |
| embeddings | `/embeddings` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | partial | R3.16 | router-contract, router-method-contract, utility-pre-send-reject |
| rerank | `/rerank` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | partial | R3.16 | router-contract, router-method-contract, utility-pre-send-reject |
| countTokens | `/models/{model}:countTokens` | POST | Gemini | OpenAI, Responses, Anthropic | upstream | Gemini | pre_send_reject | partial | R3.19 | gemini-utility-exposure, router-method-contract, utility-pre-send-reject |

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
| `ollama-upstream-incomplete` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | An Ollama /api/chat materialization path exists, but no complete direct-execution cell proves native behavior; model directories are local configured Models and are not discovered through /api/tags. |
| `ollama-stream-incomplete` | code | `server/src/proxy/runtime/transport/stream.rs` | The transport does not provide a complete native Ollama NDJSON streaming contract. |
| `advanced-transform-only` | test | `cargo run -p cyder-api --bin transform_quality_gate -- --quick` | Transform quality evidence exists, but it does not satisfy the router-to-upstream direct-execution bar for advanced dimensions. |
| `router-contract` | test | `proxy::router::tests::four_protocol_version_aliases_are_direct_and_equivalent` | Pins four public protocol prefixes, direct unversioned aliases, version variants, and local model routes. |
| `router-method-contract` | test | `proxy::router::tests::route_methods_fail_with_405_before_authentication` | Pins strict route methods before authentication or upstream work. |
| `gemini-utility-exposure` | test | `proxy::router::tests::gemini_exposes_only_count_tokens_utility_action` | Pins countTokens as the only exposed Gemini utility action. |
| `utility-pre-send-reject` | test | `proxy::direct_execution_regression::incompatible_utility_targets_are_rejected_before_any_upstream_call` | Proves incompatible embeddings, rerank, and countTokens targets fail before any HTTP upstream request. |
| `r3-10-source-aggregate` | test | `database::provider::tests::provider_aggregate_allows_zero_or_more_sources_and_preserves_family_identity` | Proves Provider aggregates preserve zero, one, and multiple Source identities without a singular or primary fallback. |
| `r3-10-source-repository` | test | `database::upstream_source::tests::source_repository_enforces_lifecycle_family_and_ownership_contracts` | Proves Source lifecycle, family uniqueness, default transitions, disabled reservation, soft-delete release, and ownership boundaries. |
| `r3-10-source-selector` | test | `proxy::direct_execution_regression::deepseek_multi_source_provider_freezes_exact_or_default_source_once_for_public_downstreams` | Proves exact-family-before-default selection, frozen Source evidence, and one-call behavior across public downstream protocols. |
| `r3-10-manager-source-contract` | test | `controller::provider::tests::manager_provider_openapi_matches_routes_safe_dtos_and_errors` | Pins the Manager Provider OpenAPI paths, aggregate DTOs, nested Source operations, opaque credentials, and removed legacy routes. |
| `r3-10-runtime-source-contract` | test | `controller::provider_runtime::tests::snapshot_builds_summary_and_filtered_items_from_one_provider_set` | Pins Source-level Runtime rows, source identity/profile evidence, summary counts, and safe endpoint output. |
| `r3-10-migration-contract` | test | `database::migration_smoke_tests::sqlite_r310_source_migration_preserves_rows_and_enforces_source_contract` | Pins the R3.9-to-R3.10 Source migration retention, ID continuity, Profile labels, and database constraint contract. |
| `r3-11-source-selector` | test | `proxy::direct_execution_regression::direct_execution_explicit_scope_is_closed_and_fail_closed` | Pins EXPLICIT model Source scope, explicit empty failure, disabled/deleted default failure, zero upstream calls, and no Provider-default escape. |
| `r3-11-model-source-config` | test | `controller::model::tests::source_config_http_lifecycle_is_atomic_and_explain_survives_disabled_entities` | Pins atomic Model creation with Source Config, complete replacement, summary shape, four-protocol Explain, and disabled entity diagnostics. |
| `r3-11-source-impact-and-check` | test | `controller::provider::tests::source_impact_http_preview_covers_all_actions_without_side_effects` | Pins bounded DISABLE, DELETE, SET_DEFAULT, and UNSET_DEFAULT impact previews without mutation or Model ID disclosure. |
| `r3-11-request-log-reason` | test | `proxy::direct_execution_regression::direct_execution_model_default_selection_reason_is_persisted_after_flush` | Pins model-default Source selection reason persistence after request-log flush while retaining one selected Source. |
| `r3-11-model-catalog-selector` | test | `proxy::models::tests::models_listing_uses_the_protocol_selector_for_source_visibility` | Pins local model directory visibility to the shared protocol Source selector and enabled Source state. |
| `r3-11-ollama-discovery-boundary` | code | `server/src/proxy/models.rs::get_accessible_models` | The public model directory is built from saved local Model records; no Ollama /api/tags discovery or import route is part of the R3.11 contract. |

## Status semantics

- Base: `verified`, `partial`, `unavailable`, `not_verified`.
- Advanced: `full`, `controlled_loss`, `explicit_reject`, `not_verified`.
- `verified` requires direct execution through routing, materialization, transport, downstream output, and call-count assertions. Transform-only evidence is insufficient.
