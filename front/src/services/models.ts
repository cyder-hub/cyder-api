import { request } from "./http";
import type {
  ModelDetailResponse,
  ModelItem,
  ModelPayload,
  ModelSourceConfigPayload,
  ModelSourceConfigSummary,
  ModelSourceExplain,
  ModelSummaryItem,
} from "./types";

export function getModelSummaryList(): Promise<ModelSummaryItem[]> {
  return request.get("/ai/manager/api/model/summary/list");
}

export function createModel(payload: ModelPayload): Promise<ModelItem> {
  return request.post("/ai/manager/api/model", payload);
}

export function updateModel(
  id: number | string,
  payload: ModelPayload,
): Promise<void> {
  return request.put(`/ai/manager/api/model/${id}`, payload);
}

export function deleteModel(id: number | string): Promise<void> {
  return request.delete(`/ai/manager/api/model/${id}`);
}

export function getModelDetail(
  id: number | string,
): Promise<ModelDetailResponse> {
  return request.get(`/ai/manager/api/model/${id}/detail`);
}

export function getModelSourceConfig(
  id: number | string,
): Promise<ModelSourceConfigSummary> {
  return request.get(`/ai/manager/api/model/${id}/source-config`);
}

export function updateModelSourceConfig(
  id: number | string,
  payload: ModelSourceConfigPayload,
): Promise<ModelSourceConfigSummary> {
  return request.put(`/ai/manager/api/model/${id}/source-config`, payload);
}

export function getModelSourceExplain(
  id: number | string,
): Promise<ModelSourceExplain> {
  return request.get(`/ai/manager/api/model/${id}/source-config/explain`);
}
