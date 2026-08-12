<!-- GENERATED FILE. DO NOT EDIT. Canonical source: `docs/protocol-compatibility.yaml`. -->
# Protocol Compatibility

This document reports current, evidence-backed runtime behavior. It is generated deterministically by `cargo run -p cyder-api --bin compatibility_matrix -- --write` from `docs/protocol-compatibility.yaml`.

Unversioned downstream routes are compatibility aliases for the current `/v1` semantics. They are not a protocol-version claim; a future `/v2` may become the alias target without removing the unversioned route.

## Protocol boundaries

- Matrix schema: v5
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
| Responses | `R3.17` |
| Anthropic | `R3.18` |
| Gemini | `R3.19` |
| Ollama | `R3.20` |

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
| Ollama | Ollama | `standard` | `bearer_api_key` | `base_url` |
| Anthropic | Anthropic | `standard` | `anthropic_api_key` | `base_url` |
| Responses | Responses | `standard` | `bearer_api_key` | `base_url` |
| GeminiOpenAI | OpenAI | `gemini_openai_compatibility` | `bearer_api_key` | `base_url` |

## OpenAI-wire Profile contracts

| Profile | Base URL requirement | Default Base URL | Customizable | Auth | Chat | Chat field policy | Embeddings | Embeddings field policy | Rerank | Rerank field policy | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | `optional_with_default` | https://api.openai.com/v1 | `true` | `bearer_api_key` | configurable (default enabled) | `official_fields_validated_unknown_extensions_passthrough` | configurable (default enabled) | `official_fields_validated_unknown_extensions_passthrough` | unsupported (default disabled) | `unsupported` | r3-16-profile-field-policy, r3-16-profile-source-contract, r3-16-embeddings-direct, r3-16-rerank-profile-guard |
| OpenAICompatible | `required` | — | `true` | `bearer_api_key` | configurable (default enabled) | `core_fields_validated_vendor_extensions_passthrough` | configurable (default disabled) | `core_fields_validated_vendor_extensions_passthrough` | configurable (default disabled) | `opaque_envelope_passthrough` | r3-16-profile-field-policy, r3-16-profile-source-contract, r3-16-embeddings-direct, r3-16-rerank-direct |
| GeminiOpenAI | `optional_with_default` | https://generativelanguage.googleapis.com/v1beta/openai | `true` | `bearer_api_key` | configurable (default enabled) | `recursive_closed_allowlist` | configurable (default enabled) | `model_input_closed_contract` | unsupported (default disabled) | `unsupported` | r3-16-profile-field-policy, r3-16-profile-source-contract, r3-16-gemini-openai-closed-policy, r3-16-embeddings-direct, r3-16-rerank-profile-guard |

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
| Gemini | OpenAI | verified | verified | verified | verified | verified | verified | — | direct-call-count, direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Gemini | Responses | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.17 | native-materializer-unavailable |
| Gemini | Anthropic | unavailable | unavailable | unavailable | unavailable | unavailable | unavailable | R3.18 | native-materializer-unavailable |
| Gemini | Gemini | verified | verified | verified | verified | verified | verified | — | direct-cancellation, direct-non-stream, direct-stream, direct-upstream-error |
| Gemini | Ollama | not_verified | partial | partial | partial | not_verified | not_verified | R3.20 | ollama-stream-incomplete, ollama-upstream-incomplete |

## Generation advanced dimensions

