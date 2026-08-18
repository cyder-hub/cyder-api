import { computed, onMounted, ref, watch, type Ref } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";

import * as modelService from "@/services/models";
import * as providerService from "@/services/providers";
import { normalizeError } from "@/utils/error";
import { toastController } from "@/services/uiFeedback";
import { useProviderStore } from "@/store/providerStore";
import { useModelStore } from "@/store/modelStore";
import { useCostPage } from "@/pages/cost/composables/useCostPage";
import type {
  CostCatalogVersion,
  ModelDetailResponse,
  ModelSourceConfigPayload,
  ModelSourceConfigSummary,
  ModelSourceExplain,
  UpstreamSource,
} from "@/services/types";
import type { EditingModelData } from "../types";
import {
  createSourceConfigDraft,
  sourceConfigPayloadEquals,
  toSourceConfigPayload,
  type SourceConfigDraft,
} from "@/components/model-source-config/sourceConfigViewModel";

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
  const providerSources = ref<UpstreamSource[]>([]);
  const sourceConfigSummary = ref<ModelSourceConfigSummary | null>(null);
  const sourceConfigDraft = ref<SourceConfigDraft>(createSourceConfigDraft());
  const sourceConfigOriginal = ref<ModelSourceConfigPayload>(
    toSourceConfigPayload(sourceConfigDraft.value),
  );
  const isSourceConfigSaving = ref(false);
  const sourceConfigError = ref<string | null>(null);
  const sourceConfigExplain = ref<ModelSourceExplain | null>(null);
  const shouldBindCreatedCatalog = ref(false);
  const costManager = useCostPage();

  let fetchSequence = 0;

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
        const providerDetail = await providerService.getProviderDetail(detail.model.provider_id);
        if (requestSequence !== fetchSequence || requestedModelId !== modelId.value) {
          return;
        }
        providerSources.value = providerDetail?.provider.upstream_sources ?? [];
        sourceConfigSummary.value = detail.source_config;
        sourceConfigDraft.value = createSourceConfigDraft(
          detail.source_config,
          providerSources.value,
        );
        sourceConfigOriginal.value = toSourceConfigPayload(sourceConfigDraft.value);
        sourceConfigError.value = null;
        sourceConfigExplain.value = null;
        editingData.value = {
          id: detail.model.id,
          provider_id: detail.model.provider_id,
          cost_catalog_id: detail.model.cost_catalog_id ?? null,
          model_name: detail.model.model_name,
          real_model_name: detail.model.real_model_name ?? "",
          model_kind: detail.model.model_kind,
          is_enabled: detail.model.is_enabled,
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

  const isSourceConfigDirty = computed(() =>
    !sourceConfigPayloadEquals(
      sourceConfigOriginal.value,
      toSourceConfigPayload(sourceConfigDraft.value),
    ),
  );

  const handleSaveSourceConfig = async (): Promise<boolean> => {
    if (!editingData.value || isSourceConfigSaving.value) return false;
    if (!isSourceConfigDirty.value) return true;

    isSourceConfigSaving.value = true;
    sourceConfigError.value = null;
    try {
      const summary = await modelService.updateModelSourceConfig(
        editingData.value.id,
        toSourceConfigPayload(sourceConfigDraft.value),
      );
      sourceConfigSummary.value = summary;
      sourceConfigDraft.value = createSourceConfigDraft(summary, providerSources.value);
      sourceConfigOriginal.value = toSourceConfigPayload(sourceConfigDraft.value);
      if (modelDetail.value) {
        modelDetail.value = { ...modelDetail.value, source_config: summary };
      }
      sourceConfigExplain.value = null;
      toastController.success(t("modelSourceConfig.alert.saveSuccess"));
      void Promise.all([providerStore.fetchProviders(), modelStore.fetchModels()]).catch(
        (error) => console.error("Failed to refresh stores after Source Config save:", error),
      );
      return true;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      sourceConfigError.value = normalizedError.message;
      toastController.error(
        t("modelSourceConfig.alert.saveFailed", { error: normalizedError.message }),
      );
      return false;
    } finally {
      isSourceConfigSaving.value = false;
    }
  };

  const handleExplainSourceConfig = async (): Promise<void> => {
    if (!editingData.value) return;
    sourceConfigError.value = null;
    try {
      sourceConfigExplain.value = await modelService.getModelSourceExplain(
        editingData.value.id,
      );
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      sourceConfigError.value = normalizedError.message;
      toastController.error(
        t("modelSourceConfig.alert.explainFailed", { error: normalizedError.message }),
      );
    }
  };

  const handleNavigateToModels = () => {
    void router.push("/model");
  };

  const handleNavigateToProviders = () => {
    void router.push("/provider");
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
    providerSources,
    sourceConfigSummary,
    sourceConfigDraft,
    isSourceConfigDirty,
    isSourceConfigSaving,
    sourceConfigError,
    sourceConfigExplain,
    costManager,
    currentProvider,
    selectedCatalog,
    selectedCatalogVersions,
    fetchData,
    handleSaveModel,
    handleSaveSourceConfig,
    handleExplainSourceConfig,
    handleNavigateToModels,
    handleNavigateToProviders,
    handleOpenSelectedCostCatalog,
    handleCreateCostCatalog,
    handleDuplicateSelectedCostCatalog,
    handleCostCatalogDialogOpenChange,
  };
}
