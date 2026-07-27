<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { onBeforeRouteLeave } from "vue-router";
import { KeyRound, Loader2, Plus, RefreshCcw } from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import StatsStrip from "@/components/StatsStrip.vue";
import { Button } from "@/components/ui/button";
import ApiKeyDetailDrawer from "./components/ApiKeyDetailDrawer.vue";
import ApiKeyEditDialog from "./components/ApiKeyEditDialog.vue";
import ApiKeySecretDialog from "./components/ApiKeySecretDialog.vue";
import ApiKeySensitiveActionDialog, {
  type ApiKeySensitiveAction,
} from "./components/ApiKeySensitiveActionDialog.vue";
import ApiKeyTable from "./components/ApiKeyTable.vue";
import { useApiKeyDetail } from "./composables/useApiKeyDetail";
import { useApiKeyGovernance } from "./composables/useApiKeyGovernance";
import { useApiKeyList } from "./composables/useApiKeyList";
import { useApiKeySecretState } from "./composables/useApiKeySecretState";
import { useAuthStore } from "@/store/authStore";

const { t } = useI18n();
const authStore = useAuthStore();

const apiKeyList = useApiKeyList(t);
const secretState = useApiKeySecretState();
const apiKeyDetail = useApiKeyDetail(
  t,
  apiKeyList.runtimeById,
  secretState.revealedSecret,
  secretState.setRevealedSecret,
  secretState.selectKey,
);
const {
  apiKeys,
  runtimeById,
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
  secretReveal,
  handleSelectKey,
  handleRevealKey,
  copySecret,
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
  setIssuedSecret: secretState.setIssuedSecret,
  clearRevealedSecret: secretState.closeDrawer,
  refreshList: apiKeyList.fetchData,
  refreshDetail: apiKeyDetail.loadSelectedKey,
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

const isDetailOpen = ref(false);
const sensitiveAction = ref<{
  action: ApiKeySensitiveAction;
  id: number;
  targetName: string;
} | null>(null);
const isSensitiveActionBusy = ref(false);
const isSensitiveActionOpen = computed({
  get: () => sensitiveAction.value !== null,
  set: (open: boolean) => {
    if (!open && !isSensitiveActionBusy.value) {
      sensitiveAction.value = null;
    }
  },
});

function onSelectKey(id: number) {
  handleSelectKey(id);
  isDetailOpen.value = true;
}

function onDetailOpenChange(open: boolean) {
  isDetailOpen.value = open;
  if (!open) {
    secretState.closeDrawer();
  }
}

function openSensitiveAction(action: ApiKeySensitiveAction, id: number) {
  const target = apiKeys.value.find((item) => item.id === id);
  sensitiveAction.value = {
    action,
    id,
    targetName: target?.name ?? String(id),
  };
}

async function confirmSensitiveAction() {
  const pending = sensitiveAction.value;
  if (!pending || isSensitiveActionBusy.value) return;
  isSensitiveActionBusy.value = true;
  try {
    if (pending.action === "reveal") {
      await handleRevealKey(pending.id);
    } else if (pending.action === "rotate") {
      await handleRotateKey(pending.id);
    } else if (await handleDeleteKey(pending.id)) {
      isDetailOpen.value = false;
    }
  } finally {
    isSensitiveActionBusy.value = false;
    sensitiveAction.value = null;
  }
}

onMounted(() => {
  void refreshSelected(selectedKeyId.value);
});

onBeforeRouteLeave(() => {
  sensitiveAction.value = null;
  secretState.leaveRoute();
});

watch(
  () => authStore.lifecycle,
  (lifecycle) => {
    if (lifecycle === "anonymous") {
      sensitiveAction.value = null;
      secretState.logout();
    }
  },
);
</script>

<template>
  <CrudPageLayout
    :title="t('apiKeyPage.title')"
    :loading="loading"
    :error="error"
    :empty="!apiKeys.length"
    header-class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between"
    page-class="flex flex-col"
    shell-class="flex flex-col"
    content-class="flex flex-col gap-4 sm:gap-5"
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

    <StatsStrip :items="summaryCards" grid-class="grid-cols-2 sm:grid-cols-3 xl:grid-cols-5" />

    <div>
      <ApiKeyTable
        :api-keys="apiKeys"
        :runtime-by-id="runtimeById"
        :selected-key-id="selectedKeyId"
        @select="onSelectKey"
      />
    </div>

    <ApiKeyDetailDrawer
      :open="isDetailOpen"
      :detail="selectedDetail"
      :runtime="selectedRuntimeView"
      :detail-loading="detailLoading"
      :secret-reveal="secretReveal"
      :provider-name-by-id="providerStore.providerNameById"
      :model-name-by-id="modelStore.modelNameById"
      @reveal="openSensitiveAction('reveal', $event)"
      @rotate="openSensitiveAction('rotate', $event)"
      @edit="handleStartEditing"
      @delete="openSensitiveAction('delete', $event)"
      @copy-secret="copySecret"
      @close-secret="secretState.setRevealedSecret(null)"
      @update:open="onDetailOpenChange"
    />

    <template #modals>
      <ApiKeyEditDialog
        v-model:is-open="showEditDialog"
        :initial-data="editingDetail"
        :providers="providerStore.providers"
        :models="modelStore.models"
        @save-success="handleSaveSuccess"
      />
      <ApiKeySensitiveActionDialog
        v-if="sensitiveAction"
        v-model:open="isSensitiveActionOpen"
        :action="sensitiveAction.action"
        :target-name="sensitiveAction.targetName"
        :loading="isSensitiveActionBusy"
        @confirm="confirmSensitiveAction"
      />
      <ApiKeySecretDialog
        :secret="secretState.issuedSecret.value"
        @copy="copySecret"
        @acknowledge="secretState.setIssuedSecret(null)"
      />
    </template>
  </CrudPageLayout>
</template>
