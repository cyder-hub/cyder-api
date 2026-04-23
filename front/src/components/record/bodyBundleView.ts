import jsonPatch from "fast-json-patch";
import type { RecordAttempt } from "../../store/types";

const { applyPatch } = jsonPatch;
const UNSUPPORTED_BUNDLE_VERSION_PREFIX = "unsupported_request_log_bundle_version:";

export type V2RequestBodies = {
  userRequestBody: string | null;
  userRequestBlobId: number | null;
  userResponseBody: string | null;
  userResponseBlobId: number | null;
  userResponseCaptureState: string | null;
};

export type V2AttemptBodies = {
  key: string;
  attemptId: number | null;
  attemptIndex: number;
  requestContent: string | null;
  requestBaseContent: string | null;
  requestRawPatchContent: string | null;
  requestPatchError: string | null;
  requestBlobId: number | null;
  requestPatchId: number | null;
  responseContent: string | null;
  responseBlobId: number | null;
  responseCaptureState: string | null;
};

export type V2AttemptRow = V2AttemptBodies & {
  status: string | null;
  schedulerAction: string | null;
  httpStatus: number | null;
  providerModelDisplay: string;
};

export type BundleNameValue = {
  name: string | null;
  value: string | null;
  valuePresent?: boolean | null;
};

export type V2RequestSnapshot = {
  requestPath: string | null;
  operationKind: string | null;
  queryParams: BundleNameValue[];
  sanitizedOriginalHeaders: BundleNameValue[];
};

export type V2CandidateManifestItem = {
  candidatePosition: number | null;
  routeId: number | null;
  routeName: string | null;
  providerId: number | null;
  providerKey: string | null;
  modelId: number | null;
  modelName: string | null;
  realModelName: string | null;
  llmApiType: string | null;
  providerApiKeyMode: string | null;
};

export type V2CandidateManifest = {
  items: V2CandidateManifestItem[];
};

export type V2TransformDiagnosticSummary = {
  count: number;
  maxLossLevel: string | null;
  kinds: string[];
  phases: string[];
};

export type V2TransformDiagnosticItem = {
  phase: string | null;
  diagnostic: Record<string, unknown> | null;
};

export type V2TransformDiagnostics = {
  summary: V2TransformDiagnosticSummary;
  items: V2TransformDiagnosticItem[];
};

export type V2Bodies = {
  kind: "v2";
  version: 2;
  request: V2RequestBodies;
  attempts: V2AttemptBodies[];
  requestSnapshot: V2RequestSnapshot | null;
  candidateManifest: V2CandidateManifest | null;
  transformDiagnostics: V2TransformDiagnostics | null;
};

export type BundleView = V2Bodies;

const decodeBytes = (value: unknown, textDecoder: TextDecoder) => {
  if (value == null) return null;
  if (value instanceof Uint8Array) return textDecoder.decode(value);
  if (Array.isArray(value)) return textDecoder.decode(new Uint8Array(value));
  return null;
};

const asNumber = (value: unknown) =>
  typeof value === "number" && Number.isFinite(value) ? value : null;

const asString = (value: unknown) =>
  typeof value === "string" && value.length > 0 ? value : null;

const asRecord = (value: unknown): Record<string, unknown> | null =>
  value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;

const decodeNameValues = (value: unknown): BundleNameValue[] =>
  (Array.isArray(value) ? value : [])
    .map((entry) => asRecord(entry))
    .filter((entry): entry is Record<string, unknown> => entry != null)
    .map((entry) => {
      const item: BundleNameValue = {
        name: asString(entry.name),
        value:
          entry.value == null
            ? null
            : typeof entry.value === "string"
              ? entry.value
              : String(entry.value),
      };
      if (typeof entry.value_present === "boolean") {
        item.valuePresent = entry.value_present;
      }
      return item;
    });

export const buildPatchedRequestContent = (
  baseContent: string | null,
  rawPatchContent: string | null,
) => {
  if (!rawPatchContent) {
    return {
      content: baseContent,
      patchError: null,
    };
  }

  if (!baseContent) {
    return {
      content: rawPatchContent,
      patchError: "Unable to apply JSON patch because the target blob is missing.",
    };
  }

  try {
    const baseJson = JSON.parse(baseContent);
    const patch = JSON.parse(rawPatchContent);
    if (!Array.isArray(patch)) {
      throw new Error("patch body is not a JSON array");
    }
    const { newDocument } = applyPatch(baseJson, patch, true, false);
    return {
      content: JSON.stringify(newDocument, null, 2),
      patchError: null,
    };
  } catch (error) {
    return {
      content: rawPatchContent,
      patchError: `Unable to apply JSON patch: ${
        error instanceof Error ? error.message : String(error)
      }`,
    };
  }
};

