import { computed, ref, watch, type Ref } from "vue";
import { useI18n } from "vue-i18n";

import * as requestPatchService from "@/services/requestPatch";
import { toastController } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import type {
  ModelSourceConfigSummary,
  RequestPatchEvaluation,
  RequestPatchPreviewResponse,
  RequestPatchVariantAggregate,
  RequestPatchVariantInput,
  UpstreamSource,
} from "@/services/types";

export interface ModelRequestPatchSourceState {
  source: UpstreamSource;
  sourceVariants: RequestPatchVariantAggregate[];
  modelVariants: RequestPatchVariantAggregate[];
  evaluations: Record<string, RequestPatchEvaluation>;
}

export function useModelRequestPatch(
  modelId: Ref<number | null>,
  providerId: Ref<number | null>,
  sources: Ref<UpstreamSource[]>,
  sourceConfig: Ref<ModelSourceConfigSummary | null>,
) {
  const { t } = useI18n();
  const isLoading = ref(false);
  const isRefreshing = ref(false);
  const loadError = ref<string | null>(null);
  const overviewVariants = ref<RequestPatchVariantAggregate[]>([]);
  const sourceStates = ref<ModelRequestPatchSourceState[]>([]);

  const visibleSources = computed(() => {
    return sources.value.filter((source) => source.deleted_at === null);
  });

  const refresh = async () => {
    if (!modelId.value || !providerId.value) {
      sourceStates.value = [];
      overviewVariants.value = [];
      return;
    }

    loadError.value = null;
    if (sourceStates.value.length === 0) isLoading.value = true;
    else isRefreshing.value = true;

    try {
      const overview = await requestPatchService.listModelRequestPatchOverview(modelId.value);
      overviewVariants.value = overview.variants;
      const modelBySource = new Map<number, RequestPatchVariantAggregate[]>();
      for (const item of overview.variants) {
        const sourceId = item.variant.source_id;
        const list = modelBySource.get(sourceId) ?? [];
        list.push(item);
        modelBySource.set(sourceId, list);
      }

      sourceStates.value = await Promise.all(
        visibleSources.value.map(async (source) => {
          const [sourceList, modelList] = await Promise.all([
            requestPatchService.listSourceRequestPatchVariants(providerId.value!, source.id),
            requestPatchService.listModelSourceRequestPatchVariants(modelId.value!, source.id),
          ]);
          const modelVariants = modelBySource.get(source.id) ?? modelList.variants;
          const suffixes = new Set<string>();
          for (const item of [...sourceList.variants, ...modelVariants]) {
            if (item.variant.suffix !== null) suffixes.add(item.variant.suffix);
          }
          const evaluations: Record<string, RequestPatchEvaluation> = {};
          evaluations.base = (
            await requestPatchService.explainModelSourceRequestPatchVariants(
              modelId.value!,
              source.id,
            )
          ).evaluation;
          await Promise.all(
            [...suffixes].map(async (suffix) => {
              evaluations[suffix] = (
                await requestPatchService.explainModelSourceRequestPatchVariants(
                  modelId.value!,
                  source.id,
                  suffix,
                )
              ).evaluation;
            }),
          );
          return {
            source,
            sourceVariants: sourceList.variants,
            modelVariants,
            evaluations,
          };
        }),
      );
    } catch (error: unknown) {
      loadError.value = normalizeError(error, t("common.unknownError")).message;
    } finally {
      isLoading.value = false;
      isRefreshing.value = false;
    }
  };

  const previewVariant = (
    sourceId: number,
    payload: RequestPatchVariantInput & { variant_id?: number | null },
  ): Promise<RequestPatchPreviewResponse> =>
    requestPatchService.previewModelSourceRequestPatchVariant(modelId.value!, sourceId, payload);

  const saveVariant = async (
    sourceId: number,
    variantId: number | null,
    payload: RequestPatchVariantInput,
  ): Promise<boolean> => {
    try {
      if (variantId === null) {
        await requestPatchService.createModelSourceRequestPatchVariant(
          modelId.value!,
          sourceId,
          payload,
        );
        toastController.success(t("requestPatchVariant.alert.created"));
      } else {
        await requestPatchService.updateModelSourceRequestPatchVariant(
          modelId.value!,
          sourceId,
          variantId,
          payload,
        );
        toastController.success(t("requestPatchVariant.alert.updated"));
      }
      await refresh();
      return true;
    } catch (error: unknown) {
      toastController.error(
        t("requestPatchVariant.alert.saveFailed", {
          error: normalizeError(error, t("common.unknownError")).message,
        }),
      );
      return false;
    }
  };

  const deleteVariant = async (sourceId: number, variantId: number): Promise<boolean> => {
    try {
      await requestPatchService.deleteModelSourceRequestPatchVariant(
        modelId.value!,
        sourceId,
        variantId,
      );
      toastController.success(t("requestPatchVariant.alert.deleted"));
      await refresh();
      return true;
    } catch (error: unknown) {
      toastController.error(
        t("requestPatchVariant.alert.deleteFailed", {
          error: normalizeError(error, t("common.unknownError")).message,
        }),
      );
      return false;
    }
  };

  watch([modelId, providerId, sources, sourceConfig], () => {
    void refresh();
  }, { deep: true, immediate: true });

  return {
    isLoading,
    isRefreshing,
    loadError,
    overviewVariants,
    sourceStates,
    visibleSources,
    refresh,
    previewVariant,
    saveVariant,
    deleteVariant,
  };
}
