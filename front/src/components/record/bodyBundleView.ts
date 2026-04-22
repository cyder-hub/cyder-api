import jsonPatch from "fast-json-patch";
import type { RecordAttempt } from "../../store/types";

const { applyPatch } = jsonPatch;

export type LegacyBodies = {
  kind: "legacy";
  version: number;
  userRequestBody: string | null;
  llmRequestBody: string | null;
  llmRequestPatchBaseBody: string | null;
  userResponseBody: string | null;
  llmResponseBody: string | null;
};

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

export type V2Bodies = {
  kind: "v2";
  version: 2;
  request: V2RequestBodies;
  attempts: V2AttemptBodies[];
};

export type BundleView = LegacyBodies | V2Bodies;

export type LegacyPatchInfo = {
  isPatch: boolean;
  patchedContent: string | null;
};

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

export const decodeLegacyBundle = (
  decoded: Record<string, unknown>,
  textDecoder: TextDecoder,
): LegacyBodies => ({
  kind: "legacy",
  version: asNumber(decoded.version) ?? 1,
  userRequestBody: decodeBytes(decoded.user_request_body, textDecoder),
  llmRequestBody: decodeBytes(decoded.llm_request_body, textDecoder),
  llmRequestPatchBaseBody: null,
  userResponseBody: decodeBytes(decoded.user_response_body, textDecoder),
  llmResponseBody: decodeBytes(decoded.llm_response_body, textDecoder),
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
    decoded.request_section && typeof decoded.request_section === "object"
      ? (decoded.request_section as Record<string, unknown>)
      : {};
  const userRequestBlobId = asNumber(requestSection.user_request_blob_id);
  const userResponseBlobId = asNumber(requestSection.user_response_blob_id);

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
  };
};

export const decodeBundleView = (
  decoded: Record<string, unknown>,
  textDecoder = new TextDecoder(),
): BundleView =>
  decoded?.version === 2
    ? decodeV2Bundle(decoded, textDecoder)
    : decodeLegacyBundle(decoded, textDecoder);

const formatProviderModelDisplay = (attempt: RecordAttempt) =>
  attempt.real_model_name_snapshot
    ? `${attempt.provider_name_snapshot ?? "unknown provider"} / ${
        attempt.model_name_snapshot ?? "unknown model"
      } -> ${attempt.real_model_name_snapshot}`
    : `${attempt.provider_name_snapshot ?? "unknown provider"} / ${
        attempt.model_name_snapshot ?? "unknown model"
      }`;

export const buildV2AttemptRows = (
  payloadAttempts: V2AttemptBodies[],
  metadataAttempts: RecordAttempt[] = [],
): V2AttemptRow[] => {
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
    const providerName = metadata?.provider_name_snapshot ?? "unknown provider";
    const modelName = metadata?.model_name_snapshot ?? "unknown model";
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
      providerModelDisplay: formatProviderModelDisplay(attempt),
    });
  }

  return rows.sort((left, right) => left.attemptIndex - right.attemptIndex);
};

export const buildLegacyPatchInfo = (bodies: LegacyBodies | null): LegacyPatchInfo => {
  const userContent = bodies?.llmRequestPatchBaseBody ?? bodies?.userRequestBody;
  const llmContent = bodies?.llmRequestBody;
  if (!userContent || !llmContent || userContent === llmContent) {
    return { isPatch: false, patchedContent: null };
  }
  try {
    const userJson = JSON.parse(userContent);
    const patch = JSON.parse(llmContent);
    if (Array.isArray(patch) && patch.every((op) => "op" in op && "path" in op)) {
      const { newDocument } = applyPatch(userJson, patch, true, false);
      return {
        isPatch: true,
        patchedContent: JSON.stringify(newDocument, null, 2),
      };
    }
  } catch (e) {}
  return { isPatch: false, patchedContent: null };
};
