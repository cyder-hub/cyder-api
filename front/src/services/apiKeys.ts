import { request } from "./http";
import type {
  ApiKeyCreatePayload,
  ApiKeyCreateResponse,
  ApiKeyDetail,
  ApiKeyItem,
  ApiKeyReveal,
  ApiKeyRuntimeSnapshot,
  ApiKeyUpdatePayload,
} from "./types";

export function getApiKeyList(): Promise<ApiKeyItem[]> {
  return request.get("/ai/manager/api/api_key/list");
}

export function getApiKeyDetail(id: number): Promise<ApiKeyDetail> {
  return request.get(`/ai/manager/api/api_key/${id}`);
}

export function getApiKeyRuntime(id: number): Promise<ApiKeyRuntimeSnapshot> {
  return request.get(`/ai/manager/api/api_key/${id}/runtime`);
}

export function getApiKeyRuntimeList(): Promise<ApiKeyRuntimeSnapshot[]> {
  return request.get("/ai/manager/api/api_key/runtime/list");
}

export function updateApiKey(
  id: number,
  payload: ApiKeyUpdatePayload,
): Promise<ApiKeyDetail> {
  return request.put(`/ai/manager/api/api_key/${id}`, payload);
}

export function createApiKey(
  payload: ApiKeyCreatePayload,
): Promise<ApiKeyCreateResponse> {
  return request.post("/ai/manager/api/api_key/", payload);
}

export function rotateApiKey(id: number): Promise<ApiKeyReveal> {
  return request.post(`/ai/manager/api/api_key/${id}/rotate`, {});
}

export function revealApiKey(id: number): Promise<ApiKeyReveal> {
  return request.get(`/ai/manager/api/api_key/${id}/reveal`);
}

export function deleteApiKey(id: number): Promise<void> {
  return request.delete(`/ai/manager/api/api_key/${id}`);
}
