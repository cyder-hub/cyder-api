<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <h3 class="text-lg font-semibold text-gray-900">
          {{ $t("providerEditPage.sectionModels") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ $t("providerEditPage.sections.modelsDescription") }}
        </p>
      </div>
      <div class="flex flex-col gap-2 sm:w-auto sm:flex-row">
        <Button
          variant="outline"
          size="sm"
          class="w-full sm:w-auto"
          @click="emit('checkBatch')"
          :disabled="!editingData.id || editingData.models.length === 0"
        >
          <Check class="mr-1.5 h-4 w-4" />
          {{ $t("providerEditPage.alert.buttonCheckAll") }}
        </Button>
        <Button
          variant="outline"
          size="sm"
          class="w-full sm:w-auto"
          @click="handleFetchRemoteModels"
          :disabled="!editingData.id"
        >
          <CloudDownload class="mr-1.5 h-4 w-4" />
          {{ $t("providerEditPage.buttonFetchRemote") }}
        </Button>
      </div>
    </div>

    <div
      v-if="editingData.models.length === 0"
      class="flex flex-col items-center justify-center rounded-xl border border-dashed border-gray-200 py-10"
    >
      <Box class="mb-2 h-10 w-10 stroke-1 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">
        {{ $t("providerEditPage.alert.noModels") }}
      </span>
    </div>

    <div v-else class="space-y-3 md:hidden">
      <MobileCrudCard
        v-for="(model, index) in editingData.models"
        :key="index"
        :title="model.model_name || $t('providerEditPage.placeholderModelId')"
        :description="model.real_model_name || '-'"
      >
        <div class="space-y-3">
          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("providerEditPage.tableHeaderModelId") }}
            </Label>
            <Input
              v-model="model.model_name"
              :placeholder="$t('providerEditPage.placeholderModelId')"
              class="font-mono text-sm"
            />
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("providerEditPage.tableHeaderMappedModelId") }}
            </Label>
            <Input
              :model-value="model.real_model_name ?? ''"
              :placeholder="$t('providerEditPage.placeholderMappedModelId')"
              class="font-mono text-sm"
              @update:model-value="
                (v: string | number) => (model.real_model_name = String(v).trim() || null)
              "
            />
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <Label class="cursor-pointer text-gray-700">
              {{ $t("providerEditPage.labelEnabled") }}
            </Label>
            <Checkbox v-model="model.is_enabled" />
          </div>

          <div class="space-y-2">
            <Label class="text-gray-700">
              {{ $t("providerEditPage.modelCapabilities.title") }}
            </Label>
            <div class="grid grid-cols-1 gap-2 min-[360px]:grid-cols-2">
              <div
                v-for="capability in capabilityItems"
                :key="capability.key"
                class="flex items-center justify-between rounded-lg border border-gray-200 p-3"
              >
                <Label class="cursor-pointer text-xs text-gray-700">
                  {{ $t(capability.labelKey) }}
                </Label>
                <Checkbox
                  :model-value="model[capability.key]"
                  @update:model-value="
                    (value: boolean | 'indeterminate') =>
                      (model[capability.key] = value === true)
                  "
                />
              </div>
            </div>
          </div>
        </div>

        <template #header>
          <Badge variant="secondary" class="font-mono text-xs">
            {{ model.id ? "saved" : "draft" }}
          </Badge>
        </template>

        <template #actions>
          <div class="grid grid-cols-1 gap-2 min-[360px]:grid-cols-2">
            <Button
              variant="default"
              size="sm"
              class="w-full"
              @click="handleSaveSingleModel(index)"
            >
              {{
                model.id
                  ? $t("common.save")
                  : $t("providerEditPage.buttonSaveModel")
              }}
            </Button>
            <Button
              variant="outline"
              size="sm"
              class="w-full"
              :title="model.checkMessage"
              @click="emit('checkSingle', index)"
            >
              <Loader2
                v-if="model.checkStatus === 'checking'"
                class="h-4 w-4 animate-spin text-blue-500"
              />
              <AlertCircle
                v-else-if="model.checkStatus === 'error'"
                class="h-4 w-4 text-red-500"
              />
              <Check
                v-else-if="model.checkStatus === 'success'"
                class="h-4 w-4 text-green-500"
              />
              <Check v-else class="h-4 w-4" />
            </Button>
            <Button
              v-if="model.id"
              variant="outline"
              size="sm"
              class="w-full"
              @click="router.push(`/model/edit/${model.id}`)"
            >
              <Edit2 class="mr-1.5 h-4 w-4" />
              {{ $t("common.edit") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="w-full text-red-600 hover:bg-red-50 hover:text-red-700"
              @click="handleDeleteModel(index)"
            >
              <Trash2 class="mr-1.5 h-4 w-4" />
              {{ $t("common.delete") }}
            </Button>
          </div>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
      <div
        class="grid grid-cols-[1fr_1fr_2fr_auto_auto] items-center gap-4 border-b border-gray-200 bg-gray-50/80 px-4 py-3"
      >
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerEditPage.tableHeaderModelId") }}
        </span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerEditPage.tableHeaderMappedModelId") }}
        </span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerEditPage.modelCapabilities.title") }}
        </span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerEditPage.labelEnabled") }}
        </span>
        <span class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("common.actions") }}
        </span>
      </div>

      <div
        v-for="(model, index) in editingData.models"
        :key="`desktop-${index}`"
        class="grid grid-cols-[1fr_1fr_2fr_auto_auto] items-center gap-4 border-b border-gray-100 px-4 py-3 last:border-0 transition-colors hover:bg-gray-50/50"
      >
        <Input
          v-model="model.model_name"
          :placeholder="$t('providerEditPage.placeholderModelId')"
          class="h-8 font-mono text-sm"
        />
        <Input
          :model-value="model.real_model_name ?? ''"
          :placeholder="$t('providerEditPage.placeholderMappedModelId')"
          class="h-8 font-mono text-sm"
          @update:model-value="
            (v: string | number) => (model.real_model_name = String(v).trim() || null)
          "
        />
        <div class="grid grid-cols-2 gap-2 xl:grid-cols-3">
          <div
            v-for="capability in capabilityItems"
            :key="`desktop-${index}-${capability.key}`"
            class="flex items-center justify-between gap-2 rounded-md border border-gray-200 px-2.5 py-2"
          >
            <Label class="cursor-pointer text-[11px] text-gray-600">
              {{ $t(capability.labelKey) }}
            </Label>
            <Checkbox
              :model-value="model[capability.key]"
              @update:model-value="
                (value: boolean | 'indeterminate') =>
                  (model[capability.key] = value === true)
              "
            />
          </div>
        </div>
        <div class="flex items-center justify-start">
          <Checkbox v-model="model.is_enabled" />
        </div>
        <div class="flex items-center justify-end space-x-1">
          <Button variant="default" size="sm" class="h-8" @click="handleSaveSingleModel(index)">
            {{
              model.id
                ? $t("common.save")
                : $t("providerEditPage.buttonSaveModel")
            }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-600"
            :title="model.checkMessage"
            @click="emit('checkSingle', index)"
          >
            <Loader2
              v-if="model.checkStatus === 'checking'"
              class="h-4 w-4 animate-spin text-blue-500"
            />
            <AlertCircle
              v-else-if="model.checkStatus === 'error'"
              class="h-4 w-4 text-red-500"
            />
            <Check
              v-else-if="model.checkStatus === 'success'"
              class="h-4 w-4 text-green-500"
            />
            <Check v-else class="h-4 w-4" />
          </Button>
          <Button
            v-if="model.id"
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-600"
            @click="router.push(`/model/edit/${model.id}`)"
          >
            <Edit2 class="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-400 hover:text-red-600"
            @click="handleDeleteModel(index)"
          >
            <Trash2 class="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>

    <div class="flex flex-col gap-2 border-t border-gray-100 pt-2 sm:flex-row sm:items-center">
      <Button variant="outline" size="sm" class="w-full sm:w-auto" @click="addModel">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.buttonAddModel") }}
      </Button>
      <Button
        v-if="hasUncommittedModels"
        variant="outline"
        size="sm"
        class="w-full border-red-200 text-red-600 hover:bg-red-50 hover:text-red-700 sm:w-auto"
        @click="handleClearUncommittedModels"
      >
        <Trash2 class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.buttonClearUncommitted") }}
      </Button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import { Api } from "@/services/request";
import { toastController } from "@/lib/toastController";
import type { EditingProviderData, LocalEditableModelItem } from "./types";
import type {
  ProviderRemoteModelItem,
  ProviderRemoteModelsResponse,
} from "@/store/types";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Check,
  CloudDownload,
  Box,
  Loader2,
  AlertCircle,
  Edit2,
  Trash2,
  Plus,
} from "lucide-vue-next";