| Downstream | Upstream | Tools | Reasoning | Multimodal | Structured output | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- |
| OpenAI | OpenAI | full | full | full | full | — | r3-16-multimodal-direct, r3-16-reasoning-direct, r3-16-structured-direct, r3-16-tools-direct |
| OpenAI | Responses | not_verified | not_verified | not_verified | not_verified | R3.17 | advanced-transform-only |
| OpenAI | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.18 | advanced-transform-only |
| OpenAI | Gemini | not_verified | not_verified | not_verified | not_verified | R3.19 | advanced-transform-only |
| OpenAI | Ollama | not_verified | not_verified | not_verified | not_verified | R3.20 | advanced-transform-only |
| Responses | OpenAI | controlled_loss | controlled_loss | controlled_loss | full | — | r3-16-multimodal-direct, r3-16-multimodal-reject, r3-16-multimodal-responses-diagnostic, r3-16-reasoning-direct, r3-16-reasoning-responses-diagnostic, r3-16-reasoning-responses-reject, r3-16-structured-direct, r3-16-structured-responses-transform, r3-16-tools-cross-wire-transform, r3-16-tools-direct, r3-16-tools-reject, r3-16-tools-stable-results |
| Responses | Responses | not_verified | not_verified | not_verified | not_verified | R3.17 | advanced-transform-only |
| Responses | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.18 | advanced-transform-only |
| Responses | Gemini | not_verified | not_verified | not_verified | not_verified | R3.19 | advanced-transform-only |
| Responses | Ollama | not_verified | not_verified | not_verified | not_verified | R3.20 | advanced-transform-only |
| Anthropic | OpenAI | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-16-multimodal-anthropic-diagnostic, r3-16-multimodal-direct, r3-16-multimodal-reject, r3-16-reasoning-anthropic-diagnostic, r3-16-reasoning-direct, r3-16-structured-anthropic-diagnostic, r3-16-structured-direct, r3-16-structured-reject, r3-16-tools-cross-wire-transform, r3-16-tools-direct, r3-16-tools-reject, r3-16-tools-stable-results |
| Anthropic | Responses | not_verified | not_verified | not_verified | not_verified | R3.17 | advanced-transform-only |
| Anthropic | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.18 | advanced-transform-only |
| Anthropic | Gemini | not_verified | not_verified | not_verified | not_verified | R3.19 | advanced-transform-only |
| Anthropic | Ollama | not_verified | not_verified | not_verified | not_verified | R3.20 | advanced-transform-only |
| Gemini | OpenAI | controlled_loss | controlled_loss | controlled_loss | controlled_loss | — | r3-16-multimodal-direct, r3-16-multimodal-gemini-diagnostic, r3-16-multimodal-reject, r3-16-reasoning-conflict-reject, r3-16-reasoning-direct, r3-16-reasoning-gemini-diagnostic, r3-16-structured-direct, r3-16-structured-gemini-diagnostic, r3-16-structured-reject, r3-16-tools-cross-wire-transform, r3-16-tools-direct, r3-16-tools-reject, r3-16-tools-stable-results |
| Gemini | Responses | not_verified | not_verified | not_verified | not_verified | R3.17 | advanced-transform-only |
| Gemini | Anthropic | not_verified | not_verified | not_verified | not_verified | R3.18 | advanced-transform-only |
| Gemini | Gemini | not_verified | not_verified | not_verified | not_verified | R3.19 | advanced-transform-only |
| Gemini | Ollama | not_verified | not_verified | not_verified | not_verified | R3.20 | advanced-transform-only |

## Utility contracts

