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
  ApiKeyUpdatePayload,
  ApiKeyCreatePayload,
  IssueTokenPayload,
  ProviderBase,
  ProviderApiKeyItem,
  ModelItem,
  ProviderListItem,
  ProviderRuntimeItem,
  ProviderRuntimeListParams,
  ProviderRuntimeSummary,
  AccessControlPolicyFromAPI,
  AccessControlPayload,
  ModelAliasListItem,
  ModelAliasDetail,
  ModelAliasPayload,
  PaginatedResponse,
  CustomFieldDefinition,
  CustomFieldPayload,
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
  ProviderPayload,
  ProviderKeyPayload,
  ModelPayload,
  ModelDetailResponse,
  CustomFieldLinkPayload,
  CustomFieldUnlinkPayload,
  ProviderRemoteModelsResponse,
  ProviderCheckPayload,
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

  // ========== System API Key ==========
  getApiKeyList(): Promise<ApiKeyItem[]> {
    return request("/ai/manager/api/system_api_key/list") as Promise<
      ApiKeyItem[]
    >;
  },
  updateApiKey(id: number, payload: ApiKeyUpdatePayload): Promise<void> {
    return request.put(`/ai/manager/api/system_api_key/${id}`, payload);
  },
  createApiKey(payload: ApiKeyCreatePayload): Promise<void> {
    return request.post("/ai/manager/api/system_api_key", payload);
  },
  deleteApiKey(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/system_api_key/${id}`);
  },
  issueApiKeyToken(
    apiKeyId: number,
    payload: IssueTokenPayload,
  ): Promise<string> {
    return request.post(
      `/ai/manager/api/system_api_key/${apiKeyId}/issue`,
      payload,
    ) as Promise<string>;
  },

  // ========== Provider ==========
  getProviderDetailList(): Promise<ProviderListItem[]> {
    return request("/ai/manager/api/provider/detail/list");
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

  // ========== Access Control ==========
  getAccessControlList(): Promise<AccessControlPolicyFromAPI[]> {
    return request("/ai/manager/api/access_control/list");
  },
  getAccessControlDetail(id: number): Promise<AccessControlPolicyFromAPI> {
    return request.get(`/ai/manager/api/access_control/${id}`);
  },
  updateAccessControl(
    id: number,
    payload: AccessControlPayload,
  ): Promise<void> {
    return request.put(`/ai/manager/api/access_control/${id}`, payload);
  },
  createAccessControl(payload: AccessControlPayload): Promise<void> {
    return request.post("/ai/manager/api/access_control", payload);
  },
  deleteAccessControl(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/access_control/${id}`);
  },

  // ========== Model Alias ==========
  getModelAliasList(): Promise<ModelAliasListItem[]> {
    return request.get("/ai/manager/api/model_alias/list");
  },
  getModelAliasDetail(id: number): Promise<ModelAliasDetail> {
    return request.get(`/ai/manager/api/model_alias/${id}`);
  },
  updateModelAlias(id: number, payload: ModelAliasPayload): Promise<void> {
    return request.put(`/ai/manager/api/model_alias/${id}`, payload);
  },
  createModelAlias(payload: ModelAliasPayload): Promise<void> {
    return request.post("/ai/manager/api/model_alias", payload);
  },
  deleteModelAlias(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/model_alias/${id}`);
  },

  // ========== Custom Field Definition ==========
  getCustomFieldList(
    pageSize = 1000,
  ): Promise<PaginatedResponse<CustomFieldDefinition>> {
    return request(
      `/ai/manager/api/custom_field_definition/list?page_size=${pageSize}`,
    );
  },
  getCustomFieldDetail(id: number): Promise<CustomFieldDefinition> {
    return request.get(`/ai/manager/api/custom_field_definition/${id}`);
  },
  updateCustomField(id: number, payload: CustomFieldPayload): Promise<void> {
    return request.put(
      `/ai/manager/api/custom_field_definition/${id}`,
      payload,
    );
  },
  createCustomField(payload: CustomFieldPayload): Promise<void> {
    return request.post("/ai/manager/api/custom_field_definition", payload);
  },
  deleteCustomField(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/custom_field_definition/${id}`);
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

  // ========== Custom Field Link/Unlink ==========
  linkCustomField(payload: CustomFieldLinkPayload): Promise<void> {
    return request.post(
      "/ai/manager/api/custom_field_definition/link",
      payload,
    );
  },
  unlinkCustomField(payload: CustomFieldUnlinkPayload): Promise<void> {
    return request.post(
      "/ai/manager/api/custom_field_definition/unlink",
      payload,
    );
  },
};
