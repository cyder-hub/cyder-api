import { ref, computed } from "vue";
import type { Ref } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { toastController } from "@/lib/toastController";
import type { EditingProviderData } from "@/components/provider/types";
import type { ProviderCheckPayload } from "@/store/types";

export function useProviderCheck(editingData: Ref<EditingProviderData | null>) {
  const { t: $t } = useI18n();

  // Modal states
  const isModelSelectModalOpen = ref(false);
  const isApiKeySelectModalOpen = ref(false);
  const apiKeyIndexToCheck = ref<number | null>(null);
  const modelIndexToUseStr = ref<string | null>(null);
  const isBatchCheckingApiKeys = ref(false);
  const modelIndexToCheck = ref<number | null>(null);
  const apiKeyIndexToUseStr = ref<string | null>(null);
  const isBatchCheckingModels = ref(false);

  // Computed options
  const modelOptionsForSelect = computed(() => {
    if (!editingData.value?.models) return [];
    return editingData.value.models.map((m, i) => ({
      value: i,
      label: m.model_name,
    }));
  });

  const apiKeyOptionsForSelect = computed(() => {
    if (!editingData.value?.provider_keys) return [];
    return editingData.value.provider_keys.map((k, i) => ({
      value: i,
      label:
        k.description ||
        $t("providerEditPage.alert.apiKeyNameFallback", {
          lastKeyChars: k.api_key.slice(-4),
        }),
    }));
  });

  const performCheck = async (modelIndex: number, apiKeyIndex: number) => {
    const data = editingData.value;
    if (!data || !data.id) {
      toastController.warn(
        $t("providerEditPage.alert.providerNotSavedForCheck"),
      );
      return;
    }

    data.models[modelIndex].checkStatus = "checking";
    data.models[modelIndex].checkMessage = undefined;
    data.provider_keys[apiKeyIndex].checkStatus = "checking";
    data.provider_keys[apiKeyIndex].checkMessage = undefined;

    const modelItem = data.models[modelIndex];
    const keyItem = data.provider_keys[apiKeyIndex];

    const payload: ProviderCheckPayload = {
      ...(modelItem.id
        ? { model_id: modelItem.id }
        : { model_name: modelItem.real_model_name || modelItem.model_name }),
      ...(keyItem.id
        ? { provider_api_key_id: keyItem.id }
        : { provider_api_key: keyItem.api_key }),
    };

    try {
      await Api.checkProviderConnection(data.id, payload);
      data.models[modelIndex].checkStatus = "success";
      data.provider_keys[apiKeyIndex].checkStatus = "success";
    } catch (error) {
      const errMsg = (error as Error).message || $t("common.unknownError");
      data.models[modelIndex].checkStatus = "error";
      data.models[modelIndex].checkMessage = errMsg;
      data.provider_keys[apiKeyIndex].checkStatus = "error";
      data.provider_keys[apiKeyIndex].checkMessage = errMsg;
    }
  };

  const performBatchModelCheck = async (apiKeyIndex: number) => {
    const data = editingData.value;
    if (!data || !data.id) return;

    const translatedType = $t("providerEditPage.alert.checkTypeModels");
    toastController.info(
      $t("providerEditPage.alert.batchChecking", { type: translatedType }),
    );

    const key = data.provider_keys[apiKeyIndex];
    data.models.forEach((m) => {
      m.checkStatus = "checking";
      m.checkMessage = undefined;
    });

    let successCount = 0;
    for (const [index, model] of data.models.entries()) {
      const payload: ProviderCheckPayload = {
        ...(model.id
          ? { model_id: model.id }
          : { model_name: model.real_model_name || model.model_name }),
        ...(key.id
          ? { provider_api_key_id: key.id }
          : { provider_api_key: key.api_key }),
      };
      try {
        await Api.checkProviderConnection(data.id!, payload);
        successCount++;
        data.models[index].checkStatus = "success";
      } catch (error) {
        const errMsg = (error as Error).message || $t("common.unknownError");
        data.models[index].checkStatus = "error";
        data.models[index].checkMessage = errMsg;
      }
    }
    toastController.info(
      $t("providerEditPage.alert.batchCheckComplete", {
        success: successCount,
        total: data.models.length,
        type: translatedType,
      }),
    );
  };

  const performBatchApiKeyCheck = async (modelIndex: number) => {
    const data = editingData.value;
    if (!data || !data.id) return;

    const translatedType = $t("providerEditPage.alert.checkTypeApiKeys");
    toastController.info(
      $t("providerEditPage.alert.batchChecking", { type: translatedType }),
    );

    const model = data.models[modelIndex];
    data.provider_keys.forEach((k) => {
      k.checkStatus = "checking";
      k.checkMessage = undefined;
    });

    let successCount = 0;
    for (const [index, key] of data.provider_keys.entries()) {
      const payload: ProviderCheckPayload = {
        ...(model.id
          ? { model_id: model.id }
          : { model_name: model.real_model_name || model.model_name }),
        ...(key.id
          ? { provider_api_key_id: key.id }
          : { provider_api_key: key.api_key }),
      };
      try {
        await Api.checkProviderConnection(data.id!, payload);
        successCount++;
        data.provider_keys[index].checkStatus = "success";
      } catch (error) {
        const errMsg = (error as Error).message || $t("common.unknownError");
        data.provider_keys[index].checkStatus = "error";
        data.provider_keys[index].checkMessage = errMsg;
      }
    }
    toastController.info(
      $t("providerEditPage.alert.batchCheckComplete", {
        success: successCount,
        total: data.provider_keys.length,
        type: translatedType,
      }),
    );
  };

  const handleCheck = async (type: "model" | "apiKey", index: number) => {
    const data = editingData.value;
    if (!data || !data.id) {
      toastController.warn(
        $t("providerEditPage.alert.providerNotSavedForCheck"),
      );
      return;
    }

    if (type === "model") {
      const apiKeys = data.provider_keys;
      if (apiKeys.length === 0) {
        toastController.warn($t("providerEditPage.alert.noApiKeyForCheck"));
        data.models[index].checkStatus = "error";
        data.models[index].checkMessage = $t(
          "providerEditPage.alert.noApiKeyForCheck",
        );
        return;
      }
      if (apiKeys.length === 1) {
        await performCheck(index, 0);
      } else {
        modelIndexToCheck.value = index;
        apiKeyIndexToUseStr.value = "0";
        isApiKeySelectModalOpen.value = true;
      }
    } else {
      const models = data.models;
      if (models.length === 0) {
        toastController.warn($t("providerEditPage.alert.noModelForCheck"));
        data.provider_keys[index].checkStatus = "error";
        data.provider_keys[index].checkMessage = $t(
          "providerEditPage.alert.noModelForCheck",
        );
        return;
      }
      if (models.length === 1) {
        await performCheck(0, index);
      } else {
        apiKeyIndexToCheck.value = index;
        modelIndexToUseStr.value = "0";
        isModelSelectModalOpen.value = true;
      }
    }
  };

  const handleBatchCheck = async (type: "models" | "api_keys") => {
    const data = editingData.value;
    if (!data || !data.id) {
      toastController.warn(
        $t("providerEditPage.alert.providerNotSavedForCheck"),
      );
      return;
    }

    if (type === "models") {
      if (data.models.length === 0) {
        toastController.info($t("providerEditPage.alert.noModelsToCheck"));
        return;
      }
      if (data.provider_keys.length === 0) {
        toastController.warn($t("providerEditPage.alert.noApiKeyForCheck"));
        return;
      }
      if (data.provider_keys.length === 1) {
        await performBatchModelCheck(0);
      } else {
        isBatchCheckingModels.value = true;
        apiKeyIndexToUseStr.value = "0";
        isApiKeySelectModalOpen.value = true;
      }
    } else {
      if (data.provider_keys.length === 0) {
        toastController.info($t("providerEditPage.alert.noApiKeysToCheck"));
        return;
      }
      if (data.models.length === 0) {
        toastController.warn($t("providerEditPage.alert.noModelForCheck"));
        return;
      }
      if (data.models.length === 1) {
        await performBatchApiKeyCheck(0);
      } else {
        isBatchCheckingApiKeys.value = true;
        modelIndexToUseStr.value = "0";
        isModelSelectModalOpen.value = true;
      }
    }
  };

  const handleConfirmModelSelection = () => {
    const akIndex = apiKeyIndexToCheck.value;
    const mIndex =
      modelIndexToUseStr.value !== null
        ? Number(modelIndexToUseStr.value)
        : null;

    isModelSelectModalOpen.value = false;

    if (mIndex !== null) {
      if (isBatchCheckingApiKeys.value) {
        performBatchApiKeyCheck(mIndex);
      } else if (akIndex !== null) {
        performCheck(mIndex, akIndex);
      }
    }

    apiKeyIndexToCheck.value = null;
    modelIndexToUseStr.value = null;
    isBatchCheckingApiKeys.value = false;
  };

  const handleConfirmApiKeySelection = () => {
    const mIndex = modelIndexToCheck.value;
    const akIndex =
      apiKeyIndexToUseStr.value !== null
        ? Number(apiKeyIndexToUseStr.value)
        : null;

    isApiKeySelectModalOpen.value = false;

    if (akIndex !== null) {
      if (isBatchCheckingModels.value) {
        performBatchModelCheck(akIndex);
      } else if (mIndex !== null) {
        performCheck(mIndex, akIndex);
      }
    }

    modelIndexToCheck.value = null;
    apiKeyIndexToUseStr.value = null;
    isBatchCheckingModels.value = false;
  };

  return {
    isModelSelectModalOpen,
    isApiKeySelectModalOpen,
    apiKeyIndexToCheck,
    modelIndexToUseStr,
    isBatchCheckingApiKeys,
    modelIndexToCheck,
    apiKeyIndexToUseStr,
    isBatchCheckingModels,
    modelOptionsForSelect,
    apiKeyOptionsForSelect,
    handleCheck,
    handleBatchCheck,
    handleConfirmModelSelection,
    handleConfirmApiKeySelection,
  };
}
