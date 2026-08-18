<template>
  <section class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader
      :title="t('requestPatchVariant.title')"
      :help="t('requestPatchVariant.description')"
      :help-label="t('requestPatchVariant.title')"
    >
      <template #actions>
        <Button variant="ghost" size="sm" :disabled="isLoading || isRefreshing" @click="refresh">
          <RefreshCw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
          {{ t("requestPatchVariant.actions.refresh") }}
        </Button>
      </template>
    </SectionHeader>

    <div v-if="isLoading" class="flex items-center justify-center py-16 text-sm text-gray-500">
      <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
      {{ t("requestPatchVariant.manager.loading") }}
    </div>

    <div v-else-if="loadError" class="mt-4 rounded-lg border border-red-200 bg-red-50 px-4 py-5">
      <p class="text-sm leading-5 text-red-700">{{ loadError }}</p>
      <Button class="mt-3" variant="outline" size="sm" @click="refresh">{{ t("common.retry") }}</Button>
    </div>

    <div v-else-if="sourceStates.length === 0" class="mt-5 rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-8 text-sm leading-6 text-gray-500">
      {{ t("requestPatchVariant.model.noSources") }}
    </div>

    <div v-else class="mt-5 space-y-5">
      <div v-if="allConflictCount > 0" class="rounded-lg border border-red-200 bg-red-50 px-4 py-4">
        <p class="text-sm font-semibold text-red-800">{{ t("requestPatchVariant.explain.conflictTitle") }}</p>
        <p class="mt-1 text-sm leading-5 text-red-700">{{ t("requestPatchVariant.explain.conflictDescription", { count: allConflictCount }) }}</p>
      </div>

      <article v-for="state in sourceStates" :key="state.source.id" class="rounded-lg border border-gray-200">
        <div class="flex flex-col gap-3 border-b border-gray-100 bg-gray-50/60 px-4 py-4 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0">
            <div class="flex flex-wrap items-center gap-1.5">
              <Badge variant="outline" class="font-mono text-[10px]">{{ state.source.profile_type }}</Badge>
              <span class="font-mono text-xs text-gray-500">#{{ state.source.id }}</span>
              <Badge v-if="!state.source.is_enabled" variant="outline" class="font-mono text-[10px] text-gray-500">{{ t("requestPatchVariant.states.sourceDisabled") }}</Badge>
              <Badge v-if="!isSourceBound(state.source.id)" variant="outline" class="font-mono text-[10px] text-gray-500">{{ t("requestPatchVariant.states.unboundDormant") }}</Badge>
            </div>
            <p class="mt-1 break-all font-mono text-xs text-gray-500">{{ state.source.base_url }}</p>
          </div>
          <div class="flex w-full flex-col gap-2 sm:w-auto sm:items-end">
            <p class="text-xs leading-5 text-gray-500">{{ t("requestPatchVariant.model.sourceSummary", { variants: variantCount(state) }) }}</p>
            <Button
              variant="outline"
              size="sm"
              :disabled="!isSourceBound(state.source.id)"
              @click="openNewSuffix(state.source.id)"
            >
              <Plus class="mr-1.5 h-4 w-4" />
              {{ t("requestPatchVariant.actions.addModelSuffix") }}
            </Button>
          </div>
        </div>

        <div class="space-y-4 p-4">
          <PatchVariantOverview
            :state="state"
            :suffix="null"
            :evaluation="state.evaluations.base"
            :source-variant="findVariant(state.sourceVariants, null)"
            :model-variant="findVariant(state.modelVariants, null)"
            :source-bound="isSourceBound(state.source.id)"
            :on-open-editor="openEditor"
            :on-restore="restoreVariant"
            :on-delete="requestDelete"
            :on-toggle-inherited="toggleInherited"
          />

          <PatchVariantOverview
            v-for="suffix in suffixesFor(state)"
            :key="`${state.source.id}-${suffix}`"
            :state="state"
            :suffix="suffix"
            :evaluation="state.evaluations[suffix]"
            :source-variant="findVariant(state.sourceVariants, suffix)"
            :model-variant="findVariant(state.modelVariants, suffix)"
            :source-bound="isSourceBound(state.source.id)"
            :on-open-editor="openEditor"
            :on-restore="restoreVariant"
            :on-delete="requestDelete"
            :on-toggle-inherited="toggleInherited"
          />
        </div>
      </article>
    </div>
  </section>

  <RequestPatchVariantEditor
    v-model:open="isEditorOpen"
    :source-id="editingSourceId"
    :model-id="modelId"
    :initial-suffix="editingSuffix"
    :variant="editingVariant"
    :require-suffix="editingRequiresSuffix"
    :on-preview="previewEditingVariant"
    :on-save="saveEditingVariant"
  />

  <Dialog :open="deletingVariant !== null" @update:open="(open) => { if (!open) deletingVariant = null }">
    <DialogContent class="border-gray-200 bg-white sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ t("requestPatchVariant.delete.title") }}</DialogTitle>
        <DialogDescription>
          {{ t("requestPatchVariant.delete.description", { variant: deletingVariantLabel }) }}
        </DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" :disabled="isDeleting" @click="deletingVariant = null">
          {{ t("common.cancel") }}
        </Button>
        <Button variant="destructive" class="w-full sm:w-auto" :disabled="isDeleting" @click="confirmDelete">
          <Loader2 v-if="isDeleting" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ t("common.delete") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { Loader2, Plus, RefreshCw } from "lucide-vue-next";

