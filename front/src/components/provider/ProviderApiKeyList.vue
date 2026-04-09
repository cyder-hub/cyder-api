<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <h3 class="text-lg font-semibold text-gray-900">
          {{ $t("providerEditPage.sectionApiKeys") }}
        </h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ editingData.provider_keys.length }} items
        </p>
      </div>
      <Button
        variant="outline"
        size="sm"
        class="w-full sm:w-auto"
        @click="emit('checkBatch')"
        :disabled="!editingData.id || editingData.provider_keys.length === 0"
      >
        <Check class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.alert.buttonCheckAll") }}
      </Button>
    </div>

    <div v-if="editingData.provider_keys.length === 0" class="flex flex-col items-center justify-center rounded-xl border border-dashed border-gray-200 py-10">
      <Key class="mb-2 h-10 w-10 stroke-1 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">{{ $t('providerEditPage.alert.noApiKeys') }}</span>
    </div>

    <div v-else class="space-y-3 md:hidden">
      <MobileCrudCard
        v-for="(keyItem, index) in editingData.provider_keys"
        :key="index"
        :title="keyItem.description || `API Key ${index + 1}`"
        :description="keyPreview(keyItem.api_key)"
      >
        <div class="space-y-3">
          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("providerEditPage.tableHeaderApiKey") }}
            </Label>
            <Input
              v-model="keyItem.api_key"
              :disabled="!!keyItem.id"
              :placeholder="$t('providerEditPage.placeholderApiKey')"
              :type="editingData.provider_type === 'VERTEX' || !!keyItem.id ? 'text' : 'password'"
              class="font-mono text-sm"
            />
          </div>
          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("providerEditPage.tableHeaderDescription") }}
            </Label>
            <Input
              :model-value="keyItem.description ?? ''"
              :disabled="!!keyItem.id && !keyItem.isEditing"
              :placeholder="$t('providerEditPage.placeholderDescription')"
              class="text-sm"
              @update:model-value="(v: string | number) => (keyItem.description = String(v) || null)"
            />
          </div>
        </div>

        <template #header>
          <Badge variant="secondary" class="font-mono text-xs">
            {{ keyItem.id ? (keyItem.isEditing ? "editing" : "saved") : "draft" }}
          </Badge>
        </template>

        <template #actions>
          <div class="grid grid-cols-1 gap-2 min-[360px]:grid-cols-2">
            <Button
              v-if="!keyItem.id && editingData.id"
              variant="default"
              size="sm"
              class="w-full"
              @click="handleSaveSingleApiKey(index)"
            >
              {{ $t("providerEditPage.buttonSaveThisKey") }}
            </Button>
            <Button
              v-if="keyItem.id && keyItem.isEditing"
              variant="default"
              size="sm"
              class="w-full"
              @click="handleSaveSingleApiKey(index)"
            >
              {{ $t("common.save") }}
            </Button>
            <Button
              variant="outline"
              size="sm"
              class="w-full"
              :title="keyItem.checkMessage"
              @click="emit('checkSingle', index)"
            >
              <Loader2 v-if="keyItem.checkStatus === 'checking'" class="h-4 w-4 animate-spin text-blue-500" />
              <AlertCircle v-else-if="keyItem.checkStatus === 'error'" class="h-4 w-4 text-red-500" />
              <Check v-else-if="keyItem.checkStatus === 'success'" class="h-4 w-4 text-green-500" />
              <Check v-else class="h-4 w-4" />
            </Button>
            <Button
              v-if="keyItem.id && !keyItem.isEditing"
              variant="outline"
              size="sm"
              class="w-full"
              @click="keyItem.isEditing = true"
            >
              <Edit2 class="mr-1.5 h-4 w-4" />
              {{ $t("common.edit") }}
            </Button>
            <Button
              v-if="keyItem.id && keyItem.isEditing"
              variant="ghost"
              size="sm"
              class="w-full"
              @click="keyItem.isEditing = false"
            >
              <X class="mr-1.5 h-4 w-4" />
              {{ $t("common.cancel") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="w-full text-red-600 hover:bg-red-50 hover:text-red-700"
              @click="handleDeleteApiKey(index)"
            >
              <Trash2 class="mr-1.5 h-4 w-4" />
              {{ $t("common.delete") }}
            </Button>
          </div>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
      <div class="grid grid-cols-[2fr_1fr_auto] gap-4 items-center border-b border-gray-200 bg-gray-50/80 px-4 py-3">
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t("providerEditPage.tableHeaderApiKey") }}</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t("providerEditPage.tableHeaderDescription") }}</span>
        <span class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t('common.actions') }}</span>
      </div>

      <div
        v-for="(keyItem, index) in editingData.provider_keys"
        :key="`desktop-${index}`"
        class="grid grid-cols-[2fr_1fr_auto] gap-4 items-center border-b border-gray-100 px-4 py-3 last:border-0 hover:bg-gray-50/50 transition-colors"
      >
        <Input
          v-model="keyItem.api_key"
          :disabled="!!keyItem.id"
          :placeholder="$t('providerEditPage.placeholderApiKey')"
          :type="editingData.provider_type === 'VERTEX' || !!keyItem.id ? 'text' : 'password'"
          class="h-8 font-mono text-sm"
        />
        <Input
          :model-value="keyItem.description ?? ''"
          :disabled="!!keyItem.id && !keyItem.isEditing"
          :placeholder="$t('providerEditPage.placeholderDescription')"
          class="h-8 text-sm"
          @update:model-value="(v: string | number) => (keyItem.description = String(v) || null)"
        />
        <div class="flex items-center justify-end space-x-1">
          <template v-if="!keyItem.id && editingData.id">
            <Button variant="default" size="sm" class="h-8" @click="handleSaveSingleApiKey(index)">
              {{ $t("providerEditPage.buttonSaveThisKey") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-8 px-2 text-gray-600"
              :title="keyItem.checkMessage"
              @click="emit('checkSingle', index)"
            >
              <Loader2 v-if="keyItem.checkStatus === 'checking'" class="h-4 w-4 animate-spin text-blue-500" />
              <AlertCircle v-else-if="keyItem.checkStatus === 'error'" class="h-4 w-4 text-red-500" />
              <Check v-else-if="keyItem.checkStatus === 'success'" class="h-4 w-4 text-green-500" />
              <Check v-else class="h-4 w-4" />
            </Button>
          </template>
          <template v-if="keyItem.id && !keyItem.isEditing">
            <Button
              variant="ghost"
              size="sm"
              class="h-8 px-2 text-gray-600"
              :title="keyItem.checkMessage"
              @click="emit('checkSingle', index)"
            >
              <Loader2 v-if="keyItem.checkStatus === 'checking'" class="h-4 w-4 animate-spin text-blue-500" />
              <AlertCircle v-else-if="keyItem.checkStatus === 'error'" class="h-4 w-4 text-red-500" />
              <Check v-else-if="keyItem.checkStatus === 'success'" class="h-4 w-4 text-green-500" />
              <Check v-else class="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-8 px-2 text-gray-600"
              @click="keyItem.isEditing = true"
            >
              <Edit2 class="h-4 w-4" />
            </Button>
          </template>
          <template v-if="keyItem.id && keyItem.isEditing">
            <Button variant="default" size="sm" class="h-8" @click="handleSaveSingleApiKey(index)">
              {{ $t("common.save") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-8 px-2 text-gray-600"
              @click="keyItem.isEditing = false"
            >
              <X class="h-4 w-4" />
            </Button>
          </template>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-400 hover:text-red-600"
            @click="handleDeleteApiKey(index)"
          >
            <Trash2 class="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>

    <div class="border-t border-gray-100 pt-2">
      <Button variant="outline" size="sm" class="w-full sm:w-auto" @click="addApiKey">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.buttonAddApiKey") }}
      </Button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { toastController } from "@/lib/toastController";
import type { EditingProviderData } from "./types";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Check, Key, Loader2, AlertCircle, Edit2, X, Trash2, Plus } from "lucide-vue-next";

const { t: $t } = useI18n();

const editingData = defineModel<EditingProviderData>("editingData", { required: true });

const emit = defineEmits<{
  (e: "checkSingle", index: number): void;
  (e: "checkBatch"): void;
}>();

const addApiKey = () => {
  editingData.value.provider_keys.push({
    id: null,
    api_key: "",
    description: null,
    isEditing: false,
    checkStatus: "unchecked",
  });
};

const keyPreview = (value: string) => {
  if (!value) return "-";
  if (value.length <= 16) return value;
  return `${value.slice(0, 6)}...${value.slice(-4)}`;
};

const handleSaveSingleApiKey = async (index: number) => {
  const data = editingData.value;
  if (!data.id) {
    toastController.warn(
      $t("providerEditPage.alert.providerNotSavedForApiKey"),
    );
    return;
  }

  const keyItem = data.provider_keys[index];
  if (!keyItem.api_key.trim()) {
    toastController.warn(
      $t("providerEditPage.alert.apiKeyRequiredWithIndex", {
        index: index + 1,
      }),
    );
    return;
  }

  if (data.provider_type === "VERTEX") {
    try {
      const parsedKey = JSON.parse(keyItem.api_key);
      const requiredFields = [
        "client_email",
        "private_key",
        "private_key_id",
        "token_uri",
      ];
      const missingFields = requiredFields.filter(
        (field) => !(field in parsedKey) || !parsedKey[field],
      );
      if (missingFields.length > 0) {
        toastController.warn(
          $t("providerEditPage.alert.vertexApiKeyMissingFields", {
            index: index + 1,
            fields: missingFields.join(", "),
          }),
        );
        return;
      }
    } catch {
      toastController.warn(
        $t("providerEditPage.alert.vertexApiKeyInvalidJson", {
          index: index + 1,
        }),
      );
      return;
    }
  }

  try {
    const savedKey = await Api.createProviderKey(data.id, {
      api_key: keyItem.api_key,
      description: keyItem.description,
    });
    keyItem.id = savedKey.id;
    keyItem.api_key = savedKey.api_key;
    keyItem.description = savedKey.description ?? null;
    keyItem.isEditing = false;
    toastController.success($t("providerEditPage.alert.apiKeySaveSuccess"));
  } catch (error) {
    console.error("Failed to save API key:", error);
    toastController.error(
      $t("providerEditPage.alert.saveApiKeyFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  }
};

const handleDeleteApiKey = async (index: number) => {
  const data = editingData.value;
  const keyItem = data.provider_keys[index];
  
  if (keyItem.id && data.id) {
    try {
      await Api.deleteProviderKey(data.id, keyItem.id);
      data.provider_keys.splice(index, 1);
      toastController.success($t("providerEditPage.alert.apiKeyDeleteSuccess"));
    } catch (error) {
      console.error("Failed to delete API key:", error);
      toastController.error(
        $t("providerEditPage.alert.deleteApiKeyFailed", {
          error: (error as Error).message || $t("common.unknownError"),
        }),
      );
    }
  } else {
    data.provider_keys.splice(index, 1);
  }
};
</script>
