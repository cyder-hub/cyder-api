import { request } from "./http";
import type {
  RuntimeFeatureCatalog,
  RuntimeFeatureConfigPayload,
  RuntimeFeatureConfigResponse,
} from "./types";

export function getRuntimeFeatureConfigCatalog(): Promise<RuntimeFeatureCatalog> {
  return request.get("/ai/manager/api/runtime_feature_config/catalog");
}

export function getProviderRuntimeFeatureConfig(
  providerId: number | string,
): Promise<RuntimeFeatureConfigResponse> {
  return request.get(
    `/ai/manager/api/provider/${providerId}/runtime_feature_config`,
  );
}

export function updateProviderRuntimeFeatureConfig(
  providerId: number | string,
  featureKey: string,
  payload: RuntimeFeatureConfigPayload,
): Promise<RuntimeFeatureConfigResponse> {
  return request.put(
    `/ai/manager/api/provider/${providerId}/runtime_feature_config/${featureKey}`,
    payload,
  );
}

export function deleteProviderRuntimeFeatureConfig(
  providerId: number | string,
  featureKey: string,
): Promise<void> {
  return request.delete(
    `/ai/manager/api/provider/${providerId}/runtime_feature_config/${featureKey}`,
  );
}

export function getModelRuntimeFeatureConfig(
  modelId: number | string,
): Promise<RuntimeFeatureConfigResponse> {
  return request.get(`/ai/manager/api/model/${modelId}/runtime_feature_config`);
}

export function updateModelRuntimeFeatureConfig(
  modelId: number | string,
  featureKey: string,
  payload: RuntimeFeatureConfigPayload,
): Promise<RuntimeFeatureConfigResponse> {
  return request.put(
    `/ai/manager/api/model/${modelId}/runtime_feature_config/${featureKey}`,
    payload,
  );
}

export function deleteModelRuntimeFeatureConfig(
  modelId: number | string,
  featureKey: string,
): Promise<void> {
  return request.delete(
    `/ai/manager/api/model/${modelId}/runtime_feature_config/${featureKey}`,
  );
}