| Utility | Route suffix | Method | Exposed on | Not exposed on | Execution | Allowed upstreams | Incompatible upstream | Verification | Owner | Evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| models | `/models` | GET | OpenAI, Responses, Anthropic, Gemini | — | local | — | not_applicable | verified | — | router-contract, router-method-contract, r3-11-model-catalog-selector, r3-11-ollama-discovery-boundary |
| embeddings | `/embeddings` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | verified | — | router-contract, router-method-contract, utility-pre-send-reject, r3-16-embeddings-direct, r3-16-embeddings-reject |
| rerank | `/rerank` | POST | OpenAI | Responses, Anthropic, Gemini | upstream | OpenAI | pre_send_reject | verified | — | router-contract, router-method-contract, utility-pre-send-reject, r3-16-rerank-direct, r3-16-rerank-profile-guard |
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
| `direct-non-stream` | test | `proxy::direct_execution_regression::direct_execution_regression_non_stream_request_response_usage_and_log_golden` | Exercises routing, request materialization, non-stream response conversion, usage, logging, and one upstream call for all four OpenAI-target cells plus the retained native Gemini cell. |
| `direct-stream` | test | `proxy::direct_execution_regression::direct_execution_regression_stream_events_usage_and_single_call_golden` | Exercises streaming request materialization, ordered downstream events, text, usage, and one upstream call for all four OpenAI-target cells plus the retained native Gemini cell. |
| `direct-upstream-error` | test | `proxy::direct_execution_regression::direct_execution_regression_upstream_429_is_authentic_logged_and_never_retried` | Exercises authentic upstream error conversion, complete protocol bodies, safe logging, and the no-retry contract for all four OpenAI-target cells plus native Gemini. |
| `direct-cancellation` | test | `proxy::direct_execution_regression::direct_execution_regression_client_cancellation_closes_upstream_and_logs_cancelled` | Exercises client cancellation, upstream connection closure, cancelled logging, and the no-retry contract for all four OpenAI-target cells plus native Gemini. |
| `direct-call-count` | test | `proxy::direct_execution_regression::four_public_downstream_generation_paths_call_upstream_at_most_once` | Pins each of the four public downstream generation paths to one frozen OpenAI Source and exactly one real Chat Completions request. |
| `r3-9-primary-source-aggregate` | test | `database::provider::tests::provider_aggregate_reads_fail_closed_for_missing_or_ambiguous_source` | Pins every readable Logical Provider to exactly one active primary Upstream Source and fails closed for zero or multiple Sources. |
| `r3-9-source-runtime-evidence` | test | `controller::provider_runtime::tests::snapshot_builds_summary_and_filtered_items_from_one_provider_set` | Pins Provider Runtime aggregation to the selected Source identity, Profile, safe endpoint snapshot, and Source-scoped circuit health. |
| `representative-fixture-scope` | test | `proxy::direct_execution_regression::direct_execution_regression_fixtures_define_four_complete_protocols` | Proves that the R3.16 baseline evidence contains exactly four public-downstream-to-OpenAI cells and no downstream Ollama fixture. |
| `reachable-materializers-without-cell-regression` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | OpenAI and Gemini upstream materializers exist, but cells outside the representative suite lack complete direct-execution evidence. |
| `native-materializer-unavailable` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | Responses and Anthropic upstream generation protocols are rejected before HTTP materialization. |
| `ollama-upstream-incomplete` | code | `server/src/proxy/runtime/materializer.rs::select_generation_prepare_kind` | An Ollama /api/chat materialization path exists, but no complete direct-execution cell proves native behavior; model directories are local configured Models and are not discovered through /api/tags. |
| `ollama-stream-incomplete` | code | `server/src/proxy/runtime/transport/stream.rs` | The transport does not provide a complete native Ollama NDJSON streaming contract. |
| `advanced-transform-only` | test | `cargo run -p cyder-api --bin transform_quality_gate -- --quick` | Transform quality evidence exists, but it does not satisfy the router-to-upstream direct-execution bar for advanced dimensions. |
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
| `r3-11-ollama-discovery-boundary` | code | `server/src/proxy/models.rs::get_accessible_models` | The public model directory is built from saved local Model records; no Ollama /api/tags discovery or import route is part of the R3.11 contract. |
| `r3-12-request-patch-contract` | test | `controller::request_patch::tests::request_patch_openapi_matches_aggregate_routes_and_current_detail_shapes` | Pins Source and Model+Source aggregate Request Patch routes, current DTO shapes, old route negatives, no-store responses, and ownership errors. |
| `r3-12-request-patch-execution` | test | `proxy::direct_execution_regression::request_patch_query_value_reaches_upstream_but_not_request_log` | Pins the Source-bound Variant through real request materialization and upstream execution while keeping patched values out of the persisted request log. |
| `r3-12-request-patch-migration` | test | `database::migration_smoke_tests::sqlite_r310_source_migration_preserves_rows_and_enforces_source_contract` | Pins the R3.11-to-R3.12 destructive migration, historical suffix rename, preserved governance/metrics, removed configuration tables, and new Source-bound constraints. |
| `r3-15-transform-quality-contract` | test | `service::transform::quality::tests::test_transform_contract_summary_covers_failures_outcomes_and_accounting` | Pins eleven production-entry transform outcomes, failure origins, semantic accounting, diagnostic overflow, and Gate closure. |
| `r3-15-transform-payload-free-contract` | test | `service::transform::quality::tests::test_transform_contract_report_omits_payload_and_safe_summary` | Pins the quality report to payload-free aggregate facts without fixture content, hashes, or per-payload safe summaries. |
| `r3-15-same-wire-passthrough` | test | `proxy::direct_execution_regression::same_wire_non_stream_observation_failure_preserves_upstream_bytes` | Pins byte-preserving same-wire response passthrough when best-effort observation degrades. |
| `r3-15-minor-loss-runtime` | test | `proxy::direct_execution_regression::cross_wire_minor_loss_succeeds_once_and_drops_only_audited_metadata` | Pins cross-wire controlled minor loss, one upstream call, internal-only diagnostics, Request Log success, and resource release. |
| `r3-15-four-protocol-stream-failure` | test | `proxy::direct_execution_regression::four_public_protocols_emit_one_native_terminal_on_cross_wire_stream_decode_failure` | Pins one protocol-native terminal after headers for source transform failure across all four public downstream protocols. |
| `r3-15-target-stream-failure` | test | `proxy::direct_execution_regression::cross_wire_target_stream_rejection_emits_one_native_terminal_and_releases_resources` | Pins target transform rejection after headers to one native terminal, an error Request Log, one upstream call, and released resources. |

## Status semantics

- Base: `verified`, `partial`, `unavailable`, `not_verified`.
- Advanced: `full`, `controlled_loss`, `explicit_reject`, `not_verified`.
- `verified` requires direct execution through routing, materialization, transport, downstream output, and call-count assertions. Transform-only evidence is insufficient.
