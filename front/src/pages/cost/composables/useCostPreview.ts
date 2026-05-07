import type { ComputedRef, Ref } from "vue";
import { computed, reactive, ref, watch } from "vue";
import type {
  CostCatalogVersion,
  CostPreviewResponse,
  UsageNormalization,
} from "../../../services/types/cost";
import {
  createPreviewSample,
  normalizePreviewResponse,
  parseRequiredNonNegativeInteger,
} from "../helpers.ts";

export interface CostPreviewApiClient {
  previewCost: (payload: {
    catalog_version_id: number;
    normalization?: UsageNormalization;
  }) => Promise<CostPreviewResponse>;
}

export interface UseCostPreviewOptions {
  api: CostPreviewApiClient;
  selectedVersion: ComputedRef<CostCatalogVersion | null>;
  selectedVersionId: Ref<number | null>;
  warn: (message: string) => void;
  error: (title: string, description?: string) => void;
  t: (key: string) => string;
  normalizeError: (error: unknown) => string;
}

export type CostPreviewDraft = ReturnType<typeof createPreviewSample>;

export function buildCostPreviewNormalization(
  previewDraft: CostPreviewDraft,
): UsageNormalization {
  return {
    total_input_tokens: parseRequiredNonNegativeInteger(
      previewDraft.total_input_tokens,
      "total_input_tokens",
    ),
    total_output_tokens: parseRequiredNonNegativeInteger(
      previewDraft.total_output_tokens,
      "total_output_tokens",
    ),
    input_text_tokens: parseRequiredNonNegativeInteger(
      previewDraft.input_text_tokens,
      "input_text_tokens",
    ),
    output_text_tokens: parseRequiredNonNegativeInteger(
      previewDraft.output_text_tokens,
      "output_text_tokens",
    ),
    input_image_tokens: parseRequiredNonNegativeInteger(
      previewDraft.input_image_tokens,
      "input_image_tokens",
    ),
    output_image_tokens: parseRequiredNonNegativeInteger(
      previewDraft.output_image_tokens,
      "output_image_tokens",
    ),
    cache_read_tokens: parseRequiredNonNegativeInteger(
      previewDraft.cache_read_tokens,
      "cache_read_tokens",
    ),
    cache_write_tokens: parseRequiredNonNegativeInteger(
      previewDraft.cache_write_tokens,
      "cache_write_tokens",
    ),
    reasoning_tokens: parseRequiredNonNegativeInteger(
      previewDraft.reasoning_tokens,
      "reasoning_tokens",
    ),
    warnings: [],
  };
}

export function useCostPreview(options: UseCostPreviewOptions) {
  const previewDraft = reactive<CostPreviewDraft>(createPreviewSample());
  const previewResponse = ref<CostPreviewResponse | null>(null);
  const isRunningPreview = ref(false);
  const canPreview = computed(() => options.selectedVersion.value !== null);

  watch(
    () => options.selectedVersionId.value,
    () => {
      previewResponse.value = null;
    },
  );

  const applyPreviewSample = () => {
    Object.assign(previewDraft, createPreviewSample());
  };

  const resetPreview = () => {
    Object.assign(previewDraft, createPreviewSample());
    previewResponse.value = null;
  };

  const runPreview = async () => {
    if (!options.selectedVersion.value) {
      options.warn(options.t("costPage.alert.selectVersionFirst"));
      return;
    }

    let normalization: UsageNormalization;
    try {
      normalization = buildCostPreviewNormalization(previewDraft);
    } catch {
      options.warn(options.t("costPage.alert.invalidPreviewNumber"));
      return;
    }

    isRunningPreview.value = true;
    try {
      const response = await options.api.previewCost({
        catalog_version_id: options.selectedVersion.value.id,
        normalization,
      });
      previewResponse.value = normalizePreviewResponse(response);
    } catch (error: unknown) {
      options.error(
        options.t("costPage.alert.previewFailed"),
        options.normalizeError(error),
      );
    } finally {
      isRunningPreview.value = false;
    }
  };

  return {
    previewDraft,
    previewResponse,
    isRunningPreview,
    canPreview,
    applyPreviewSample,
    resetPreview,
    runPreview,
  };
}
