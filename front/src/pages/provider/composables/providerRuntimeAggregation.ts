import type {
  ProviderRuntimeItem,
  ProviderRuntimeLevel,
} from "@/services/types";
import type { ProviderRuntimeLevelMap } from "../types";

const runtimeLevelPriority: Record<ProviderRuntimeLevel, number> = {
  open: 5,
  half_open: 4,
  degraded: 3,
  healthy: 2,
  no_traffic: 1,
};

export function aggregateProviderRuntimeLevels(
  runtimeItems: Pick<ProviderRuntimeItem, "provider_id" | "runtime_level">[],
): ProviderRuntimeLevelMap {
  return runtimeItems.reduce<ProviderRuntimeLevelMap>((levels, item) => {
    const current = levels[item.provider_id];
    if (
      current === undefined ||
      runtimeLevelPriority[item.runtime_level] > runtimeLevelPriority[current]
    ) {
      levels[item.provider_id] = item.runtime_level;
    }
    return levels;
  }, {});
}
