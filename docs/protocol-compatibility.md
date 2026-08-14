<!-- GENERATED FILE. DO NOT EDIT. Canonical source: `docs/protocol-compatibility.yaml`. -->
# Protocol Compatibility

This document reports current, evidence-backed runtime behavior. It is generated deterministically by `cargo run -p cyder-api --bin compatibility_matrix -- --write` from `docs/protocol-compatibility.yaml`.

Unversioned downstream routes are compatibility aliases for the current `/v1` semantics. They are not a protocol-version claim; a future `/v2` may become the alias target without removing the unversioned route.

## Protocol boundaries

- Matrix schema: v6
- Downstream: OpenAI, Responses, Anthropic, Gemini
- Upstream: OpenAI, Responses, Anthropic, Gemini

## Downstream error contracts

- Scope: HTTP error envelopes apply before response headers are committed: `true`.
- After headers are committed, stream/error ownership remains with R3.7, R3.8, R3.15-R3.21.
- Provider error extension location: `top_level`.
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

## Transform runtime contract

- Owner: `R3.15`. Same-wire behavior is `byte_preserving_passthrough`; observation failure is `observation_degraded`.
- Cross-wire pipeline: `source_decode -> unified_ir -> target_encode`; any pipeline failure is `fail_closed`.
- Unknown-field policy: ordinary object fields `serde_default_silent_ignore`; unknown tagged semantics `pre_send_explicit_reject`; registered conflicts `targeted_borrowed_value_check`; full second schema audit `false`; strict target exception `gemini_openai_recursive_closed_allowlist`.
- Loss policy: minor `controlled_loss_with_internal_fact`, major `explicit_reject`, deterministic text downgrade `fixture_backed_controlled_loss`.
- Header boundary: before commit `downstream_native_error_envelope`; after commit `single_downstream_native_error_terminal_or_body_error`; normal terminal after failure `false`.
- Diagnostics: visibility `internal_only`, retained fact cap `32`, overflow accounted `true`, payload-free `true`, public extensions `false`.
- Persistence active in R3.15: `false`; persistence owner `R4.6`; final matrix owner `R3.21`.

### Advanced cell owners

| Upstream protocol | Owner |
| --- | --- |

Evidence: r3-15-transform-quality-contract, r3-15-transform-payload-free-contract, r3-15-same-wire-passthrough, r3-15-minor-loss-runtime, r3-15-four-protocol-stream-failure, r3-15-target-stream-failure.

## Upstream Source contract

- Owner: `R3.12`; aggregate field `upstream_sources` has `zero_to_many` cardinality and `source_id_only` identity. Source state is `explicit_enabled_and_default`, default semantics are `optional`, and Profile mutability is `create_only`.
- Family uniqueness is `one_active_source_per_wire_family`; disabled Sources reserve family capacity: `true`; deleted Sources release it: `true`.
- Selection order: `protocol_match -> inherit_all_provider_default_transform -> explicit_model_default_transform`; no fallback after selection: `true`. Credentials remain `provider` scoped and `opaque` representation; models use `model_source_selection_mode_and_visible_bindings`.
- Model owner: `R3.11`; Request Patch scope: `source_bound_variant`; Reasoning scope: `protocol_transform_only`; configuration owner: `R3.12`.
- Evidence: r3-10-source-aggregate, r3-10-source-repository, r3-10-source-selector, r3-10-manager-source-contract, r3-10-runtime-source-contract, r3-10-migration-contract, r3-11-source-selector, r3-11-model-source-config, r3-11-source-impact-and-check, r3-11-request-log-reason, r3-12-request-patch-contract, r3-12-request-patch-execution, r3-12-request-patch-migration.

## Upstream Source profiles

| Profile type | Upstream protocol | Dialect | Auth | Endpoint |
| --- | --- | --- | --- | --- |
| OpenAI | OpenAI | `standard` | `bearer_api_key` | `base_url` |
| Gemini | Gemini | `standard` | `gemini_api_key` | `base_url` |
| Vertex | Gemini | `standard` | `vertex_oauth` | `vertex_gemini` |
| OpenAICompatible | OpenAI | `standard` | `bearer_api_key` | `base_url` |
| Anthropic | Anthropic | `standard` | `anthropic_api_key` | `base_url` |
| Responses | Responses | `standard` | `bearer_api_key` | `base_url` |
| GeminiOpenAI | OpenAI | `gemini_openai_compatibility` | `bearer_api_key` | `base_url` |

## OpenAI-wire Profile contracts

| Profile | Base URL requirement | Default Base URL | Customizable | Auth | Chat | Chat field policy | Embeddings | Embeddings field policy | Rerank | Rerank field policy | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | `optional_with_default` | https://api.openai.com/v1 | `true` | `bearer_api_key` | configurable (default enabled) | `official_fields_validated_unknown_extensions_passthrough` | configurable (default enabled) | `official_fields_validated_unknown_extensions_passthrough` | unsupported (default disabled) | `unsupported` | r3-16-profile-field-policy, r3-16-profile-source-contract, r3-16-embeddings-direct, r3-16-rerank-profile-guard |
| OpenAICompatible | `required` | — | `true` | `bearer_api_key` | configurable (default enabled) | `core_fields_validated_vendor_extensions_passthrough` | configurable (default disabled) | `core_fields_validated_vendor_extensions_passthrough` | configurable (default disabled) | `opaque_envelope_passthrough` | r3-16-profile-field-policy, r3-16-profile-source-contract, r3-16-embeddings-direct, r3-16-rerank-direct, r3-20-openai-compatible-chat, r3-20-openai-compatible-stream, r3-20-openai-compatible-embeddings |
| GeminiOpenAI | `optional_with_default` | https://generativelanguage.googleapis.com/v1beta/openai | `true` | `bearer_api_key` | configurable (default enabled) | `recursive_closed_allowlist` | configurable (default enabled) | `model_input_closed_contract` | unsupported (default disabled) | `unsupported` | r3-16-profile-field-policy, r3-16-profile-source-contract, r3-16-gemini-openai-closed-policy, r3-16-embeddings-direct, r3-16-rerank-profile-guard |

## Gemini-wire Profile and surface contract

| Profile | Auth | Model collection |
| --- | --- | --- |
| Gemini | `x_goog_api_key` | `normalized_model_collection` |
| Vertex | `oauth_bearer` | `vertex_publisher_model_collection` |

### Model operations

| Action | Suffix | Query |
| --- | --- | --- |
| `generate_content` | `:generateContent` | `—` |
| `stream_generate_content` | `:streamGenerateContent` | `alt=sse` |
| `count_tokens` | `:countTokens` | `—` |

