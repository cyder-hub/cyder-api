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
                v-for="tab in [
                  { id: 'base', label: $t('providerEditPage.tabs.base') },
                  { id: 'models', label: $t('providerEditPage.tabs.models') },
                  { id: 'credentials', label: $t('providerEditPage.tabs.credentials') },
                  { id: 'advanced', label: $t('providerEditPage.tabs.advanced') }
                ]"
                :key="tab.id"
                type="button"
                class="border-b-2 px-4 py-2.5 text-sm font-medium transition-colors"
                :class="
                  activeTab === tab.id
                    ? 'border-gray-900 text-gray-900'
                    : 'border-transparent text-gray-500 hover:text-gray-900 hover:border-gray-300'
                "
                @click="activeTab = tab.id as any"
              >
                {{ tab.label }}
              </button>
            </div>
          </div>

          <template v-if="activeTab === 'base'">
            <ProviderBaseInfoForm v-model:editingData="editingData" />
          </template>

          <template v-else-if="activeTab === 'models'">
            <ProviderModelList
              v-model:editingData="editingData"
              @check-single="(index) => handleCheck('model', index)"
              @check-batch="() => handleBatchCheck('models')"
            />
          </template>

          <template v-else-if="activeTab === 'credentials'">
            <ProviderApiKeyList
              v-model:editingData="editingData"
              @check-single="(index) => handleCheck('apiKey', index)"
              @check-batch="() => handleBatchCheck('api_keys')"
            />
          </template>

          <template v-else-if="activeTab === 'advanced'">
            <div v-if="editingData.id">
              <ReasoningConfigPanel
                owner-kind="provider"
                :owner-id="editingData.id"
                :actions="reasoningActions"
                :title="$t('providerEditPage.sections.advancedConfig.title')"
                :provider-type="editingData.provider_type"
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

        <Dialog
          :open="isModelSelectModalOpen"
          @update:open="(v: boolean) => (isModelSelectModalOpen = v)"
        >
          <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-md">
            <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
              <DialogTitle class="text-lg font-semibold text-gray-900">{{
                $t("providerEditPage.modalSelectModel.title")
              }}</DialogTitle>
            </DialogHeader>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
              <p class="text-sm text-gray-500">
                {{ $t("providerEditPage.modalSelectModel.description") }}
              </p>
              <p class="font-mono text-xs text-gray-600">
                {{ $t("providerEditPage.modalSelectModel.target", { target: selectedApiKeyCheckTargetLabel }) }}
              </p>
              <Select v-model="modelIndexToUseStr">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      $t('providerEditPage.modalSelectModel.selectPlaceholder')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="opt in modelOptionsForSelect"
                    :key="opt.value"
                    :value="String(opt.value)"
                  >
                    {{ opt.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
              <Button
                variant="ghost"
                class="w-full text-gray-600 sm:w-auto"
                @click="isModelSelectModalOpen = false"
                >{{ $t("common.cancel") }}</Button
              >
              <Button
                variant="default"
                class="w-full sm:w-auto"
                @click="handleConfirmModelSelection"
                :disabled="modelIndexToUseStr === null"
                >{{ $t("common.check") }}</Button
              >
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog
          :open="isApiKeySelectModalOpen"
          @update:open="(v: boolean) => (isApiKeySelectModalOpen = v)"
        >
          <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-md">
            <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
              <DialogTitle class="text-lg font-semibold text-gray-900">{{
                $t("providerEditPage.modalSelectApiKey.title")
              }}</DialogTitle>
            </DialogHeader>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
              <p class="text-sm text-gray-500">
                {{ $t("providerEditPage.modalSelectApiKey.description") }}
              </p>
              <p class="font-mono text-xs text-gray-600">
                {{ $t("providerEditPage.modalSelectApiKey.target", { target: selectedModelCheckTargetLabel }) }}
              </p>
              <Select v-model="apiKeyIndexToUseStr">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      $t('providerEditPage.modalSelectApiKey.selectPlaceholder')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="opt in apiKeyOptionsForSelect"
                    :key="opt.value"
                    :value="String(opt.value)"
                  >
                    {{ opt.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
              <Button
                variant="ghost"
                class="w-full text-gray-600 sm:w-auto"
                @click="isApiKeySelectModalOpen = false"
                >{{ $t("common.cancel") }}</Button
              >
              <Button
                variant="default"
                class="w-full sm:w-auto"
                @click="handleConfirmApiKeySelection"
                :disabled="apiKeyIndexToUseStr === null"
                >{{ $t("common.check") }}</Button
              >
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, type Ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import PageHeader from "@/components/PageHeader.vue";
import SectionHeader from "@/components/SectionHeader.vue";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ArrowLeft,
  Loader2,
  AlertCircle,
} from "lucide-vue-next";

import type { EditingProviderData } from "./types";
import { useProviderEdit } from "./composables/useProviderEdit";
import { useProviderCheck } from "./composables/useProviderCheck";
import ProviderBaseInfoForm from "./components/ProviderBaseInfoForm.vue";
import ProviderModelList from "./components/ProviderModelList.vue";
import ProviderApiKeyList from "./components/ProviderApiKeyList.vue";
import ProviderRequestPatchPanel from "./components/ProviderRequestPatchPanel.vue";
import ReasoningConfigPanel from "@/components/reasoning/ReasoningConfigPanel.vue";
import RuntimeFeatureConfigPanel from "@/components/runtime-feature/RuntimeFeatureConfigPanel.vue";

const { t: $t } = useI18n();
const router = useRouter();
const {
  isLoading,
  errorMsg,
  editingData,
  pageTitle,
  reasoningActions,
  handleReasoningConfigSaved,
  handleRuntimeFeatureConfigSaved,
} = useProviderEdit();

const activeTab = ref<"base" | "models" | "credentials" | "advanced">("base");

const {
  isModelSelectModalOpen,
  isApiKeySelectModalOpen,
  modelIndexToUseStr,
  apiKeyIndexToUseStr,
  modelOptionsForSelect,
  apiKeyOptionsForSelect,
  selectedModelCheckTargetLabel,
  selectedApiKeyCheckTargetLabel,
  handleCheck,
  handleBatchCheck,
  handleConfirmModelSelection,
  handleConfirmApiKeySelection,
} = useProviderCheck(editingData as Ref<EditingProviderData | null>);
</script>
