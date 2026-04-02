<template>
  <div class="space-y-4 pt-4 border-t border-gray-100">
    <div class="flex justify-between items-center">
      <h3 class="text-lg font-semibold text-gray-900">
        {{ $t("providerEditPage.sectionApiKeys") }}
      </h3>
      <Button
        variant="outline"
        size="sm"
        @click="emit('checkBatch')"
        :disabled="!editingData.id || editingData.provider_keys.length === 0"
      >
        <Check class="h-4 w-4 mr-1.5" />
        {{ $t("providerEditPage.alert.buttonCheckAll") }}
      </Button>
    </div>

    <div class="border border-gray-200 rounded-lg overflow-hidden">
      <!-- Header -->
      <div class="grid grid-cols-[2fr_1fr_auto] gap-4 items-center px-4 py-3 bg-gray-50/80 border-b border-gray-200">
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">{{ $t("providerEditPage.tableHeaderApiKey") }}</span>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">{{ $t("providerEditPage.tableHeaderDescription") }}</span>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right">{{ $t('common.actions') }}</span>
      </div>

      <div v-if="editingData.provider_keys.length === 0" class="flex flex-col items-center justify-center py-10">
        <Key class="h-10 w-10 stroke-1 text-gray-400 mb-2" />
        <span class="text-sm font-medium text-gray-500">{{ $t('providerEditPage.alert.noApiKeys') }}</span>
      </div>

      <!-- API Key rows -->
      <div
        v-for="(keyItem, index) in editingData.provider_keys"
        :key="index"
        class="grid grid-cols-[2fr_1fr_auto] gap-4 items-center px-4 py-3 border-b border-gray-100 last:border-0 hover:bg-gray-50/50 transition-colors"
      >
        <Input
          v-model="keyItem.api_key"
          :disabled="!!keyItem.id"
          :placeholder="$t('providerEditPage.placeholderApiKey')"
          :type="
            editingData.provider_type === 'VERTEX' || !!keyItem.id
              ? 'text'
              : 'password'
          "
          class="font-mono text-sm h-8"
        />
        <Input
          :model-value="keyItem.description ?? ''"
          @update:model-value="(v: string | number) => (keyItem.description = String(v) || null)"
          :disabled="!!keyItem.id && !keyItem.isEditing"
          :placeholder="$t('providerEditPage.placeholderDescription')"
          class="text-sm h-8"
        />
        <div class="flex items-center space-x-1 justify-end">
          <template v-if="!keyItem.id && editingData.id">
            <Button variant="default" size="sm" class="h-8" @click="handleSaveSingleApiKey(index)">
              {{ $t("providerEditPage.buttonSaveThisKey") }}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-8 text-gray-600 px-2"
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
              class="h-8 text-gray-600 px-2"
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
              class="h-8 text-gray-600 px-2"
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
              class="h-8 text-gray-600 px-2"
              @click="keyItem.isEditing = false"
            >
              <X class="h-4 w-4" />
            </Button>
          </template>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 text-gray-400 hover:text-red-600 px-2"
            @click="handleDeleteApiKey(index)"
          >
            <Trash2 class="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
    <div class="pt-2">
      <Button variant="outline" size="sm" @click="addApiKey">
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("providerEditPage.buttonAddApiKey") }}
      </Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { toastController } from "@/lib/toastController";
import type { EditingProviderData } from "./types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
