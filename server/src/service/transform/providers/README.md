# Transform Provider Owner Checklist

This directory owns provider-specific payload DTOs, request codecs, response codecs,
stream codecs, provider metadata mappers, and provider tests. Keep cross-provider
orchestration in `adapter.rs`, `request.rs`, `response.rs`, `stream/*`, and
`quality/*`.

## New Provider Flow

1. Add `providers/<provider>/mod.rs` with `payload.rs`, `request.rs`,
   `response.rs`, `stream.rs`, and `tests.rs`. Add `metadata.rs`,
   `lifecycle.rs`, `sanitize.rs`, or bridge modules only when the provider has
   a real protocol-specific lifecycle or compatibility layer.
2. Define serde DTOs in `payload.rs`. Keep provider wire names stable with
   serde attributes; do not leak Unified IR field names into provider JSON.
3. Implement `<Provider>RequestPayload -> UnifiedRequest` and
   `UnifiedRequest -> <Provider>RequestPayload` in `request.rs`.
4. Implement `<Provider>Response -> UnifiedResponse` and
   `UnifiedResponse -> <Provider>Response` in `response.rs`, including usage,
   finish reason, provider metadata, refusal, files, citations, and reasoning
   where supported.
5. Implement stream source decode and target encode in `stream.rs`. Prefer
   native `UnifiedStreamEvent` encoders; use legacy chunk bridging only when the
   provider cannot express event-native lifecycle.
6. Declare the provider capability matrix in `capability.rs`, then update
   `policy.rs` only when a new semantic value needs a new downgrade or reject
   decision.
7. Register the provider exactly once in `adapter.rs`; OpenAI-compatible aliases
   may dispatch to the OpenAI adapter only when the wire contract is actually
   OpenAI-compatible.
8. Add focused provider tests in `providers/<provider>/tests.rs`, plus quality
   replay fixtures when the provider affects text, tool calls, reasoning, finish
   reason, usage, binary payloads, schema conformance, or stream lifecycle.
9. Run `rtk cargo fmt --all`,
   `rtk cargo test -p cyder-api transform::providers::<provider> -- --nocapture`,
   `rtk cargo test -p cyder-api transform::adapter::tests:: -- --nocapture`,
   `rtk cargo test -p cyder-api transform::capability::tests:: -- --nocapture`,
   and `rtk cargo test -p cyder-api transform:: -- --nocapture`.

## New Field Flow

Before adding a protocol field, answer these in the code review and tests:

- Does Unified IR need a new field, extension metadata, or passthrough key?
- Which providers support the field natively in request, response, and stream?
- How is support represented in `ProtocolCapabilityMatrix`?
- Does `PolicyEngine` send, drop with a diagnostic, or reject the field?
- Which fixture or provider test proves lossless preservation or expected
  downgrade behavior?
- Does the quality replay summary need a new semantic preservation counter?

## Diagnostics Boundary

Provider modules must not hand-write transform diagnostic JSON. Use only the
diagnostics owner entry points:

- `diagnostics::build_transform_diagnostic`
- `diagnostics::build_stream_diagnostic_sse`
- `diagnostics::build_fatal_stream_error_payload`

Provider-specific code may build provider metadata, lifecycle state, and native
payloads. Lossy conversion, capability downgrade, and fatal transform errors must
flow through the diagnostics owner so replay, request logs, and stream snapshots
keep a stable schema.
