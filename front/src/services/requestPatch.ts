import { request } from "./http";
import type {
  ModelEffectiveRequestPatchResponse,
  ModelReasoningConfigPayload,
  ProviderReasoningConfigPayload,
  ProviderReasoningConfigPreviewPayload,
  ReasoningConfigCatalog,
  ReasoningConfigPreview,
  ReasoningConfigResponse,
  RequestPatchExplainResponse,
  RequestPatchMutationOutcome,
  RequestPatchPayload,
  RequestPatchRule,
  RequestPatchUpdatePayload,
} from "./types";

export function getReasoningConfigCatalog(): Promise<ReasoningConfigCatalog> {
  return request.get("/ai/manager/api/reasoning_config/catalog");
}

export function getProviderReasoningConfig(
  providerId: number | string,
): Promise<ReasoningConfigResponse> {
  return request.get(`/ai/manager/api/provider/${providerId}/reasoning_config`);
}

export function updateProviderReasoningConfig(
  providerId: number | string,
  payload: ProviderReasoningConfigPayload,
): Promise<ReasoningConfigResponse> {
  return request.put(
    `/ai/manager/api/provider/${providerId}/reasoning_config`,
    payload,
  );
}

export function deleteProviderReasoningConfig(
  providerId: number | string,
): Promise<void> {
  return request.delete(
    `/ai/manager/api/provider/${providerId}/reasoning_config`,
  );
}

export function previewProviderReasoningConfig(
  providerId: number | string,
): Promise<ReasoningConfigPreview> {
  return request.get(
    `/ai/manager/api/provider/${providerId}/reasoning_config/preview`,
  );
}

export function previewProviderReasoningConfigDraft(
  providerId: number | string,
  payload: ProviderReasoningConfigPreviewPayload,
): Promise<ReasoningConfigPreview> {
  return request.post(
    `/ai/manager/api/provider/${providerId}/reasoning_config/preview`,
    payload,
  );
}

export function getModelReasoningConfig(
  modelId: number | string,
): Promise<ReasoningConfigResponse> {
  return request.get(`/ai/manager/api/model/${modelId}/reasoning_config`);
}

export function updateModelReasoningConfig(
  modelId: number | string,
  payload: ModelReasoningConfigPayload,
): Promise<ReasoningConfigResponse> {
  return request.put(`/ai/manager/api/model/${modelId}/reasoning_config`, payload);
}

export function deleteModelReasoningConfig(
  modelId: number | string,
): Promise<void> {
  return request.delete(`/ai/manager/api/model/${modelId}/reasoning_config`);
}

export function previewModelReasoningConfig(
  modelId: number | string,
): Promise<ReasoningConfigPreview> {
  return request.get(
    `/ai/manager/api/model/${modelId}/reasoning_config/preview`,
  );
}

export function previewModelReasoningConfigDraft(
  modelId: number | string,
  payload: ModelReasoningConfigPayload,
): Promise<ReasoningConfigPreview> {
  return request.post(
    `/ai/manager/api/model/${modelId}/reasoning_config/preview`,
    payload,
  );
}

export function listProviderRequestPatches(
  id: number | string,
): Promise<RequestPatchRule[]> {
  return request.get(`/ai/manager/api/provider/${id}/request_patch`);
}

export function createProviderRequestPatch(
  id: number | string,
  payload: RequestPatchPayload,
): Promise<RequestPatchMutationOutcome> {
  return request.post(`/ai/manager/api/provider/${id}/request_patch`, payload);
}

export function updateProviderRequestPatch(
  id: number | string,
  ruleId: number | string,
  payload: RequestPatchUpdatePayload,
): Promise<RequestPatchMutationOutcome> {
  return request.put(`/ai/manager/api/provider/${id}/request_patch/${ruleId}`, payload);
}

export function deleteProviderRequestPatch(
  id: number | string,
  ruleId: number | string,
): Promise<void> {
  return request.delete(`/ai/manager/api/provider/${id}/request_patch/${ruleId}`);
}

export function listModelRequestPatches(
  id: number | string,
): Promise<RequestPatchRule[]> {
  return request.get(`/ai/manager/api/model/${id}/request_patch`);
}

export function createModelRequestPatch(
  id: number | string,
  payload: RequestPatchPayload,
): Promise<RequestPatchMutationOutcome> {
  return request.post(`/ai/manager/api/model/${id}/request_patch`, payload);
}

export function updateModelRequestPatch(
  id: number | string,
  ruleId: number | string,
  payload: RequestPatchUpdatePayload,
): Promise<RequestPatchMutationOutcome> {
  return request.put(`/ai/manager/api/model/${id}/request_patch/${ruleId}`, payload);
}

export function deleteModelRequestPatch(
  id: number | string,
  ruleId: number | string,
): Promise<void> {
  return request.delete(`/ai/manager/api/model/${id}/request_patch/${ruleId}`);
}

export function getModelEffectiveRequestPatches(
  id: number | string,
): Promise<ModelEffectiveRequestPatchResponse> {
  return request.get(`/ai/manager/api/model/${id}/request_patch/effective`);
}

export function getModelRequestPatchExplain(
  id: number | string,
): Promise<RequestPatchExplainResponse> {
  return request.get(`/ai/manager/api/model/${id}/request_patch/explain`);
}
