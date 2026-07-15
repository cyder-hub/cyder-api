import { computed, onMounted, ref, watch, type Ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";

import * as modelService from "@/services/models";
import * as requestPatchService from "@/services/requestPatch";
import { normalizeError } from "@/utils/error";
import { toastController } from "@/services/uiFeedback";
import { useProviderStore } from "@/store/providerStore";
import { useModelStore } from "@/store/modelStore";
import { MODEL_CAPABILITY_ITEMS } from "@/pages/model/composables/useModelList";
import { useCostPage } from "@/pages/cost/composables/useCostPage";
import type { CostCatalogVersion, ModelDetailResponse } from "@/services/types";
import type { ReasoningConfigActions } from "@/components/reasoning/types";
import type { EditingModelData } from "../types";

export function useModelEdit(
  propsModelId?: Ref<number | null>,
  autoLoad = true,
) {
  const { t } = useI18n();
  const route = useRoute();
  const router = useRouter();
  const providerStore = useProviderStore();
  const modelStore = useModelStore();

  const modelId = computed(() =>
    propsModelId ? propsModelId.value : parseInt(route.params.id as string),
  );
  const isLoading = ref(true);
  const isSaving = ref(false);
  const modelDetail = ref<ModelDetailResponse | null>(null);
  const editingData = ref<EditingModelData | null>(null);
  const shouldBindCreatedCatalog = ref(false);
  const costManager = useCostPage();

  const capabilityItems = MODEL_CAPABILITY_ITEMS;
  let fetchSequence = 0;

  const reasoningActions: ReasoningConfigActions = {
    getCatalog: requestPatchService.getReasoningConfigCatalog,
    getConfig: requestPatchService.getModelReasoningConfig,
    previewSaved: requestPatchService.previewModelReasoningConfig,
    previewDraft: (ownerId, payload) =>
      requestPatchService.previewModelReasoningConfigDraft(
        ownerId,
        payload as import("@/services/types").ModelReasoningConfigPayload,
      ),
    updateConfig: (ownerId, payload) =>
      requestPatchService.updateModelReasoningConfig(
        ownerId,
        payload as import("@/services/types").ModelReasoningConfigPayload,
      ),
    deleteConfig: requestPatchService.deleteModelReasoningConfig,
  };

  const currentProvider = computed(() =>
    providerStore.getProviderById(editingData.value?.provider_id),
  );

  const selectedCatalog = computed(() =>
    costManager.catalogs.value.find(
      (item) => item.catalog.id === editingData.value?.cost_catalog_id,
    ) ?? null,
  );

  const selectedCatalogVersions = computed<CostCatalogVersion[]>(() =>
    selectedCatalog.value?.versions ?? [],
  );

  const fetchData = async () => {
    const requestedModelId = modelId.value;
    if (requestedModelId === null || Number.isNaN(requestedModelId)) {
      toastController.error(
        t("modelEditPage.alert.loadDataFailed", { modelId: route.params.id }),
      );
      isLoading.value = false;
      return;
    }

    const requestSequence = ++fetchSequence;
    try {
      isLoading.value = true;
      modelDetail.value = null;
      editingData.value = null;
      const [detail] = await Promise.all([
        modelService.getModelDetail(requestedModelId),
        providerStore.fetchProviders(),
        costManager.refreshCostData(),
      ]);

      if (
        requestSequence !== fetchSequence ||
        requestedModelId !== modelId.value
      ) {
        return;
      }

      modelDetail.value = detail;

      if (detail) {
        editingData.value = {
          id: detail.model.id,
          provider_id: detail.model.provider_id,
          cost_catalog_id: detail.model.cost_catalog_id ?? null,
          model_name: detail.model.model_name,
          real_model_name: detail.model.real_model_name ?? "",
          supports_streaming: detail.model.supports_streaming,
          supports_tools: detail.model.supports_tools,
          supports_reasoning: detail.model.supports_reasoning,
          supports_image_input: detail.model.supports_image_input,
          supports_embeddings: detail.model.supports_embeddings,
          supports_rerank: detail.model.supports_rerank,
          is_enabled: detail.model.is_enabled,
          request_patches: detail.request_patches || [],
        };
      }
    } catch (error: unknown) {
      if (
        requestSequence !== fetchSequence ||
        requestedModelId !== modelId.value
      ) {
        return;
      }

      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("modelEditPage.alert.loadDataFailed", { modelId: requestedModelId }),
        normalizedError.message,
      );
    } finally {
      if (requestSequence === fetchSequence) {
        isLoading.value = false;
      }
    }
  };

  const handleSaveModel = async (): Promise<boolean> => {
    if (!editingData.value || isSaving.value) return false;

    if (!editingData.value.model_name.trim()) {
      toastController.warn(t("modelEditPage.alert.nameRequired"));
      return false;
    }

    const payload = {
      model_name: editingData.value.model_name,
      real_model_name: editingData.value.real_model_name || null,
      supports_streaming: editingData.value.supports_streaming,
      supports_tools: editingData.value.supports_tools,
      supports_reasoning: editingData.value.supports_reasoning,
      supports_image_input: editingData.value.supports_image_input,
      supports_embeddings: editingData.value.supports_embeddings,
      supports_rerank: editingData.value.supports_rerank,
      is_enabled: editingData.value.is_enabled,
      cost_catalog_id: editingData.value.cost_catalog_id,
    };

    isSaving.value = true;
    try {
      await modelService.updateModel(editingData.value.id, payload);
      toastController.success(t("modelEditPage.alert.updateSuccess"));
      void providerStore.fetchProviders().catch((error) => {
        console.error("Failed to refresh providers after saving model:", error);
      });
      void fetchData();
      return true;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("modelEditPage.alert.saveFailed", {
          error: normalizedError.message,
        }),
      );
      return false;
    } finally {
      isSaving.value = false;
    }
  };

  const handleReasoningConfigSaved = () => {
    void Promise.all([
      providerStore.fetchProviders(),
      modelStore.fetchModels(),
    ]).catch((error) => {
      console.error("Failed to refresh stores after reasoning config save:", error);
    });
  };

  const handleRuntimeFeatureConfigSaved = () => {
    void Promise.all([
      providerStore.fetchProviders(),
      modelStore.fetchModels(),
    ]).catch((error) => {
      console.error("Failed to refresh stores after runtime feature config save:", error);
    });
  };

  const handleNavigateToModels = () => {
    void router.push("/model");
  };

  const handleNavigateToProviders = () => {
    void router.push("/provider");
  };

  const handleNavigateToRoutes = () => {
    void router.push("/model_route");
  };

  const handleOpenSelectedCostCatalog = () => {
    if (!editingData.value?.cost_catalog_id) {
      toastController.warn(t("costPage.alert.selectCatalogFirst"));
      return;
    }
    costManager.openCatalogWorkspace(editingData.value.cost_catalog_id);
  };

  const handleCreateCostCatalog = () => {
    shouldBindCreatedCatalog.value = true;
    costManager.openCreateCatalogDialog(true);
  };

  const handleDuplicateSelectedCostCatalog = async () => {
    if (!selectedCatalog.value || !editingData.value) {
      toastController.warn(t("costPage.alert.selectCatalogFirst"));
      return;
    }

    const duplicatedCatalogId = await costManager.duplicateCatalog(selectedCatalog.value);
    if (duplicatedCatalogId !== null) {
      editingData.value.cost_catalog_id = duplicatedCatalogId;
    }
  };

  const handleCostCatalogDialogOpenChange = (open: boolean) => {
    costManager.isCatalogDialogOpen.value = open;
    if (!open) {
      shouldBindCreatedCatalog.value = false;
    }
  };

  watch(
    () => costManager.selectedCatalogId.value,
    (catalogId) => {
      if (shouldBindCreatedCatalog.value && catalogId !== null && editingData.value) {
        editingData.value.cost_catalog_id = catalogId;
        shouldBindCreatedCatalog.value = false;
      }
    },
  );

  watch(modelId, (newVal, oldVal) => {
    if (autoLoad && newVal !== oldVal) {
      void fetchData();
    }
  });

  onMounted(() => {
    if (autoLoad) {
      void fetchData();
    }
  });

  return {
    modelId,
    isLoading,
    isSaving,
    modelDetail,
    editingData,
    costManager,
    capabilityItems,
    currentProvider,
    selectedCatalog,
    selectedCatalogVersions,
    reasoningActions,
    fetchData,
    handleSaveModel,
    handleReasoningConfigSaved,
    handleRuntimeFeatureConfigSaved,
    handleNavigateToModels,
    handleNavigateToProviders,
    handleNavigateToRoutes,
    handleOpenSelectedCostCatalog,
    handleCreateCostCatalog,
    handleDuplicateSelectedCostCatalog,
    handleCostCatalogDialogOpenChange,
  };
}
