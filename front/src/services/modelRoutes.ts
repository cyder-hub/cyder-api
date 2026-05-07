import { request } from "./http";
import type {
  ModelRouteDetail,
  ModelRouteListItem,
  ModelRoutePayload,
  ModelRouteUpdatePayload,
  ReasoningRoutePreview,
} from "./types";

export function getModelRouteList(): Promise<ModelRouteListItem[]> {
  return request.get("/ai/manager/api/model_route/list");
}

export function getModelRouteDetail(id: number): Promise<ModelRouteDetail> {
  return request.get(`/ai/manager/api/model_route/${id}`);
}

export function updateModelRoute(
  id: number,
  payload: ModelRouteUpdatePayload,
): Promise<ModelRouteDetail> {
  return request.put(`/ai/manager/api/model_route/${id}`, payload);
}

export function createModelRoute(
  payload: ModelRoutePayload,
): Promise<ModelRouteDetail> {
  return request.post("/ai/manager/api/model_route", payload);
}

export function deleteModelRoute(id: number): Promise<void> {
  return request.delete(`/ai/manager/api/model_route/${id}`);
}

export function getModelRouteReasoningPreview(
  id: number,
): Promise<ReasoningRoutePreview> {
  return request.get(`/ai/manager/api/model_route/${id}/reasoning_preview`);
}
