<script setup lang="ts">
import { onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { KeyRound, Loader2, Plus, RefreshCcw } from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import { Button } from "@/components/ui/button";
import ApiKeyDetailDrawer from "./components/ApiKeyDetailDrawer.vue";
import ApiKeyEditDialog from "./components/ApiKeyEditDialog.vue";
import ApiKeyTable from "./components/ApiKeyTable.vue";
import { useApiKeyDetail } from "./composables/useApiKeyDetail";
import { useApiKeyGovernance } from "./composables/useApiKeyGovernance";
import { useApiKeyList } from "./composables/useApiKeyList";

const { t } = useI18n();

const apiKeyList = useApiKeyList(t);
const apiKeyDetail = useApiKeyDetail(t, apiKeyList.apiKeys, apiKeyList.runtimeById);
const {
  apiKeys,
  runtimeById,
  routeNameById,
  modelRoutes,
  loading,
  error,
  summaryCards,
  providerStore,
  modelStore,
} = apiKeyList;
const {
  detailLoading,
  selectedKeyId,
  selectedDetail,
  selectedRuntimeView,
  selectedListKey,
  showMobileKeyPicker,
  secretReveal,
  handleSelectKey,
  handleRevealKey,
  copySecret,
  setSecretReveal,
} = apiKeyDetail;

async function refreshSelected(preferredSelectedId: number | null) {
  const nextSelectedId = await apiKeyList.fetchData(preferredSelectedId);
  await apiKeyDetail.loadSelectedKey(nextSelectedId);
}

const apiKeyGovernance = useApiKeyGovernance({
  t,
  apiKeys: apiKeyList.apiKeys,
  selectedKeyId: apiKeyDetail.selectedKeyId,
  selectedDetail: apiKeyDetail.selectedDetail,
  setSecretReveal: apiKeyDetail.setSecretReveal,
  refreshSelected,
});
const {
  showEditDialog,
  editingDetail,
  handleStartEditing,
  handleSaveSuccess,
  handleRotateKey,
  handleDeleteKey,
} = apiKeyGovernance;

function handleRefresh() {
  void refreshSelected(selectedKeyId.value);
}

onMounted(() => {
  void refreshSelected(selectedKeyId.value);
});
</script>

<template>
  <CrudPageLayout
    :title="t('apiKeyPage.title')"
    :description="t('apiKeyPage.description')"
    :loading="loading"
    :error="error"
    :empty="!apiKeys.length"
    header-class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between"
    page-class="flex flex-col xl:h-full xl:min-h-0 xl:overflow-hidden"
    shell-class="flex flex-col xl:min-h-0 xl:flex-1"
    content-class="flex flex-col gap-4 sm:gap-5 xl:min-h-0 xl:flex-1"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" @click="handleRefresh">
        <RefreshCcw class="mr-1.5 h-4 w-4" />
        {{ t("common.refresh") }}
      </Button>
      <Button
        variant="outline"
        class="w-full sm:w-auto"
        @click="handleStartEditing()"
      >
        <Plus class="mr-1.5 h-4 w-4" />
        {{ t("apiKeyPage.addApiKey") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex flex-col items-center justify-center py-20">
        <Loader2 class="mb-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ t("apiKeyPage.loading") }}
        </span>
      </div>
    </template>

    <template #error="{ error: pageError }">
      <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ pageError }}
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20">
        <KeyRound class="mb-4 h-10 w-10 stroke-1 text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ t("apiKeyPage.noData") }}
        </span>
      </div>
    </template>

    <div class="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-gray-200 bg-gray-100 sm:grid-cols-3 xl:grid-cols-5">
      <div
        v-for="card in summaryCards"
        :key="card.key"
        class="bg-white px-4 py-3"
      >
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ card.label }}
        </p>
        <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
          {{ card.value }}
        </p>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-4 xl:min-h-0 xl:flex-1 xl:grid-cols-12">
      <div class="xl:col-span-4 xl:min-h-0 2xl:col-span-3">
        <ApiKeyTable
          v-model:mobile-open="showMobileKeyPicker"
          :api-keys="apiKeys"
          :runtime-by-id="runtimeById"
          :selected-key-id="selectedKeyId"
          :selected-list-key="selectedListKey"
          @select="handleSelectKey"
        />
      </div>

      <div class="xl:col-span-8 xl:min-h-0 2xl:col-span-9">
        <ApiKeyDetailDrawer
          :detail="selectedDetail"
          :runtime="selectedRuntimeView"
          :detail-loading="detailLoading"
          :secret-reveal="secretReveal"
          :provider-name-by-id="providerStore.providerNameById"
          :model-name-by-id="modelStore.modelNameById"
          :route-name-by-id="routeNameById"
          @reveal="handleRevealKey"
          @rotate="handleRotateKey"
          @edit="handleStartEditing"
          @delete="handleDeleteKey"
          @copy-secret="copySecret"
          @close-secret="setSecretReveal(null)"
        />
      </div>
    </div>

    <template #modals>
      <ApiKeyEditDialog
        v-model:is-open="showEditDialog"
        :initial-data="editingDetail"
        :model-routes="modelRoutes"
        :providers="providerStore.providers"
        :models="modelStore.models"
        @save-success="handleSaveSuccess"
      />
    </template>
  </CrudPageLayout>
</template>