export const decodeV2Bundle = (
  decoded: Record<string, unknown>,
  textDecoder: TextDecoder,
): V2Bodies => {
  const blobsById = new Map<number, Record<string, unknown>>();
  for (const blob of Array.isArray(decoded.blob_pool) ? decoded.blob_pool : []) {
    if (blob && typeof blob === "object") {
      const blobId = asNumber((blob as Record<string, unknown>).blob_id);
      if (blobId != null) {
        blobsById.set(blobId, blob as Record<string, unknown>);
      }
    }
  }

  const patchesById = new Map<number, Record<string, unknown>>();
  for (const patch of Array.isArray(decoded.patch_pool) ? decoded.patch_pool : []) {
    if (patch && typeof patch === "object") {
      const patchId = asNumber((patch as Record<string, unknown>).patch_id);
      if (patchId != null) {
        patchesById.set(patchId, patch as Record<string, unknown>);
      }
    }
  }

  const blobText = (blobId: number | null) =>
    blobId == null ? null : decodeBytes(blobsById.get(blobId)?.body, textDecoder);

  const patchText = (patchId: number | null) =>
    patchId == null
      ? null
      : decodeBytes(patchesById.get(patchId)?.patch_body, textDecoder);

  const requestSection =
    asRecord(decoded.request_section) ?? {};
  const userRequestBlobId = asNumber(requestSection.user_request_blob_id);
  const userResponseBlobId = asNumber(requestSection.user_response_blob_id);
  const requestSnapshot = asRecord(decoded.request_snapshot);
  const candidateManifest = asRecord(decoded.candidate_manifest);
  const transformDiagnostics = asRecord(decoded.transform_diagnostics);
  const transformDiagnosticsSummary = asRecord(transformDiagnostics?.summary);
  const transformDiagnosticKinds = Array.isArray(transformDiagnosticsSummary?.kinds)
    ? transformDiagnosticsSummary.kinds
    : [];
  const transformDiagnosticPhases = Array.isArray(transformDiagnosticsSummary?.phases)
    ? transformDiagnosticsSummary.phases
    : [];

  const attempts = (
    Array.isArray(decoded.attempt_sections) ? decoded.attempt_sections : []
  )
    .filter((item): item is Record<string, unknown> => Boolean(item && typeof item === "object"))
    .map((section, index): V2AttemptBodies => {
      const attemptId = asNumber(section.attempt_id);
      const attemptIndex = asNumber(section.attempt_index) ?? index + 1;
      const requestBlobId = asNumber(section.llm_request_blob_id);
      const requestPatchId = asNumber(section.llm_request_patch_id);
      const responseBlobId = asNumber(section.llm_response_blob_id);
      const requestBaseContent = blobText(requestBlobId);
      const requestRawPatchContent = patchText(requestPatchId);
      const patched = buildPatchedRequestContent(
        requestBaseContent,
        requestRawPatchContent,
      );

      return {
        key: `${attemptId ?? "attempt"}-${attemptIndex}`,
        attemptId,
        attemptIndex,
        requestContent: patched.content,
        requestBaseContent,
        requestRawPatchContent,
        requestPatchError: patched.patchError,
        requestBlobId,
        requestPatchId,
        responseContent: blobText(responseBlobId),
        responseBlobId,
        responseCaptureState: asString(section.llm_response_capture_state),
      };
    });

  return {
    kind: "v2",
    version: 2,
    request: {
      userRequestBody: blobText(userRequestBlobId),
      userRequestBlobId,
      userResponseBody: blobText(userResponseBlobId),
      userResponseBlobId,
      userResponseCaptureState: asString(requestSection.user_response_capture_state),
    },
    attempts,
    requestSnapshot: requestSnapshot
      ? {
          requestPath: asString(requestSnapshot.request_path),
          operationKind: asString(requestSnapshot.operation_kind),
          queryParams: decodeNameValues(requestSnapshot.query_params),
          sanitizedOriginalHeaders: decodeNameValues(
            requestSnapshot.sanitized_original_headers,
          ),
        }
      : null,
    candidateManifest: candidateManifest
      ? {
          items: (Array.isArray(candidateManifest.items) ? candidateManifest.items : [])
            .map((entry) => asRecord(entry))
            .filter((entry): entry is Record<string, unknown> => entry != null)
            .map((entry) => ({
              candidatePosition: asNumber(entry.candidate_position),
              routeId: asNumber(entry.route_id),
              routeName: asString(entry.route_name),
              providerId: asNumber(entry.provider_id),
              providerKey: asString(entry.provider_key),
              modelId: asNumber(entry.model_id),
              modelName: asString(entry.model_name),
              realModelName: asString(entry.real_model_name),
              llmApiType: asString(entry.llm_api_type),
              providerApiKeyMode: asString(entry.provider_api_key_mode),
            })),
        }
      : null,
    transformDiagnostics: transformDiagnostics
        ? {
            summary: {
              count: asNumber(transformDiagnosticsSummary?.count) ?? 0,
              maxLossLevel: asString(transformDiagnosticsSummary?.max_loss_level),
              kinds: transformDiagnosticKinds.filter(
                (entry): entry is string => typeof entry === "string",
              ),
              phases: transformDiagnosticPhases.filter(
                (entry): entry is string => typeof entry === "string",
              ),
            },
          items: (Array.isArray(transformDiagnostics.items)
            ? transformDiagnostics.items
            : []
          )
            .map((entry) => asRecord(entry))
            .filter((entry): entry is Record<string, unknown> => entry != null)
            .map((entry) => ({
              phase: asString(entry.phase),
              diagnostic: asRecord(entry.diagnostic),
            })),
        }
      : null,
  };
};

