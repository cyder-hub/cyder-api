import { request } from "./http";
import { buildSystemConfigHistoryQuery } from "./query";
import type {
  SystemConfigApplyRequest,
  SystemConfigChangeRequest,
  SystemConfigHistoryItem,
  SystemConfigHistoryQuery,
  SystemConfigPreviewResponse,
  SystemConfigReport,
  SystemConfigResetApplyRequest,
} from "./types";

export function getSystemConfig(): Promise<SystemConfigReport> {
  return request.get("/ai/manager/api/system/config");
}

export function previewSystemConfig(
  payload: SystemConfigChangeRequest,
): Promise<SystemConfigPreviewResponse> {
  return request.post("/ai/manager/api/system/config/preview", payload);
}

export function applySystemConfig(
  payload: SystemConfigApplyRequest,
): Promise<SystemConfigReport> {
  return request.post("/ai/manager/api/system/config/apply", payload);
}

export function resetSystemConfig(
  payload: SystemConfigResetApplyRequest,
): Promise<SystemConfigReport> {
  return request.post("/ai/manager/api/system/config/reset", payload);
}

export function reloadSystemConfig(): Promise<SystemConfigReport> {
  return request.post("/ai/manager/api/system/config/reload", {});
}

export function getSystemConfigHistory(
  params: SystemConfigHistoryQuery = {},
): Promise<SystemConfigHistoryItem[]> {
  const qs = buildSystemConfigHistoryQuery(params);
  return request.get(
    `/ai/manager/api/system/config/history${qs ? `?${qs}` : ""}`,
  );
}
