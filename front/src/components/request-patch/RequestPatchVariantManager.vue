<template>
  <div class="flex h-full min-h-0 flex-col">
    <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 sm:flex-row sm:items-start sm:justify-between sm:px-6">
      <div class="min-w-0">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.manager.eyebrow") }}</p>
        <h2 class="mt-1 truncate text-lg font-semibold text-gray-900">{{ props.title }}</h2>
        <p class="mt-1 text-sm leading-5 text-gray-500">{{ t("requestPatchVariant.manager.description") }}</p>
      </div>
      <div class="flex shrink-0 gap-2">
        <Button variant="ghost" size="sm" :disabled="isLoading || isRefreshing" @click="refresh">
          <RefreshCw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isRefreshing }" />
          {{ t("requestPatchVariant.actions.refresh") }}
        </Button>
        <Button size="sm" :disabled="isLoading" @click="openCreate">
          <Plus class="mr-1.5 h-4 w-4" />
          {{ t("requestPatchVariant.actions.addVariant") }}
        </Button>
      </div>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto px-4 py-5 sm:px-6">
      <div v-if="isLoading" class="flex items-center justify-center py-16 text-sm text-gray-500">
        <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
        {{ t("requestPatchVariant.manager.loading") }}
      </div>

      <div v-else-if="loadError" class="rounded-lg border border-red-200 bg-red-50 px-4 py-5">
        <p class="text-sm leading-5 text-red-700">{{ loadError }}</p>
        <Button class="mt-3" variant="outline" size="sm" @click="refresh">{{ t("common.retry") }}</Button>
      </div>

      <div v-else class="space-y-5">
        <div class="grid gap-3 sm:grid-cols-3">
          <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.manager.totalVariants") }}</p>
            <p class="mt-1 font-mono text-lg text-gray-900">{{ variants.length }}</p>
          </div>
          <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.manager.totalRules") }}</p>
            <p class="mt-1 font-mono text-lg text-gray-900">{{ ruleCount }}</p>
          </div>
          <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-3 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.manager.suffixVariants") }}</p>
            <p class="mt-1 font-mono text-lg text-gray-900">{{ suffixVariants.length }}</p>
          </div>
        </div>

        <div v-if="variants.length === 0" class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-10 text-center">
          <p class="text-sm font-medium text-gray-600">{{ t("requestPatchVariant.manager.empty") }}</p>
          <p class="mt-1 text-xs leading-5 text-gray-500">{{ t("requestPatchVariant.manager.emptyDescription") }}</p>
          <Button class="mt-4" variant="outline" size="sm" @click="openCreate">
            <Plus class="mr-1.5 h-4 w-4" />
            {{ t("requestPatchVariant.actions.addVariant") }}
          </Button>
        </div>

        <template v-else>
          <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
            <Table>
              <TableHeader>
                <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ t("requestPatchVariant.fields.variant") }}</TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ t("requestPatchVariant.fields.status") }}</TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ t("requestPatchVariant.fields.rules") }}</TableHead>
                  <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ t("requestPatchVariant.fields.expose") }}</TableHead>
                  <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">{{ t("common.actions") }}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow v-for="item in variants" :key="item.variant.id">
                  <TableCell>
                    <div class="flex flex-wrap items-center gap-1.5">
                      <Badge variant="outline" class="font-mono text-[10px]">{{ variantLabel(item) }}</Badge>
                      <span class="font-mono text-[11px] text-gray-500">#{{ item.variant.id }}</span>
                    </div>
                    <p v-if="isDisabledTombstone(item)" class="mt-1 text-xs text-gray-500">{{ t("requestPatchVariant.states.tombstone") }}</p>
                    <p v-else-if="isMetadataOnly(item)" class="mt-1 text-xs text-gray-500">{{ t("requestPatchVariant.states.metadataOverride") }}</p>
                  </TableCell>
                  <TableCell><Badge :variant="item.variant.enabled ? 'secondary' : 'outline'" class="font-mono text-[10px]">{{ item.variant.enabled ? t("requestPatchVariant.states.enabled") : t("requestPatchVariant.states.disabled") }}</Badge></TableCell>
                  <TableCell class="font-mono text-xs text-gray-700">{{ item.rules.length }}</TableCell>
                  <TableCell><Badge variant="outline" class="font-mono text-[10px]">{{ item.variant.expose_in_models ? t("common.yes") : t("common.no") }}</Badge></TableCell>
                  <TableCell class="text-right">
                    <div class="flex items-center justify-end gap-1">
                      <Button v-if="isDisabledTombstone(item)" variant="outline" size="sm" :disabled="isBusy(item.variant.id)" @click="restoreVariant(item)">{{ t("requestPatchVariant.actions.restore") }}</Button>
                      <Button v-else variant="ghost" size="icon-sm" class="text-gray-600" :aria-label="t('requestPatchVariant.actions.editVariant')" @click="openEdit(item)"><Pencil class="h-4 w-4" /></Button>
                      <Button v-if="!isDisabledTombstone(item)" variant="ghost" size="icon-sm" class="text-gray-400 hover:text-red-600" :aria-label="t('requestPatchVariant.actions.deleteVariant')" :disabled="isBusy(item.variant.id)" @click="askDelete(item)"><Trash2 class="h-4 w-4" /></Button>
                    </div>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>

          <div class="space-y-3 md:hidden">
            <MobileCrudCard v-for="item in variants" :key="item.variant.id" :title="variantLabel(item)" :description="`#${item.variant.id}`">
              <div class="flex flex-wrap gap-1.5">
                <Badge :variant="item.variant.enabled ? 'secondary' : 'outline'" class="font-mono text-[10px]">{{ item.variant.enabled ? t("requestPatchVariant.states.enabled") : t("requestPatchVariant.states.disabled") }}</Badge>
                <Badge variant="outline" class="font-mono text-[10px]">{{ t("requestPatchVariant.manager.ruleCount", { count: item.rules.length }) }}</Badge>
              </div>
              <p v-if="isDisabledTombstone(item)" class="mt-2 text-xs leading-5 text-gray-500">{{ t("requestPatchVariant.states.tombstoneDescription") }}</p>
              <p v-else-if="isMetadataOnly(item)" class="mt-2 text-xs leading-5 text-gray-500">{{ t("requestPatchVariant.states.metadataOverrideDescription") }}</p>
              <template #actions>
                <Button v-if="isDisabledTombstone(item)" variant="outline" size="sm" class="w-full justify-center" :disabled="isBusy(item.variant.id)" @click="restoreVariant(item)">{{ t("requestPatchVariant.actions.restore") }}</Button>
                <Button v-else variant="ghost" size="sm" class="w-full justify-center" @click="openEdit(item)"><Pencil class="mr-1.5 h-3.5 w-3.5" />{{ t("common.edit") }}</Button>
                <Button v-if="!isDisabledTombstone(item)" variant="ghost" size="sm" class="w-full justify-center text-gray-400 hover:text-red-600" :disabled="isBusy(item.variant.id)" @click="askDelete(item)"><Trash2 class="mr-1.5 h-3.5 w-3.5" />{{ t("common.delete") }}</Button>
              </template>
            </MobileCrudCard>
          </div>
        </template>
      </div>
    </div>
  </div>

  <RequestPatchVariantEditor
    v-model:open="isEditorOpen"
    :source-id="props.sourceId"
    :model-id="props.modelId"
    :variant="editingVariant"
    :on-preview="previewVariant"
    :on-save="saveVariant"
  />

  <Dialog :open="isDeleteDialogOpen" @update:open="(open) => (isDeleteDialogOpen = open)">
    <DialogContent class="border border-gray-200 bg-white sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ deletingIsTombstone ? t("requestPatchVariant.restore.title") : t("requestPatchVariant.delete.title") }}</DialogTitle>
        <DialogDescription>
          {{ deletingIsTombstone
            ? t("requestPatchVariant.restore.description", { variant: deletingVariant ? variantLabel(deletingVariant) : "" })
            : t("requestPatchVariant.delete.description", { variant: deletingVariant ? variantLabel(deletingVariant) : "" }) }}
        </DialogDescription>
      </DialogHeader>
      <DialogFooter>
        <Button variant="ghost" class="text-gray-600" :disabled="isDeleting" @click="isDeleteDialogOpen = false">{{ t("common.cancel") }}</Button>
        <Button :variant="deletingIsTombstone ? 'default' : 'destructive'" :disabled="isDeleting" @click="deleteVariant">
          <Loader2 v-if="isDeleting" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ deletingIsTombstone ? t("requestPatchVariant.restore.confirm") : t("common.delete") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
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
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { normalizeError } from "@/utils/error";
import { toastController } from "@/services/uiFeedback";
import * as requestPatchService from "@/services/requestPatch";
import type {
  RequestPatchPreviewResponse,
  RequestPatchVariantAggregate,
  RequestPatchVariantInput,
} from "@/services/types";
import RequestPatchVariantEditor from "./RequestPatchVariantEditor.vue";
import { Loader2, Pencil, Plus, RefreshCw, Trash2 } from "lucide-vue-next";