- Exposed Gemini downstream surface: models, generate_content, stream_generate_content, count_tokens.
- Prohibited Gemini downstream products: interactions, live, batch, files, caching, embeddings, media_generation, remote_model_discovery.
- Models execution: `local_catalog`; countTokens allowed upstreams: Gemini.
- Evidence: r3-19-profile-equivalence, r3-19-profile-registry, r3-19-source-check, r3-19-route-boundary, r3-19-count-tokens, r3-19-count-tokens-reject.

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
| OpenAI | Responses | verified | verified | verified | verified | verified | verified | — | r3-17-body-limit, r3-17-cancellation, r3-17-http-error, r3-17-materializer, r3-17-openai-base, r3-17-stream-eof, r3-17-stream-terminal-error, r3-17-terminal-cost |
| OpenAI | Anthropic | verified | verified | verified | verified | verified | verified | — | r3-18-cancellation, r3-18-materializer, r3-18-non-stream-fail-closed, r3-18-openai-base, r3-18-openai-upstream-error, r3-18-stream-eof, r3-18-stream-success-terminal, r3-18-stream-terminal-error |
| OpenAI | Gemini | verified | verified | verified | verified | verified | verified | — | r3-19-openai-base |
| Responses | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Responses | Responses | verified | verified | verified | verified | verified | verified | — | r3-17-body-limit, r3-17-cancellation, r3-17-http-error, r3-17-materializer, r3-17-responses-base, r3-17-stream-eof, r3-17-stream-terminal-error, r3-17-terminal-cost |
| Responses | Anthropic | verified | verified | verified | verified | verified | verified | — | r3-18-cancellation, r3-18-materializer, r3-18-non-stream-fail-closed, r3-18-responses-base, r3-18-responses-upstream-error, r3-18-stream-eof, r3-18-stream-success-terminal, r3-18-stream-terminal-error |
| Responses | Gemini | verified | verified | verified | verified | verified | verified | — | r3-19-responses-base |
| Anthropic | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Anthropic | Responses | verified | verified | verified | verified | verified | verified | — | r3-17-anthropic-base, r3-17-body-limit, r3-17-cancellation, r3-17-http-error, r3-17-materializer, r3-17-stream-eof, r3-17-stream-terminal-error, r3-17-terminal-cost |
| Anthropic | Anthropic | verified | verified | verified | verified | verified | verified | — | r3-18-anthropic-base, r3-18-anthropic-upstream-error, r3-18-cancellation, r3-18-materializer, r3-18-non-stream-fail-closed, r3-18-stream-eof, r3-18-stream-success-terminal, r3-18-stream-terminal-error |
| Anthropic | Gemini | verified | verified | verified | verified | verified | verified | — | r3-19-anthropic-base |
| Gemini | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Gemini | Responses | verified | verified | verified | verified | verified | verified | — | r3-17-body-limit, r3-17-cancellation, r3-17-gemini-base, r3-17-http-error, r3-17-materializer, r3-17-stream-eof, r3-17-stream-terminal-error, r3-17-terminal-cost |
| Gemini | Anthropic | verified | verified | verified | verified | verified | verified | — | r3-18-cancellation, r3-18-gemini-base, r3-18-gemini-upstream-error, r3-18-materializer, r3-18-non-stream-fail-closed, r3-18-stream-eof, r3-18-stream-success-terminal, r3-18-stream-terminal-error |
| Gemini | Gemini | verified | verified | verified | verified | verified | verified | — | r3-19-gemini-base |

## Generation advanced dimensions

| Downstream | Upstream | Tools | Reasoning | Multimodal | Structured output | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | OpenAI | full | full | full | full | — | r3-16-multimodal-direct, r3-16-reasoning-direct, r3-16-structured-direct, r3-16-tools-direct |
| OpenAI | Responses | full | controlled_loss | full | full | — | r3-17-openai-multimodal-cell, r3-17-openai-reasoning-cell, r3-17-openai-structured-cell, r3-17-openai-tools-cell, r3-17-reasoning-reject |
| OpenAI | Anthropic | full | controlled_loss | controlled_loss | controlled_loss | — | r3-18-multimodal-reject, r3-18-openai-multimodal-cell, r3-18-openai-reasoning-cell, r3-18-openai-structured-cell, r3-18-openai-tools-cell, r3-18-reasoning-reject, r3-18-structured-reject |
| OpenAI | Gemini | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-19-multimodal-reject, r3-19-openai-multimodal-cell, r3-19-openai-reasoning-cell, r3-19-openai-structured-cell, r3-19-openai-tools-cell, r3-19-reasoning-reject, r3-19-structured-reject, r3-19-tools-reject |
| Responses | OpenAI | controlled_loss | controlled_loss | controlled_loss | full | — | r3-16-multimodal-direct, r3-16-multimodal-reject, r3-16-multimodal-responses-diagnostic, r3-16-reasoning-direct, r3-16-reasoning-responses-diagnostic, r3-16-reasoning-responses-reject, r3-16-structured-direct, r3-16-structured-responses-transform, r3-16-tools-cross-wire-transform, r3-16-tools-direct, r3-16-tools-reject, r3-16-tools-stable-results |
| Responses | Responses | full | full | full | full | — | r3-17-responses-multimodal-cell, r3-17-responses-reasoning-cell, r3-17-responses-structured-cell, r3-17-responses-tools-cell |
| Responses | Anthropic | full | controlled_loss | controlled_loss | controlled_loss | — | r3-18-multimodal-reject, r3-18-reasoning-reject, r3-18-responses-multimodal-cell, r3-18-responses-reasoning-cell, r3-18-responses-structured-cell, r3-18-responses-tools-cell, r3-18-structured-reject |
| Responses | Gemini | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-19-multimodal-reject, r3-19-reasoning-reject, r3-19-responses-multimodal-cell, r3-19-responses-reasoning-cell, r3-19-responses-structured-cell, r3-19-responses-tools-cell, r3-19-structured-reject, r3-19-tools-reject |
| Anthropic | OpenAI | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-16-multimodal-anthropic-diagnostic, r3-16-multimodal-direct, r3-16-multimodal-reject, r3-16-reasoning-anthropic-diagnostic, r3-16-reasoning-direct, r3-16-structured-anthropic-diagnostic, r3-16-structured-direct, r3-16-structured-reject, r3-16-tools-cross-wire-transform, r3-16-tools-direct, r3-16-tools-reject, r3-16-tools-stable-results |
| Anthropic | Responses | full | controlled_loss | full | controlled_loss | — | r3-17-anthropic-multimodal-cell, r3-17-anthropic-reasoning-cell, r3-17-anthropic-structured-cell, r3-17-anthropic-tools-cell, r3-17-reasoning-reject, r3-17-structured-reject |
| Anthropic | Anthropic | full | full | full | full | — | r3-18-anthropic-multimodal-cell, r3-18-anthropic-reasoning-cell, r3-18-anthropic-structured-cell, r3-18-anthropic-tools-cell |
| Anthropic | Gemini | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-19-anthropic-multimodal-cell, r3-19-anthropic-reasoning-cell, r3-19-anthropic-structured-cell, r3-19-anthropic-tools-cell, r3-19-multimodal-reject, r3-19-reasoning-reject, r3-19-structured-reject, r3-19-tools-reject |
| Gemini | OpenAI | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-16-multimodal-direct, r3-16-multimodal-gemini-diagnostic, r3-16-multimodal-reject, r3-16-reasoning-conflict-reject, r3-16-reasoning-direct, r3-16-reasoning-gemini-diagnostic, r3-16-structured-direct, r3-16-structured-gemini-diagnostic, r3-16-structured-reject, r3-16-tools-cross-wire-transform, r3-16-tools-direct, r3-16-tools-reject, r3-16-tools-stable-results |
| Gemini | Responses | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-17-gemini-multimodal-cell, r3-17-gemini-reasoning-cell, r3-17-gemini-structured-cell, r3-17-gemini-tools-cell, r3-17-multimodal-reject, r3-17-reasoning-reject, r3-17-structured-reject, r3-17-tools-reject |
| Gemini | Anthropic | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-18-gemini-multimodal-cell, r3-18-gemini-reasoning-cell, r3-18-gemini-structured-cell, r3-18-gemini-tools-cell, r3-18-multimodal-reject, r3-18-reasoning-reject, r3-18-structured-reject, r3-18-tools-reject |
| Gemini | Gemini | full | full | full | full | — | r3-19-gemini-multimodal-cell, r3-19-gemini-reasoning-cell, r3-19-gemini-structured-cell, r3-19-gemini-tools-cell |

