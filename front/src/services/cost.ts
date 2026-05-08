import { request } from "./http";
import type {
  CostCatalog,
  CostCatalogListItem,
  CostCatalogPayload,
  CostCatalogVersion,
  CostCatalogVersionDetail,
  CostCatalogVersionPayload,
  CostComponent,
  CostComponentPayload,
  CostComponentUpdatePayload,
  CostPreviewPayload,
  CostPreviewResponse,
  CostTemplateSummary,
  ImportCostTemplatePayload,
  ImportCostTemplateResponse,
} from "./types";

export function getCostTemplateList(): Promise<CostTemplateSummary[]> {
  return request.get("/ai/manager/api/cost/template/list");
}

export function importCostTemplate(
  payload: ImportCostTemplatePayload,
): Promise<ImportCostTemplateResponse> {
  return request.post("/ai/manager/api/cost/template/import", payload);
}

export function getCostCatalogList(): Promise<CostCatalogListItem[]> {
  return request.get("/ai/manager/api/cost/catalog/list");
}

export function createCostCatalog(
  payload: CostCatalogPayload,
): Promise<CostCatalog> {
  return request.post("/ai/manager/api/cost/catalog", payload);
}

export function updateCostCatalog(
  id: number,
  payload: CostCatalogPayload,
): Promise<CostCatalog> {
  return request.put(`/ai/manager/api/cost/catalog/${id}`, payload);
}

export function deleteCostCatalog(id: number): Promise<void> {
  return request.delete(`/ai/manager/api/cost/catalog/${id}`);
}

export function createCostCatalogVersion(
  catalogId: number,
  payload: CostCatalogVersionPayload,
): Promise<CostCatalogVersion> {
  return request.post(`/ai/manager/api/cost/catalog/${catalogId}/version`, payload);
}

export function enableCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
  return request.post(`/ai/manager/api/cost/version/${id}/enable`, {});
}

export function disableCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
  return request.post(`/ai/manager/api/cost/version/${id}/disable`, {});
}

export function archiveCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
  return request.post(`/ai/manager/api/cost/version/${id}/archive`, {});
}

export function unarchiveCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
  return request.post(`/ai/manager/api/cost/version/${id}/unarchive`, {});
}

export function duplicateCostCatalogVersion(
  id: number,
  payload?: { version?: string | null },
): Promise<CostCatalogVersion> {
  return request.post(`/ai/manager/api/cost/version/${id}/duplicate`, payload ?? {});
}

export function deleteCostCatalogVersion(id: number): Promise<void> {
  return request.delete(`/ai/manager/api/cost/version/${id}`);
}

export function getCostCatalogVersion(
  id: number,
): Promise<CostCatalogVersionDetail> {
  return request.get(`/ai/manager/api/cost/version/${id}`);
}

export function createCostComponent(
  payload: CostComponentPayload,
): Promise<CostComponent> {
  return request.post("/ai/manager/api/cost/component", payload);
}

export function updateCostComponent(
  id: number,
  payload: CostComponentUpdatePayload,
): Promise<CostComponent> {
  return request.put(`/ai/manager/api/cost/component/${id}`, payload);
}

export function deleteCostComponent(id: number): Promise<void> {
  return request.delete(`/ai/manager/api/cost/component/${id}`);
}

export function previewCost(payload: CostPreviewPayload): Promise<CostPreviewResponse> {
  return request.post("/ai/manager/api/cost/preview", payload);
}
