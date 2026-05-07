import { computed, ref, watch } from "vue";
import type {
  RecordArtifactResponse,
  RecordAttempt,
  RecordReplayArtifact,
  RecordReplayKind,
  RecordReplayMode,
  RecordReplayPreviewResponse,
  RecordReplayRun,
  RecordAttemptReplayPreviewParams,
  RecordAttemptReplayPreviewResponse,
  RecordAttemptReplayExecuteParams,
  RecordGatewayReplayPreviewParams,
  RecordGatewayReplayPreviewResponse,
  RecordGatewayReplayExecuteParams,
} from "../../../services/types";

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

export type RecordReplayTranslator = (
  key: string,
  params?: Record<string, unknown>,
) => string;

export type RecordReplayI18n = {
  t: RecordReplayTranslator;
  te?: (key: string) => boolean;
};

export interface UseRecordReplayOptions {
  recordId: { value: number };
  attempts: { value: RecordAttempt[] };
  artifacts: { value: RecordArtifactResponse | null };
  loading: { value: boolean };
  error: { value: string | null };
  selectedAttemptId: { value: number | null };
  selectedReplayRunId: { value: number | null };
  i18n: RecordReplayI18n;
  api: {
    getRecordReplayRuns: (id: number | string) => Promise<RecordReplayRun[]>;
    previewAttemptReplay: (
      id: number | string,
      attemptId: number | string,
      payload?: RecordAttemptReplayPreviewParams,
    ) => Promise<RecordAttemptReplayPreviewResponse>;
    previewGatewayReplay: (
      id: number | string,
      payload?: RecordGatewayReplayPreviewParams,
    ) => Promise<RecordGatewayReplayPreviewResponse>;
    executeAttemptReplay: (
      id: number | string,
      attemptId: number | string,
      payload: RecordAttemptReplayExecuteParams,
    ) => Promise<RecordReplayRun>;
    executeGatewayReplay: (
      id: number | string,
      payload: RecordGatewayReplayExecuteParams,
    ) => Promise<RecordReplayRun>;
    getRecordReplayRun: (
      id: number | string,
      replayRunId: number | string,
    ) => Promise<RecordReplayRun>;
    getRecordReplayArtifacts: (
      id: number | string,
      replayRunId: number | string,
    ) => Promise<RecordReplayArtifact>;
  };
  emitSelectedAttemptId?: (value: number | null) => void;
  emitSelectedReplayRunId?: (value: number | null) => void;
}

