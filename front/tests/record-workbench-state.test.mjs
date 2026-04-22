import test from "node:test";
import assert from "node:assert/strict";

import {
  RECORD_DETAIL_TABS,
  shouldLoadRecordArtifacts,
  shouldRenderPayloadViewer,
} from "../src/components/record/recordDetailState.ts";
import {
  buildAttemptReplayPayload,
  canExecuteReplay,
  canSaveReplayDryRun,
  canPreviewReplay,
  getReplayCapability,
  hasReplayArtifactBundle,
  initialReplayAttemptId,
  isProviderApiKeyOverrideInvalid,
  normalizeProviderApiKeyOverrideForKind,
  replayableAttempts,
  resolveReplayArtifactViewState,
  shouldShowProviderApiKeyOverrideError,
} from "../src/components/record/recordReplayState.ts";

const artifactResponse = {
  payload_manifest: {
    bundle_version: 2,
  },
  replay_capability: {
    attempt_upstream: {
      available: true,
      reasons: [],
      attempt_ids: [101, 103],
    },
    gateway_request: {
      available: true,
      reasons: [],
      attempt_ids: [],
    },
  },
};

const unavailableGatewayArtifactResponse = {
  payload_manifest: {
    bundle_version: 2,
  },
  replay_capability: {
    attempt_upstream: {
      available: true,
      reasons: [],
      attempt_ids: [101, 103],
    },
    gateway_request: {
      available: false,
      reasons: ["request_snapshot_missing"],
      attempt_ids: [],
    },
  },
};

const noBundleArtifactResponse = {
  payload_manifest: {
    bundle_version: null,
  },
  replay_capability: {
    attempt_upstream: {
      available: false,
      reasons: ["bundle_missing"],
      attempt_ids: [],
    },
    gateway_request: {
      available: false,
      reasons: ["request_snapshot_missing"],
      attempt_ids: [],
    },
  },
};

const attempts = [
  { id: 101, attempt_index: 1, attempt_status: "ERROR" },
  { id: 102, attempt_index: 2, attempt_status: "SKIPPED" },
  { id: 103, attempt_index: 3, attempt_status: "SUCCESS" },
];

test("record detail workbench exposes fixed tabs and lazy-load boundaries", () => {
  assert.deepEqual(
    RECORD_DETAIL_TABS.map((tab) => tab.value),
    ["overview", "attempts", "diagnostics", "payloads", "replay"],
  );

  assert.equal(shouldLoadRecordArtifacts("overview"), false);
  assert.equal(shouldLoadRecordArtifacts("attempts"), false);
  assert.equal(shouldLoadRecordArtifacts("payloads"), false);
  assert.equal(shouldLoadRecordArtifacts("diagnostics"), true);
  assert.equal(shouldLoadRecordArtifacts("replay"), true);

  assert.equal(shouldRenderPayloadViewer("overview", "FILE_SYSTEM"), false);
  assert.equal(shouldRenderPayloadViewer("payloads", null), false);
  assert.equal(shouldRenderPayloadViewer("payloads", "FILE_SYSTEM"), true);
});

test("record replay state uses server capability and selected attempt for preview", () => {
  assert.deepEqual(getReplayCapability(null, "attempt_upstream"), {
    available: false,
    reasons: ["artifact_not_loaded"],
    attempt_ids: [],
  });
  assert.equal(
    getReplayCapability(unavailableGatewayArtifactResponse, "gateway_request").reasons[0],
    "request_snapshot_missing",
  );
  assert.equal(initialReplayAttemptId(artifactResponse), 101);
  assert.deepEqual(
    replayableAttempts(attempts, artifactResponse).map((attempt) => attempt.id),
    [101, 103],
  );

  assert.equal(
    canPreviewReplay({
      kind: "attempt_upstream",
      artifacts: artifactResponse,
      selectedAttemptId: null,
      providerApiKeyOverride: "",
    }),
    false,
  );
  assert.equal(
    canPreviewReplay({
      kind: "attempt_upstream",
      artifacts: artifactResponse,
      selectedAttemptId: 101,
      providerApiKeyOverride: "abc",
    }),
    false,
  );
  assert.equal(
    canPreviewReplay({
      kind: "attempt_upstream",
      artifacts: artifactResponse,
      selectedAttemptId: 101,
      providerApiKeyOverride: "",
    }),
    true,
  );
  assert.equal(
    canPreviewReplay({
      kind: "gateway_request",
      artifacts: artifactResponse,
      selectedAttemptId: null,
      providerApiKeyOverride: "",
    }),
    true,
  );
});

