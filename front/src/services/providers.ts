import { request } from "./http";
import type {
  ProviderApiKeyItem,
  ProviderBase,
  ProviderBootstrapPayload,
  ProviderBootstrapResponse,
  ProviderCheckPayload,
  ProviderKeyPayload,
  ProviderListItem,
  ProviderPayload,
  ProviderRemoteModelsResponse,
  ProviderSummaryItem,
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

export function createProvider(payload: ProviderPayload): Promise<ProviderBase> {
  return request.post("/ai/manager/api/provider", payload);
}

export function updateProvider(
  id: number | string,
  payload: ProviderPayload,
): Promise<void> {
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

export function getProviderRemoteModels(
  id: number | string,
): Promise<ProviderRemoteModelsResponse> {
  return request.get(`/ai/manager/api/provider/${id}/remote_models`);
}

export function createProviderKey(
  id: number | string,
  payload: ProviderKeyPayload,
): Promise<ProviderApiKeyItem> {
  return request.post(`/ai/manager/api/provider/${id}/provider_key`, payload);
}

export function deleteProviderKey(
  id: number | string,
  keyId: number | string,
): Promise<void> {
  return request.delete(
    `/ai/manager/api/provider/${id}/provider_key/${keyId}`,
  );
}

export function checkProviderConnection(
  id: number | string,
  payload?: ProviderCheckPayload,
): Promise<null> {
  return request.post(`/ai/manager/api/provider/${id}/check`, payload || {});
}