const normalizeReplayErrorMessage = (err: unknown) => {
  if (err instanceof Error) return err.message;
  if (typeof err === "object" && err !== null && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
};

export function useRecordReplay(options: UseRecordReplayOptions) {
  const api = options.api;
  const { t, te } = options.i18n;
  const selectedKind = ref<RecordReplayKind>("attempt_upstream");
  const selectedAttemptValue = ref(
    options.selectedAttemptId.value != null
      ? String(options.selectedAttemptId.value)
      : "",
  );
  const providerApiKeyOverride = ref("");
  const confirmLiveRequest = ref(false);
  const previewLoading = ref(false);
  const executeLoading = ref(false);
  const executeMode = ref<RecordReplayMode | null>(null);
  const actionError = ref<string | null>(null);
  const preview = ref<RecordReplayPreviewResponse | null>(null);
  const replayRuns = ref<RecordReplayRun[]>([]);
  const historyLoading = ref(false);
  const historySelectionLoading = ref(false);
  const historyError = ref<string | null>(null);
  const run = ref<RecordReplayRun | null>(null);
  const artifact = ref<RecordReplayArtifact | null>(null);

  const selectedCapability = computed(() =>
    getReplayCapability(options.artifacts.value, selectedKind.value),
  );

  const formatReplayUnavailableReason = (reason: string) => {
    const key = `recordPage.detailDialog.replay.reasons.${reason}`;
    return te?.(key) ? t(key) : reason;
  };

  const selectedUnavailableReasons = computed(() =>
    selectedCapability.value.reasons.map(formatReplayUnavailableReason),
  );

  const artifactLoadError = computed(() => options.error.value ?? "");

  const artifactViewState = computed(() =>
    resolveReplayArtifactViewState({
      kind: selectedKind.value,
      artifacts: options.artifacts.value,
      loading: options.loading.value,
      error: options.error.value,
    }),
  );

  const hasReadyArtifacts = computed(
    () =>
      options.artifacts.value != null &&
      hasReplayArtifactBundle(options.artifacts.value),
  );

  const replayableAttemptOptions = computed(() =>
    replayableAttempts(options.attempts.value, options.artifacts.value),
  );

  const selectedAttemptId = computed(() => {
    const value = Number(selectedAttemptValue.value);
    return Number.isInteger(value) && value > 0 ? value : null;
  });

  const overrideInvalid = computed(() =>
    shouldShowProviderApiKeyOverrideError(
      selectedKind.value,
      providerApiKeyOverride.value,
    ),
  );

  const canPreview = computed(() => {
    if (!hasReadyArtifacts.value) return false;
    return canPreviewReplay({
      kind: selectedKind.value,
      artifacts: options.artifacts.value,
      selectedAttemptId: selectedAttemptId.value,
      providerApiKeyOverride: providerApiKeyOverride.value,
    });
  });

  const canExecute = computed(() =>
    canExecuteReplay({
      hasPreview: Boolean(preview.value),
      previewFingerprint: preview.value?.preview_fingerprint,
      canPreview: canPreview.value,
      confirmLiveRequest: confirmLiveRequest.value,
    }),
  );

  const canSaveDryRun = computed(() =>
    canSaveReplayDryRun({
      hasPreview: Boolean(preview.value),
      previewFingerprint: preview.value?.preview_fingerprint,
      canPreview: canPreview.value,
    }),
  );

  const resetReplayResult = () => {
    preview.value = null;
    run.value = null;
    artifact.value = null;
    actionError.value = null;
    confirmLiveRequest.value = false;
    options.emitSelectedReplayRunId?.(null);
  };

  const setSelectedAttemptValue = (value: string) => {
    selectedAttemptValue.value = value;
    const parsed = Number(value);
    options.emitSelectedAttemptId?.(
      Number.isInteger(parsed) && parsed > 0 ? parsed : null,
    );
  };

  const setSelectedReplayRun = (value: number | null) => {
    options.emitSelectedReplayRunId?.(value);
  };

  watch(
    () => options.artifacts.value,
    (artifacts) => {
      if (!artifacts) return;
      const requestedAttemptId = options.selectedAttemptId.value;
      const attemptIds = new Set(
        artifacts.replay_capability.attempt_upstream.attempt_ids ?? [],
      );
      const firstAttemptId =
        requestedAttemptId != null && attemptIds.has(requestedAttemptId)
          ? requestedAttemptId
          : initialReplayAttemptId(artifacts);
      selectedAttemptValue.value =
        firstAttemptId != null ? String(firstAttemptId) : "";
      options.emitSelectedAttemptId?.(firstAttemptId ?? null);
    },
    { immediate: true },
  );

  watch(
    () => options.selectedAttemptId.value,
    (attemptId) => {
      const nextValue = attemptId != null ? String(attemptId) : "";
      if (selectedAttemptValue.value !== nextValue) {
        selectedAttemptValue.value = nextValue;
      }
    },
  );

  watch(selectedKind, (kind) => {
    providerApiKeyOverride.value = normalizeProviderApiKeyOverrideForKind(
      kind,
      providerApiKeyOverride.value,
    );
    resetReplayResult();
  });

  watch(selectedAttemptValue, resetReplayResult);
  watch(providerApiKeyOverride, resetReplayResult);

  const loadReplayHistory = async (force = false) => {
    if (historyLoading.value) return;
    if (replayRuns.value.length > 0 && !force) return;
    historyLoading.value = true;
    historyError.value = null;
    try {
      replayRuns.value = await api.getRecordReplayRuns(options.recordId.value);
    } catch (err) {
      historyError.value = normalizeReplayErrorMessage(err);
    } finally {
      historyLoading.value = false;
    }
  };

  watch(
    () => options.recordId.value,
    () => {
      replayRuns.value = [];
      historyError.value = null;
      preview.value = null;
      run.value = null;
      artifact.value = null;
      actionError.value = null;
      confirmLiveRequest.value = false;
      void loadReplayHistory(true);
    },
    { immediate: true },
  );

  const previewPayload = () => buildAttemptReplayPayload(providerApiKeyOverride.value);

  const handlePreview = async () => {
    if (!canPreview.value) return;
    previewLoading.value = true;
    actionError.value = null;
    run.value = null;
    artifact.value = null;

    try {
      if (selectedKind.value === "attempt_upstream") {
        const attemptId = selectedAttemptId.value;
        if (attemptId == null) return;
        preview.value = await api.previewAttemptReplay(
          options.recordId.value,
          attemptId,
          previewPayload(),
        );
      } else {
        preview.value = await api.previewGatewayReplay(options.recordId.value, {});
      }
    } catch (err) {
      actionError.value = normalizeReplayErrorMessage(err);
    } finally {
      previewLoading.value = false;
    }
  };

  const executeAttemptReplayRun = async (mode: RecordReplayMode) => {
    const attemptId = selectedAttemptId.value;
    if (attemptId == null) {
      throw new Error(t("recordPage.detailDialog.replay.noAttemptSelected"));
    }
    return api.executeAttemptReplay(options.recordId.value, attemptId, {
      ...previewPayload(),
      replay_mode: mode,
      confirm_live_request: mode === "live",
      preview_fingerprint: preview.value?.preview_fingerprint ?? "",
    });
  };

  const handleExecute = async (mode: RecordReplayMode) => {
    if (mode === "live" && !canExecute.value) return;
    if (mode === "dry_run" && !canSaveDryRun.value) return;
    executeLoading.value = true;
    executeMode.value = mode;
    actionError.value = null;

    try {
      const executedRun =
        selectedKind.value === "attempt_upstream"
          ? await executeAttemptReplayRun(mode)
          : await api.executeGatewayReplay(options.recordId.value, {
              replay_mode: mode,
              confirm_live_request: mode === "live",
              preview_fingerprint: preview.value?.preview_fingerprint ?? "",
            });
      run.value = await api.getRecordReplayRun(options.recordId.value, executedRun.id);
      artifact.value = await api.getRecordReplayArtifacts(
        options.recordId.value,
        executedRun.id,
      );
      setSelectedReplayRun(executedRun.id);
      await loadReplayHistory(true);
    } catch (err) {
      actionError.value = normalizeReplayErrorMessage(err);
    } finally {
      executeLoading.value = false;
      executeMode.value = null;
    }
  };

  const openReplayRun = async (replayRunId: number) => {
    if (historySelectionLoading.value) return;
    historySelectionLoading.value = true;
    setSelectedReplayRun(replayRunId);
    actionError.value = null;
    run.value = null;
    artifact.value = null;
    try {
      const [loadedRun, loadedArtifact] = await Promise.all([
        api.getRecordReplayRun(options.recordId.value, replayRunId),
        api.getRecordReplayArtifacts(options.recordId.value, replayRunId),
      ]);
      run.value = loadedRun;
      artifact.value = loadedArtifact;
    } catch (err) {
      actionError.value = normalizeReplayErrorMessage(err);
    } finally {
      historySelectionLoading.value = false;
    }
  };

  watch(
    () => options.selectedReplayRunId.value,
    (replayRunId) => {
      if (replayRunId != null) {
        void openReplayRun(replayRunId);
      } else {
        run.value = null;
        artifact.value = null;
      }
    },
    { immediate: true },
  );

  return {
    selectedKind,
    selectedAttemptValue,
    providerApiKeyOverride,
    confirmLiveRequest,
    previewLoading,
    executeLoading,
    executeMode,
    actionError,
    preview,
    replayRuns,
    historyLoading,
    historySelectionLoading,
    historyError,
    run,
    artifact,
    selectedCapability,
    selectedUnavailableReasons,
    artifactLoadError,
    artifactViewState,
    replayableAttempts: replayableAttemptOptions,
    selectedAttemptId,
    overrideInvalid,
    canPreview,
    canExecute,
    canSaveDryRun,
    setSelectedAttemptValue,
    loadReplayHistory,
    handlePreview,
    handleExecute,
    openReplayRun,
  };
}
