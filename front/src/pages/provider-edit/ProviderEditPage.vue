<template>
  <div class="app-page">
    <div class="app-page-shell app-page-shell--narrow">
      <PageHeader :title="pageTitle" actions-class="sm:flex-col">
        <template #actions>
          <Button variant="outline" @click="router.push('/provider')">
            <ArrowLeft class="h-4 w-4 mr-1.5" />
            {{ $t("providerEditPage.buttonBackToList") }}
          </Button>
        </template>
      </PageHeader>

      <div v-if="isLoading" class="flex items-center justify-center py-16">
        <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("providerEditPage.loadingData")
        }}</span>
      </div>

      <div
        v-else-if="errorMsg"
        class="flex flex-col items-center justify-center py-20"
      >
        <AlertCircle class="h-10 w-10 stroke-1 text-red-500 mb-2" />
        <span class="text-sm font-medium text-red-500">{{ errorMsg }}</span>
      </div>

      <template v-else-if="editingData">
        <div class="space-y-5 sm:space-y-6">
          <div class="border-b border-gray-200 app-scroll-x mb-4">
            <div class="flex min-w-max gap-1">
              <button
                v-for="tab in providerEditTabs"
                :key="tab.id"
                type="button"
                class="border-b-2 px-4 py-2.5 text-sm font-medium transition-colors"
                :class="
                  activeTab === tab.id
                    ? 'border-gray-900 text-gray-900'
                    : 'border-transparent text-gray-500 hover:text-gray-900 hover:border-gray-300'
                "
                @click="activeTab = tab.id"
              >
                {{ $t(tab.labelKey) }}
              </button>
            </div>
          </div>

          <template v-if="activeTab === 'base'">
            <ProviderBaseInfoForm v-model:editingData="editingData" />
          </template>

          <template v-else-if="activeTab === 'sources'">
            <ProviderSourceList
              v-model:editingData="editingData"
              @check-source="handleSourceCheck"
              @request-patch="openRequestPatch"
            />
          </template>

          <template v-else-if="activeTab === 'models'">
            <ProviderModelList
              v-model:editingData="editingData"
              @check-single="(index) => handleCheck('model', index)"
            />
          </template>

          <template v-else-if="activeTab === 'credentials'">
            <ProviderApiKeyList
              v-model:editingData="editingData"
              @check-single="(index) => handleCheck('apiKey', index)"
            />
          </template>

          <template v-else-if="activeTab === 'advanced'">
            <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
              <p class="text-sm leading-6 text-gray-600">
                {{ $t("providerEditPage.sections.advanced.description") }}
              </p>
            </div>
          </template>

          <div class="flex flex-col gap-2 border-t border-gray-100 pt-4 sm:flex-row sm:justify-end">
            <Button variant="secondary" class="w-full sm:w-auto" @click="router.push('/provider')">{{
              $t("providerEditPage.buttonBackToList")
            }}</Button>
          </div>
        </div>

        <ProviderCheckDialog
          v-model:open="isCheckDialogOpen"
          :kind="checkDialogKind"
          :target-label="checkDialogTargetLabel"
          :source-options="checkDialogSourceOptions"
          :model-options="checkDialogModelOptions"
          :api-key-options="checkDialogApiKeyOptions"
          :source-value="checkDialogSourceValue"
          :model-value="checkDialogModelValue"
          :api-key-value="checkDialogApiKeyValue"
          @update:source-value="checkDialogSourceValue = $event"
          @update:model-value="checkDialogModelValue = $event"
          @update:api-key-value="checkDialogApiKeyValue = $event"
          @confirm="handleConfirmCheck"
        />

        <ProviderRequestPatchPanel
          v-if="editingData.id && selectedSource"
          v-model:open="isRequestPatchOpen"
          :provider-id="editingData.id"
          :source-id="selectedSource.id"
          :source-title="`${selectedSource.profile_type} · #${selectedSource.id}`"
        />
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import PageHeader from "@/components/PageHeader.vue";
import { Button } from "@/components/ui/button";
import {
  ArrowLeft,
  Loader2,
  AlertCircle,
} from "lucide-vue-next";

import { useProviderEdit } from "./composables/useProviderEdit";
import { useProviderCheck } from "./composables/useProviderCheck";
import ProviderBaseInfoForm from "./components/ProviderBaseInfoForm.vue";
import ProviderSourceList from "./components/ProviderSourceList.vue";
import ProviderModelList from "./components/ProviderModelList.vue";
import ProviderApiKeyList from "./components/ProviderApiKeyList.vue";
import ProviderCheckDialog from "./components/ProviderCheckDialog.vue";
import ProviderRequestPatchPanel from "./components/ProviderRequestPatchPanel.vue";

const { t: $t } = useI18n();
const router = useRouter();
const route = useRoute();
const {
  isLoading,
  errorMsg,
  editingData,
  pageTitle,
} = useProviderEdit();

const selectedSourceId = ref<number | null>(null);
const isRequestPatchOpen = ref(false);
const selectedSource = computed(() =>
  editingData.value?.upstream_sources.find((source) => source.id === selectedSourceId.value) ?? null,
);
const openRequestPatch = (sourceId: number) => {
  selectedSourceId.value = sourceId;
  isRequestPatchOpen.value = true;
};

type ProviderEditTab = "base" | "sources" | "models" | "credentials" | "advanced";
const routeTab = route.query.tab;
const activeTab = ref<ProviderEditTab>(
  routeTab === "sources" ||
    routeTab === "models" ||
    routeTab === "credentials" ||
    routeTab === "advanced"
    ? routeTab
    : "base",
);
const providerEditTabs = [
  { id: "base", labelKey: "providerEditPage.tabs.base" },
  { id: "sources", labelKey: "providerEditPage.tabs.sources" },
  { id: "models", labelKey: "providerEditPage.tabs.models" },
  { id: "credentials", labelKey: "providerEditPage.tabs.credentials" },
  { id: "advanced", labelKey: "providerEditPage.tabs.advanced" },
] as const;

const {
  isCheckDialogOpen,
  checkDialogKind,
  checkDialogTargetLabel,
  checkDialogSourceOptions,
  checkDialogModelOptions,
  checkDialogApiKeyOptions,
  checkDialogSourceValue,
  checkDialogModelValue,
  checkDialogApiKeyValue,
  handleCheck,
  handleSourceCheck,
  handleConfirmCheck,
} = useProviderCheck(editingData);
</script>
