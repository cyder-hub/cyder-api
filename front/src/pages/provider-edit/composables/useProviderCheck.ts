import { computed, ref, watch, type Ref } from "vue";
import { useI18n } from "vue-i18n";

import * as providerService from "@/services/providers";
import { toastController } from "@/services/uiFeedback";
import type { ProviderCheckPayload } from "@/services/types";
import type { EditingProviderData } from "../types";
import {
  buildEnabledApiKeyOptions,
  buildEnabledModelOptions,
  buildEnabledSourceOptions,
  formatCheckSourceEvidence,
  modelAllowsSource,
  resolveAutomaticSource,
  type CheckDialogKind,
  type CheckOption,
} from "./providerCheckViewModel";

export function useProviderCheck(
  editingData: Ref<EditingProviderData | null>,
) {
  const { t: $t } = useI18n();

  const isCheckDialogOpen = ref(false);
  const checkDialogKind = ref<CheckDialogKind | null>(null);
  const checkDialogSourceValue = ref<string | null>(null);
  const checkDialogModelValue = ref<string | null>(null);
  const checkDialogApiKeyValue = ref<string | null>(null);
  const targetModelIndex = ref<number | null>(null);
  const targetApiKeyIndex = ref<number | null>(null);
  const targetSourceId = ref<number | null>(null);

  const selectedModelForCheck = computed(() => {
    const data = editingData.value;
    const kind = checkDialogKind.value;
    if (!data || !kind) return null;
    const modelIndex =
      kind === "model"
        ? targetModelIndex.value
        : checkDialogModelValue.value === null
          ? null
          : Number(checkDialogModelValue.value);
    return modelIndex !== null && Number.isInteger(modelIndex)
      ? data.models[modelIndex] ?? null
      : null;
  });

  const selectedSourceForModel = computed(() => {
    const kind = checkDialogKind.value;
    if (kind === "source") return targetSourceId.value;
    if (checkDialogSourceValue.value === null) return null;
    const sourceId = Number(checkDialogSourceValue.value);
    return Number.isInteger(sourceId) ? sourceId : null;
  });

  const checkDialogTargetLabel = computed(() => {
    const data = editingData.value;
    if (!data || !checkDialogKind.value) return "";
    if (checkDialogKind.value === "model" && targetModelIndex.value !== null) {
      return data.models[targetModelIndex.value]?.model_name || $t("providerEditPage.placeholderModelId");
    }
    if (checkDialogKind.value === "apiKey" && targetApiKeyIndex.value !== null) {
      const key = data.provider_keys[targetApiKeyIndex.value];
      return key?.description || $t("providerEditPage.alert.apiKeyNameFallback", {
        lastKeyChars: key?.key_last4 || "",
      });
    }
    if (checkDialogKind.value === "source" && targetSourceId.value !== null) {
      const source = data.upstream_sources.find((item) => item.id === targetSourceId.value);
      return source ? `${source.profile_type} · #${source.id}` : `#${targetSourceId.value}`;
    }
    return "";
  });

  const checkDialogSourceOptions = computed<CheckOption[]>(() =>
    buildEnabledSourceOptions(
      editingData.value?.upstream_sources ?? [],
      selectedModelForCheck.value,
    ),
  );
  const checkDialogModelOptions = computed<CheckOption[]>(() => {
    const sourceId = selectedSourceForModel.value;
    return buildEnabledModelOptions(
      editingData.value?.models ?? [],
      (model) => model.model_name || $t("providerEditPage.placeholderModelId"),
      sourceId,
    );
  });
  const checkDialogApiKeyOptions = computed<CheckOption[]>(() =>
    buildEnabledApiKeyOptions(
      editingData.value?.provider_keys ?? [],
      (key) =>
        key.description ||
        $t("providerEditPage.alert.apiKeyNameFallback", {
          lastKeyChars: key.key_last4,
        }),
    ),
  );

  const setTargetStatus = (
    kind: CheckDialogKind,
    index: number | null,
    status: "unchecked" | "checking" | "success" | "error",
    message?: string,
  ) => {
    const data = editingData.value;
    if (!data) return;
    if (kind === "model" && index !== null && data.models[index]) {
      data.models[index].checkStatus = status;
      data.models[index].checkMessage = message;
    }
    if (kind === "apiKey" && index !== null && data.provider_keys[index]) {
      data.provider_keys[index].checkStatus = status;
      data.provider_keys[index].checkMessage = message;
    }
  };

  const failTarget = (
    kind: CheckDialogKind,
    index: number | null,
    message: string,
  ) => {
    setTargetStatus(kind, index, "error", message);
    toastController.warn(message);
  };

  watch(
    [checkDialogSourceValue, checkDialogModelValue],
    () => {
      if (checkDialogKind.value !== "apiKey") return;
      const data = editingData.value;
      const sourceId = selectedSourceForModel.value;
      const model = selectedModelForCheck.value;
      if (!data || sourceId === null || !model) return;
      if (!modelAllowsSource(model, sourceId)) {
        checkDialogSourceValue.value = null;
      }
    },
  );

  const performCheck = async (
    kind: CheckDialogKind,
    modelIndex: number,
    apiKeyIndex: number,
    sourceId: number,
  ) => {
    const data = editingData.value;
    if (!data?.id) {
      toastController.warn($t("providerEditPage.alert.providerNotSavedForCheck"));
      return;
    }

    const model = data.models[modelIndex];
    const key = data.provider_keys[apiKeyIndex];
    if (!model || !key) return;

    const targetIndex = kind === "model" ? modelIndex : kind === "apiKey" ? apiKeyIndex : null;
    setTargetStatus(kind, targetIndex, "checking");
    const payload: ProviderCheckPayload = {
      ...(model.id
        ? { model_id: model.id }
        : { model_name: model.real_model_name || model.model_name }),
      provider_api_key_id: key.id,
    };

    try {
      const result = await providerService.checkProviderConnection(data.id, sourceId, payload);
      const evidence = formatCheckSourceEvidence(result);
      setTargetStatus(kind, targetIndex, "success", evidence);
      toastController.success($t("providerEditPage.alert.checkSuccess"), evidence);
    } catch (error) {
      const message = (error as Error).message || $t("common.unknownError");
      setTargetStatus(kind, targetIndex, "error", message);
      toastController.error($t("providerEditPage.alert.checkFailed", { error: message }));
    }
  };

  const closeDialog = () => {
    isCheckDialogOpen.value = false;
    checkDialogKind.value = null;
    checkDialogSourceValue.value = null;
    checkDialogModelValue.value = null;
    checkDialogApiKeyValue.value = null;
    targetModelIndex.value = null;
    targetApiKeyIndex.value = null;
    targetSourceId.value = null;
  };

  const openCheckFlow = (kind: CheckDialogKind, indexOrId: number) => {
    const data = editingData.value;
    if (!data?.id) {
      toastController.warn($t("providerEditPage.alert.providerNotSavedForCheck"));
      return;
    }

    targetModelIndex.value = kind === "model" ? indexOrId : null;
    targetApiKeyIndex.value = kind === "apiKey" ? indexOrId : null;
    targetSourceId.value = kind === "source" ? indexOrId : null;
    checkDialogKind.value = kind;
    checkDialogSourceValue.value = null;
    checkDialogModelValue.value = null;
    checkDialogApiKeyValue.value = null;

    if (kind === "apiKey") {
      const allModelOptions = buildEnabledModelOptions(
        data.models,
        (model) => model.model_name || $t("providerEditPage.placeholderModelId"),
      );
      if (allModelOptions.length === 0) {
        failTarget(kind, indexOrId, $t("providerEditPage.alert.noModelForCheck"));
        return;
      }
      if (allModelOptions.length === 1) {
        checkDialogModelValue.value = String(allModelOptions[0].value);
      }
    }

    const selectedModel =
      kind === "model" || kind === "apiKey"
        ? selectedModelForCheck.value
        : null;
    const sourceChoice =
      kind === "source"
        ? { status: "selected" as const, sourceId: indexOrId }
        : resolveAutomaticSource(data.upstream_sources, selectedModel);
    if (sourceChoice.status === "none") {
      failTarget(
        kind,
        kind === "model" ? indexOrId : kind === "apiKey" ? indexOrId : null,
        $t("providerEditPage.alert.noSourceForCheck"),
      );
      return;
    }
    if (kind !== "source" && sourceChoice.sourceId !== null) {
      checkDialogSourceValue.value = String(sourceChoice.sourceId);
    }

    if (kind === "source" || kind === "apiKey") {
      const modelOptions = checkDialogModelOptions.value;
      if (modelOptions.length === 0) {
        failTarget(
          kind,
          kind === "apiKey" ? indexOrId : null,
          $t("providerEditPage.alert.noModelForCheck"),
        );
        return;
      }
      if (kind === "source") {
        if (modelOptions.length === 1) checkDialogModelValue.value = String(modelOptions[0].value);
      } else if (modelOptions.length === 1) {
        checkDialogModelValue.value = String(modelOptions[0].value);
      }
    }

    if (kind === "apiKey" && checkDialogSourceValue.value !== null) {
      const compatibleModels = buildEnabledModelOptions(
        data.models,
        (model) => model.model_name || $t("providerEditPage.placeholderModelId"),
        Number(checkDialogSourceValue.value),
      );
      if (compatibleModels.length === 0) {
        checkDialogSourceValue.value = null;
      } else if (compatibleModels.length === 1) {
        checkDialogModelValue.value = String(compatibleModels[0].value);
      }
    }

    if (kind === "source" || kind === "model") {
      const apiKeyOptions = checkDialogApiKeyOptions.value;
      if (apiKeyOptions.length === 0) {
        failTarget(
          kind,
          kind === "model" ? indexOrId : null,
          $t("providerEditPage.alert.noApiKeyForCheck"),
        );
        return;
      }
      if (kind === "source") {
        if (apiKeyOptions.length === 1) checkDialogApiKeyValue.value = String(apiKeyOptions[0].value);
      } else if (apiKeyOptions.length === 1) {
        checkDialogApiKeyValue.value = String(apiKeyOptions[0].value);
      }
    }

    const sourceId =
      kind === "source"
        ? targetSourceId.value
        : checkDialogSourceValue.value === null
          ? null
          : Number(checkDialogSourceValue.value);
    const modelIndex =
      kind === "model"
        ? targetModelIndex.value
        : checkDialogModelValue.value === null
          ? null
          : Number(checkDialogModelValue.value);
    const apiKeyIndex =
      kind === "apiKey"
        ? targetApiKeyIndex.value
        : checkDialogApiKeyValue.value === null
          ? null
          : Number(checkDialogApiKeyValue.value);
    if (
      sourceId !== null &&
      modelIndex !== null &&
      apiKeyIndex !== null &&
      Number.isInteger(sourceId) &&
      Number.isInteger(modelIndex) &&
      Number.isInteger(apiKeyIndex)
    ) {
      void performCheck(kind, modelIndex, apiKeyIndex, sourceId);
      closeDialog();
      return;
    }

    isCheckDialogOpen.value = true;
  };

  const handleConfirmCheck = () => {
    const kind = checkDialogKind.value;
    if (!kind) return;
    const sourceId =
      kind === "source"
        ? targetSourceId.value
        : checkDialogSourceValue.value === null
          ? null
          : Number(checkDialogSourceValue.value);
    const modelIndex =
      kind === "model"
        ? targetModelIndex.value
        : checkDialogModelValue.value === null
          ? null
          : Number(checkDialogModelValue.value);
    const apiKeyIndex =
      kind === "apiKey"
        ? targetApiKeyIndex.value
        : checkDialogApiKeyValue.value === null
          ? null
          : Number(checkDialogApiKeyValue.value);
    if (
      sourceId === null || !Number.isInteger(sourceId) ||
      modelIndex === null || !Number.isInteger(modelIndex) ||
      apiKeyIndex === null || !Number.isInteger(apiKeyIndex)
    ) {
      return;
    }
    closeDialog();
    void performCheck(kind, modelIndex, apiKeyIndex, sourceId);
  };

  return {
    isCheckDialogOpen,
    checkDialogKind,
    checkDialogTargetLabel,
    checkDialogSourceOptions,
    checkDialogModelOptions,
    checkDialogApiKeyOptions,
    checkDialogSourceValue,
    checkDialogModelValue,
    checkDialogApiKeyValue,
    handleCheck: (type: "model" | "apiKey", index: number) => openCheckFlow(type, index),
    handleSourceCheck: (sourceId: number) => openCheckFlow("source", sourceId),
    handleConfirmCheck,
    closeCheckDialog: closeDialog,
  };
}