import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import RequestPatchVariantEditor from "@/components/request-patch/RequestPatchVariantEditor.vue";
import PatchVariantOverview from "./PatchVariantOverview.vue";
import type {
  ModelSourceConfigSummary,
  RequestPatchPreviewResponse,
  RequestPatchVariantAggregate,
  RequestPatchVariantInput,
  UpstreamSource,
} from "@/services/types";
import { useModelRequestPatch, type ModelRequestPatchSourceState } from "../composables/useModelRequestPatch";

const props = defineProps<{
  modelId: number | null;
  providerId: number | null;
  providerName?: string | null;
  providerKey?: string | null;
  sources: UpstreamSource[];
  sourceConfig: ModelSourceConfigSummary | null;
}>();

const { t } = useI18n();
const modelIdRef = computed(() => props.modelId);
const providerIdRef = computed(() => props.providerId);
const sourcesRef = computed(() => props.sources);
const sourceConfigRef = computed(() => props.sourceConfig);
const {
  isLoading,
  isRefreshing,
  loadError,
  sourceStates,
  refresh,
  previewVariant,
  saveVariant,
  deleteVariant,
} = useModelRequestPatch(modelIdRef, providerIdRef, sourcesRef, sourceConfigRef);

const isEditorOpen = ref(false);
const editingSourceId = ref(0);
const editingSuffix = ref<string | null>(null);
const editingVariant = ref<RequestPatchVariantAggregate | null>(null);
const editingRequiresSuffix = ref(false);
const deletingSourceId = ref<number | null>(null);
const deletingVariant = ref<RequestPatchVariantAggregate | null>(null);
const isDeleting = ref(false);

const deletingVariantLabel = computed(() =>
  deletingVariant.value?.variant.suffix ?? t("requestPatchVariant.states.base"),
);

const allConflictCount = computed(() =>
  sourceStates.value.reduce(
    (total, state) => total + Object.values(state.evaluations).reduce((count, evaluation) => count + evaluation.conflicts.length, 0),
    0,
  ),
);

function findVariant(variants: RequestPatchVariantAggregate[], suffix: string | null) {
  return variants.find((item) => item.variant.suffix === suffix) ?? null;
}

function suffixesFor(state: ModelRequestPatchSourceState) {
  const suffixes = new Set<string>();
  for (const item of [...state.sourceVariants, ...state.modelVariants]) {
    if (item.variant.suffix !== null) suffixes.add(item.variant.suffix);
  }
  return [...suffixes].sort();
}

function variantCount(state: ModelRequestPatchSourceState) {
  return new Set([
    ...state.sourceVariants.map((item) => item.variant.suffix ?? ""),
    ...state.modelVariants.map((item) => item.variant.suffix ?? ""),
  ]).size;
}

function isSourceBound(sourceId: number) {
  const summary = props.sourceConfig;
  if (!summary || summary.source_selection_mode !== "EXPLICIT") return true;
  return summary.bindings.some((binding) => binding.source_id === sourceId);
}

function openEditor(sourceId: number, suffix: string | null, variant: RequestPatchVariantAggregate | null) {
  editingSourceId.value = sourceId;
  editingSuffix.value = suffix;
  editingVariant.value = variant;
  editingRequiresSuffix.value = false;
  isEditorOpen.value = true;
}

function openNewSuffix(sourceId: number) {
  editingSourceId.value = sourceId;
  editingSuffix.value = null;
  editingVariant.value = null;
  editingRequiresSuffix.value = true;
  isEditorOpen.value = true;
}

async function toggleInherited(sourceId: number, suffix: string | null, allowed: boolean) {
  if (allowed || !props.modelId) return;
  await saveVariant(sourceId, null, {
    source_id: sourceId,
    model_id: props.modelId,
    suffix,
    enabled: false,
    expose_in_models: false,
    rules: [],
  });
}

async function restoreVariant(sourceId: number, variant: RequestPatchVariantAggregate) {
  await deleteVariant(sourceId, variant.variant.id);
}

function requestDelete(sourceId: number, variant: RequestPatchVariantAggregate) {
  deletingSourceId.value = sourceId;
  deletingVariant.value = variant;
}

async function confirmDelete() {
  if (deletingSourceId.value === null || deletingVariant.value === null) return;
  isDeleting.value = true;
  try {
    const deleted = await deleteVariant(
      deletingSourceId.value,
      deletingVariant.value.variant.id,
    );
    if (deleted) {
      deletingVariant.value = null;
      deletingSourceId.value = null;
    }
  } finally {
    isDeleting.value = false;
  }
}

function previewEditingVariant(payload: RequestPatchVariantInput & { variant_id?: number | null }): Promise<RequestPatchPreviewResponse> {
  return previewVariant(editingSourceId.value, payload);
}

function saveEditingVariant(payload: RequestPatchVariantInput): Promise<boolean> {
  return saveVariant(editingSourceId.value, editingVariant.value?.variant.id ?? null, payload);
}
</script>
