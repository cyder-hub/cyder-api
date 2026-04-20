import { request } from "./api";
import type {
  SystemOverviewStats,
  TodayRequestLogStats,
  DashboardResponse,
  DashboardKpiSection,
  DashboardResourcesSection,
  DashboardAlertsSection,
  UsageStatsPeriod,
  ApiKeyItem,
  ApiKeyDetail,
  ApiKeyReveal,
  ApiKeyCreateResponse,
  ApiKeyRuntimeSnapshot,
  ApiKeyUpdatePayload,
  ApiKeyCreatePayload,
  ProviderBase,
  ProviderApiKeyItem,
  ModelItem,
  ProviderSummaryItem,
  ModelSummaryItem,
  ProviderListItem,
  ProviderRuntimeItem,
  ProviderRuntimeListParams,
  ProviderRuntimeSummary,
  ModelRouteListItem,
  ModelRouteDetail,
  ModelRoutePayload,
  ModelRouteUpdatePayload,
  PaginatedResponse,
  CostCatalog,
  CostCatalogListItem,
  CostCatalogPayload,
  CostCatalogVersion,
  CostCatalogVersionDetail,
  CostCatalogVersionPayload,
  CostComponent,
  CostComponentPayload,
  CostComponentUpdatePayload,
  CostTemplateSummary,
  CostPreviewPayload,
  CostPreviewResponse,
  ImportCostTemplatePayload,
  ImportCostTemplateResponse,
  RecordListItem,
  RecordDetail,
  RecordListParams,
  ProviderBootstrapPayload,
  ProviderBootstrapResponse,
  ProviderPayload,
  ProviderKeyPayload,
  ModelPayload,
  ModelDetailResponse,
  ProviderRemoteModelsResponse,
  ProviderCheckPayload,
  RequestPatchPayload,
  RequestPatchRule,
  RequestPatchUpdatePayload,
  RequestPatchMutationOutcome,
  ModelEffectiveRequestPatchResponse,
  RequestPatchExplainResponse,
} from "@/store/types";

