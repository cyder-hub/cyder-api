<template>
  <div class="space-y-3">
    <SectionHeader
      :title="t(`${textPrefix}.directTitle`)"
      :help="t(`${textPrefix}.directDescription`)"
      :help-label="t(`${textPrefix}.directTitle`)"
    >
      <template #actions>
      <Button
        variant="outline"
        size="sm"
        class="w-full sm:w-auto"
        :disabled="!ownerReady"
        @click="openCreateDialog"
      >
        <Plus class="mr-1.5 h-4 w-4" />
        {{ t(`${textPrefix}.addRule`) }}
      </Button>
      </template>
    </SectionHeader>

    <div
      v-if="rules.length === 0"
      class="flex flex-col items-center justify-center rounded-lg border border-dashed border-gray-200 py-10"
    >
      <FileText class="mb-2 h-10 w-10 stroke-1 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">
        {{ t(`${textPrefix}.emptyDirect`) }}
      </span>
    </div>

    <div
      v-else
      class="divide-y divide-gray-100 border-y border-gray-100"
    >
      <div
        v-for="rule in rules"
        :key="rule.id"
        class="py-4"
      >
        <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0 flex-1 space-y-3">
            <div class="flex flex-wrap items-center gap-2">
              <Badge variant="outline" class="font-mono text-[11px]">
                {{ placementLabel(rule.placement) }}
              </Badge>
              <Badge variant="secondary" class="font-mono text-[11px]">
                {{ operationLabel(rule.operation) }}
              </Badge>
              <Badge
                :variant="resolveRuleState(rule).variant"
                class="text-[11px]"
              >
                {{ resolveRuleState(rule).label }}
              </Badge>
            </div>

            <p class="break-all font-mono text-sm text-gray-900">
              {{ rule.target }}
            </p>

            <div class="grid gap-3 text-sm text-gray-600 sm:grid-cols-2">
              <div>
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ t(`${textPrefix}.fields.value`) }}
                </p>
                <p class="mt-1 break-all font-mono text-sm text-gray-700">
                  {{ formatRequestPatchValueForDisplay(rule.value_json) }}
                </p>
              </div>
              <div>
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ t(`${textPrefix}.fields.description`) }}
                </p>
                <p class="mt-1 text-sm text-gray-600">
                  {{ rule.description || t(`${textPrefix}.noDescription`) }}
                </p>
              </div>
              <div>
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ t(`${textPrefix}.fields.ruleId`) }}
                </p>
                <p class="mt-1 font-mono text-xs text-gray-600">#{{ rule.id }}</p>
              </div>
              <div v-if="resolveRuleTrace(rule)">
                <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                  {{ t(`${textPrefix}.fields.trace`) }}
                </p>
                <p class="mt-1 text-sm text-gray-600">
                  {{ resolveRuleTrace(rule) }}
                </p>
              </div>
            </div>
          </div>

          <div class="flex flex-wrap items-center gap-2 sm:ml-4 sm:justify-end">
            <div class="flex items-center gap-2 rounded-md bg-gray-50/80 px-3 py-2">
              <span class="text-xs font-medium text-gray-500">
                {{ t(`${textPrefix}.fields.enabled`) }}
              </span>
              <Checkbox
                :model-value="rule.is_enabled"
                :disabled="isRuleBusy(rule.id)"
                @update:model-value="(checked) => handleToggleEnabled(rule, checked === true)"
              />
            </div>
            <Button
              variant="ghost"
              size="sm"
              class="h-8 px-2 text-gray-600"
              :disabled="isRuleBusy(rule.id)"
              @click="openEditDialog(rule)"
            >
              <Pencil class="h-4 w-4" />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="h-8 px-2 text-gray-400 hover:text-red-600"
              :disabled="isRuleBusy(rule.id)"
              @click="handleDeleteRule(rule)"
            >
              <Trash2 class="h-4 w-4" />
            </Button>
          </div>
        </div>
      </div>
    </div>

    <Dialog :open="isEditorOpen" @update:open="handleEditorOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-2xl">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{
              editorMode === "create"
                ? t(`${textPrefix}.editor.addTitle`)
                : t(`${textPrefix}.editor.editTitle`)
            }}
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            {{ t(`${textPrefix}.editor.description`) }}
          </DialogDescription>
        </DialogHeader>

        <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ t(`${textPrefix}.editor.placement`) }}
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Select v-model="editorForm.placement">
                <SelectTrigger class="w-full">
                  <SelectValue :placeholder="t(`${textPrefix}.editor.selectPlacement`)" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in placementOptions"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ option.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>

            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ t(`${textPrefix}.editor.operation`) }}
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Select v-model="editorForm.operation">
                <SelectTrigger class="w-full">
                  <SelectValue :placeholder="t(`${textPrefix}.editor.selectOperation`)" />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in operationOptions"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ option.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ t(`${textPrefix}.editor.target`) }}
              <span class="ml-0.5 text-red-500">*</span>
            </Label>
            <Input
              v-model="editorForm.target"
              :placeholder="targetPlaceholder"
              class="font-mono text-sm"
            />
            <p class="text-xs text-gray-500">
              {{ targetHelpText }}
            </p>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ t(`${textPrefix}.editor.valueJson`) }}
              <span v-if="editorForm.operation === 'SET'" class="ml-0.5 text-red-500">*</span>
            </Label>
            <textarea
              v-model="editorForm.value_json_text"
              :disabled="editorForm.operation === 'REMOVE'"
              :placeholder="valueJsonPlaceholder"
              class="min-h-32 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200 disabled:cursor-not-allowed disabled:bg-gray-50 disabled:text-gray-400"
            />
            <p class="text-xs text-gray-500">
              {{ t(`${textPrefix}.editor.valueHelp`) }}
            </p>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ t(`${textPrefix}.editor.descriptionLabel`) }}
            </Label>
            <textarea
              v-model="editorForm.description"
              :placeholder="t(`${textPrefix}.editor.descriptionPlaceholder`)"
              class="min-h-24 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200"
            />
          </div>

          <div class="flex items-center justify-between rounded-md bg-gray-50/80 p-3.5">
            <div>
              <p class="text-sm font-medium text-gray-900">
                {{ t(`${textPrefix}.editor.enabledTitle`) }}
              </p>
              <p class="mt-1 text-xs text-gray-500">
                {{ t(`${textPrefix}.editor.enabledDescription`) }}
              </p>
            </div>
            <Checkbox v-model="editorForm.is_enabled" />
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
          <Button
            variant="ghost"
            class="w-full text-gray-600 sm:w-auto"
            :disabled="isSubmitting"
            @click="isEditorOpen = false"
          >
            {{ t("common.cancel") }}
          </Button>
          <Button
            variant="default"
            class="w-full sm:w-auto"
            :disabled="isSubmitting"
            @click="handleSaveRule"
          >
            <Loader2 v-if="isSubmitting" class="mr-1.5 h-4 w-4 animate-spin" />
            {{
              editorMode === "create"
                ? t(`${textPrefix}.editor.create`)
                : t(`${textPrefix}.editor.save`)
            }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="isDangerDialogOpen" @update:open="handleDangerDialogOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-lg">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{ t(`${textPrefix}.editor.dangerousTitle`) }}
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            {{ t(`${textPrefix}.editor.dangerousDescription`) }}
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-4 px-4 py-4 sm:px-6">
          <div class="rounded-md bg-gray-50/80 px-4 py-3">
            <p class="text-xs font-medium uppercase tracking-wider text-gray-500">
              {{ t(`${textPrefix}.editor.target`) }}
            </p>
            <p class="mt-1 font-mono text-sm text-gray-900">
              {{ pendingDangerousConfirmation?.confirmation.placement }}
              {{ pendingDangerousConfirmation?.confirmation.target }}
            </p>
          </div>

          <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
            {{ pendingDangerousConfirmation?.confirmation.reason }}
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
          <Button
            variant="ghost"
            class="w-full text-gray-600 sm:w-auto"
            :disabled="isConfirmingDangerous"
            @click="cancelDangerousConfirmation"
          >
            {{ t("common.cancel") }}
          </Button>
          <Button
            variant="default"
            class="w-full sm:w-auto"
            :disabled="isConfirmingDangerous"
            @click="confirmDangerousSubmission"
          >
            <Loader2
              v-if="isConfirmingDangerous"
              class="mr-1.5 h-4 w-4 animate-spin"
            />
            {{ t(`${textPrefix}.editor.saveAnyway`) }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { normalizeError } from "@/utils/error";
import SectionHeader from "@/components/SectionHeader.vue";
import {
  buildRequestPatchPayloadFromEditor,
  formatRequestPatchValueForDisplay,
  formatRequestPatchValueForEditor,
} from "@/utils/requestPatch";
import { toastController } from "@/services/uiFeedback";
import type {
  RequestPatchDangerousTargetConfirmation,
  RequestPatchOperation,
  RequestPatchPayload,
  RequestPatchPlacement,
  RequestPatchRule,
  RequestPatchUpdatePayload,
} from "@/services/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  FileText,
  Loader2,
  Pencil,
  Plus,
  Trash2,
} from "lucide-vue-next";
import type {
  RequestPatchOwnerKind,
  RequestPatchRuleActions,
  RequestPatchRuleStateResolver,
  RequestPatchRuleTraceResolver,
} from "./types";