const { t: $t } = useI18n();
const router = useRouter();
const editingData = defineModel<EditingProviderData>("editingData", { required: true });

const capabilityItems = [
  { key: "supports_streaming", labelKey: "modelCapabilities.streaming" },
  { key: "supports_tools", labelKey: "modelCapabilities.tools" },
  { key: "supports_reasoning", labelKey: "modelCapabilities.reasoning" },
  { key: "supports_image_input", labelKey: "modelCapabilities.imageInput" },
  { key: "supports_embeddings", labelKey: "modelCapabilities.embeddings" },
  { key: "supports_rerank", labelKey: "modelCapabilities.rerank" },
] as const;

const emit = defineEmits<{
  (e: "checkSingle", index: number): void;
  (e: "checkBatch"): void;
}>();

const hasUncommittedModels = computed(() => {
  if (!editingData.value) return false;
  return editingData.value.models.some((m) => m.id === null);
});

const addModel = () => {
  editingData.value.models.push({
    id: null,
    model_name: "",
    real_model_name: null,
    supports_streaming: true,
    supports_tools: true,
    supports_reasoning: true,
    supports_image_input: true,
    supports_embeddings: true,
    supports_rerank: true,
    is_enabled: true,
    isEditing: false,
    checkStatus: "unchecked",
  });
};

