import { request } from "./http";
import type {
  ProviderApiKeyReveal,
  ProviderApiKeySummary,
  ProviderBase,
  ProviderBootstrapPayload,
  ProviderBootstrapResponse,
  ProviderCheckPayload,
  ProviderCheckResponse,
  ProviderCreatePayload,
  ProviderKeyPayload,
  ProviderKeyReplacePayload,
  ProviderKeyUpdatePayload,
  ProviderListItem,
  SourceImpactAction,
  SourceImpactReport,
  ProviderSummaryItem,
  ProviderUpdatePayload,
  UpstreamSource,
  UpstreamSourcePayload,
  UpstreamSourceUpdatePayload,
} from "./types";

export function getProviderDetailList(): Promise<ProviderListItem[]> {
  return request("/ai/manager/api/provider/detail/list");
}

export function getProviderSummaryList(): Promise<ProviderSummaryItem[]> {
  return request.get("/ai/manager/api/provider/summary/list");
}

export function bootstrapProvider(
  payload: ProviderBootstrapPayload,
): Promise<ProviderBootstrapResponse> {
  return request.post("/ai/manager/api/provider/bootstrap", payload);
}

export function createProvider(payload: ProviderCreatePayload): Promise<ProviderBase> {
  return request.post("/ai/manager/api/provider", payload);
}

export function updateProvider(
  id: number | string,
  payload: ProviderUpdatePayload,
): Promise<ProviderBase> {
  return request.put(`/ai/manager/api/provider/${id}`, payload);
}

export function deleteProvider(id: number | string): Promise<void> {
  return request.delete(`/ai/manager/api/provider/${id}`);
}

export function getProviderDetail(
  id: number | string,
): Promise<ProviderListItem> {
  return request.get(`/ai/manager/api/provider/${id}/detail`);
}

export function createProviderSource(
  providerId: number | string,
  payload: UpstreamSourcePayload,
): Promise<UpstreamSource> {
  return request.post(`/ai/manager/api/provider/${providerId}/sources`, payload);
}

export function updateProviderSource(
  providerId: number | string,
  sourceId: number | string,
  payload: UpstreamSourceUpdatePayload,
): Promise<UpstreamSource> {
  return request.put(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}`,
    payload,
  );
}

export function deleteProviderSource(
  providerId: number | string,
  sourceId: number | string,
): Promise<void> {
  return request.delete(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}`,
  );
}

export function previewProviderSourceImpact(
  providerId: number | string,
  sourceId: number | string,
  action: SourceImpactAction,
): Promise<SourceImpactReport> {
  return request.post(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/model-impact`,
    { action },
  );
}

export function createProviderKey(
  id: number | string,
  payload: ProviderKeyPayload,
): Promise<ProviderApiKeySummary> {
  return request.post(`/ai/manager/api/provider/${id}/provider_keys`, payload);
}

export function getProviderKeys(id: number | string): Promise<ProviderApiKeySummary[]> {
  return request.get(`/ai/manager/api/provider/${id}/provider_keys`);
}

export function updateProviderKey(
  id: number | string,
  keyId: number | string,
  payload: ProviderKeyUpdatePayload,
): Promise<ProviderApiKeySummary> {
  return request.put(`/ai/manager/api/provider/${id}/provider_keys/${keyId}`, payload);
}

export function replaceProviderKey(
  id: number | string,
  keyId: number | string,
  payload: ProviderKeyReplacePayload,
): Promise<ProviderApiKeySummary> {
  return request.post(`/ai/manager/api/provider/${id}/provider_keys/${keyId}/replace`, payload);
}

export function revealProviderKey(
  id: number | string,
  keyId: number | string,
): Promise<ProviderApiKeyReveal> {
  return request.post(
    `/ai/manager/api/provider/${id}/provider_keys/${keyId}/reveal`,
  );
}

export function deleteProviderKey(
  id: number | string,
  keyId: number | string,
): Promise<void> {
  return request.delete(
    `/ai/manager/api/provider/${id}/provider_keys/${keyId}`,
  );
}

export function checkProviderConnection(
  providerId: number | string,
  sourceId: number | string,
  payload?: ProviderCheckPayload,
): Promise<ProviderCheckResponse> {
  return request.post(
    `/ai/manager/api/provider/${providerId}/sources/${sourceId}/check`,
    payload || {},
  );
}
