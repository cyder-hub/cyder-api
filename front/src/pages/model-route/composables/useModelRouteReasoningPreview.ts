import { ref } from "vue";
import { useI18n } from "vue-i18n";

import * as modelRouteService from "@/services/modelRoutes";
import type {
  ReasoningRouteCandidatePreview,
  ReasoningRoutePreview,
} from "@/services/types";
import { normalizeError } from "@/utils/error";

export function formatRouteReasoningConfigSource(
  candidate: ReasoningRouteCandidatePreview,
) {
  if (candidate.runtime_status === "stale_skipped") {
    return "stale skipped";
  }

  switch (candidate.config_source) {
    case "provider_default":
      return "provider default";
    case "model_custom":
      return "model custom";
    case "model_disabled":
      return "model disabled";
    case "missing":
    case null:
    case undefined:
      return "missing config";
    default:
      return candidate.config_source;
  }
}

export function useModelRouteReasoningPreview() {
  const { t } = useI18n();

  const showReasoningPreviewModal = ref(false);
  const reasoningPreview = ref<ReasoningRoutePreview | null>(null);
  const isReasoningPreviewLoading = ref(false);
  const reasoningPreviewError = ref<string | null>(null);

  const openReasoningPreview = async (id: number) => {
    showReasoningPreviewModal.value = true;
    isReasoningPreviewLoading.value = true;
    reasoningPreviewError.value = null;
    reasoningPreview.value = null;

    try {
      reasoningPreview.value =
        await modelRouteService.getModelRouteReasoningPreview(id);
    } catch (err: unknown) {
      reasoningPreviewError.value = normalizeError(
        err,
        t("common.unknownError"),
      ).message;
    } finally {
      isReasoningPreviewLoading.value = false;
    }
  };

  const setShowReasoningPreviewModal = (value: boolean) => {
    showReasoningPreviewModal.value = value;
  };

  return {
    showReasoningPreviewModal,
    reasoningPreview,
    isReasoningPreviewLoading,
    reasoningPreviewError,
    openReasoningPreview,
    setShowReasoningPreviewModal,
    formatRouteReasoningConfigSource,
  };
}