export const Api = {
  // ========== Auth ==========
  refreshToken(refreshToken: string): Promise<string> {
    return request.post(
      "/ai/manager/api/auth/refresh_token",
      {},
      {
        headers: { Authorization: `Bearer ${refreshToken}` },
      },
    );
  },
  login(password: string): Promise<string> {
    return request.post("/ai/manager/api/auth/login", { key: password });
  },

  // ========== System / Dashboard ==========
  getSystemOverview(): Promise<SystemOverviewStats> {
    return request.get("/ai/manager/api/system/overview");
  },
  getTodayLogStats(): Promise<TodayRequestLogStats> {
    return request.get("/ai/manager/api/system/today_log_stats");
  },
  getSystemDashboard(): Promise<DashboardResponse> {
    return request.get("/ai/manager/api/system/dashboard");
  },
  getSystemDashboardKpi(): Promise<DashboardKpiSection> {
    return request.get("/ai/manager/api/system/dashboard/kpi");
  },
  getSystemDashboardResources(): Promise<DashboardResourcesSection> {
    return request.get("/ai/manager/api/system/dashboard/resources");
  },
  getSystemDashboardAlerts(): Promise<DashboardAlertsSection> {
    return request.get("/ai/manager/api/system/dashboard/alerts");
  },
  getUsageStats(params: URLSearchParams): Promise<UsageStatsPeriod[]> {
    return request(`/ai/manager/api/system/usage_stats?${params.toString()}`);
  },

  // ========== API Key ==========
  getApiKeyList(): Promise<ApiKeyItem[]> {
    return request.get("/ai/manager/api/api_key/list");
  },
  getApiKeyDetail(id: number): Promise<ApiKeyDetail> {
    return request.get(`/ai/manager/api/api_key/${id}`);
  },
  getApiKeyRuntime(id: number): Promise<ApiKeyRuntimeSnapshot> {
    return request.get(`/ai/manager/api/api_key/${id}/runtime`);
  },
  getApiKeyRuntimeList(): Promise<ApiKeyRuntimeSnapshot[]> {
    return request.get("/ai/manager/api/api_key/runtime/list");
  },
  updateApiKey(id: number, payload: ApiKeyUpdatePayload): Promise<ApiKeyDetail> {
    return request.put(`/ai/manager/api/api_key/${id}`, payload);
  },
  createApiKey(payload: ApiKeyCreatePayload): Promise<ApiKeyCreateResponse> {
    return request.post("/ai/manager/api/api_key/", payload);
  },
  rotateApiKey(id: number): Promise<ApiKeyReveal> {
    return request.post(`/ai/manager/api/api_key/${id}/rotate`, {});
  },
  revealApiKey(id: number): Promise<ApiKeyReveal> {
    return request.get(`/ai/manager/api/api_key/${id}/reveal`);
  },
  deleteApiKey(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/api_key/${id}`);
  },

  // ========== Provider ==========
  getProviderDetailList(): Promise<ProviderListItem[]> {
    return request("/ai/manager/api/provider/detail/list");
  },
  getProviderSummaryList(): Promise<ProviderSummaryItem[]> {
    return request.get("/ai/manager/api/provider/summary/list");
  },
  getProviderRuntimeList(
    params: ProviderRuntimeListParams = {},
  ): Promise<ProviderRuntimeItem[]> {
    const validParams: Record<string, string> = {};
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined && value !== null && value !== "") {
        validParams[key] = String(value);
      }
    }
    const qs = new URLSearchParams(validParams).toString();
    return request.get(`/ai/manager/api/provider/runtime/list?${qs}`);
  },
  getProviderRuntimeSummary(
    window?: ProviderRuntimeListParams["window"],
  ): Promise<ProviderRuntimeSummary> {
    const qs = new URLSearchParams();
    if (window) {
      qs.set("window", window);
    }
    const suffix = qs.toString();
    return request.get(
      `/ai/manager/api/provider/runtime/summary${suffix ? `?${suffix}` : ""}`,
    );
  },
  // ========== Model Route ==========
  getModelRouteList(): Promise<ModelRouteListItem[]> {
    return request.get("/ai/manager/api/model_route/list");
  },
  getModelRouteDetail(id: number): Promise<ModelRouteDetail> {
    return request.get(`/ai/manager/api/model_route/${id}`);
  },
  updateModelRoute(
    id: number,
    payload: ModelRouteUpdatePayload,
  ): Promise<ModelRouteDetail> {
    return request.put(`/ai/manager/api/model_route/${id}`, payload);
  },
  createModelRoute(payload: ModelRoutePayload): Promise<ModelRouteDetail> {
    return request.post("/ai/manager/api/model_route", payload);
  },
  deleteModelRoute(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/model_route/${id}`);
  },

  // ========== Cost ==========
  getCostTemplateList(): Promise<CostTemplateSummary[]> {
    return request.get("/ai/manager/api/cost/template/list");
  },
  importCostTemplate(
    payload: ImportCostTemplatePayload,
  ): Promise<ImportCostTemplateResponse> {
    return request.post("/ai/manager/api/cost/template/import", payload);
  },
  getCostCatalogList(): Promise<CostCatalogListItem[]> {
    return request.get("/ai/manager/api/cost/catalog/list");
  },
  createCostCatalog(payload: CostCatalogPayload): Promise<CostCatalog> {
    return request.post("/ai/manager/api/cost/catalog", payload);
  },
  updateCostCatalog(
    id: number,
    payload: CostCatalogPayload,
  ): Promise<CostCatalog> {
    return request.put(`/ai/manager/api/cost/catalog/${id}`, payload);
  },
  deleteCostCatalog(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/cost/catalog/${id}`);
  },
  createCostCatalogVersion(
    catalogId: number,
    payload: CostCatalogVersionPayload,
  ): Promise<CostCatalogVersion> {
    return request.post(`/ai/manager/api/cost/catalog/${catalogId}/version`, payload);
  },
  enableCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
    return request.post(`/ai/manager/api/cost/version/${id}/enable`, {});
  },
  disableCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
    return request.post(`/ai/manager/api/cost/version/${id}/disable`, {});
  },
  archiveCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
    return request.post(`/ai/manager/api/cost/version/${id}/archive`, {});
  },
  unarchiveCostCatalogVersion(id: number): Promise<CostCatalogVersion> {
    return request.post(`/ai/manager/api/cost/version/${id}/unarchive`, {});
  },
  duplicateCostCatalogVersion(
    id: number,
    payload?: { version?: string | null },
  ): Promise<CostCatalogVersion> {
    return request.post(`/ai/manager/api/cost/version/${id}/duplicate`, payload ?? {});
  },
  deleteCostCatalogVersion(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/cost/version/${id}`);
  },
  getCostCatalogVersion(id: number): Promise<CostCatalogVersionDetail> {
    return request.get(`/ai/manager/api/cost/version/${id}`);
  },
  createCostComponent(payload: CostComponentPayload): Promise<CostComponent> {
    return request.post("/ai/manager/api/cost/component", payload);
  },
  updateCostComponent(
    id: number,
    payload: CostComponentUpdatePayload,
  ): Promise<CostComponent> {
    return request.put(`/ai/manager/api/cost/component/${id}`, payload);
  },
  deleteCostComponent(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/cost/component/${id}`);
  },
  previewCost(payload: CostPreviewPayload): Promise<CostPreviewResponse> {
    return request.post("/ai/manager/api/cost/preview", payload);
  },

  // ========== Request Log ==========
  getRecordList(
    params: RecordListParams,
  ): Promise<PaginatedResponse<RecordListItem>> {
    // Filter undefined/null/empty values
    const validParams: Record<string, string> = {};
    for (const [key, value] of Object.entries(params)) {
      if (value !== undefined && value !== null && value !== "") {
        validParams[key] = String(value);
      }
    }
    const qs = new URLSearchParams(validParams).toString();
    return request.get(`/ai/manager/api/request_log/list?${qs}`);
  },
  getRecordDetail(id: number | string): Promise<RecordDetail> {
    return request.get(`/ai/manager/api/request_log/${id}`);
  },
  getRecordContent(id: number | string): Promise<ArrayBuffer> {
    return request.get(`/ai/manager/api/request_log/${id}/content`, {
      headers: { "Content-Type": "application/msgpack" },
      responseType: "arraybuffer",
    });
  },

  // ========== Provider CRUD (Extended) ==========
  bootstrapProvider(
    payload: ProviderBootstrapPayload,
  ): Promise<ProviderBootstrapResponse> {
    return request.post("/ai/manager/api/provider/bootstrap", payload);
  },
  createProvider(payload: ProviderPayload): Promise<ProviderBase> {
    return request.post("/ai/manager/api/provider", payload);
  },
  updateProvider(id: number | string, payload: ProviderPayload): Promise<void> {
    return request.put(`/ai/manager/api/provider/${id}`, payload);
  },
  deleteProvider(id: number | string): Promise<void> {
    return request.delete(`/ai/manager/api/provider/${id}`);
  },
  getProviderDetail(id: number | string): Promise<ProviderListItem> {
    return request.get(`/ai/manager/api/provider/${id}/detail`);
  },
  getProviderRemoteModels(
    id: number | string,
  ): Promise<ProviderRemoteModelsResponse> {
    return request.get(`/ai/manager/api/provider/${id}/remote_models`);
  },
  listProviderRequestPatches(id: number | string): Promise<RequestPatchRule[]> {
    return request.get(`/ai/manager/api/provider/${id}/request_patch`);
  },
  createProviderRequestPatch(
    id: number | string,
    payload: RequestPatchPayload,
  ): Promise<RequestPatchMutationOutcome> {
    return request.post(`/ai/manager/api/provider/${id}/request_patch`, payload);
  },
  updateProviderRequestPatch(
    id: number | string,
    ruleId: number | string,
    payload: RequestPatchUpdatePayload,
  ): Promise<RequestPatchMutationOutcome> {
    return request.put(`/ai/manager/api/provider/${id}/request_patch/${ruleId}`, payload);
  },
  deleteProviderRequestPatch(
    id: number | string,
    ruleId: number | string,
  ): Promise<void> {
    return request.delete(`/ai/manager/api/provider/${id}/request_patch/${ruleId}`);
  },
  createProviderKey(
    id: number | string,
    payload: ProviderKeyPayload,
  ): Promise<ProviderApiKeyItem> {
    return request.post(`/ai/manager/api/provider/${id}/provider_key`, payload);
  },
  deleteProviderKey(
    id: number | string,
    keyId: number | string,
  ): Promise<void> {
    return request.delete(
      `/ai/manager/api/provider/${id}/provider_key/${keyId}`,
    );
  },
  checkProviderConnection(
    id: number | string,
    payload?: ProviderCheckPayload,
  ): Promise<null> {
    return request.post(`/ai/manager/api/provider/${id}/check`, payload || {});
  },

  // ========== Model CRUD ==========
  getModelSummaryList(): Promise<ModelSummaryItem[]> {
    return request.get("/ai/manager/api/model/summary/list");
  },
  createModel(payload: ModelPayload): Promise<ModelItem> {
    return request.post("/ai/manager/api/model", payload);
  },
  updateModel(id: number | string, payload: ModelPayload): Promise<void> {
    return request.put(`/ai/manager/api/model/${id}`, payload);
  },
  deleteModel(id: number | string): Promise<void> {
    return request.delete(`/ai/manager/api/model/${id}`);
  },
  getModelDetail(id: number | string): Promise<ModelDetailResponse> {
    return request.get(`/ai/manager/api/model/${id}/detail`);
  },
  listModelRequestPatches(id: number | string): Promise<RequestPatchRule[]> {
    return request.get(`/ai/manager/api/model/${id}/request_patch`);
  },
  createModelRequestPatch(
    id: number | string,
    payload: RequestPatchPayload,
  ): Promise<RequestPatchMutationOutcome> {
    return request.post(`/ai/manager/api/model/${id}/request_patch`, payload);
  },
  updateModelRequestPatch(
    id: number | string,
    ruleId: number | string,
    payload: RequestPatchUpdatePayload,
  ): Promise<RequestPatchMutationOutcome> {
    return request.put(`/ai/manager/api/model/${id}/request_patch/${ruleId}`, payload);
  },
  deleteModelRequestPatch(
    id: number | string,
    ruleId: number | string,
  ): Promise<void> {
    return request.delete(`/ai/manager/api/model/${id}/request_patch/${ruleId}`);
  },
  getModelEffectiveRequestPatches(
    id: number | string,
  ): Promise<ModelEffectiveRequestPatchResponse> {
    return request.get(`/ai/manager/api/model/${id}/request_patch/effective`);
  },
  getModelRequestPatchExplain(
    id: number | string,
  ): Promise<RequestPatchExplainResponse> {
    return request.get(`/ai/manager/api/model/${id}/request_patch/explain`);
  },
};
