import { ref } from "vue";
import { useI18n } from "vue-i18n";

import * as modelRouteService from "@/services/modelRoutes";
import { toastController } from "@/services/uiFeedback";
import { useModelStore } from "@/store/modelStore";
import { useProviderStore } from "@/store/providerStore";
import { normalizeError } from "@/utils/error";
import type { EditingRoute } from "../types";
import {
  buildModelRoutePayload,
  createModelRouteTemplate,
  mapModelRouteDetailToEditingRoute,
  validateModelRouteEditor,
} from "./modelRouteQueue";

interface UseModelRouteEditorOptions {
  afterSave: () => Promise<void>;
}

export function useModelRouteEditor(options: UseModelRouteEditorOptions) {
  const { t } = useI18n();
  const providerStore = useProviderStore();
  const modelStore = useModelStore();

  const showEditModal = ref(false);
  const editingRoute = ref<EditingRoute>(createModelRouteTemplate());
  const isSaving = ref(false);

  const ensureDependencies = async () => {
    await Promise.all([
      providerStore.providers.length > 0
        ? Promise.resolve(providerStore.providers)
        : providerStore.fetchProviders(),
      modelStore.models.length > 0
        ? Promise.resolve(modelStore.models)
        : modelStore.fetchModels(),
    ]);
  };

  const setShowEditModal = (value: boolean) => {
    showEditModal.value = value;
  };

  const closeEditor = () => {
    showEditModal.value = false;
  };

  const openAddModal = async () => {
    try {
      await ensureDependencies();
      editingRoute.value = createModelRouteTemplate();
      showEditModal.value = true;
    } catch (err: unknown) {
      toastController.error(
        normalizeError(err, t("common.unknownError")).message,
      );
    }
  };

  const openEditModal = async (id: number) => {
    try {
      const [, detail] = await Promise.all([
        ensureDependencies(),
        modelRouteService.getModelRouteDetail(id),
      ]);
      editingRoute.value = mapModelRouteDetailToEditingRoute(detail);
      showEditModal.value = true;
    } catch (err: unknown) {
      console.error("Failed to fetch model route detail:", err);
      toastController.error(t("modelRoutePage.alert.loadDetailFailed"));
    }
  };

  const validateEditor = () => {
    const result = validateModelRouteEditor(editingRoute.value);
    if (result.valid) {
      return true;
    }

    switch (result.issue) {
      case "route_name_required":
        toastController.error(t("modelRoutePage.alert.routeNameRequired"));
        break;
      case "candidate_required":
        toastController.error(t("modelRoutePage.alert.candidateRequired"));
        break;
      case "candidate_model_required":
        toastController.error(t("modelRoutePage.alert.candidateModelRequired"));
        break;
      case "duplicate_candidate":
        toastController.error(t("modelRoutePage.alert.duplicateCandidate"));
        break;
      default:
        toastController.error(t("common.unknownError"));
        break;
    }

    return false;
  };

  const saveRoute = async () => {
    if (!validateEditor()) {
      return;
    }

    const payload = buildModelRoutePayload(editingRoute.value);
    isSaving.value = true;
    try {
      if (editingRoute.value.id) {
        await modelRouteService.updateModelRoute(editingRoute.value.id, payload);
      } else {
        await modelRouteService.createModelRoute(payload);
      }
      toastController.success(t("modelRoutePage.alert.saveSuccess"));
      showEditModal.value = false;
      await options.afterSave();
    } catch (err: unknown) {
      console.error("Failed to save model route:", err);
      toastController.error(
        t("modelRoutePage.alert.saveFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      isSaving.value = false;
    }
  };

  return {
    showEditModal,
    editingRoute,
    isSaving,
    setShowEditModal,
    closeEditor,
    openAddModal,
    openEditModal,
    saveRoute,
  };
}
