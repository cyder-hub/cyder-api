import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";

import * as modelRouteService from "@/services/modelRoutes";
import { confirm, toastController } from "@/services/uiFeedback";
import type { ModelRouteListItem } from "@/services/types";
import { normalizeError } from "@/utils/error";
import type { ModelRouteSummaryCard } from "../types";

export function useModelRouteList() {
  const { t } = useI18n();

  const routes = ref<ModelRouteListItem[]>([]);
  const loading = ref(true);
  const error = ref<string | null>(null);
  const actionRouteId = ref<number | null>(null);

  const summaryCards = computed<ModelRouteSummaryCard[]>(() => {
    const total = routes.value.length;
    const enabled = routes.value.filter((item) => item.route.is_enabled).length;
    const exposed = routes.value.filter((item) => item.route.expose_in_models).length;
    const candidates = routes.value.reduce(
      (sum, item) => sum + item.candidate_count,
      0,
    );

    return [
      { key: "total", label: t("modelRoutePage.summary.total"), value: total },
      { key: "enabled", label: t("modelRoutePage.summary.enabled"), value: enabled },
      { key: "exposed", label: t("modelRoutePage.summary.exposed"), value: exposed },
      { key: "candidates", label: t("modelRoutePage.summary.candidates"), value: candidates },
    ];
  });

  const fetchRouteList = async () => {
    loading.value = true;
    error.value = null;
    try {
      routes.value = (await modelRouteService.getModelRouteList()) || [];
    } catch (err: unknown) {
      error.value = normalizeError(err, t("common.unknownError")).message;
    } finally {
      loading.value = false;
    }
  };

  const toggleEnabled = async (
    item: ModelRouteListItem,
    isEnabled: boolean,
  ) => {
    actionRouteId.value = item.route.id;
    try {
      await modelRouteService.updateModelRoute(item.route.id, {
        is_enabled: isEnabled,
      });
      await fetchRouteList();
    } catch (err: unknown) {
      console.error("Failed to toggle model route enabled state:", err);
      toastController.error(
        t("modelRoutePage.alert.toggleFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionRouteId.value = null;
    }
  };

  const toggleExpose = async (
    item: ModelRouteListItem,
    exposeInModels: boolean,
  ) => {
    actionRouteId.value = item.route.id;
    try {
      await modelRouteService.updateModelRoute(item.route.id, {
        expose_in_models: exposeInModels,
      });
      await fetchRouteList();
    } catch (err: unknown) {
      console.error("Failed to toggle model route exposure:", err);
      toastController.error(
        t("modelRoutePage.alert.toggleExposeFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionRouteId.value = null;
    }
  };

  const deleteRoute = async (item: ModelRouteListItem) => {
    if (
      !(await confirm({
        title: t("modelRoutePage.confirmDelete", {
          name: item.route.route_name,
        }),
      }))
    ) {
      return;
    }

    actionRouteId.value = item.route.id;
    try {
      await modelRouteService.deleteModelRoute(item.route.id);
      toastController.success(t("modelRoutePage.alert.deleteSuccess"));
      await fetchRouteList();
    } catch (err: unknown) {
      console.error("Failed to delete model route:", err);
      toastController.error(
        t("modelRoutePage.alert.deleteFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      actionRouteId.value = null;
    }
  };

  onMounted(() => {
    void fetchRouteList();
  });

  return {
    routes,
    loading,
    error,
    actionRouteId,
    summaryCards,
    fetchRouteList,
    toggleEnabled,
    toggleExpose,
    deleteRoute,
  };
}
