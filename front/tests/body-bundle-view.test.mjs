import test from "node:test";
import assert from "node:assert/strict";

import { buildV2AttemptRows, decodeBundleView } from "../src/components/record/bodyBundleView.ts";

const encoder = new TextEncoder();
const bytes = (value) => encoder.encode(value);

test("request log bundle view rejects non-v2 bundle versions", () => {
  assert.throws(
    () =>
      decodeBundleView({
        version: 1,
        user_request_body: bytes('{"model":"route","input":"hello"}'),
      }),
    /unsupported_request_log_bundle_version:1/,
  );
});

test("v2 request log bundle reconstructs attempt request body from blob and patch pools", () => {
  const view = decodeBundleView({
    version: 2,
    request_section: {
      user_request_blob_id: 1,
      user_response_blob_id: 3,
      user_response_capture_state: "complete",
    },
    attempt_sections: [
      {
        attempt_id: 11,
        attempt_index: 1,
        llm_request_blob_id: 1,
        llm_request_patch_id: 1,
        llm_response_blob_id: 2,
        llm_response_capture_state: "complete",
      },
    ],
    blob_pool: [
      {
        blob_id: 1,
        body: bytes('{"model":"route","messages":[{"role":"user","content":"hello"}]}'),
      },
      {
        blob_id: 2,
        body: bytes('{"choices":[{"message":{"content":"ok"}}]}'),
      },
      {
        blob_id: 3,
        body: bytes('{"id":"chatcmpl","choices":[]}'),
      },
    ],
    patch_pool: [
      {
        patch_id: 1,
        patch_body: bytes('[{"op":"replace","path":"/model","value":"candidate-a"}]'),
      },
    ],
  });

  assert.equal(view.kind, "v2");
  assert.equal(view.request.userResponseCaptureState, "complete");
  assert.deepEqual(JSON.parse(view.attempts[0].requestContent), {
    model: "candidate-a",
    messages: [{ role: "user", content: "hello" }],
  });
  assert.deepEqual(JSON.parse(view.attempts[0].responseContent), {
    choices: [{ message: { content: "ok" } }],
  });
  assert.equal(view.attempts[0].requestRawPatchContent, '[{"op":"replace","path":"/model","value":"candidate-a"}]');
  assert.equal(view.requestSnapshot, null);
  assert.equal(view.candidateManifest, null);
  assert.equal(view.transformDiagnostics, null);
});

test("v2 request log bundle decodes diagnostic assets without affecting payload reconstruction", () => {
  const view = decodeBundleView({
    version: 2,
    request_section: {
      user_request_blob_id: 1,
    },
    request_snapshot: {
      request_path: "/ai/responses",
      operation_kind: "responses_create",
      query_params: [
        { name: "stream", value: "true" },
        { name: "verbose" },
      ],
      sanitized_original_headers: [
        { name: "x-trace-id", value: "trace-123" },
      ],
    },
    candidate_manifest: {
      items: [
        {
          candidate_position: 1,
          route_id: 8,
          route_name: "manual-smoke-route",
          provider_id: 2,
          provider_key: "provider-a",
          model_id: 3,
          model_name: "gpt-test",
          real_model_name: "real-gpt-test",
          llm_api_type: "RESPONSES",
          provider_api_key_mode: "QUEUE",
        },
      ],
    },
    transform_diagnostics: {
      summary: {
        count: 1,
        max_loss_level: "lossy_major",
        kinds: ["capability_downgrade"],
        phases: ["request"],
      },
      items: [
        {
          phase: "request",
          diagnostic: {
            type: "transform_diagnostic",
            diagnostic_kind: "capability_downgrade",
            loss_level: "lossy_major",
            reason: "reasoning summary was dropped",
          },
        },
      ],
    },
    attempt_sections: [],
    blob_pool: [
      {
        blob_id: 1,
        body: bytes('{"model":"route","input":"hello"}'),
      },
    ],
    patch_pool: [],
  });

  assert.equal(view.kind, "v2");
  assert.equal(view.request.userRequestBody, '{"model":"route","input":"hello"}');
  assert.deepEqual(view.requestSnapshot, {
    requestPath: "/ai/responses",
    operationKind: "responses_create",
    queryParams: [
      { name: "stream", value: "true" },
      { name: "verbose", value: null },
    ],
    sanitizedOriginalHeaders: [
      { name: "x-trace-id", value: "trace-123" },
    ],
  });
  assert.deepEqual(view.candidateManifest, {
    items: [
      {
        candidatePosition: 1,
        routeId: 8,
        routeName: "manual-smoke-route",
        providerId: 2,
        providerKey: "provider-a",
        modelId: 3,
        modelName: "gpt-test",
        realModelName: "real-gpt-test",
        llmApiType: "RESPONSES",
        providerApiKeyMode: "QUEUE",
      },
    ],
  });
  assert.deepEqual(view.transformDiagnostics, {
    summary: {
      count: 1,
      maxLossLevel: "lossy_major",
      kinds: ["capability_downgrade"],
      phases: ["request"],
    },
    items: [
      {
        phase: "request",
        diagnostic: {
          type: "transform_diagnostic",
          diagnostic_kind: "capability_downgrade",
          loss_level: "lossy_major",
          reason: "reasoning summary was dropped",
        },
      },
    ],
  });
});

test("v2 attempt rows merge persisted metadata and include metadata-only attempts", () => {
  const payloadAttempts = [
    {
      key: "11-1",
      attemptId: 11,
      attemptIndex: 1,
      requestContent: '{"model":"candidate-a"}',
      requestBaseContent: '{"model":"route"}',
      requestRawPatchContent: '[{"op":"replace","path":"/model","value":"candidate-a"}]',
      requestPatchError: null,
      requestBlobId: 1,
      requestPatchId: 1,
      responseContent: '{"error":"rate limited"}',
      responseBlobId: 2,
      responseCaptureState: "complete",
    },
  ];
  const metadataAttempts = [
    {
      id: 11,
      attempt_index: 1,
      attempt_status: "ERROR",
      scheduler_action: "FALLBACK_NEXT_CANDIDATE",
      http_status: 429,
      provider_name_snapshot: "OpenAI",
      model_name_snapshot: "candidate-a",
      real_model_name_snapshot: null,
    },
    {
      id: 12,
      attempt_index: 2,
      attempt_status: "SUCCESS",
      scheduler_action: "RETURN_SUCCESS",
      http_status: 200,
      provider_name_snapshot: "Anthropic",
      model_name_snapshot: "candidate-b",
      real_model_name_snapshot: "claude-real",
      llm_request_blob_id: 3,
      llm_request_patch_id: null,
      llm_response_blob_id: 4,
      llm_response_capture_state: "complete",
    },
  ];

  const rows = buildV2AttemptRows(payloadAttempts, metadataAttempts);

  assert.equal(rows.length, 2);
  assert.equal(rows[0].status, "ERROR");
  assert.equal(rows[0].schedulerAction, "FALLBACK_NEXT_CANDIDATE");
  assert.equal(rows[0].providerModelDisplay, "OpenAI / candidate-a");
  assert.equal(rows[1].status, "SUCCESS");
  assert.equal(rows[1].requestContent, null);
  assert.equal(rows[1].responseBlobId, 4);
  assert.equal(rows[1].providerModelDisplay, "Anthropic / candidate-b -> claude-real");
});