const handleSaveSingleModel = async (index: number) => {
  const data = editingData.value;
  if (!data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForModel"));
    return;
  }

  const modelItem = data.models[index];
  if (!modelItem.model_name.trim()) {
    toastController.warn(
      $t("providerEditPage.alert.modelIdRequiredWithIndex", {
        index: index + 1,
      }),
    );
    return;
  }

  const payload = {
    model_name: modelItem.model_name.trim(),
    real_model_name: modelItem.real_model_name?.trim() || null,
    supports_streaming: modelItem.supports_streaming,
    supports_tools: modelItem.supports_tools,
    supports_reasoning: modelItem.supports_reasoning,
    supports_image_input: modelItem.supports_image_input,
    supports_embeddings: modelItem.supports_embeddings,
    supports_rerank: modelItem.supports_rerank,
    is_enabled: modelItem.is_enabled,
  };

  try {
    if (modelItem.id) {
      await Api.updateModel(modelItem.id, payload);
      toastController.success($t("providerEditPage.alert.modelUpdateSuccess"));
    } else {
      const savedModel = await Api.createModel({
        provider_id: data.id,
        ...payload,
      });
      modelItem.id = savedModel.id;
      modelItem.model_name = savedModel.model_name;
      modelItem.real_model_name = savedModel.real_model_name ?? null;
      modelItem.supports_streaming = savedModel.supports_streaming;
      modelItem.supports_tools = savedModel.supports_tools;
      modelItem.supports_reasoning = savedModel.supports_reasoning;
      modelItem.supports_image_input = savedModel.supports_image_input;
      modelItem.supports_embeddings = savedModel.supports_embeddings;
      modelItem.supports_rerank = savedModel.supports_rerank;
      modelItem.is_enabled = savedModel.is_enabled;
      toastController.success($t("providerEditPage.alert.modelSaveSuccess"));
    }
    modelItem.isEditing = false;
  } catch (error) {
    console.error("Failed to save model:", error);
    toastController.error(
      $t("providerEditPage.alert.saveModelFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  }
};

const handleDeleteModel = async (index: number) => {
  const data = editingData.value;
  const modelItem = data.models[index];

  if (modelItem.id) {
    if (!data.id) {
      toastController.warn(
        $t("providerEditPage.alert.providerNotSavedForModelDelete"),
      );
      return;
    }

    try {
      await Api.deleteModel(modelItem.id);
      data.models.splice(index, 1);
      toastController.success($t("providerEditPage.alert.modelDeleteSuccess"));
    } catch (error) {
      console.error("Failed to delete model:", error);
      toastController.error(
        $t("providerEditPage.alert.deleteModelFailed", {
          error: (error as Error).message || $t("common.unknownError"),
        }),
      );
    }
  } else {
    data.models.splice(index, 1);
  }
};

const handleFetchRemoteModels = async () => {
  const data = editingData.value;
  if (!data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForModel"));
    return;
  }

  try {
    const response = await Api.getProviderRemoteModels(data.id);

    let remoteModels: ProviderRemoteModelItem[] = [];
    let isGeminiLike = false;
    if (response) {
      const wrappedResponse = response as Exclude<
        ProviderRemoteModelsResponse,
        ProviderRemoteModelItem[]
      >;
      if (Array.isArray(wrappedResponse.data)) {
        remoteModels = wrappedResponse.data;
      } else if (Array.isArray(wrappedResponse.models)) {
        remoteModels = wrappedResponse.models;
        isGeminiLike = true;
      } else if (Array.isArray(response)) {
        remoteModels = response;
      }
    }

    if (!remoteModels || remoteModels.length === 0) {
      toastController.warn($t("providerEditPage.alert.noRemoteModels"));
      return;
    }

    const existingModelNames = new Set<string>();
    data.models.forEach((m) => {
      existingModelNames.add(m.model_name);
      if (m.real_model_name) existingModelNames.add(m.real_model_name);
    });

    const newModels: LocalEditableModelItem[] = [];
    remoteModels.forEach((item) => {
      let model_name = (item.id as string) || (item.name as string);
      const providerType = data.provider_type;
      const isGoogleOwned = item.owned_by === "google";
      const isGeminiProvider =
        providerType === "GEMINI" || providerType === "VERTEX";

      if (
        (isGeminiProvider || isGeminiLike || isGoogleOwned) &&
        model_name &&
        model_name.startsWith("models/")
      ) {
        model_name = model_name.substring("models/".length);
      }

      if (model_name && !existingModelNames.has(model_name)) {
        newModels.push({
          id: null,
          model_name,
          real_model_name: null,
          supports_streaming: true,
          supports_tools: true,
          supports_reasoning: true,
          supports_image_input: true,
          supports_embeddings: true,
          supports_rerank: true,
          is_enabled: true,
          isEditing: false,
          checkStatus: "unchecked",
        });
        existingModelNames.add(model_name);
      }
    });

    if (newModels.length > 0) {
      data.models.push(...newModels);
      toastController.success(
        $t("providerEditPage.alert.newModelsAdded", {
          count: newModels.length,
        }),
      );
    } else {
      toastController.info($t("providerEditPage.alert.noNewModels"));
    }
  } catch (error) {
    console.error("Failed to fetch remote models:", error);
    toastController.error(
      $t("providerEditPage.alert.fetchRemoteModelsFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  }
};

const handleClearUncommittedModels = () => {
  const originalCount = editingData.value.models.length;
  editingData.value.models = editingData.value.models.filter(
    (m) => m.id !== null,
  );
  if (editingData.value.models.length < originalCount) {
    toastController.info($t("providerEditPage.alert.uncommittedCleared"));
  } else {
    toastController.info($t("providerEditPage.alert.noUncommittedToClear"));
  }
};
</script>
