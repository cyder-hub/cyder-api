import { request } from "./http";
import type {
  PortableApplyRequest,
  PortableApplyResult,
  PortableExportRequest,
  PortableExportResponse,
  PortableImportPreviewRequest,
  PortableModuleRegistryResponse,
  PortablePreviewResponse,
} from "./types";

export function getPortableModules(): Promise<PortableModuleRegistryResponse> {
  return request.get("/ai/manager/api/system/portable/modules");
}

export function exportPortableConfig(
  payload: PortableExportRequest,
): Promise<PortableExportResponse> {
  return request.post("/ai/manager/api/system/portable/export", payload);
}

export function previewPortableImport(
  payload: PortableImportPreviewRequest,
): Promise<PortablePreviewResponse> {
  return request.post("/ai/manager/api/system/portable/import/preview", payload);
}

export function applyPortableImport(
  payload: PortableApplyRequest,
): Promise<PortableApplyResult> {
  return request.post("/ai/manager/api/system/portable/import/apply", payload);
}
