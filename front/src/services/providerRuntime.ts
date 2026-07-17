import { request } from "./http";
import { buildProviderRuntimeSnapshotQuery } from "./query";
import type {
  ProviderRuntimeListParams,
  ProviderRuntimeSnapshot,
} from "./types";

export function getProviderRuntimeSnapshot(
  params: ProviderRuntimeListParams = {},
): Promise<ProviderRuntimeSnapshot> {
  const qs = buildProviderRuntimeSnapshotQuery(params);
  return request.get(
    `/ai/manager/api/provider/runtime/snapshot${qs ? `?${qs}` : ""}`,
  );
}
