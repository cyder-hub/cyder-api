import { request } from "./http";
import {
  buildProviderRuntimeListQuery,
  buildProviderRuntimeSummaryQuery,
} from "./query";
import type {
  ProviderRuntimeItem,
  ProviderRuntimeListParams,
  ProviderRuntimeSummary,
} from "./types";

export function getProviderRuntimeList(
  params: ProviderRuntimeListParams = {},
): Promise<ProviderRuntimeItem[]> {
  const qs = buildProviderRuntimeListQuery(params);
  return request.get(`/ai/manager/api/provider/runtime/list${qs ? `?${qs}` : ""}`);
}

export function getProviderRuntimeSummary(
  window?: ProviderRuntimeListParams["window"],
): Promise<ProviderRuntimeSummary> {
  const qs = buildProviderRuntimeSummaryQuery(window);
  return request.get(
    `/ai/manager/api/provider/runtime/summary${qs ? `?${qs}` : ""}`,
  );
}
