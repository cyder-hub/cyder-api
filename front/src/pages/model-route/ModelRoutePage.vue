<script setup lang="ts">
import { AlertCircle, Inbox, Loader2, Plus, RefreshCcw } from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import { Button } from "@/components/ui/button";
import ModelRouteEditorDialog from "./components/ModelRouteEditorDialog.vue";
import ModelRouteReasoningPreviewDialog from "./components/ModelRouteReasoningPreviewDialog.vue";
import ModelRouteTable from "./components/ModelRouteTable.vue";
import { useModelRouteEditor } from "./composables/useModelRouteEditor";
import { useModelRouteList } from "./composables/useModelRouteList";
import { useModelRouteQueue } from "./composables/useModelRouteQueue";
import { useModelRouteReasoningPreview } from "./composables/useModelRouteReasoningPreview";

const {
  routes,
  loading,
  error,
  actionRouteId,
  summaryCards,
  fetchRouteList,
  toggleEnabled,
  toggleExpose,
  deleteRoute,
} = useModelRouteList();

const {
  showEditModal,
  editingRoute,
  isSaving,
  closeEditor,
  openAddModal,
  openEditModal,
  saveRoute,
} = useModelRouteEditor({
  afterSave: fetchRouteList,
});

const queue = useModelRouteQueue(editingRoute);

const {
  showReasoningPreviewModal,
  reasoningPreview,
  isReasoningPreviewLoading,
  reasoningPreviewError,
  openReasoningPreview,
  setShowReasoningPreviewModal,
  formatRouteReasoningConfigSource,
} = useModelRouteReasoningPreview();
</script>

<template>
  <CrudPageLayout
    :title="$t('modelRoutePage.title')"
    :description="$t('modelRoutePage.description')"
    :loading="loading"
    :error="error"
    :empty="!routes.length"
    content-class="space-y-4"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" :disabled="loading" @click="fetchRouteList">
        <RefreshCcw class="mr-1.5 h-4 w-4" />
        {{ $t("common.refresh") }}
      </Button>
      <Button variant="default" class="w-full sm:w-auto" :disabled="loading" @click="openAddModal">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("modelRoutePage.addRoute") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center rounded-lg border border-gray-200 bg-white py-16">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("modelRoutePage.loading") }}</span>
      </div>
    </template>

    <template #error="{ error: pageError }">
      <div class="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-white py-20">
        <AlertCircle class="mb-4 h-10 w-10 stroke-1 text-red-400" />
        <span class="text-sm font-medium text-red-500">
          {{ $t("modelRoutePage.errorPrefix") }} {{ pageError }}
        </span>
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center rounded-lg border border-gray-200 bg-white py-20">
        <Inbox class="mb-4 h-10 w-10 stroke-1 text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ $t("modelRoutePage.noData") }}
        </span>
      </div>
    </template>

    <div class="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-gray-200 bg-gray-100 sm:grid-cols-4">
      <div v-for="card in summaryCards" :key="card.key" class="bg-white px-4 py-3">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ card.label }}
        </p>
        <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
          {{ card.value }}
        </p>
      </div>
    </div>

    <ModelRouteTable
      :routes="routes"
      :action-route-id="actionRouteId"
      @edit="openEditModal"
      @delete="deleteRoute"
      @reasoning="openReasoningPreview"
      @toggle-enabled="toggleEnabled"
      @toggle-expose="toggleExpose"
    />

    <template #modals>
      <ModelRouteEditorDialog
        v-model:open="showEditModal"
        v-model:route="editingRoute"
        :saving="isSaving"
        :provider-options="queue.providerOptions.value"
        :get-model-options="queue.getModelOptions"
        :get-candidate-summary="queue.getCandidateSummary"
        @close="closeEditor"
        @save="saveRoute"
        @add-candidate="queue.addCandidate"
        @remove-candidate="queue.removeCandidate"
        @move-candidate="queue.moveCandidate"
        @candidate-provider-change="queue.setCandidateProvider"
        @candidate-model-change="queue.setCandidateModel"
        @candidate-enabled-change="queue.setCandidateEnabled"
      />

      <ModelRouteReasoningPreviewDialog
        v-model:open="showReasoningPreviewModal"
        :preview="reasoningPreview"
        :loading="isReasoningPreviewLoading"
        :error="reasoningPreviewError"
        :format-config-source="formatRouteReasoningConfigSource"
        @update:open="setShowReasoningPreviewModal"
      />
    </template>
  </CrudPageLayout>
</template>