type EditorMode = "create" | "edit";

interface RequestPatchEditorForm {
  id: number | null;
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json_text: string;
  description: string;
  is_enabled: boolean;
}

interface PendingDangerousSubmission {
  mode: EditorMode;
  ruleId: number | null;
  payload: RequestPatchPayload | RequestPatchUpdatePayload;
  confirmation: RequestPatchDangerousTargetConfirmation;
}

const props = withDefaults(
  defineProps<{
    ownerKind: RequestPatchOwnerKind;
    ownerReady: boolean;
    rules: RequestPatchRule[];
    actions: RequestPatchRuleActions;
    getRuleState?: RequestPatchRuleStateResolver;
    getRuleTrace?: RequestPatchRuleTraceResolver;
  }>(),
  {
    getRuleState: undefined,
    getRuleTrace: undefined,
  },
);

const emit = defineEmits<{
  changed: [];
}>();

const { t } = useI18n();

const textPrefix = computed(() =>
  props.ownerKind === "provider"
    ? "providerEditPage.requestPatch"
    : "modelEditPage.requestPatch",
);

const placementOptions = computed<
  Array<{ value: RequestPatchPlacement; label: string }>
>(() => [
  {
    value: "HEADER",
    label: t(`${textPrefix.value}.placements.HEADER`),
  },
  {
    value: "QUERY",
    label: t(`${textPrefix.value}.placements.QUERY`),
  },
  { value: "BODY", label: t(`${textPrefix.value}.placements.BODY`) },
]);

