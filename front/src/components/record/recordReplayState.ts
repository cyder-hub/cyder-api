import type {
  RecordArtifactResponse,
  RecordAttempt,
  RecordReplayKind,
} from "@/store/types";

export type ReplayArtifactViewState =
  | "lazy"
  | "loading"
  | "error"
  | "no_bundle"
  | "unavailable"
  | "ready";

export const kindUsesProviderApiKeyOverride = (kind: RecordReplayKind) =>
  kind === "attempt_upstream";

export const getReplayCapability = (
  artifacts: RecordArtifactResponse | null,
  kind: RecordReplayKind,
) => {
  const fallback = {
    available: false,
    reasons: ["artifact_not_loaded"],
    attempt_ids: [],
  };
  return artifacts?.replay_capability[kind] ?? fallback;
};

export const hasReplayArtifactBundle = (
  artifacts: RecordArtifactResponse | null,
) => Boolean(artifacts?.payload_manifest.bundle_version);

export const replayableAttempts = (
  attempts: RecordAttempt[],
  artifacts: RecordArtifactResponse | null,
) => {
  const ids = new Set(
    artifacts?.replay_capability.attempt_upstream.attempt_ids ?? [],
  );
  return attempts.filter((attempt) => ids.has(attempt.id));
};

export const initialReplayAttemptId = (
  artifacts: RecordArtifactResponse | null,
) => artifacts?.replay_capability.attempt_upstream.attempt_ids[0] ?? null;

export const parseProviderApiKeyOverride = (raw: string) => {
  const trimmed = raw.trim();
  if (!trimmed) return null;
  const parsed = Number(trimmed);
  return Number.isInteger(parsed) && parsed > 0 ? parsed : Number.NaN;
};

export const isProviderApiKeyOverrideInvalid = (raw: string) =>
  Number.isNaN(parseProviderApiKeyOverride(raw));

export const shouldShowProviderApiKeyOverrideError = (
  kind: RecordReplayKind,
  raw: string,
) => kindUsesProviderApiKeyOverride(kind) && isProviderApiKeyOverrideInvalid(raw);

export const normalizeProviderApiKeyOverrideForKind = (
  kind: RecordReplayKind,
  raw: string,
) => (kindUsesProviderApiKeyOverride(kind) ? raw : "");

export const buildAttemptReplayPayload = (providerApiKeyOverride: string) => {
  const provider_api_key_id_override = parseProviderApiKeyOverride(
    providerApiKeyOverride,
  );
  return provider_api_key_id_override == null ||
    Number.isNaN(provider_api_key_id_override)
    ? {}
    : { provider_api_key_id_override };
};

export const canPreviewReplay = (input: {
  kind: RecordReplayKind;
  artifacts: RecordArtifactResponse | null;
  selectedAttemptId: number | null;
  providerApiKeyOverride: string;
}) => {
  const capability = getReplayCapability(input.artifacts, input.kind);
  if (!capability.available) return false;
  if (
    kindUsesProviderApiKeyOverride(input.kind) &&
    isProviderApiKeyOverrideInvalid(input.providerApiKeyOverride)
  ) {
    return false;
  }
  if (input.kind === "attempt_upstream") {
    return input.selectedAttemptId != null;
  }
  return true;
};

export const canExecuteReplay = (input: {
  hasPreview: boolean;
  previewFingerprint?: string | null;
  canPreview: boolean;
  confirmLiveRequest: boolean;
}) =>
  input.hasPreview &&
  Boolean(input.previewFingerprint?.trim()) &&
  input.canPreview &&
  input.confirmLiveRequest;

export const canSaveReplayDryRun = (input: {
  hasPreview: boolean;
  previewFingerprint?: string | null;
  canPreview: boolean;
}) =>
  input.hasPreview &&
  Boolean(input.previewFingerprint?.trim()) &&
  input.canPreview;

export const resolveReplayArtifactViewState = (input: {
  kind: RecordReplayKind;
  artifacts: RecordArtifactResponse | null;
  loading: boolean;
  error: string | null;
}): ReplayArtifactViewState => {
  if (input.loading) return "loading";
  if (input.error) return "error";
  if (!input.artifacts) return "lazy";
  if (!hasReplayArtifactBundle(input.artifacts)) return "no_bundle";
  if (!getReplayCapability(input.artifacts, input.kind).available) {
    return "unavailable";
  }
  return "ready";
};