export const decodeBundleView = (
  decoded: Record<string, unknown>,
  textDecoder = new TextDecoder(),
): BundleView => {
  const version = asNumber(decoded?.version);
  if (version !== 2) {
    throw new Error(
      `${UNSUPPORTED_BUNDLE_VERSION_PREFIX}${version == null ? "unknown" : String(version)}`,
    );
  }
  return decodeV2Bundle(decoded, textDecoder);
};

type BuildV2AttemptRowsOptions = {
  unknownProvider?: string;
  unknownModel?: string;
};

const formatProviderModelDisplay = (
  attempt: RecordAttempt,
  options: Required<BuildV2AttemptRowsOptions>,
) =>
  attempt.real_model_name_snapshot
    ? `${attempt.provider_name_snapshot ?? options.unknownProvider} / ${
        attempt.model_name_snapshot ?? options.unknownModel
      } -> ${attempt.real_model_name_snapshot}`
    : `${attempt.provider_name_snapshot ?? options.unknownProvider} / ${
        attempt.model_name_snapshot ?? options.unknownModel
      }`;

export const buildV2AttemptRows = (
  payloadAttempts: V2AttemptBodies[],
  metadataAttempts: RecordAttempt[] = [],
  options: BuildV2AttemptRowsOptions = {},
): V2AttemptRow[] => {
  const labels = {
    unknownProvider: options.unknownProvider ?? "unknown provider",
    unknownModel: options.unknownModel ?? "unknown model",
  };
  const metadataById = new Map<number, RecordAttempt>();
  const metadataByIndex = new Map<number, RecordAttempt>();
  for (const attempt of metadataAttempts) {
    metadataById.set(attempt.id, attempt);
    metadataByIndex.set(attempt.attempt_index, attempt);
  }

  const rows = payloadAttempts.map((attempt) => {
    const metadata =
      (attempt.attemptId != null ? metadataById.get(attempt.attemptId) : null) ??
      metadataByIndex.get(attempt.attemptIndex) ??
      null;
    const providerName = metadata?.provider_name_snapshot ?? labels.unknownProvider;
    const modelName = metadata?.model_name_snapshot ?? labels.unknownModel;
    const realModelName = metadata?.real_model_name_snapshot;

    return {
      ...attempt,
      status: metadata?.attempt_status ?? null,
      schedulerAction: metadata?.scheduler_action ?? null,
      httpStatus: metadata?.http_status ?? null,
      providerModelDisplay: realModelName
        ? `${providerName} / ${modelName} -> ${realModelName}`
        : `${providerName} / ${modelName}`,
    };
  });

  const existingIndexes = new Set(rows.map((row) => row.attemptIndex));
  for (const attempt of metadataAttempts) {
    if (existingIndexes.has(attempt.attempt_index)) continue;
    rows.push({
      key: `metadata-${attempt.id}-${attempt.attempt_index}`,
      attemptId: attempt.id,
      attemptIndex: attempt.attempt_index,
      requestContent: null,
      requestBaseContent: null,
      requestRawPatchContent: null,
      requestPatchError: null,
      requestBlobId: attempt.llm_request_blob_id,
      requestPatchId: attempt.llm_request_patch_id,
      responseContent: null,
      responseBlobId: attempt.llm_response_blob_id,
      responseCaptureState: attempt.llm_response_capture_state,
      status: attempt.attempt_status,
      schedulerAction: attempt.scheduler_action,
      httpStatus: attempt.http_status,
      providerModelDisplay: formatProviderModelDisplay(attempt, labels),
    });
  }

  return rows.sort((left, right) => left.attemptIndex - right.attemptIndex);
};