const props = defineProps<{
  open: boolean;
  providerId?: number | null;
  modelId?: number | null;
  sourceId: number;
  title: string;
  mode: "source" | "model";
}>();

const { t } = useI18n();
const variants = ref<RequestPatchVariantAggregate[]>([]);
const isLoading = ref(false);
const isRefreshing = ref(false);
const loadError = ref<string | null>(null);
const isEditorOpen = ref(false);
const editingVariant = ref<RequestPatchVariantAggregate | null>(null);
const isDeleteDialogOpen = ref(false);
const deletingVariant = ref<RequestPatchVariantAggregate | null>(null);
const isDeleting = ref(false);
const busyIds = ref(new Set<number>());
const deletingIsTombstone = computed(() =>
  deletingVariant.value ? isDisabledTombstone(deletingVariant.value) : false,
);

const suffixVariants = computed(() => variants.value.filter((item) => item.variant.suffix !== null));
const ruleCount = computed(() => variants.value.reduce((total, item) => total + item.rules.length, 0));

function isBusy(id: number) {
  return busyIds.value.has(id);
}

function isDisabledTombstone(item: RequestPatchVariantAggregate) {
  return !item.variant.enabled && item.rules.length === 0;
}

function isMetadataOnly(item: RequestPatchVariantAggregate) {
  return item.variant.enabled && item.rules.length === 0;
}