## Utility contracts

| Utility | Route suffix | Method | Exposed on | Not exposed on | Execution | Allowed upstreams | Incompatible upstream | Verification | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| models | `/models` | GET | OpenAI, Responses, Anthropic, Gemini | — | local | — | not_applicable | verified | — | router-contract, router-method-contract, r3-11-model-catalog-selector, r3-17-models-boundary |
| embeddings | `/embeddings` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | verified | — | router-contract, router-method-contract, utility-pre-send-reject, r3-16-embeddings-direct, r3-16-embeddings-reject |
| rerank | `/rerank` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | verified | — | router-contract, router-method-contract, utility-pre-send-reject, r3-16-rerank-direct, r3-16-rerank-profile-guard |
| countTokens | `/models/{model}:countTokens` | POST | Gemini | OpenAI, Responses, Anthropic | upstream | Gemini | pre_send_reject | verified | — | r3-19-count-tokens, r3-19-count-tokens-reject, r3-19-route-boundary |

## Evidence registry

| ID | Kind | Reference | Summary |
| --- | --- | --- | --- |
| `error-contract-unit` | test | `proxy::error::response::tests::protocol_error_contracts_cover_all_116_proxy_and_8_router_combinations` | Exhaustively pins all 116 ProxyError and 8 Router Rejection protocol envelopes, code paths, headers, and upstream extensions. |
| `error-contract-golden` | test | `proxy::error_contract_regression::four_downstream_error_contracts_match_golden_fixtures` | Exercises authentic Provider 429 responses through all four real downstream routers with complete protocol bodies and one upstream call. |
| `error-contract-router` | test | `proxy::error_contract_regression::router_and_ingress_rejections_use_protocol_contracts` | Exercises four-protocol ingress, extractor, utility, 404, 405, CORS, and retired-protocol boundaries through the real router. |
| `response-limit-raw-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_use_existing_envelopes_for_non_stream_response_limit` | Exercises raw response hard-limit failures through all four real downstream routers with stable pre-commit envelopes, one request log, and one upstream call. |
| `response-limit-decoded-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_use_existing_envelopes_for_decoded_response_limit` | Exercises gzip decoded-body hard-limit failures through all four real downstream routers without exposing an upstream_error extension. |
| `response-encoding-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_reject_sse_encoding_before_headers` | Exercises unsupported SSE response encoding before header commit through all four protocol adapters, including request identity and lifecycle assertions. |
| `stream-resource-postcommit-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_terminate_body_on_sse_parser_failure` | Exercises post-commit SSE parser failure through all four real downstream routers, proving one Body error, no second envelope, and one terminal lifecycle. |
| `provider-error-hard-limit-four-protocol` | test | `proxy::direct_execution_regression::four_public_protocols_preserve_bounded_provider_error_when_body_reaches_hard_limit` | Exercises bounded Provider 429 payloads at the overall response hard limit through all four protocol adapters without replacing the authentic status or extension. |
| `direct-non-stream` | test | `proxy::direct_execution_regression::direct_execution_regression_non_stream_request_response_usage_and_log_golden` | Exercises routing, request materialization, non-stream response conversion, usage, logging, and one upstream call for all four OpenAI-target cells plus the retained native Gemini cell. |
| `direct-stream` | test | `proxy::direct_execution_regression::direct_execution_regression_stream_events_usage_and_single_call_golden` | Exercises streaming request materialization, ordered downstream events, text, usage, and one upstream call for all four OpenAI-target cells plus the retained native Gemini cell. |
| `direct-upstream-error` | test | `proxy::direct_execution_regression::direct_execution_regression_upstream_429_is_authentic_logged_and_never_retried` | Exercises authentic upstream error conversion, complete protocol bodies, safe logging, and the no-retry contract for all four OpenAI-target cells plus native Gemini. |
| `direct-cancellation` | test | `proxy::direct_execution_regression::direct_execution_regression_client_cancellation_closes_upstream_and_logs_cancelled` | Exercises client cancellation, upstream connection closure, cancelled logging, and the no-retry contract for all four OpenAI-target cells plus native Gemini. |
| `direct-call-count` | test | `proxy::direct_execution_regression::four_public_downstream_generation_paths_call_upstream_at_most_once` | Pins each of the four public downstream generation paths to one frozen OpenAI Source and exactly one real Chat Completions request. |
| `r3-9-primary-source-aggregate` | test | `database::provider::tests::provider_aggregate_reads_fail_closed_for_missing_or_ambiguous_source` | Pins every readable Logical Provider to exactly one active primary Upstream Source and fails closed for zero or multiple Sources. |
| `r3-9-source-runtime-evidence` | test | `controller::provider_runtime::tests::snapshot_builds_summary_and_filtered_items_from_one_provider_set` | Pins Provider Runtime aggregation to the selected Source identity, Profile, safe endpoint snapshot, and Source-scoped circuit health. |
| `representative-fixture-scope` | test | `proxy::direct_execution_regression::direct_execution_regression_fixtures_define_four_complete_protocols` | Proves that the R3.16 baseline evidence contains exactly four public-downstream-to-OpenAI cells. |
| `reachable-materializers-without-cell-regression` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | OpenAI and Gemini upstream materializers exist, but cells outside the representative suite lack complete direct-execution evidence. |
| `advanced-transform-only` | test | `cargo run -p cyder-api --bin transform_quality_gate -- --quick` | Transform quality evidence exists, but it does not satisfy the router-to-upstream direct-execution bar for advanced dimensions. |
| `r3-17-fixture-scope` | test | `proxy::direct_execution_regression::responses_target_fixtures_define_four_complete_protocols_and_native_evidence` | Freezes exactly four public-downstream-to-Responses fixtures with complete native request, response, stream, error, usage, and cancellation evidence. |
| `r3-17-stateless-policy` | test | `proxy::direct_execution_regression::responses_stateful_controls_are_rejected_before_credential_or_upstream_use` | Pins store, previous-response, conversation, and background state controls to payload-safe rejection before Provider credentials and network access. |
| `r3-17-materializer` | test | `proxy::direct_execution_regression::responses_target_materializes_native_requests_for_all_public_downstreams_and_modes` | Exercises all four downstream requests in non-stream and stream modes through the native Responses materializer, exact POST /responses body, Bearer auth, and one call. |
| `r3-17-source-check` | test | `controller::provider::tests::responses_source_check_saved_and_draft_keys_share_native_contract_without_proxy_logs` | Pins saved and draft Responses Source Check to one native stateless POST /responses without downstream quota or Request Log side effects. |
| `r3-17-openai-base` | test | `proxy::direct_execution_regression::openai_to_responses_base_cell_success_is_verified` | Executes OpenAI downstream non-stream and stream text through one frozen Responses Source with terminal usage, cost, connection closure, and one request log. |
| `r3-17-responses-base` | test | `proxy::direct_execution_regression::responses_to_responses_base_cell_success_is_verified` | Executes Responses downstream non-stream and stream text through one frozen Responses Source with same-wire lifecycle, terminal usage, cost, and one request log. |
| `r3-17-anthropic-base` | test | `proxy::direct_execution_regression::anthropic_to_responses_base_cell_success_is_verified` | Executes Anthropic downstream non-stream and stream text through one frozen Responses Source with native target materialization, terminal usage, cost, and one request log. |
| `r3-17-gemini-base` | test | `proxy::direct_execution_regression::gemini_to_responses_base_cell_success_is_verified` | Executes Gemini downstream non-stream and stream text through one frozen Responses Source with native target materialization, terminal usage, cost, and one request log. |
| `r3-17-http-error` | test | `proxy::direct_execution_regression::responses_target_http_429_is_authentic_bounded_and_never_retried_for_all_downstreams` | Exercises authentic bounded Responses Provider 429 errors through all four downstream envelopes with one call, one Error log, lease release, and no cost. |
| `r3-17-cancellation` | test | `proxy::direct_execution_regression::responses_target_precommit_client_cancellation_returns_499_and_releases_once` | Exercises pre-commit client cancellation for all four downstreams, proving HTTP 499, upstream closure, one Cancelled log, one lease release, and no cost. |
| `r3-17-terminal-cost` | test | `proxy::direct_execution_regression::responses_non_stream_incomplete_is_billed_but_failed_and_illegal_finals_are_not` | Pins incomplete as billable success while failed and illegal 2xx final states remain Error without token or cost settlement. |
| `r3-17-stream-terminal-error` | test | `proxy::direct_execution_regression::responses_stream_failed_and_error_events_emit_one_terminal_without_cost_for_all_downstreams` | Pins response.failed and error to one downstream-native stream terminal, one Error log, upstream closure, and no cost for every downstream. |
| `r3-17-stream-eof` | test | `proxy::direct_execution_regression::responses_stream_eof_without_terminal_fails_closed_for_all_downstreams` | Pins Responses SSE EOF without a legal terminal to one downstream-native failure, one Error log, lease release, and no cost. |
| `r3-17-body-limit` | test | `proxy::direct_execution_regression::responses_target_success_body_limits_fail_without_cost_for_all_downstreams` | Exercises raw and gzip-decoded Responses success body limits for all downstreams, proving one 502 failure and no usage or cost settlement. |
| `r3-17-route-boundary` | test | `proxy::direct_execution_regression::responses_stateful_resource_paths_are_unregistered_and_have_zero_side_effects` | Enumerates unversioned and v1 stateful Responses resources and single-item routes as protocol-native 404s before credentials, upstream access, or generation logging. |
| `r3-17-models-boundary` | test | `proxy::direct_execution_regression::models_routes_apply_static_kind_and_source_operation_filters_without_exposing_kind` | Pins both Responses Models aliases to the local statically executable CHAT catalog without upstream discovery or internal model-kind exposure. |
| `r3-17-openai-tools-cell` | test | `proxy::direct_execution_regression::openai_to_responses_tools_cell_is_full` | Executes the portable OpenAI function definition, selection, call, and result lifecycle through one Responses request with zero controlled loss. |
| `r3-17-responses-tools-cell` | test | `proxy::direct_execution_regression::responses_to_responses_tools_cell_is_full` | Executes the portable Responses function lifecycle through one same-wire Responses request with zero controlled loss. |
| `r3-17-anthropic-tools-cell` | test | `proxy::direct_execution_regression::anthropic_to_responses_tools_cell_is_full` | Executes the portable Anthropic function lifecycle through one Responses request with zero controlled loss. |
| `r3-17-gemini-tools-cell` | test | `proxy::direct_execution_regression::gemini_to_responses_tools_cell_has_typed_controlled_loss` | Executes the portable Gemini function lifecycle through one Responses request and asserts its payload-free typed controlled-loss fact. |
| `r3-17-openai-reasoning-cell` | test | `proxy::direct_execution_regression::openai_to_responses_reasoning_cell_has_typed_controlled_loss` | Executes OpenAI qualitative reasoning through one Responses request and asserts its payload-free typed controlled-loss fact. |
| `r3-17-responses-reasoning-cell` | test | `proxy::direct_execution_regression::responses_to_responses_reasoning_cell_is_full` | Executes Responses effort and summary controls through one same-wire Responses request with zero controlled loss. |
| `r3-17-anthropic-reasoning-cell` | test | `proxy::direct_execution_regression::anthropic_to_responses_reasoning_cell_has_typed_controlled_loss` | Executes Anthropic qualitative reasoning through one Responses request and asserts its payload-free typed controlled-loss fact. |
| `r3-17-gemini-reasoning-cell` | test | `proxy::direct_execution_regression::gemini_to_responses_reasoning_cell_has_typed_controlled_loss` | Executes Gemini qualitative reasoning through one Responses request and asserts its payload-free typed controlled-loss fact. |
| `r3-17-openai-multimodal-cell` | test | `proxy::direct_execution_regression::openai_to_responses_multimodal_cell_is_full` | Executes portable OpenAI image and inline file inputs through one Responses request with zero controlled loss. |
| `r3-17-responses-multimodal-cell` | test | `proxy::direct_execution_regression::responses_to_responses_multimodal_cell_is_full` | Executes portable Responses image and inline file inputs through one same-wire Responses request with zero controlled loss. |
| `r3-17-anthropic-multimodal-cell` | test | `proxy::direct_execution_regression::anthropic_to_responses_multimodal_cell_is_full` | Executes portable Anthropic image and inline document inputs through one Responses request with zero controlled loss. |
| `r3-17-gemini-multimodal-cell` | test | `proxy::direct_execution_regression::gemini_to_responses_multimodal_cell_has_typed_controlled_loss` | Executes portable Gemini inline media through one Responses request and asserts its payload-free typed controlled-loss fact. |
| `r3-17-openai-structured-cell` | test | `proxy::direct_execution_regression::openai_to_responses_structured_output_cell_is_full` | Executes OpenAI JSON schema output controls through one Responses request with zero controlled loss. |
| `r3-17-responses-structured-cell` | test | `proxy::direct_execution_regression::responses_to_responses_structured_output_cell_is_full` | Executes Responses JSON schema output controls through one same-wire Responses request with zero controlled loss. |
| `r3-17-anthropic-structured-cell` | test | `proxy::direct_execution_regression::anthropic_to_responses_structured_output_cell_has_typed_controlled_loss` | Executes Anthropic JSON schema output controls through one Responses request and asserts stable synthetic-envelope controlled loss. |
| `r3-17-gemini-structured-cell` | test | `proxy::direct_execution_regression::gemini_to_responses_structured_output_cell_has_typed_controlled_loss` | Executes Gemini JSON schema output controls through one Responses request and asserts property-ordering controlled loss. |
| `r3-17-tools-reject` | test | `proxy::direct_execution_regression::responses_target_rejects_invalid_or_forced_nonportable_tools_before_credentials` | Pins invalid and forced nonportable tools to payload-safe zero-call rejection before Provider credential decryption. |
| `r3-17-reasoning-reject` | test | `proxy::direct_execution_regression::responses_target_rejects_reasoning_conflicts_before_credentials` | Pins conflicting or unrepresentable reasoning controls to payload-safe zero-call rejection before Provider credential decryption. |
| `r3-17-multimodal-reject` | test | `proxy::direct_execution_regression::responses_target_rejects_all_unportable_media_before_credentials` | Pins unportable media, foreign IDs, hosted URIs, and invalid inline payloads to payload-safe zero-call rejection. |
| `r3-17-structured-reject` | test | `proxy::direct_execution_regression::responses_target_rejects_invalid_structured_outputs_before_credentials` | Pins conflicting and unrepresentable structured-output controls to payload-safe zero-call rejection before credentials. |
| `r3-18-fixture-scope` | test | `proxy::direct_execution_regression::anthropic_target_fixtures_define_four_complete_protocols_and_reject_bad_overlays` | Freezes exactly four public-downstream-to-Anthropic fixtures and their native request, response, stream, error, usage, and cancellation data. |
| `r3-18-evidence-registry` | test | `proxy::direct_execution_regression::anthropic_target_evidence_registry_covers_24_base_dimensions_and_16_advanced_cells` | Freezes all 24 Anthropic base-dimension mappings, all 16 unique advanced references, and the strict 6 full / 10 controlled-loss split. |
| `r3-18-materializer` | test | `proxy::direct_execution_regression::anthropic_target_materializes_native_requests_for_all_public_downstreams_and_modes` | Exercises all four downstreams in both modes through native POST /messages materialization, fixed headers, authentic 429 handling, and one call. |
| `r3-18-source-check` | test | `controller::provider::tests::anthropic_check_request_uses_messages_and_version_header` | Pins Anthropic Source Check to the shared safe Messages URL, fixed version, Source Patch, credential, model, and final request contract. |
| `r3-18-openai-base` | test | `proxy::direct_execution_regression::openai_to_anthropic_base_cell_success_is_verified` | Executes OpenAI non-stream and stream text through one Anthropic Source with terminal cache usage, cost, logging, lease release, and one call. |
| `r3-18-responses-base` | test | `proxy::direct_execution_regression::responses_to_anthropic_base_cell_success_is_verified` | Executes Responses non-stream and stream text through one Anthropic Source with terminal cache usage, cost, logging, lease release, and one call. |
| `r3-18-anthropic-base` | test | `proxy::direct_execution_regression::anthropic_to_anthropic_base_cell_success_is_verified` | Executes same-wire Anthropic non-stream and stream text with native termination, cache usage, cost, logging, lease release, and one call. |
| `r3-18-gemini-base` | test | `proxy::direct_execution_regression::gemini_to_anthropic_base_cell_success_is_verified` | Executes Gemini non-stream and stream text through one Anthropic Source with terminal cache usage, cost, logging, lease release, and one call. |
| `r3-18-openai-upstream-error` | test | `proxy::direct_execution_regression::openai_to_anthropic_base_request_is_verified` | Pins the OpenAI-to-Anthropic request, Source Patch, credential, authentic 429 envelope, error log, lease release, and one-call behavior. |
| `r3-18-responses-upstream-error` | test | `proxy::direct_execution_regression::responses_to_anthropic_base_request_is_verified` | Pins the Responses-to-Anthropic request, Source Patch, credential, authentic 429 envelope, error log, lease release, and one-call behavior. |
| `r3-18-anthropic-upstream-error` | test | `proxy::direct_execution_regression::anthropic_to_anthropic_base_request_is_verified` | Pins the same-wire Anthropic request, Source Patch, credential, authentic 429 envelope, error log, lease release, and one-call behavior. |
| `r3-18-gemini-upstream-error` | test | `proxy::direct_execution_regression::gemini_to_anthropic_base_request_is_verified` | Pins the Gemini-to-Anthropic request, Source Patch, credential, authentic 429 envelope, error log, lease release, and one-call behavior. |
| `r3-18-cancellation` | test | `proxy::direct_execution_regression::anthropic_target_precommit_client_cancellation_returns_499_and_releases_once` | Exercises pre-commit cancellation for all four downstreams with HTTP 499, upstream closure, one Cancelled log, no cost, and one lease release. |
| `r3-18-stream-success-terminal` | test | `proxy::direct_execution_regression::anthropic_stream_message_stop_closes_hanging_upstream_for_all_downstreams` | Pins message_stop as the only successful terminal for all downstreams, closing a hanging upstream and settling usage/cost exactly once. |
| `r3-18-stream-terminal-error` | test | `proxy::direct_execution_regression::anthropic_stream_error_is_raw_same_wire_and_one_native_terminal_cross_wire` | Pins raw same-wire Anthropic error SSE and exactly one cross-wire native error terminal, with no partial usage or cost settlement. |
| `r3-18-stream-eof` | test | `proxy::direct_execution_regression::anthropic_stream_eof_emits_one_native_error_and_clears_partial_usage` | Pins EOF without message_stop to one native downstream failure, one Error log, lease release, and cleared partial usage/cost. |
| `r3-18-non-stream-fail-closed` | test | `proxy::direct_execution_regression::anthropic_unknown_non_stream_terminal_fails_closed_cross_wire_without_cost` | Pins unknown cross-wire non-stream terminal semantics to a payload-free 502 with no usage or cost settlement and one upstream call. |
| `r3-18-route-boundary` | test | `proxy::router::tests::anthropic_public_surface_exposes_only_messages_and_local_models` | Pins Anthropic public routes to Messages and local Models only, with Count, Batches, Files, wrong-version, and wrong-method zero-side-effect rejection. |
| `r3-18-openai-tools-cell` | test | `proxy::direct_execution_regression::openai_to_anthropic_tools_cell_is_full` | Executes the portable OpenAI function lifecycle through one Anthropic request with zero controlled loss. |
| `r3-18-responses-tools-cell` | test | `proxy::direct_execution_regression::responses_to_anthropic_tools_cell_is_full` | Executes the portable Responses function lifecycle through one Anthropic request with zero controlled loss. |
| `r3-18-anthropic-tools-cell` | test | `proxy::direct_execution_regression::anthropic_to_anthropic_tools_cell_is_full` | Executes the same-wire Anthropic function lifecycle through one native request with zero controlled loss. |
| `r3-18-gemini-tools-cell` | test | `proxy::direct_execution_regression::gemini_to_anthropic_tools_cell_has_typed_controlled_loss` | Executes the Gemini function lifecycle through one Anthropic request and asserts its payload-free synthetic-correlation loss. |
| `r3-18-openai-reasoning-cell` | test | `proxy::direct_execution_regression::openai_to_anthropic_reasoning_cell_has_typed_controlled_loss` | Executes OpenAI qualitative reasoning through one Anthropic request and asserts its payload-free typed controlled loss. |
| `r3-18-responses-reasoning-cell` | test | `proxy::direct_execution_regression::responses_to_anthropic_reasoning_cell_has_typed_controlled_loss` | Executes Responses qualitative reasoning through one Anthropic request and asserts its payload-free typed controlled loss. |
| `r3-18-anthropic-reasoning-cell` | test | `proxy::direct_execution_regression::anthropic_to_anthropic_reasoning_cell_is_full` | Executes same-wire Anthropic thinking controls through one native request with raw signature isolation and zero controlled loss. |
| `r3-18-gemini-reasoning-cell` | test | `proxy::direct_execution_regression::gemini_to_anthropic_reasoning_cell_has_typed_controlled_loss` | Executes Gemini qualitative reasoning through one Anthropic request and asserts its payload-free typed controlled loss. |
| `r3-18-openai-multimodal-cell` | test | `proxy::direct_execution_regression::openai_to_anthropic_multimodal_cell_has_typed_controlled_loss` | Executes portable OpenAI image and document inputs through one Anthropic request with typed metadata loss. |
| `r3-18-responses-multimodal-cell` | test | `proxy::direct_execution_regression::responses_to_anthropic_multimodal_cell_has_typed_controlled_loss` | Executes portable Responses image and document inputs through one Anthropic request with typed metadata loss. |
| `r3-18-anthropic-multimodal-cell` | test | `proxy::direct_execution_regression::anthropic_to_anthropic_multimodal_cell_is_full` | Executes same-wire Anthropic native image and document inputs through one request with zero controlled loss. |
| `r3-18-gemini-multimodal-cell` | test | `proxy::direct_execution_regression::gemini_to_anthropic_multimodal_cell_has_typed_controlled_loss` | Executes portable Gemini inline media through one Anthropic request with typed metadata loss. |
| `r3-18-openai-structured-cell` | test | `proxy::direct_execution_regression::openai_to_anthropic_structured_output_cell_has_typed_controlled_loss` | Executes OpenAI JSON Schema output controls through one Anthropic request with typed metadata loss. |
| `r3-18-responses-structured-cell` | test | `proxy::direct_execution_regression::responses_to_anthropic_structured_output_cell_has_typed_controlled_loss` | Executes Responses JSON Schema output controls through one Anthropic request with typed metadata loss. |
| `r3-18-anthropic-structured-cell` | test | `proxy::direct_execution_regression::anthropic_to_anthropic_structured_output_cell_is_full` | Executes same-wire Anthropic GA output_config.format through one native request with zero controlled loss. |
| `r3-18-gemini-structured-cell` | test | `proxy::direct_execution_regression::gemini_to_anthropic_structured_output_cell_has_typed_controlled_loss` | Executes Gemini JSON Schema output controls through one Anthropic request with typed property-ordering loss. |
| `r3-18-tools-reject` | test | `proxy::direct_execution_regression::anthropic_tool_invalid_references_and_forced_nonportable_are_zero_call` | Pins invalid tool references and forced nonportable tools to payload-safe rejection before credentials and network. |
| `r3-18-reasoning-reject` | test | `proxy::direct_execution_regression::gemini_positive_thinking_budget_to_anthropic_is_zero_call` | Pins unportable positive Gemini thinking budgets to payload-safe rejection before credentials and network. |
| `r3-18-multimodal-reject` | test | `proxy::direct_execution_regression::unportable_multimodal_inputs_to_anthropic_are_zero_call` | Pins unportable media, invalid text, and executable content to payload-safe zero-call rejection. |
| `r3-18-structured-reject` | test | `proxy::direct_execution_regression::invalid_structured_outputs_to_anthropic_are_zero_call` | Pins non-schema, conflicting, and invalid structured controls to payload-safe rejection before credentials and network. |
| `r3-19-base-registry` | test | `proxy::direct_execution_regression::r3_19_gemini_target_evidence_registry_covers_exactly_24_base_dimensions` | Freezes four unique Gemini target base-cell references and all 24 base-dimension mappings. |
| `r3-19-openai-base` | test | `proxy::direct_execution_regression::openai_to_gemini_base_cell_is_verified` | Executes OpenAI non-stream, stream, usage, terminal, 429, and cancellation paths through one Gemini Source. |
| `r3-19-responses-base` | test | `proxy::direct_execution_regression::responses_to_gemini_base_cell_is_verified` | Executes Responses non-stream, stream, usage, terminal, 429, and cancellation paths through one Gemini Source. |
| `r3-19-anthropic-base` | test | `proxy::direct_execution_regression::anthropic_to_gemini_base_cell_is_verified` | Executes Anthropic non-stream, stream, usage, terminal, 429, and cancellation paths through one Gemini Source. |
| `r3-19-gemini-base` | test | `proxy::direct_execution_regression::gemini_to_gemini_base_cell_is_verified` | Executes same-wire Gemini non-stream, stream, usage, terminal, 429, and cancellation paths with native fidelity. |
| `r3-19-advanced-registry` | test | `proxy::direct_execution_regression::r3_19_gemini_advanced_evidence_registry_has_16_unique_cells_and_4_full_12_loss` | Freezes 16 unique Gemini advanced cell references and the exact 4 full / 12 controlled-loss split. |
| `r3-19-openai-tools-cell` | test | `proxy::direct_execution_regression::openai_to_gemini_tools_cell_has_typed_controlled_loss` | Executes OpenAI tools through one Gemini request and asserts payload-free strictness loss. |
| `r3-19-responses-tools-cell` | test | `proxy::direct_execution_regression::responses_to_gemini_tools_cell_has_typed_controlled_loss` | Executes Responses tools through one Gemini request and asserts payload-free strictness loss. |
| `r3-19-anthropic-tools-cell` | test | `proxy::direct_execution_regression::anthropic_to_gemini_tools_cell_has_typed_controlled_loss` | Executes Anthropic tools through one Gemini request and asserts payload-free parallel-policy loss. |
| `r3-19-gemini-tools-cell` | test | `proxy::direct_execution_regression::gemini_to_gemini_tools_cell_is_full` | Executes native Gemini tool IDs, calls, results, and thought signatures with zero controlled loss. |
| `r3-19-openai-reasoning-cell` | test | `proxy::direct_execution_regression::openai_to_gemini_reasoning_cell_has_typed_controlled_loss` | Executes OpenAI reasoning through one Gemini request and asserts payload-free high-end effort clamping. |
| `r3-19-responses-reasoning-cell` | test | `proxy::direct_execution_regression::responses_to_gemini_reasoning_cell_has_typed_controlled_loss` | Executes Responses reasoning through one Gemini request and asserts payload-free high-end effort clamping. |
| `r3-19-anthropic-reasoning-cell` | test | `proxy::direct_execution_regression::anthropic_to_gemini_reasoning_cell_has_typed_controlled_loss` | Executes Anthropic adaptive reasoning through one Gemini request and asserts payload-free effort clamping. |
| `r3-19-gemini-reasoning-cell` | test | `proxy::direct_execution_regression::gemini_to_gemini_reasoning_cell_is_full` | Executes native Gemini thinking controls and future fields with zero controlled loss. |
| `r3-19-openai-multimodal-cell` | test | `proxy::direct_execution_regression::openai_to_gemini_multimodal_cell_has_typed_controlled_loss` | Executes OpenAI inline image input through one Gemini request and asserts payload-free detail loss. |
| `r3-19-responses-multimodal-cell` | test | `proxy::direct_execution_regression::responses_to_gemini_multimodal_cell_has_typed_controlled_loss` | Executes Responses inline image input through one Gemini request and asserts payload-free detail loss. |
| `r3-19-anthropic-multimodal-cell` | test | `proxy::direct_execution_regression::anthropic_to_gemini_multimodal_cell_has_typed_controlled_loss` | Executes Anthropic inline document input through one Gemini request and asserts payload-free filename loss. |
| `r3-19-gemini-multimodal-cell` | test | `proxy::direct_execution_regression::gemini_to_gemini_multimodal_cell_is_full` | Executes native Gemini inlineData, fileData, and future media fields with zero controlled loss. |
| `r3-19-openai-structured-cell` | test | `proxy::direct_execution_regression::openai_to_gemini_structured_output_cell_has_typed_controlled_loss` | Executes OpenAI JSON Schema output through one Gemini request and asserts payload-free envelope metadata loss. |
| `r3-19-responses-structured-cell` | test | `proxy::direct_execution_regression::responses_to_gemini_structured_output_cell_has_typed_controlled_loss` | Executes Responses JSON Schema output through one Gemini request and asserts payload-free envelope metadata loss. |
| `r3-19-anthropic-structured-cell` | test | `proxy::direct_execution_regression::anthropic_to_gemini_structured_output_cell_has_typed_controlled_loss` | Executes Anthropic JSON Schema output through one Gemini request and asserts payload-free envelope loss. |
| `r3-19-gemini-structured-cell` | test | `proxy::direct_execution_regression::gemini_to_gemini_structured_output_cell_is_full` | Executes native Gemini structured-output controls and future schema fields with zero controlled loss. |
| `r3-19-tools-reject` | test | `proxy::direct_execution_regression::r3_19_invalid_tools_and_unsigned_history_are_precredential_zero_call` | Pins invalid tools and unsigned tool history to payload-safe rejection before credentials and network. |
| `r3-19-reasoning-reject` | test | `proxy::direct_execution_regression::r3_19_invalid_reasoning_controls_are_precredential_zero_call` | Pins invalid and conflicting reasoning controls to payload-safe rejection before credentials and network. |
| `r3-19-multimodal-reject` | test | `proxy::direct_execution_regression::r3_19_invalid_multimodal_inputs_are_precredential_zero_call` | Pins remote, foreign, invalid, or wrong-role media to payload-safe rejection before credentials and network. |
| `r3-19-structured-reject` | test | `proxy::direct_execution_regression::r3_19_invalid_structured_outputs_are_precredential_zero_call` | Pins invalid or unrepresentable structured outputs to payload-safe rejection before credentials and network. |
| `r3-19-profile-equivalence` | test | `proxy::direct_execution_regression::r3_19_gemini_and_vertex_profiles_execute_equivalent_generate_and_stream_contracts` | Executes equivalent Gemini and Vertex generate/stream requests with profile-specific collection URL and authentication. |
| `r3-19-profile-registry` | test | `proxy::direct_execution_regression::r3_19_gemini_vertex_profile_evidence_registry_covers_all_four_operations` | Freezes Gemini and Vertex evidence for generate, stream, countTokens, and Source Check operations. |
| `r3-19-source-check` | test | `controller::provider::tests::gemini_and_vertex_source_check_send_shared_minimal_contract_once` | Pins both Gemini-family Source Checks to one shared safe URL, body, query, header, and response contract. |
| `r3-19-route-boundary` | test | `proxy::router::tests::r3_19_gemini_public_surface_aliases_methods_and_prohibited_products_are_closed` | Pins Gemini aliases, methods, local Models, generation, countTokens, and all prohibited product routes. |
| `r3-19-count-tokens` | test | `proxy::direct_execution_regression::r3_19_count_tokens_two_legal_shapes_use_gemini_and_vertex_once_without_usage_or_cost` | Executes both legal CountTokens shapes once through Gemini and Vertex without generation usage or cost settlement. |
| `r3-19-count-tokens-reject` | test | `proxy::direct_execution_regression::r3_19_count_tokens_invalid_shapes_and_incompatible_source_are_precredential_zero_call` | Pins invalid CountTokens shapes and non-Gemini targets to precredential zero-call rejection. |
| `r3-16-reasoning-direct` | test | `proxy::direct_execution_regression::all_public_downstream_reasoning_controls_reach_the_openai_target` | Executes the representative reasoning main path from every public downstream through one frozen OpenAI Source. |
| `r3-16-reasoning-responses-diagnostic` | test | `service::transform::facade::tests::responses_reasoning_effort_maps_to_openai_and_summary_is_safely_ignored` | Pins Responses effort mapping and the typed controlled-loss fact for ignored summary semantics. |
| `r3-16-reasoning-anthropic-diagnostic` | test | `service::transform::facade::tests::anthropic_qualitative_reasoning_maps_to_openai_without_injecting_cot` | Pins Anthropic qualitative reasoning mapping and proves no chain-of-thought text is synthesized. |
| `r3-16-reasoning-gemini-diagnostic` | test | `service::transform::facade::tests::gemini_reasoning_controls_map_to_openai_with_budget_sentinels` | Pins Gemini thinking-level and sentinel mapping with bounded controlled-loss diagnostics. |
| `r3-16-reasoning-conflict-reject` | test | `proxy::direct_execution_regression::registered_request_conflicts_are_rejected_before_credential_or_upstream_use` | Pins positive Gemini thinkingBudget rejection before credentials and network; Anthropic budget_tokens uses an audited qualitative downgrade. |
| `r3-16-reasoning-responses-reject` | test | `proxy::direct_execution_regression::malformed_responses_reasoning_is_rejected_before_credential_or_upstream_use` | Pins a representative unrepresentable Responses reasoning shape to a payload-safe zero-call rejection. |
| `r3-16-multimodal-direct` | test | `proxy::direct_execution_regression::all_public_downstream_multimodal_inputs_reach_the_openai_target` | Executes image, supported audio, file_data, and same-target file inputs through one OpenAI call for every downstream. |
| `r3-16-multimodal-responses-diagnostic` | test | `service::transform::facade::tests::responses_multimodal_input_maps_to_openai_image_audio_and_file_parts` | Pins Responses multimodal mapping and audited safe metadata loss. |
| `r3-16-multimodal-anthropic-diagnostic` | test | `service::transform::facade::tests::anthropic_multimodal_input_maps_to_openai_without_textualizing_payloads` | Pins Anthropic image/document mapping without textualizing binary payloads. |
| `r3-16-multimodal-gemini-diagnostic` | test | `service::transform::facade::tests::gemini_multimodal_input_classifies_inline_media_for_openai` | Pins Gemini inline image/audio/file classification and controlled metadata loss. |
| `r3-16-multimodal-reject` | test | `proxy::direct_execution_regression::all_public_downstreams_reject_unportable_media_before_credentials` | Pins representative unportable media and foreign file references to payload-safe zero-call rejection. |
| `r3-16-structured-direct` | test | `proxy::direct_execution_regression::all_public_downstream_structured_outputs_reach_the_openai_target` | Executes JSON schema requirements from every downstream and preserves constraints at the OpenAI target. |
| `r3-16-structured-responses-transform` | test | `service::transform::facade::tests::responses_structured_outputs_preserve_json_object_and_schema_contracts` | Pins lossless Responses JSON object/schema mapping including name, strict, and schema constraints. |
| `r3-16-structured-anthropic-diagnostic` | test | `service::transform::facade::tests::anthropic_structured_output_synthesizes_a_stable_openai_name` | Pins Anthropic JSON schema mapping with stable synthesized name and a typed controlled-loss fact. |
| `r3-16-structured-gemini-diagnostic` | test | `service::transform::facade::tests::gemini_structured_output_preserves_constraints_and_drops_only_property_ordering` | Pins Gemini JSON schema constraint preservation and the audited property-ordering loss. |
| `r3-16-structured-reject` | test | `proxy::direct_execution_regression::all_public_downstreams_reject_unrepresentable_structured_outputs_before_credentials` | Pins grammar and non-JSON structured requirements to payload-safe zero-call rejection. |
| `r3-16-tools-direct` | test | `proxy::direct_execution_regression::all_public_downstream_portable_tool_lifecycles_reach_the_openai_target` | Executes portable function definitions, selection, parallel control, call/result correlation, and strict through one OpenAI call. |
| `r3-16-tools-cross-wire-transform` | test | `service::transform::facade::tests::portable_tool_controls_from_each_cross_wire_protocol_reach_openai` | Pins typed tool-control mappings for Responses, Anthropic, and Gemini with controlled-loss diagnostics. |
| `r3-16-tools-stable-results` | test | `service::transform::facade::tests::missing_tool_call_ids_are_stable_and_structured_results_are_canonical_text` | Pins stable synthesized IDs and deterministic structured-result textualization without logging payloads. |
| `r3-16-tools-reject` | test | `proxy::direct_execution_regression::forced_nonportable_cross_wire_tools_reject_before_credentials` | Pins forced built-in/custom provider tools to payload-safe zero-call rejection. |
| `r3-16-profile-field-policy` | test | `service::transform::providers::openai::target::tests::profiles_apply_distinct_unknown_field_policies` | Pins official-known/unknown passthrough, compatible core passthrough, and Gemini recursive-closed Chat field policy. |
| `r3-16-profile-source-contract` | test | `database::migration_smoke_tests::sqlite_r316_destructive_upgrade_enforces_openai_upstream_contract` | Pins OpenAI-wire defaults, Base URL persistence, operation columns, unsupported combinations, and destructive enum replacement. |
| `r3-16-gemini-openai-closed-policy` | test | `proxy::direct_execution_regression::gemini_openai_profile_rejects_unknown_and_conflicting_chat_fields_before_credentials` | Pins Gemini OpenAI recursive allowlist and reasoning conflict rejection before credentials and network. |
| `r3-16-embeddings-direct` | test | `proxy::direct_execution_regression::embeddings_execute_once_for_each_openai_wire_profile_and_preserve_the_response` | Pins all three OpenAI-wire Embeddings field policies, exact operation path, response preservation, usage, and cost. |
| `r3-16-embeddings-reject` | test | `proxy::direct_execution_regression::invalid_embeddings_requests_are_rejected_before_credentials_and_network` | Pins Profile-specific invalid Embeddings requests to zero-decrypt, zero-network rejection. |
| `r3-16-rerank-direct` | test | `proxy::direct_execution_regression::compatible_rerank_is_a_single_call_transparent_transport_without_private_usage_parsing` | Pins the explicitly enabled OPENAI_COMPATIBLE opaque Rerank envelope and one-call transparent response. |
| `r3-16-rerank-profile-guard` | test | `proxy::direct_execution_regression::rerank_requires_an_enabled_compatible_source_before_credential_decryption` | Pins unsupported or disabled Rerank Profile combinations to zero-decrypt, zero-network rejection. |
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
| `r3-12-request-patch-contract` | test | `controller::request_patch::tests::request_patch_openapi_matches_aggregate_routes_and_current_detail_shapes` | Pins Source and Model+Source aggregate Request Patch routes, current DTO shapes, old route negatives, no-store responses, and ownership errors. |
| `r3-12-request-patch-execution` | test | `proxy::direct_execution_regression::request_patch_query_value_reaches_upstream_but_not_request_log` | Pins the Source-bound Variant through real request materialization and upstream execution while keeping patched values out of the persisted request log. |
| `r3-12-request-patch-migration` | test | `database::migration_smoke_tests::sqlite_r310_source_migration_preserves_rows_and_enforces_source_contract` | Pins the R3.11-to-R3.12 destructive migration, historical suffix rename, preserved governance/metrics, removed configuration tables, and new Source-bound constraints. |
| `r3-15-transform-quality-contract` | test | `service::transform::quality::tests::test_transform_contract_summary_covers_failures_outcomes_and_accounting` | Pins eleven production-entry transform outcomes, failure origins, semantic accounting, diagnostic overflow, and Gate closure. |
| `r3-15-transform-payload-free-contract` | test | `service::transform::quality::tests::test_transform_contract_report_omits_payload_and_safe_summary` | Pins the quality report to payload-free aggregate facts without fixture content, hashes, or per-payload safe summaries. |
| `r3-15-same-wire-passthrough` | test | `proxy::direct_execution_regression::same_wire_non_stream_observation_failure_preserves_upstream_bytes` | Pins byte-preserving same-wire response passthrough when best-effort observation degrades. |
| `r3-15-minor-loss-runtime` | test | `proxy::direct_execution_regression::retained_cross_wire_minor_loss_succeeds_once_and_drops_only_audited_metadata` | Pins cross-wire controlled minor loss, one upstream call, internal-only diagnostics, Request Log success, and resource release. |
| `r3-15-four-protocol-stream-failure` | test | `proxy::direct_execution_regression::four_public_protocols_emit_one_native_terminal_on_cross_wire_stream_decode_failure` | Pins one protocol-native terminal after headers for source transform failure across all four public downstream protocols. |
| `r3-15-target-stream-failure` | test | `proxy::direct_execution_regression::cross_wire_target_stream_rejection_emits_one_native_terminal_and_releases_resources` | Pins target transform rejection after headers to one native terminal, an error Request Log, one upstream call, and released resources. |
| `r3-20-openai-compatible-chat` | test | `proxy::direct_execution_regression::ollama_openai_compatible_recipe_targets_v1_chat_with_placeholder_bearer` | Proves a regular OPENAI_COMPATIBLE Source reaches /v1/chat/completions once with a placeholder Bearer credential, usage, and Request Log. |
| `r3-20-openai-compatible-stream` | test | `proxy::direct_execution_regression::ollama_openai_compatible_recipe_streams_v1_chat_once` | Proves the regular OPENAI_COMPATIBLE Source uses the existing OpenAI SSE and [DONE] lifecycle at /v1/chat/completions without a native stream dialect. |
| `r3-20-openai-compatible-embeddings` | test | `proxy::direct_execution_regression::ollama_openai_compatible_recipe_targets_v1_embeddings_once` | Proves explicitly enabled Embeddings on a regular OPENAI_COMPATIBLE Source reaches /v1/embeddings once and records OpenAI usage. |

## Status semantics

- Base: `verified`, `partial`, `unavailable`, `not_verified`.
- Advanced: `full`, `controlled_loss`, `explicit_reject`, `not_verified`.
- `verified` requires direct execution through routing, materialization, transport, downstream output, and call-count assertions. Transform-only evidence is insufficient.
