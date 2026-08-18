import { request } from "./http";
import type {
  ModelRequestPatchOverviewResponse,
  RequestPatchExplainResponse,
  RequestPatchPreviewResponse,
  RequestPatchVariantAggregate,
  RequestPatchVariantInput,
  RequestPatchVariantListResponse,
} from "./types";

export function listSourceRequestPatchVariants(
  providerId: number,
  sourceId: number,
): Promise<RequestPatchVariantListResponse> {
  return request.get(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/request_patch`,
  );
}

export function createSourceRequestPatchVariant(
  providerId: number,
  sourceId: number,
  payload: RequestPatchVariantInput,
): Promise<RequestPatchVariantAggregate> {
  return request.post(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/request_patch/variants`,
    payload,
  );
}

export function updateSourceRequestPatchVariant(
  providerId: number,
  sourceId: number,
  variantId: number,
  payload: RequestPatchVariantInput,
): Promise<RequestPatchVariantAggregate> {
  return request.put(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/request_patch/variants/${variantId}`,
    payload,
  );
}

export function deleteSourceRequestPatchVariant(
  providerId: number,
  sourceId: number,
  variantId: number,
): Promise<RequestPatchVariantAggregate> {
  return request.delete(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/request_patch/variants/${variantId}`,
  );
}

export function previewSourceRequestPatchVariant(
  providerId: number,
  sourceId: number,
  payload: RequestPatchVariantInput & { variant_id?: number | null },
): Promise<RequestPatchPreviewResponse> {
  return request.post(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/request_patch/preview`,
    payload,
  );
}

export function explainSourceRequestPatchVariants(
  providerId: number,
  sourceId: number,
  suffix?: string | null,
): Promise<RequestPatchExplainResponse> {
  return request.get(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/request_patch/explain`,
    suffix ? { params: { suffix } } : undefined,
  );
}

export function listModelRequestPatchOverview(
  modelId: number,
): Promise<ModelRequestPatchOverviewResponse> {
  return request.get(`/ai/manager/api/model/${modelId}/request_patch`);
}

export function listModelSourceRequestPatchVariants(
  modelId: number,
  sourceId: number,
): Promise<RequestPatchVariantListResponse> {
  return request.get(
    `/ai/manager/api/model/${modelId}/sources/${sourceId}/request_patch`,
  );
}

export function createModelSourceRequestPatchVariant(
  modelId: number,
  sourceId: number,
  payload: RequestPatchVariantInput,
): Promise<RequestPatchVariantAggregate> {
  return request.post(
    `/ai/manager/api/model/${modelId}/sources/${sourceId}/request_patch/variants`,
    payload,
  );
}

export function updateModelSourceRequestPatchVariant(
  modelId: number,
  sourceId: number,
  variantId: number,
  payload: RequestPatchVariantInput,
): Promise<RequestPatchVariantAggregate> {
  return request.put(
    `/ai/manager/api/model/${modelId}/sources/${sourceId}/request_patch/variants/${variantId}`,
    payload,
  );
}

export function deleteModelSourceRequestPatchVariant(
  modelId: number,
  sourceId: number,
  variantId: number,
): Promise<RequestPatchVariantAggregate> {
  return request.delete(
    `/ai/manager/api/model/${modelId}/sources/${sourceId}/request_patch/variants/${variantId}`,
  );
}

export function previewModelSourceRequestPatchVariant(
  modelId: number,
  sourceId: number,
  payload: RequestPatchVariantInput & { variant_id?: number | null },
): Promise<RequestPatchPreviewResponse> {
  return request.post(
    `/ai/manager/api/model/${modelId}/sources/${sourceId}/request_patch/preview`,
    payload,
  );
}

export function explainModelSourceRequestPatchVariants(
  modelId: number,
  sourceId: number,
  suffix?: string | null,
): Promise<RequestPatchExplainResponse> {
  return request.get(
    `/ai/manager/api/model/${modelId}/sources/${sourceId}/request_patch/explain`,
    suffix ? { params: { suffix } } : undefined,
  );
}