const operationOptions = computed<
  Array<{ value: RequestPatchOperation; label: string }>
>(() => [
  { value: "SET", label: t(`${textPrefix.value}.operations.SET`) },
  {
    value: "REMOVE",
    label: t(`${textPrefix.value}.operations.REMOVE`),
  },
]);

const isEditorOpen = ref(false);
const isDangerDialogOpen = ref(false);
const isSubmitting = ref(false);
const isConfirmingDangerous = ref(false);
const activeRuleId = ref<number | null>(null);
const editorMode = ref<EditorMode>("create");
const pendingDangerousConfirmation = ref<PendingDangerousSubmission | null>(null);
const editorForm = ref<RequestPatchEditorForm>(createEmptyForm());

const targetPlaceholder = computed(() => {
  switch (editorForm.value.placement) {
    case "HEADER":
      return t(`${textPrefix.value}.editor.targetPlaceholderHeader`);
    case "QUERY":
      return t(`${textPrefix.value}.editor.targetPlaceholderQuery`);
    case "BODY":
      return t(`${textPrefix.value}.editor.targetPlaceholderBody`);
  }
});

const targetHelpText = computed(() => {
  switch (editorForm.value.placement) {
    case "HEADER":
      return t(`${textPrefix.value}.editor.targetHelpHeader`);
    case "QUERY":
      return t(`${textPrefix.value}.editor.targetHelpQuery`);
    case "BODY":
      return t(`${textPrefix.value}.editor.targetHelpBody`);
  }
});

const valueJsonPlaceholder = computed(() =>
  editorForm.value.operation === "REMOVE"
    ? t(`${textPrefix.value}.editor.removeValuePlaceholder`)
    : t(`${textPrefix.value}.editor.valuePlaceholder`),
);

function createEmptyForm(): RequestPatchEditorForm {
  return {
    id: null,
    placement: "BODY",
    target: "",
    operation: "SET",
    value_json_text: "",
    description: "",
    is_enabled: true,
  };
}

function placementLabel(placement: RequestPatchPlacement): string {
  return t(`${textPrefix.value}.placements.${placement}`);
}

function operationLabel(operation: RequestPatchOperation): string {
  return t(`${textPrefix.value}.operations.${operation}`);
}

function resolveRuleState(rule: RequestPatchRule) {
  if (props.getRuleState) return props.getRuleState(rule);
  if (!rule.is_enabled) {
    return {
      label: t(`${textPrefix.value}.states.disabled`),
      variant: "outline" as const,
    };
  }
  return {
    label: t(`${textPrefix.value}.states.enabled`),
    variant: "secondary" as const,
  };
}

function resolveRuleTrace(rule: RequestPatchRule): string | null {
  return props.getRuleTrace?.(rule) ?? null;
}

function isRuleBusy(ruleId: number): boolean {
  return activeRuleId.value === ruleId;
}

function ensureOwnerReady(): boolean {
  if (props.ownerReady) return true;
  toastController.warn(t(`${textPrefix.value}.alert.saveBeforeEdit`));
  return false;
}

function openCreateDialog() {
  if (!ensureOwnerReady()) return;

  editorMode.value = "create";
  editorForm.value = createEmptyForm();
  isEditorOpen.value = true;
}

