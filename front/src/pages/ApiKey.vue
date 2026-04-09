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

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:hidden">
      <MobileCrudCard
        v-for="key in apiKeyStore.apiKeys"
        :key="key.id"
        :title="key.name"
        :description="key.description || '/'"
      >
        <template #header>
          <Badge variant="secondary" class="font-mono text-[11px]">
            {{ key.is_enabled ? $t("common.yes") : $t("common.no") }}
          </Badge>
        </template>

        <div class="space-y-2 text-sm">
          <div class="flex items-center justify-between gap-3 rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5">
            <span class="text-xs text-gray-500">{{ $t("apiKeyPage.table.apiKeyPartial") }}</span>
            <div class="flex items-center gap-2">
              <span class="font-mono text-xs text-gray-700">{{
                key.api_key
                  ? `${key.api_key.substring(0, 3)}...${key.api_key.substring(key.api_key.length - 4)}`
                  : "N/A"
              }}</span>
              <Button
                variant="ghost"
                size="sm"
                class="h-8 w-8 p-0 text-gray-400 hover:text-gray-900"
                @click="copyApiKeyToClipboard(key.api_key, key.id)"
                :title="$t('apiKeyPage.copy')"
              >
                <Check
                  v-if="copiedKeyId === key.id"
                  class="h-3.5 w-3.5 text-green-600"
                />
                <Copy v-else class="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
            <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("apiKeyPage.table.enabled") }}</span>
              <Checkbox
                :checked="key.is_enabled"
                @update:checked="(val: boolean) => handleToggleEnable(key, val)"
              />
            </div>
            <div class="flex items-center justify-between gap-3 rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("apiKeyPage.table.accessControlPolicy") }}</span>
              <Badge
                v-if="key.access_control_policy_name"
                variant="secondary"
                class="max-w-[12rem] truncate font-mono text-[11px]"
              >
                {{ key.access_control_policy_name }}
              </Badge>
              <span v-else class="text-gray-400">-</span>
            </div>
            <div class="flex items-start justify-between gap-3 rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("apiKeyPage.table.createdAt") }}</span>
              <span class="text-right text-gray-700">
                {{ key.created_at_formatted || key.created_at }}
              </span>
            </div>
            <div class="flex items-start justify-between gap-3 rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("apiKeyPage.table.updatedAt") }}</span>
              <span class="text-right text-gray-700">
                {{ key.updated_at_formatted || key.updated_at }}
              </span>
            </div>
          </div>
        </div>

        <template #actions>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-500 hover:text-gray-900"
            @click="handleStartEditing(key)"
          >
            <Pencil class="h-3.5 w-3.5 mr-1" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-400 hover:text-red-600"
            @click="handleDeleteApiKey(key)"
          >
            <Trash class="h-3.5 w-3.5 mr-1" />
            {{ $t("common.delete") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden border border-gray-200 rounded-lg overflow-hidden md:block">
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
              <div class="flex items-center gap-2">
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
                v-if="key.access_control_policy_name"
                class="font-mono text-xs"
              >
                {{ key.access_control_policy_name }}
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
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { useAccessControlStore } from "@/store/accessControlStore";
import type { ApiKeyItem } from "@/store/types";
import { normalizeError } from "@/lib/error";
import { copyText } from "@/lib/clipboard";
import { toastController } from "@/lib/toastController";
import { confirm } from "@/lib/confirmController";

const { t: $t } = useI18n();

const apiKeyStore = useApiKeyStore();
const accessControlStore = useAccessControlStore();

const showEditModal = ref(false);
const selectedApiKey = ref<ApiKeyItem | null>(null);
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
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    loading.value = false;
  }
};

onMounted(() => {
  fetchData();
});

const handleStartEditing = (apiKey?: ApiKeyItem) => {
  selectedApiKey.value = apiKey || null;
  showEditModal.value = true;
};

const handleToggleEnable = async (apiKey: ApiKeyItem, newVal: boolean) => {
  const updatedApiKey = { ...apiKey, is_enabled: newVal };
  try {
    const payload = {
      name: updatedApiKey.name,
      api_key: updatedApiKey.api_key,
      description: updatedApiKey.description,
      is_enabled: updatedApiKey.is_enabled,
      access_control_policy_id: updatedApiKey.access_control_policy_id ?? null,
    };
    await Api.updateApiKey(updatedApiKey.id, payload);
    await apiKeyStore.fetchApiKeys(); // Refetch to update data
  } catch (err: unknown) {
    const normalizedError = normalizeError(err, $t("common.unknownError"));
    toastController.error(
      $t("apiKeyPage.toggleStatusFailed", {
        error: normalizedError.message,
      }),
    );
    try {
      await apiKeyStore.fetchApiKeys(); // Refetch to revert
    } catch (refreshErr: unknown) {
      toastController.error(
        $t("apiKeyPage.errorPrefix"),
        normalizeError(refreshErr, $t("common.unknownError")).message,
      );
    }
  }
};

const handleSaveSuccess = async () => {
  try {
    await apiKeyStore.fetchApiKeys();
    showEditModal.value = false;
    selectedApiKey.value = null;
  } catch (err: unknown) {
    toastController.error(
      $t("apiKeyPage.errorPrefix"),
      normalizeError(err, $t("common.unknownError")).message,
    );
  }
};

const handleDeleteApiKey = async (apiKey: ApiKeyItem) => {
  if (
    await confirm($t("apiKeyPage.confirmDelete", { name: apiKey.name }))
  ) {
    try {
      await Api.deleteApiKey(apiKey.id);
      await apiKeyStore.fetchApiKeys();
    } catch (err: unknown) {
      toastController.error(
        $t("apiKeyPage.deleteFailed", {
          error: normalizeError(err, $t("common.unknownError")).message,
        }),
      );
    }
  }
};

const copyApiKeyToClipboard = async (apiKeyString: string, keyId: number) => {
  if (!apiKeyString) return;
  const copied = await copyText(apiKeyString);
  if (!copied) {
    toastController.error($t("apiKeyPage.copyFailed"));
    return;
  }

  copiedKeyId.value = keyId;
  setTimeout(() => {
    copiedKeyId.value = null;
  }, 2000);
};
</script>