test("record replay execute requires preview and explicit live confirmation", () => {
  assert.equal(isProviderApiKeyOverrideInvalid(""), false);
  assert.equal(isProviderApiKeyOverrideInvalid("42"), false);
  assert.equal(isProviderApiKeyOverrideInvalid("0"), true);
  assert.equal(isProviderApiKeyOverrideInvalid("not-a-number"), true);
  assert.deepEqual(buildAttemptReplayPayload(""), {});
  assert.deepEqual(buildAttemptReplayPayload("42"), {
    provider_api_key_id_override: 42,
  });

  assert.equal(
    canExecuteReplay({
      hasPreview: false,
      previewFingerprint: "request-replay-preview-v1:1:abc",
      canPreview: true,
      confirmLiveRequest: true,
    }),
    false,
  );
  assert.equal(
    canSaveReplayDryRun({
      hasPreview: true,
      previewFingerprint: "request-replay-preview-v1:1:abc",
      canPreview: true,
    }),
    true,
  );
  assert.equal(
    canSaveReplayDryRun({
      hasPreview: true,
      previewFingerprint: "",
      canPreview: true,
    }),
    false,
  );
  assert.equal(
    canExecuteReplay({
      hasPreview: true,
      previewFingerprint: "request-replay-preview-v1:1:abc",
      canPreview: true,
      confirmLiveRequest: false,
    }),
    false,
  );
  assert.equal(
    canExecuteReplay({
      hasPreview: true,
      previewFingerprint: "",
      canPreview: true,
      confirmLiveRequest: true,
    }),
    false,
  );
  assert.equal(
    canExecuteReplay({
      hasPreview: true,
      previewFingerprint: "request-replay-preview-v1:1:abc",
      canPreview: true,
      confirmLiveRequest: true,
    }),
    true,
  );
});

test("gateway replay ignores hidden attempt override input and clears invalid hint", () => {
  assert.equal(
    canPreviewReplay({
      kind: "attempt_upstream",
      artifacts: artifactResponse,
      selectedAttemptId: 101,
      providerApiKeyOverride: "abc",
    }),
    false,
  );
  assert.equal(
    shouldShowProviderApiKeyOverrideError("attempt_upstream", "abc"),
    true,
  );
  assert.equal(
    canPreviewReplay({
      kind: "gateway_request",
      artifacts: artifactResponse,
      selectedAttemptId: null,
      providerApiKeyOverride: "abc",
    }),
    true,
  );
  assert.equal(
    shouldShowProviderApiKeyOverrideError("gateway_request", "abc"),
    false,
  );
  assert.equal(
    normalizeProviderApiKeyOverrideForKind("gateway_request", "abc"),
    "",
  );
  assert.equal(
    normalizeProviderApiKeyOverrideForKind("attempt_upstream", "abc"),
    "abc",
  );
});

test("replay artifact view state distinguishes lazy loading, failure, no bundle, and unavailable", () => {
  assert.equal(hasReplayArtifactBundle(artifactResponse), true);
  assert.equal(
    resolveReplayArtifactViewState({
      kind: "attempt_upstream",
      artifacts: null,
      loading: false,
      error: null,
    }),
    "lazy",
  );
  assert.equal(
    resolveReplayArtifactViewState({
      kind: "attempt_upstream",
      artifacts: null,
      loading: true,
      error: null,
    }),
    "loading",
  );
  assert.equal(
    resolveReplayArtifactViewState({
      kind: "attempt_upstream",
      artifacts: null,
      loading: false,
      error: "boom",
    }),
    "error",
  );
  assert.equal(
    resolveReplayArtifactViewState({
      kind: "attempt_upstream",
      artifacts: noBundleArtifactResponse,
      loading: false,
      error: null,
    }),
    "no_bundle",
  );
  assert.equal(
    resolveReplayArtifactViewState({
      kind: "gateway_request",
      artifacts: {
        ...artifactResponse,
        replay_capability: {
          ...artifactResponse.replay_capability,
          gateway_request: {
            available: false,
            reasons: ["user_request_body_missing"],
            attempt_ids: [],
          },
        },
      },
      loading: false,
      error: null,
    }),
    "unavailable",
  );
  assert.equal(
    resolveReplayArtifactViewState({
      kind: "attempt_upstream",
      artifacts: {
        ...artifactResponse,
      },
      loading: false,
      error: null,
    }),
    "ready",
  );
});
