import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute } from "vue-router";

import * as providerService from "@/services/providers";
import { toastController } from "@/services/uiFeedback";
import type { ProviderListItem } from "@/services/types";
import type { EditingProviderData } from "../types";
import { createEmptyEditingProviderData } from "./providerEditState";
import { mapProviderApiKeySummary } from "./providerEditState";

export function useProviderEdit() {
  const { t } = useI18n();
  const route = useRoute();

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
          is_enabled: detail.provider.is_enabled,
          provider_api_key_mode: detail.provider.provider_api_key_mode,
          upstream_sources: detail.provider.upstream_sources.map((source) => ({
            id: source.id,
            provider_id: source.provider_id,
            profile_type: source.profile_type,
            endpoint: source.endpoint,
            use_proxy: source.use_proxy,
            is_enabled: source.is_enabled,
            is_default: source.is_default,
            deleted_at: source.deleted_at,
            created_at: source.created_at,
            updated_at: source.updated_at,
          })),
          models: detail.models.map((m) => ({
            id: m.model.id,
            model_name: m.model.model_name,
            real_model_name: m.model.real_model_name ?? null,
            source_config: m.source_config,
            is_enabled: m.model.is_enabled,
            isEditing: false,
            checkStatus: "unchecked" as const,
          })),
          provider_keys: detail.provider_keys
            .map(mapProviderApiKeySummary)
            .filter((key): key is NonNullable<typeof key> => key !== null),
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
    loadProvider,
  };
}
