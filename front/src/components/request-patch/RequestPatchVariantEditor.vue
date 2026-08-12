<template>
  <Drawer :open="props.open" @update:open="(open) => emit('update:open', open)">
    <DrawerContent
      :class="[
        'flex max-h-[94dvh] flex-col border-gray-200 bg-white p-0 outline-none',
        'md:h-full md:max-h-full md:max-w-2xl md:rounded-none',
      ]"
    >
      <DrawerHeader class="border-b border-gray-100 px-4 py-4 text-left sm:px-6">
        <DrawerTitle class="text-lg font-semibold text-gray-900">
          {{ props.variant ? t("requestPatchVariant.editor.editTitle") : t("requestPatchVariant.editor.addTitle") }}
        </DrawerTitle>
        <DrawerDescription class="mt-1 text-sm leading-6 text-gray-500">
          {{ props.readOnlyRules ? t("requestPatchVariant.editor.readOnlyDescription") : t("requestPatchVariant.editor.description") }}
        </DrawerDescription>
      </DrawerHeader>

      <div class="min-h-0 flex-1 space-y-5 overflow-y-auto px-4 py-5 sm:px-6">
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-1.5 sm:col-span-2">
            <Label class="text-gray-700">{{ t("requestPatchVariant.fields.suffix") }}</Label>
            <Input
              v-model="suffixText"
              :placeholder="t('requestPatchVariant.editor.suffixPlaceholder')"
              class="font-mono text-sm"
            />
            <p class="text-xs leading-5 text-gray-500">
              {{ t("requestPatchVariant.editor.suffixHelp") }}
            </p>
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <div>
              <Label class="text-gray-700">{{ t("requestPatchVariant.fields.enabled") }}</Label>
              <p class="mt-1 text-xs text-gray-500">{{ t("requestPatchVariant.editor.enabledHelp") }}</p>
            </div>
            <Checkbox v-model="draft.enabled" />
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <div>
              <Label class="text-gray-700">{{ t("requestPatchVariant.fields.expose") }}</Label>
              <p class="mt-1 text-xs text-gray-500">{{ t("requestPatchVariant.editor.exposeHelp") }}</p>
            </div>
            <Checkbox v-model="draft.expose_in_models" />
          </div>
        </div>

        <section class="space-y-3">
          <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h3 class="text-sm font-semibold text-gray-900">{{ t("requestPatchVariant.fields.rules") }}</h3>
              <p class="mt-1 text-xs leading-5 text-gray-500">
                {{ t("requestPatchVariant.editor.rulesHelp") }}
              </p>
            </div>
            <Button
              v-if="!props.readOnlyRules"
              variant="outline"
              size="sm"
              class="w-full sm:w-auto"
              @click="openRuleEditor()"
            >
              <Plus class="mr-1.5 h-4 w-4" />
              {{ t("requestPatchVariant.actions.addRule") }}
            </Button>
          </div>

          <div
            v-if="draft.rules.length === 0"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-7 text-sm text-gray-500"
          >
            {{ t("requestPatchVariant.emptyRules") }}
          </div>

          <div v-else class="divide-y divide-gray-100 rounded-lg border border-gray-200">
            <div v-for="(rule, index) in draft.rules" :key="`${rule.target}-${index}`" class="space-y-3 px-3 py-3.5 sm:px-4">
              <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0 flex-1 space-y-2">
                  <div class="flex flex-wrap items-center gap-1.5">
                    <Badge variant="outline" class="font-mono text-[10px]">
                      {{ t(`requestPatchVariant.placements.${rule.placement}`) }}
                    </Badge>
                    <Badge variant="secondary" class="font-mono text-[10px]">
                      {{ t(`requestPatchVariant.operations.${rule.operation}`) }}
                    </Badge>
                  </div>
                  <p class="break-all font-mono text-sm text-gray-900">{{ rule.target }}</p>
                  <p class="break-all font-mono text-xs text-gray-500">
                    {{ formatRequestPatchValueForDisplayFromText(rule.value_json_text, rule.operation) }}
                  </p>
                  <p v-if="rule.description" class="text-xs leading-5 text-gray-500">{{ rule.description }}</p>
                </div>
                <div v-if="!props.readOnlyRules" class="flex shrink-0 items-center gap-1 sm:justify-end">
                  <Button variant="ghost" size="icon-sm" class="text-gray-600" :aria-label="t('requestPatchVariant.actions.editRule')" @click="openRuleEditor(index)">
                    <Pencil class="h-4 w-4" />
                  </Button>
                  <Button variant="ghost" size="icon-sm" class="text-gray-400 hover:text-red-600" :aria-label="t('requestPatchVariant.actions.deleteRule')" @click="removeRule(index)">
                    <Trash2 class="h-4 w-4" />
                  </Button>
                </div>
              </div>
            </div>
          </div>
        </section>

        <div v-if="previewResponse" class="space-y-3 rounded-lg border border-gray-200 bg-gray-50/60 p-4">
          <div class="flex flex-wrap items-center justify-between gap-2">
            <h3 class="text-sm font-semibold text-gray-900">{{ t("requestPatchVariant.preview.title") }}</h3>
            <Badge :variant="previewResponse.preview.valid ? 'secondary' : 'destructive'" class="font-mono text-[10px]">
              {{ previewResponse.preview.valid ? t("requestPatchVariant.preview.valid") : t("requestPatchVariant.preview.invalid") }}
            </Badge>
          </div>
          <div class="grid gap-3 text-xs text-gray-600 sm:grid-cols-3">
            <div>
              <p class="font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.preview.ruleCount") }}</p>
              <p class="mt-1 font-mono text-gray-900">{{ previewResponse.preview.rule_count }}</p>
            </div>
            <div>
              <p class="font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.preview.affectedModels") }}</p>
              <p class="mt-1 font-mono text-gray-900">{{ previewResponse.preview.affected_model_count }}</p>
            </div>
            <div>
              <p class="font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.preview.conflicts") }}</p>
              <p class="mt-1 font-mono text-gray-900">{{ previewResponse.preview.conflicts.length }}</p>
            </div>
          </div>
          <p v-if="previewResponse.preview.failure_reason" class="text-xs leading-5 text-red-700">
            {{ previewResponse.preview.failure_reason }}
          </p>
          <div v-if="previewResponse.preview.conflicts.length > 0" class="space-y-2 text-xs text-red-700">
            <p v-for="conflict in previewResponse.preview.conflicts" :key="`${conflict.existing_variant_id}-${conflict.candidate_target}-${conflict.existing_target}`">
              {{ conflict.reason }}
            </p>
          </div>
          <div v-if="previewResponse.evaluation" class="space-y-3 rounded-md border border-gray-200 bg-white p-3">
            <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ t("requestPatchVariant.preview.layers") }}
            </p>
            <div class="flex flex-wrap gap-1.5">
              <Badge
                v-for="layer in previewResponse.evaluation.layers"
                :key="`${layer.origin}-${layer.variant_id}`"
                :variant="statusVariant(layer.status)"
                class="font-mono text-[10px]"
                :title="layer.reason ?? undefined"
              >
                {{ t(`requestPatchVariant.origins.${layer.origin}`) }} · {{ t(`requestPatchVariant.status.${layer.status}`) }}
              </Badge>
            </div>
            <div class="grid gap-3 text-xs text-gray-600 sm:grid-cols-3">
              <div>
                <p class="font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.preview.effectiveRules") }}</p>
                <p class="mt-1 font-mono text-gray-900">{{ previewResponse.evaluation.effective_rules.length }}</p>
              </div>
              <div>
                <p class="font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.preview.executable") }}</p>
                <p class="mt-1 font-medium text-gray-900">
                  {{ previewResponse.evaluation.executable ? t("requestPatchVariant.preview.yes") : t("requestPatchVariant.preview.no") }}
                </p>
              </div>
              <div>
                <p class="font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.preview.exposed") }}</p>
                <p class="mt-1 font-medium text-gray-900">
                  {{ previewResponse.evaluation.exposed_in_models ? t("requestPatchVariant.preview.yes") : t("requestPatchVariant.preview.no") }}
                </p>
              </div>
            </div>
            <p v-if="previewResponse.evaluation.failure_reason" class="text-xs leading-5 text-red-700">
              {{ previewResponse.evaluation.failure_reason }}
            </p>
          </div>
        </div>

        <p v-if="formError" class="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm leading-5 text-red-700">
          {{ formError }}
        </p>
      </div>

      <DrawerFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" :disabled="isBusy" @click="emit('update:open', false)">
          {{ t("common.cancel") }}
        </Button>
        <Button variant="outline" class="w-full sm:w-auto" :disabled="isBusy" @click="handlePreview">
          <Loader2 v-if="isPreviewing" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ t("requestPatchVariant.actions.preview") }}
        </Button>
        <Button class="w-full sm:w-auto" :disabled="isBusy" @click="handleSave">
          <Loader2 v-if="isSaving" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ t("common.save") }}
        </Button>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>

  <Drawer :open="isRuleEditorOpen" :direction="isDesktop ? 'right' : 'bottom'" @update:open="(open) => (isRuleEditorOpen = open)">
    <DrawerContent class="flex max-h-[94dvh] flex-col border-gray-200 bg-white p-0 outline-none md:h-full md:max-h-full md:max-w-xl md:rounded-none">
      <DrawerHeader class="border-b border-gray-100 px-4 py-4 text-left sm:px-6">
        <DrawerTitle class="text-lg font-semibold text-gray-900">
          {{ activeRuleIndex === null ? t("requestPatchVariant.editor.addRuleTitle") : t("requestPatchVariant.editor.editRuleTitle") }}
        </DrawerTitle>
        <DrawerDescription class="mt-1 text-sm text-gray-500">
          {{ t("requestPatchVariant.editor.ruleDescription") }}
        </DrawerDescription>
      </DrawerHeader>

      <div class="min-h-0 flex-1 space-y-4 overflow-y-auto px-4 py-5 sm:px-6">
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="space-y-1.5">
            <Label class="text-gray-700">{{ t("requestPatchVariant.fields.placement") }}</Label>
            <Select v-model="ruleForm.placement">
              <SelectTrigger class="w-full"><SelectValue :placeholder="t('requestPatchVariant.editor.selectPlacement')" /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="placement in placementOptions" :key="placement" :value="placement">
                  {{ t(`requestPatchVariant.placements.${placement}`) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <div class="space-y-1.5">
            <Label class="text-gray-700">{{ t("requestPatchVariant.fields.operation") }}</Label>
            <Select v-model="ruleForm.operation">
              <SelectTrigger class="w-full"><SelectValue :placeholder="t('requestPatchVariant.editor.selectOperation')" /></SelectTrigger>
              <SelectContent>
                <SelectItem v-for="operation in operationOptions" :key="operation" :value="operation">
                  {{ t(`requestPatchVariant.operations.${operation}`) }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div class="space-y-1.5">
          <Label class="text-gray-700">{{ t("requestPatchVariant.fields.target") }}</Label>
          <Input v-model="ruleForm.target" :placeholder="targetPlaceholder" class="font-mono text-sm" />
          <p class="text-xs leading-5 text-gray-500">{{ targetHelp }}</p>
        </div>

        <div class="space-y-1.5">
          <Label class="text-gray-700">
            {{ t("requestPatchVariant.fields.value") }}
            <span v-if="ruleForm.operation === 'SET'" class="ml-0.5 text-red-500">*</span>
          </Label>
          <textarea
            v-model="ruleForm.value_json_text"
            :disabled="ruleForm.operation === 'REMOVE'"
            :placeholder="ruleForm.operation === 'REMOVE' ? t('requestPatchVariant.editor.removeValuePlaceholder') : t('requestPatchVariant.editor.valuePlaceholder')"
            class="min-h-32 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200 disabled:cursor-not-allowed disabled:bg-gray-50 disabled:text-gray-400"
          />
          <p class="text-xs leading-5 text-gray-500">{{ t("requestPatchVariant.editor.valueHelp") }}</p>
        </div>

        <div class="space-y-1.5">
          <Label class="text-gray-700">{{ t("requestPatchVariant.fields.description") }}</Label>
          <textarea v-model="ruleForm.description" :placeholder="t('requestPatchVariant.editor.descriptionPlaceholder')" class="min-h-24 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200" />
        </div>

        <p v-if="ruleError" class="rounded-md border border-red-200 bg-red-50 px-3 py-2 text-sm leading-5 text-red-700">{{ ruleError }}</p>
      </div>

      <DrawerFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" @click="isRuleEditorOpen = false">{{ t("common.cancel") }}</Button>
        <Button class="w-full sm:w-auto" @click="saveRuleDraft">{{ t("common.save") }}</Button>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>

</template>

<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { useMediaQuery } from "@vueuse/core";
import { Loader2, Pencil, Plus, Trash2 } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { normalizeError } from "@/utils/error";
import {
  buildRequestPatchRuleInput,
  buildRequestPatchVariantPayload,
  formatRequestPatchValueForDisplay,
  type RequestPatchRuleEditorState,
  type RequestPatchVariantEditorState,
  variantEditorStateFromAggregate,
} from "@/utils/requestPatch";
import type {
  RequestPatchExplainStatus,
  RequestPatchOperation,
  RequestPatchPlacement,
  RequestPatchPreviewResponse,
  RequestPatchVariantAggregate,
  RequestPatchVariantInput,
} from "@/services/types";

type PreviewPayload = RequestPatchVariantInput & { variant_id?: number | null };

const props = defineProps<{
  open: boolean;
  sourceId: number;
  modelId?: number | null;
  initialSuffix?: string | null;
  variant: RequestPatchVariantAggregate | null;
  readOnlyRules?: boolean;
  requireSuffix?: boolean;
  onPreview: (payload: PreviewPayload) => Promise<RequestPatchPreviewResponse>;
  onSave: (payload: RequestPatchVariantInput) => Promise<boolean>;
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
}>();

const { t } = useI18n();
const isDesktop = useMediaQuery("(min-width: 768px)");
const draft = ref<RequestPatchVariantEditorState>(createEmptyVariant());
const ruleForm = ref<RequestPatchRuleEditorState>(createEmptyRule());
const activeRuleIndex = ref<number | null>(null);
const isRuleEditorOpen = ref(false);
const isPreviewing = ref(false);
const isSaving = ref(false);
const formError = ref<string | null>(null);
const ruleError = ref<string | null>(null);
const previewResponse = ref<RequestPatchPreviewResponse | null>(null);

function statusVariant(status: RequestPatchExplainStatus) {
  if (status === "Conflicted") return "destructive" as const;
  if (status === "Effective") return "secondary" as const;
  return "outline" as const;
}

const suffixText = computed({
  get: () => draft.value.suffix ?? "",
  set: (value: string) => {
    draft.value.suffix = value.trim() || null;
  },
});

function createEmptyRule(): RequestPatchRuleEditorState {
  return {
    placement: "HEADER",
    target: "",
    operation: "SET",
    value_json_text: "",
    description: "",
  };
}

function createEmptyVariant(): RequestPatchVariantEditorState {
  return {
    source_id: props.sourceId,
    model_id: props.modelId ?? null,
    suffix: props.initialSuffix ?? null,
    enabled: true,
    expose_in_models: false,
    rules: [],
  };
}

function resetDraft() {
  draft.value = props.variant
    ? variantEditorStateFromAggregate(props.variant)
    : createEmptyVariant();
  previewResponse.value = null;
  formError.value = null;
  ruleError.value = null;
}

watch(
  () => [props.open, props.variant?.variant.id, props.sourceId, props.modelId, props.initialSuffix],
  ([open]) => {
    if (open) resetDraft();
  },
  { immediate: true },
);

const placementOptions: RequestPatchPlacement[] = ["HEADER", "QUERY", "BODY"];
const operationOptions: RequestPatchOperation[] = ["SET", "REMOVE"];

const isBusy = computed(() => isPreviewing.value || isSaving.value);
const targetPlaceholder = computed(() => {
  switch (ruleForm.value.placement) {
    case "QUERY":
      return t("requestPatchVariant.editor.targetPlaceholderQuery");
    case "BODY":
      return t("requestPatchVariant.editor.targetPlaceholderBody");
    default:
      return t("requestPatchVariant.editor.targetPlaceholderHeader");
  }
});
const targetHelp = computed(() => t(`requestPatchVariant.editor.targetHelp${ruleForm.value.placement}`));

function formatRequestPatchValueForDisplayFromText(value: string, operation: RequestPatchOperation) {
  if (operation === "REMOVE") return t("requestPatchVariant.editor.removeValuePlaceholder");
  try {
    return formatRequestPatchValueForDisplay(JSON.stringify(JSON.parse(value)));
  } catch {
    return value || t("requestPatchVariant.editor.valuePlaceholder");
  }
}

function openRuleEditor(index?: number) {
  activeRuleIndex.value = index ?? null;
  ruleForm.value = index === undefined ? createEmptyRule() : { ...draft.value.rules[index] };
  ruleError.value = null;
  isRuleEditorOpen.value = true;
}

function saveRuleDraft() {
  const result = buildRequestPatchRuleInput(ruleForm.value);
  if (result.error) {
    ruleError.value = t(`requestPatchVariant.errors.${result.error}`);
    return;
  }
  if (activeRuleIndex.value === null) {
    draft.value.rules.push({ ...ruleForm.value, target: ruleForm.value.target.trim() });
  } else {
    draft.value.rules[activeRuleIndex.value] = { ...ruleForm.value, target: ruleForm.value.target.trim() };
  }
  isRuleEditorOpen.value = false;
  previewResponse.value = null;
}

function removeRule(index: number) {
  draft.value.rules.splice(index, 1);
  previewResponse.value = null;
}

function buildPayload(): { payload: RequestPatchVariantInput | null; error: string | null } {
  const state = {
    ...draft.value,
    source_id: props.sourceId,
    model_id: props.modelId ?? null,
    suffix: draft.value.suffix?.trim() || null,
  };
  if (props.requireSuffix && state.suffix === null) {
    return { payload: null, error: "suffixRequired" };
  }
  return buildRequestPatchVariantPayload(state);
}

async function runPreview(payload: RequestPatchVariantInput): Promise<RequestPatchPreviewResponse | null> {
  isPreviewing.value = true;
  formError.value = null;
  try {
    const result = await props.onPreview({
      ...payload,
      variant_id: props.variant?.variant.id ?? null,
    });
    previewResponse.value = result;
    return result;
  } catch (error: unknown) {
    formError.value = normalizeError(error, t("common.unknownError")).message;
    return null;
  } finally {
    isPreviewing.value = false;
  }
}

async function handlePreview() {
  const result = buildPayload();
  if (result.error || !result.payload) {
    formError.value = t(`requestPatchVariant.errors.${result.error ?? "invalid"}`);
    return;
  }
  await runPreview(result.payload);
}

async function handleSave() {
  const result = buildPayload();
  if (result.error || !result.payload) {
    formError.value = t(`requestPatchVariant.errors.${result.error ?? "invalid"}`);
    return;
  }
  const preview = await runPreview(result.payload);
  if (!preview) return;
  if (!preview.preview.valid) {
    formError.value = preview.preview.failure_reason || t("requestPatchVariant.errors.previewInvalid");
    return;
  }
  await commit(result.payload);
}

async function commit(payload: RequestPatchVariantInput) {
  isSaving.value = true;
  formError.value = null;
  try {
    const saved = await props.onSave(payload);
    if (saved) {
      emit("update:open", false);
    }
  } catch (error: unknown) {
    formError.value = normalizeError(error, t("common.unknownError")).message;
  } finally {
    isSaving.value = false;
  }
}
</script>