function openEditDialog(rule: RequestPatchRule) {
  editorMode.value = "edit";
  editorForm.value = {
    id: rule.id,
    placement: rule.placement,
    target: rule.target,
    operation: rule.operation,
    value_json_text:
      rule.operation === "REMOVE"
        ? ""
        : formatRequestPatchValueForEditor(rule.value_json),
    description: rule.description || "",
    is_enabled: rule.is_enabled,
  };
  isEditorOpen.value = true;
}

function handleEditorOpenChange(open: boolean) {
  isEditorOpen.value = open;
  if (!open && !isSubmitting.value) {
    editorForm.value = createEmptyForm();
  }
}

function handleDangerDialogOpenChange(open: boolean) {
  if (!open && !isConfirmingDangerous.value) {
    pendingDangerousConfirmation.value = null;
  }
  isDangerDialogOpen.value = open;
}

function cancelDangerousConfirmation() {
  pendingDangerousConfirmation.value = null;
  isDangerDialogOpen.value = false;
}

function buildPayload(
  confirmDangerousTarget = false,
): RequestPatchPayload | RequestPatchUpdatePayload {
  if (!editorForm.value.target.trim()) {
    throw new Error(t(`${textPrefix.value}.alert.targetRequired`));
  }
  if (
    editorForm.value.operation === "SET" &&
    !editorForm.value.value_json_text.trim()
  ) {
    throw new Error(t(`${textPrefix.value}.alert.valueRequired`));
  }

  return buildRequestPatchPayloadFromEditor(
    editorForm.value,
    confirmDangerousTarget,
  );
}

async function submitRule(
  payload: RequestPatchPayload | RequestPatchUpdatePayload,
  mode: EditorMode,
  ruleId: number | null,
) {
  if (!ensureOwnerReady()) return;

  const outcome =
    mode === "create"
      ? await props.actions.createRule(payload as RequestPatchPayload)
      : await props.actions.updateRule(
          ruleId!,
          payload as RequestPatchUpdatePayload,
        );

  if (outcome.result === "confirmation_required") {
    pendingDangerousConfirmation.value = {
      mode,
      ruleId,
      payload,
      confirmation: outcome.confirmation,
    };
    isDangerDialogOpen.value = true;
    return;
  }

  emit("changed");
  isEditorOpen.value = false;
  pendingDangerousConfirmation.value = null;
  isDangerDialogOpen.value = false;
  editorForm.value = createEmptyForm();
  toastController.success(
    mode === "create"
      ? t(`${textPrefix.value}.alert.createSuccess`)
      : t(`${textPrefix.value}.alert.updateSuccess`),
  );
}

async function handleSaveRule() {
  if (!ensureOwnerReady()) return;

  try {
    isSubmitting.value = true;
    const payload = buildPayload(false);
    await submitRule(payload, editorMode.value, editorForm.value.id);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t(`${textPrefix.value}.alert.saveFailed`),
      normalizedError.message,
    );
  } finally {
    isSubmitting.value = false;
  }
}

async function confirmDangerousSubmission() {
  const pending = pendingDangerousConfirmation.value;
  if (!pending) return;

  try {
    isConfirmingDangerous.value = true;
    const payload = {
      ...(pending.payload as Record<string, unknown>),
      confirm_dangerous_target: true,
    } as RequestPatchPayload | RequestPatchUpdatePayload;
    await submitRule(payload, pending.mode, pending.ruleId);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t(`${textPrefix.value}.alert.dangerousConfirmFailed`),
      normalizedError.message,
    );
  } finally {
    isConfirmingDangerous.value = false;
  }
}

async function handleToggleEnabled(rule: RequestPatchRule, nextValue: boolean) {
  if (!ensureOwnerReady()) return;

  try {
    activeRuleId.value = rule.id;
    await props.actions.updateRule(rule.id, {
      is_enabled: nextValue,
    });
    emit("changed");
    toastController.success(
      nextValue
        ? t(`${textPrefix.value}.alert.enableSuccess`)
        : t(`${textPrefix.value}.alert.disableSuccess`),
    );
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t(`${textPrefix.value}.alert.toggleFailed`),
      normalizedError.message,
    );
  } finally {
    activeRuleId.value = null;
  }
}

async function handleDeleteRule(rule: RequestPatchRule) {
  if (!ensureOwnerReady()) return;

  try {
    activeRuleId.value = rule.id;
    await props.actions.deleteRule(rule.id);
    emit("changed");
    toastController.success(t(`${textPrefix.value}.alert.deleteSuccess`));
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t(`${textPrefix.value}.alert.deleteFailed`),
      normalizedError.message,
    );
  } finally {
    activeRuleId.value = null;
  }
}
</script>
