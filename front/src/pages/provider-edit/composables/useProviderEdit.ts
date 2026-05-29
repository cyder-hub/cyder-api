import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute } from "vue-router";

import * as providerService from "@/services/providers";
import * as requestPatchService from "@/services/requestPatch";
import { toastController } from "@/services/uiFeedback";
import { useProviderStore } from "@/store/providerStore";
import type { ProviderListItem } from "@/services/types";
import type { ReasoningConfigActions } from "@/components/reasoning/types";
import type { EditingProviderData } from "../types";
import { createEmptyEditingProviderData } from "./providerEditState";

export function useProviderEdit() {
  const { t } = useI18n();
  const route = useRoute();
  const providerStore = useProviderStore();

  const isLoading = ref(true);
  const errorMsg = ref<string | null>(null);
  const editingData = ref<EditingProviderData | null>(null);

  const providerId = computed(() => {
    const id = route.params.id;
    if (id) {
      const num = parseInt(id as string, 10);
      return isNaN(num) ? null : num;
    }
    return null;
  });

  const pageTitle = computed(() =>
    providerId.value || editingData.value?.id
      ? t("providerEditPage.titleEdit")
      : t("providerEditPage.titleAdd"),
  );

  const reasoningActions: ReasoningConfigActions = {
    getCatalog: requestPatchService.getReasoningConfigCatalog,
    getConfig: requestPatchService.getProviderReasoningConfig,
    previewSaved: requestPatchService.previewProviderReasoningConfig,
    previewDraft: requestPatchService.previewProviderReasoningConfigDraft,
    updateConfig: (ownerId, payload) =>
      requestPatchService.updateProviderReasoningConfig(
        ownerId,
        payload as import("@/services/types").ProviderReasoningConfigPayload,
      ),
    deleteConfig: requestPatchService.deleteProviderReasoningConfig,
  };

  const fetchProviderDetail = async (
    id: number,
  ): Promise<ProviderListItem | null> => {
    try {
      const response = await providerService.getProviderDetail(id);
      return response || null;
    } catch (error) {
      console.error(
        t("providerEditPage.alert.fetchDetailFailed", { providerId: id }),
        error,
      );
      toastController.error(
        t("providerEditPage.alert.fetchDetailFailed", { providerId: id }),
      );
      return null;
    }
  };

  const getEmptyProvider = (): EditingProviderData => ({
    ...createEmptyEditingProviderData(),
  });

  const handleReasoningConfigSaved = () => {
    void providerStore.fetchProviders().catch((error) => {
      console.error("Failed to refresh providers after reasoning config save:", error);
    });
  };

  const handleRuntimeFeatureConfigSaved = () => {
    void providerStore.fetchProviders().catch((error) => {
      console.error("Failed to refresh providers after runtime feature config save:", error);
    });
  };

  const loadProvider = async () => {
    isLoading.value = true;
    errorMsg.value = null;

    if (providerId.value) {
      const detail = await fetchProviderDetail(providerId.value);
      if (detail) {
        editingData.value = {
          id: detail.provider.id,
          name: detail.provider.name,
          provider_key: detail.provider.provider_key,
          provider_type: detail.provider.provider_type || "OPENAI",
          endpoint: detail.provider.endpoint,
          use_proxy: detail.provider.use_proxy,
          models: detail.models.map((m) => ({
            id: m.model.id,
            model_name: m.model.model_name,
            real_model_name: m.model.real_model_name ?? null,
            supports_streaming: m.model.supports_streaming,
            supports_tools: m.model.supports_tools,
            supports_reasoning: m.model.supports_reasoning,
            supports_image_input: m.model.supports_image_input,
            supports_embeddings: m.model.supports_embeddings,
            supports_rerank: m.model.supports_rerank,
            is_enabled: m.model.is_enabled,
            isEditing: false,
            checkStatus: "unchecked" as const,
          })),
          provider_keys: detail.provider_keys.map((k) => ({
            id: k.id,
            api_key: k.api_key,
            description: k.description ?? null,
            isEditing: false,
            checkStatus: "unchecked" as const,
          })),
          request_patches: detail.request_patches || [],
        };
      } else {
        errorMsg.value = t("providerEditPage.alert.loadDataFailed", {
          providerId: providerId.value,
        });
      }
    } else {
      editingData.value = getEmptyProvider();
    }

    isLoading.value = false;
  };

  onMounted(() => {
    void loadProvider();
  });

  return {
    providerId,
    isLoading,
    errorMsg,
    editingData,
    pageTitle,
    reasoningActions,
    handleReasoningConfigSaved,
    handleRuntimeFeatureConfigSaved,
    loadProvider,
  };
}
