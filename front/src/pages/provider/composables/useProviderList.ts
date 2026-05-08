import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import { confirm, toastController } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import * as providerRuntimeService from "@/services/providerRuntime";
import * as providerService from "@/services/providers";
import { useProviderStore } from "@/store/providerStore";
import type { ProviderRuntimeLevel, ProviderSummaryItem } from "@/services/types";
import type { ProviderRuntimeLevelMap, ProviderSummaryCard } from "../types";

export function useProviderList() {
  const { t } = useI18n();
  const store = useProviderStore();

  const isLoading = ref(true);
  const error = ref<string | null>(null);
  const providerRuntimeLevelMap = ref<ProviderRuntimeLevelMap>({});

  const providers = computed(() => store.providers);

  const summaryCards = computed<ProviderSummaryCard[]>(() => {
    const total = store.providers.length;
    const enabled = store.providers.filter((item) => item.is_enabled).length;
    const disabled = total - enabled;
    const runtimeIssues = Object.values(providerRuntimeLevelMap.value).filter(
      (level) => level === "open" || level === "half_open" || level === "degraded",
    ).length;

    return [
      { key: "total", label: t("providerPage.summary.total"), value: total },
      { key: "enabled", label: t("providerPage.summary.enabled"), value: enabled },
      { key: "disabled", label: t("providerPage.summary.disabled"), value: disabled },
      { key: "runtime", label: t("providerPage.summary.runtimeIssues"), value: runtimeIssues },
    ];
  });

  const runtimeLevelLabel = (level: ProviderRuntimeLevel) =>
    t(`providerRuntimePage.status.${level}`);

  const runtimeBadgeClass = (level: ProviderRuntimeLevel) => {
    switch (level) {
      case "open":
        return "border-red-200 bg-red-50 text-red-700 hover:bg-red-50";
      case "half_open":
        return "border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-50";
      case "degraded":
        return "border-orange-200 bg-orange-50 text-orange-700 hover:bg-orange-50";
      case "healthy":
        return "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-50";
      case "no_traffic":
        return "border-gray-200 bg-gray-100 text-gray-600 hover:bg-gray-100";
    }
  };

  const providerStateLabel = (provider: ProviderSummaryItem) =>
    provider.is_enabled ? t("providerPage.state.enabled") : t("providerPage.state.disabled");

  const providerStateClass = (provider: ProviderSummaryItem) =>
    provider.is_enabled
      ? "border-emerald-200 bg-emerald-50 text-emerald-700"
      : "border-gray-200 bg-gray-100 text-gray-500";

  const loadRuntimeLevels = async () => {
    try {
      const runtimeItems = await providerRuntimeService.getProviderRuntimeList({
        window: "1h",
        only_enabled: false,
      });
      providerRuntimeLevelMap.value = Object.fromEntries(
        runtimeItems.map((item) => [item.provider_id, item.runtime_level]),
      );
    } catch (err) {
      console.error("Failed to fetch provider runtime levels:", err);
      providerRuntimeLevelMap.value = {};
    }
  };

  const loadData = async () => {
    isLoading.value = true;
    error.value = null;
    try {
      await store.fetchProviders();
      await loadRuntimeLevels();
    } catch (err: unknown) {
      error.value = normalizeError(err, t("common.unknownError")).message;
    } finally {
      isLoading.value = false;
    }
  };

  const deleteProvider = async (provider: ProviderSummaryItem) => {
    await confirm(t("providerPage.confirmDelete", { name: provider.name }));
    try {
      await providerService.deleteProvider(provider.id);
      toastController.success(t("providerPage.deleteSuccess"));
      await loadData();
    } catch (err: unknown) {
      console.error("Failed to delete provider:", err);
      const errorMessage = normalizeError(err, t("common.unknownError")).message;
      toastController.error(t("providerPage.deleteFailed", { error: errorMessage }));
    }
  };

  onMounted(() => {
    void loadData();
  });

  return {
    providers,
    isLoading,
    error,
    providerRuntimeLevelMap,
    summaryCards,
    runtimeLevelLabel,
    runtimeBadgeClass,
    providerStateLabel,
    providerStateClass,
    loadData,
    deleteProvider,
  };
}
