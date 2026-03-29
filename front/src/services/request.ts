import { request } from "./api";
import type {
  SystemOverviewStats,
  TodayRequestLogStats,
  UsageStatsPeriod,
  ApiKeyItem,
  ApiKeyUpdatePayload,
  ApiKeyCreatePayload,
  IssueTokenPayload,
  ProviderListItem,
  AccessControlPolicyFromAPI,
  AccessControlPayload,
  ModelAliasListItem,
  ModelAliasDetail,
  ModelAliasPayload,
  PaginatedResponse,
  CustomFieldDefinition,
  CustomFieldPayload,
  BillingPlan,
  BillingPlanPayload,
  PriceRule,
  PriceRulePayload,
  RecordListItem,
  RecordDetail,
  ProviderPayload,
  ProviderKeyPayload,
  ModelPayload,
  CustomFieldLinkPayload,
  CustomFieldUnlinkPayload,
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

  // ========== Price / Billing Plan ==========
  getBillingPlanList(): Promise<BillingPlan[]> {
    return request("/ai/manager/api/price/plan/list");
  },
  updateBillingPlan(id: number, payload: BillingPlanPayload): Promise<void> {
    return request.put(`/ai/manager/api/price/plan/${id}`, payload);
  },
  createBillingPlan(payload: BillingPlanPayload): Promise<void> {
    return request.post("/ai/manager/api/price/plan", payload);
  },
  deleteBillingPlan(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/price/plan/${id}`);
  },

  // ========== Price Rule ==========
  getPriceRuleListByPlan(planId: number): Promise<PriceRule[]> {
    return request(`/ai/manager/api/price/rule/list_by_plan?plan_id=${planId}`);
  },
  updatePriceRule(id: number, payload: PriceRulePayload): Promise<void> {
    return request.put(`/ai/manager/api/price/rule/${id}`, payload);
  },
  createPriceRule(payload: PriceRulePayload): Promise<void> {
    return request.post("/ai/manager/api/price/rule", payload);
  },
  deletePriceRule(id: number): Promise<void> {
    return request.delete(`/ai/manager/api/price/rule/${id}`);
  },

  // ========== Request Log ==========
  getRecordList(
    params: Record<string, any>,
  ): Promise<PaginatedResponse<RecordListItem>> {
    // Filter undefined/null/empty values
    const validParams: Record<string, string> = {};
    for (const key in params) {
      if (Object.prototype.hasOwnProperty.call(params, key)) {
        const v = params[key];
        if (v !== undefined && v !== null && v !== "") {
          validParams[key] = String(v);
        }
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
  createProvider(payload: ProviderPayload): Promise<void> {
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
  getProviderRemoteModels(id: number | string): Promise<any> {
    return request.get(`/ai/manager/api/provider/${id}/remote_models`);
  },
  createProviderKey(
    id: number | string,
    payload: ProviderKeyPayload,
  ): Promise<void> {
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
    payload?: Record<string, any>,
  ): Promise<any> {
    return request.post(`/ai/manager/api/provider/${id}/check`, payload || {});
  },

  // ========== Model CRUD ==========
  createModel(payload: ModelPayload): Promise<void> {
    return request.post("/ai/manager/api/model", payload);
  },
  updateModel(id: number | string, payload: ModelPayload): Promise<void> {
    return request.put(`/ai/manager/api/model/${id}`, payload);
  },
  deleteModel(id: number | string): Promise<void> {
    return request.delete(`/ai/manager/api/model/${id}`);
  },
  getModelDetail(id: number | string): Promise<any> {
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
