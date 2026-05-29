import type {
  RuntimeFeatureCatalog,
  RuntimeFeatureConfigFeature,
  RuntimeFeatureConfigResponse,
} from "@/services/types";

export type RuntimeFeatureDrafts = Record<string, boolean>;

export function createRuntimeFeatureDrafts(
  catalog: RuntimeFeatureCatalog | null,
  config: RuntimeFeatureConfigResponse | null,
): RuntimeFeatureDrafts {
  const responseByKey = new Map(
    (config?.features ?? []).map((feature) => [feature.feature_key, feature]),
  );

  return Object.fromEntries(
    (catalog?.features ?? []).map((feature) => {
      const response = responseByKey.get(feature.feature_key);
      return [
        feature.feature_key,
        response?.owner_config?.enabled ??
          response?.effective_enabled ??
          feature.default_enabled,
      ];
    }),
  );
}

export function runtimeFeatureDraftSnapshot(
  drafts: RuntimeFeatureDrafts,
): string {
  return JSON.stringify(
    Object.entries(drafts).sort(([left], [right]) =>
      left.localeCompare(right),
    ),
  );
}

export function findRuntimeFeatureResponse(
  config: RuntimeFeatureConfigResponse | null,
  featureKey: string,
): RuntimeFeatureConfigFeature | null {
  return (
    config?.features.find((feature) => feature.feature_key === featureKey) ??
    null
  );
}
