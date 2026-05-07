import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { useModelStore } from "@/store/modelStore";
import type { ModelCapabilityItem, ModelSummaryCard } from "../types";
import { buildModelPageState } from "./modelViewModel";

export const MODEL_CAPABILITY_ITEMS: ModelCapabilityItem[] = [
  { key: "supports_streaming", labelKey: "modelCapabilities.streaming" },
  { key: "supports_tools", labelKey: "modelCapabilities.tools" },
  { key: "supports_reasoning", labelKey: "modelCapabilities.reasoning" },
  { key: "supports_image_input", labelKey: "modelCapabilities.imageInput" },
  { key: "supports_embeddings", labelKey: "modelCapabilities.embeddings" },
  { key: "supports_rerank", labelKey: "modelCapabilities.rerank" },
];

export { buildModelPageState } from "./modelViewModel";

export function useModelList() {
  const { t } = useI18n();
  const modelStore = useModelStore();
  const query = ref("");

  const modelPageState = computed(() =>
    buildModelPageState(modelStore.models, query.value),
  );

  const summaryCards = computed<ModelSummaryCard[]>(() => {
    const total = modelStore.models.length;
    const enabled = modelStore.models.filter((item) => item.is_enabled).length;
    const providers = new Set(modelStore.models.map((item) => item.provider_id)).size;
    const mapped = modelStore.models.filter((item) => item.real_model_name).length;

    return [
      { key: "total", label: t("modelPage.summary.total"), value: total },
      { key: "enabled", label: t("modelPage.summary.enabled"), value: enabled },
      { key: "providers", label: t("modelPage.summary.providers"), value: providers },
      { key: "mapped", label: t("modelPage.summary.mapped"), value: mapped },
    ];
  });

  const loadData = async () => {
    try {
      await modelStore.fetchModels();
    } catch (err: unknown) {
      console.error("Failed to load model summaries:", err);
    }
  };

  onMounted(() => {
    void loadData();
  });

  return {
    query,
    modelStore,
    modelPageState,
    summaryCards,
    capabilityItems: MODEL_CAPABILITY_ITEMS,
    loadData,
  };
}
