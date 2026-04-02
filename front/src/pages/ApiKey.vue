<template>
  <CrudPageLayout
    :title="$t('apiKeyPage.title')"
    :description="$t('apiKeyPage.description') || $t('apiKeyPage.title')"
    :loading="loading"
    :error="error"
    :empty="!apiKeyStore.apiKeys.length"
    header-class="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4"
  >
    <template #actions>
      <Button variant="outline" @click="handleStartEditing()">
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("apiKeyPage.addApiKey") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex flex-col items-center justify-center py-20">
        <Loader2 class="h-5 w-5 animate-spin text-gray-400 mb-2" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("apiKeyPage.loading")
        }}</span>
      </div>
    </template>

    <template #error="{ error }">
      <div class="flex flex-col items-center justify-center py-20">
        <div
          class="text-red-600 bg-red-50 border border-red-200 rounded-lg p-4 max-w-lg text-sm"
        >
          {{ $t("apiKeyPage.errorPrefix") }} {{ error }}
        </div>
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20">
        <KeyRound class="h-10 w-10 stroke-1 text-gray-400 mb-4" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("apiKeyPage.noData")
        }}</span>
      </div>
    </template>

    <div class="border border-gray-200 rounded-lg overflow-hidden">
      <Table>
        <TableHeader class="bg-gray-50/80 hover:bg-gray-50/80">
          <TableRow>
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.name") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.apiKeyPartial") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.description") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.enabled") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.accessControlPolicy") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.createdAt") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("apiKeyPage.table.updatedAt") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
              >{{ $t("apiKeyPage.table.actions") }}</TableHead
            >
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="key in apiKeyStore.apiKeys" :key="key.id">
            <TableCell class="font-medium text-gray-900">{{
              key.name
            }}</TableCell>
            <TableCell>
              <div class="flex items-center space-x-2">
                <span class="font-mono text-xs text-gray-600">{{
                  key.api_key
                    ? `${key.api_key.substring(0, 3)}...${key.api_key.substring(key.api_key.length - 4)}`
                    : "N/A"
                }}</span>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-6 w-6 p-0 text-gray-400 hover:text-gray-900"
                  @click="copyApiKeyToClipboard(key.api_key, key.id)"
                  :title="$t('apiKeyPage.copy')"
                >
                  <Check
                    v-if="copiedKeyId === key.id"
                    class="h-3 w-3 text-green-600"
                  />
                  <Copy v-else class="h-3 w-3" />
                </Button>
              </div>
            </TableCell>
            <TableCell
              class="max-w-xs truncate text-gray-500"
              :title="key.description || ''"
              >{{ key.description || "/" }}</TableCell
            >
            <TableCell>
              <Checkbox
                :checked="key.is_enabled"
                @update:checked="(val: boolean) => handleToggleEnable(key, val)"
              />
            </TableCell>
            <TableCell>
              <Badge
                variant="secondary"
                v-if="(key as any).access_control_policy_name"
                class="font-mono text-xs"
              >
                {{ (key as any).access_control_policy_name }}
              </Badge>
              <span v-else class="text-gray-400">-</span>
            </TableCell>
            <TableCell class="text-gray-500 text-sm">{{
              key.created_at_formatted || key.created_at
            }}</TableCell>
            <TableCell class="text-gray-500 text-sm">{{
              key.updated_at_formatted || key.updated_at
            }}</TableCell>
            <TableCell class="text-right">
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-500 hover:text-gray-900 px-2"
                @click="handleStartEditing(key)"
              >
                <Pencil class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.edit") }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600 px-2"
                @click="handleDeleteApiKey(key)"
              >
                <Trash class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.delete") }}
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <template #modals>
      <!-- Edit Modal -->
      <ApiKeyEditModal
        v-model:isOpen="showEditModal"
        :initial-data="selectedApiKey"
        @save-success="handleSaveSuccess"
      />
    </template>
  </CrudPageLayout>
</template>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { Button } from "@/components/ui/button";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Checkbox } from "@/components/ui/checkbox";
import { Badge } from "@/components/ui/badge";
import {
  Plus,
  Loader2,
  KeyRound,
  Copy,
  Check,
  Pencil,
  Trash,
} from "lucide-vue-next";
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import ApiKeyEditModal from "@/components/ApiKeyEditModal.vue";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { useAccessControlStore } from "@/store/accessControlStore";
import { toastController } from "@/lib/toastController";
import { confirm } from "@/lib/confirmController";

const { t: $t } = useI18n();

const apiKeyStore = useApiKeyStore();
const accessControlStore = useAccessControlStore();

const showEditModal = ref(false);
const selectedApiKey = ref<any | null>(null);
const copiedKeyId = ref<number | null>(null);
const loading = ref(true);
const error = ref<string | null>(null);

const fetchData = async () => {
  loading.value = true;
  error.value = null;
  try {
    await Promise.all([
      apiKeyStore.fetchApiKeys(),
      accessControlStore.fetchPolicies(),
    ]);
  } catch (err: any) {
    error.value = err.message || $t("common.unknownError");
  } finally {
    loading.value = false;
  }
};

onMounted(() => {
  fetchData();
});

const handleStartEditing = (apiKey?: any) => {
  selectedApiKey.value = apiKey || null;
  showEditModal.value = true;
};

const handleToggleEnable = async (apiKey: any, newVal: boolean) => {
  const updatedApiKey = { ...apiKey, is_enabled: newVal };
  try {
    const payload = {
      name: updatedApiKey.name,
      api_key: updatedApiKey.api_key,
      description: updatedApiKey.description,
      is_enabled: updatedApiKey.is_enabled,
      access_control_policy_id: updatedApiKey.access_control_policy_id,
    };
    await Api.updateApiKey(updatedApiKey.id, payload);
    await apiKeyStore.fetchApiKeys(); // Refetch to update data
  } catch (err: any) {
    console.error("Failed to toggle API key status:", err);
    toastController.error(
      $t("apiKeyPage.toggleStatusFailed", {
        error: err.message || $t("common.unknownError"),
      }),
    );
    await apiKeyStore.fetchApiKeys(); // Refetch to revert
  }
};

const handleSaveSuccess = async () => {
  await apiKeyStore.fetchApiKeys();
  showEditModal.value = false;
  selectedApiKey.value = null;
};

const handleDeleteApiKey = async (apiKey: any) => {
  if (
    await confirm($t("apiKeyPage.confirmDelete", { name: apiKey.name }))
  ) {
    try {
      await Api.deleteApiKey(apiKey.id);
      await apiKeyStore.fetchApiKeys();
    } catch (err: any) {
      console.error("Failed to delete API key:", err);
      toastController.error(
        $t("apiKeyPage.deleteFailed", { error: err.message || $t("common.unknownError") }),
      );
    }
  }
};

const copyApiKeyToClipboard = async (apiKeyString: string, keyId: number) => {
  if (!apiKeyString) return;
  try {
    await navigator.clipboard.writeText(apiKeyString);
    copiedKeyId.value = keyId;
    setTimeout(() => {
      copiedKeyId.value = null;
    }, 2000);
  } catch (err) {
    console.error("Failed to copy API key: ", err);
    toastController.error($t("apiKeyPage.copyFailed"));
  }
};
</script>