function variantLabel(item: RequestPatchVariantAggregate) {
  return item.variant.suffix ?? t("requestPatchVariant.states.base");
}

async function refresh() {
  const sourceId = props.sourceId;
  if (!sourceId || (props.mode === "source" && !props.providerId) || (props.mode === "model" && !props.modelId)) return;
  loadError.value = null;
  if (variants.value.length === 0) isLoading.value = true;
  else isRefreshing.value = true;
  try {
    const response = props.mode === "source"
      ? await requestPatchService.listSourceRequestPatchVariants(props.providerId!, sourceId)
      : await requestPatchService.listModelSourceRequestPatchVariants(props.modelId!, sourceId);
    variants.value = response.variants;
  } catch (error: unknown) {
    loadError.value = normalizeError(error, t("common.unknownError")).message;
  } finally {
    isLoading.value = false;
    isRefreshing.value = false;
  }
}

watch(() => [props.open, props.sourceId, props.providerId, props.modelId, props.mode], ([open]) => {
  if (open) void refresh();
}, { immediate: true });

function openCreate() {
  editingVariant.value = null;
  isEditorOpen.value = true;
}

function openEdit(item: RequestPatchVariantAggregate) {
  editingVariant.value = item;
  isEditorOpen.value = true;
}

function askDelete(item: RequestPatchVariantAggregate) {
  deletingVariant.value = item;
  isDeleteDialogOpen.value = true;
}

async function deleteVariant() {
  if (!deletingVariant.value) return;
  const id = deletingVariant.value.variant.id;
  const restoring = isDisabledTombstone(deletingVariant.value);
  busyIds.value = new Set(busyIds.value).add(id);
  isDeleting.value = true;
  try {
    if (props.mode === "source") {
      await requestPatchService.deleteSourceRequestPatchVariant(props.providerId!, props.sourceId, id);
    } else {
      await requestPatchService.deleteModelSourceRequestPatchVariant(props.modelId!, props.sourceId, id);
    }
    toastController.success(t(restoring ? "requestPatchVariant.alert.restored" : "requestPatchVariant.alert.deleted"));
    isDeleteDialogOpen.value = false;
    await refresh();
  } catch (error: unknown) {
    toastController.error(t("requestPatchVariant.alert.deleteFailed", { error: normalizeError(error, t("common.unknownError")).message }));
  } finally {
    isDeleting.value = false;
    const next = new Set(busyIds.value);
    next.delete(id);
    busyIds.value = next;
  }
}

async function restoreVariant(item: RequestPatchVariantAggregate) {
  askDelete(item);
}

function previewVariant(payload: RequestPatchVariantInput & { variant_id?: number | null }): Promise<RequestPatchPreviewResponse> {
  return props.mode === "source"
    ? requestPatchService.previewSourceRequestPatchVariant(props.providerId!, props.sourceId, payload)
    : requestPatchService.previewModelSourceRequestPatchVariant(props.modelId!, props.sourceId, payload);
}

async function saveVariant(payload: RequestPatchVariantInput): Promise<boolean> {
  try {
    if (editingVariant.value) {
      if (props.mode === "source") {
        await requestPatchService.updateSourceRequestPatchVariant(props.providerId!, props.sourceId, editingVariant.value.variant.id, payload);
      } else {
        await requestPatchService.updateModelSourceRequestPatchVariant(props.modelId!, props.sourceId, editingVariant.value.variant.id, payload);
      }
      toastController.success(t("requestPatchVariant.alert.updated"));
    } else {
      if (props.mode === "source") {
        await requestPatchService.createSourceRequestPatchVariant(props.providerId!, props.sourceId, payload);
      } else {
        await requestPatchService.createModelSourceRequestPatchVariant(props.modelId!, props.sourceId, payload);
      }
      toastController.success(t("requestPatchVariant.alert.created"));
    }
    await refresh();
    return true;
  } catch (error: unknown) {
    toastController.error(t("requestPatchVariant.alert.saveFailed", { error: normalizeError(error, t("common.unknownError")).message }));
    return false;
  }
}
</script>
