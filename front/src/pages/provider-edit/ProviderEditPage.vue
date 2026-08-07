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
            <div v-if="editingData.id">
              <div class="mb-4 rounded-lg border border-gray-200 bg-gray-50/60 px-3.5 py-3 text-xs leading-5 text-gray-600">
                {{ $t("providerEditPage.sections.advancedConfig.scopeDescription") }}
              </div>
              <ReasoningConfigPanel
                owner-kind="provider"
                :owner-id="editingData.id"
                :actions="reasoningActions"
                :title="$t('providerEditPage.sections.advancedConfig.title')"
                @saved="handleReasoningConfigSaved"
              >
                <template #runtime-feature>
                  <RuntimeFeatureConfigPanel
                    owner-kind="provider"
                    :owner-id="editingData.id"
                    embedded
                    @saved="handleRuntimeFeatureConfigSaved"
                  />
                </template>
              </ReasoningConfigPanel>
            </div>

            <SectionHeader
              :title="$t('providerEditPage.sections.advanced.title')"
              :help="$t('providerEditPage.sections.advanced.description')"
              :help-label="$t('providerEditPage.sections.advanced.title')"
              class="border-t border-gray-200 pt-5 mt-5"
            />

            <ProviderRequestPatchPanel v-model:editingData="editingData" />
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
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import PageHeader from "@/components/PageHeader.vue";
import SectionHeader from "@/components/SectionHeader.vue";
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
import ReasoningConfigPanel from "@/components/reasoning/ReasoningConfigPanel.vue";
import RuntimeFeatureConfigPanel from "@/components/runtime-feature/RuntimeFeatureConfigPanel.vue";

const { t: $t } = useI18n();
const router = useRouter();
const route = useRoute();
const {
  isLoading,
  errorMsg,
  editingData,
  pageTitle,
  reasoningActions,
  handleReasoningConfigSaved,
  handleRuntimeFeatureConfigSaved,
} = useProviderEdit();

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
