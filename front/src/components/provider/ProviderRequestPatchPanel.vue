<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div>
        <h3 class="text-lg font-semibold text-gray-900">Request Patches</h3>
        <p class="mt-1 text-sm text-gray-500">
          {{ editingData.request_patches.length }} direct rules. These rules are
          inherited by downstream models, and exact-target model rules can override
          them.
        </p>
      </div>
      <Button
        variant="outline"
        size="sm"
        class="w-full sm:w-auto"
        :disabled="!editingData.id"
        @click="openCreateDialog"
      >
        <Plus class="mr-1.5 h-4 w-4" />
        Add Rule
      </Button>
    </div>

    <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-4 py-3 text-sm text-gray-500">
      Provider rules only define direct patches. Inherited, effective, and conflict
      views stay on the model page.
    </div>

    <div
      v-if="editingData.request_patches.length === 0"
      class="flex flex-col items-center justify-center rounded-xl border border-dashed border-gray-200 py-10"
    >
      <FileText class="mb-2 h-10 w-10 stroke-1 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">
        No direct request patches configured.
      </span>
    </div>

    <div v-else class="space-y-3 md:hidden">
      <MobileCrudCard
        v-for="rule in editingData.request_patches"
        :key="rule.id"
        :title="rule.target"
        :description="rule.description || rule.operation"
      >
        <div class="space-y-3">
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              Placement
            </span>
            <Badge variant="outline" class="font-mono text-xs">
              {{ rule.placement }}
            </Badge>
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              Operation
            </span>
            <Badge variant="secondary" class="font-mono text-xs">
              {{ rule.operation }}
            </Badge>
          </div>

          <div class="rounded-lg border border-gray-100 px-3 py-2.5">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              Value
            </p>
            <p class="mt-1 break-all font-mono text-sm text-gray-700">
              {{ formatRequestPatchValueForDisplay(rule.value_json) }}
            </p>
          </div>

          <div class="rounded-lg border border-gray-100 px-3 py-2.5">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              Description
            </p>
            <p class="mt-1 text-sm text-gray-600">
              {{ rule.description || "No description" }}
            </p>
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <span class="text-sm text-gray-700">Enabled</span>
            <Checkbox
              :model-value="rule.is_enabled"
              :disabled="isRuleBusy(rule.id)"
              @update:model-value="(checked) => handleToggleEnabled(rule, checked === true)"
            />
          </div>
        </div>

        <template #actions>
          <div class="grid grid-cols-2 gap-2">
            <Button
              variant="outline"
              size="sm"
              class="w-full"
              :disabled="isRuleBusy(rule.id)"
              @click="openEditDialog(rule)"
            >
              <Pencil class="mr-1.5 h-4 w-4" />
              Edit
            </Button>
            <Button
              variant="ghost"
              size="sm"
              class="w-full text-red-600 hover:bg-red-50 hover:text-red-700"
              :disabled="isRuleBusy(rule.id)"
              @click="handleDeleteRule(rule)"
            >
              <Trash2 class="mr-1.5 h-4 w-4" />
              Delete
            </Button>
          </div>
        </template>
      </MobileCrudCard>
    </div>

    <div
      v-if="editingData.request_patches.length > 0"
      class="hidden overflow-hidden rounded-lg border border-gray-200 md:block"
    >
      <div
        class="grid grid-cols-[0.8fr_1.3fr_0.8fr_1.5fr_1.1fr_0.7fr_auto] items-center gap-4 border-b border-gray-200 bg-gray-50/80 px-4 py-3"
      >
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">Placement</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">Target</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">Operation</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">Value</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">Description</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">Enabled</span>
        <span class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">Actions</span>
      </div>

      <div
        v-for="rule in editingData.request_patches"
        :key="rule.id"
        class="grid grid-cols-[0.8fr_1.3fr_0.8fr_1.5fr_1.1fr_0.7fr_auto] items-center gap-4 border-b border-gray-100 px-4 py-3 last:border-0 hover:bg-gray-50/50"
      >
        <Badge variant="outline" class="w-fit font-mono text-xs">
          {{ rule.placement }}
        </Badge>
        <p class="break-all font-mono text-sm text-gray-900">
          {{ rule.target }}
        </p>
        <Badge variant="secondary" class="w-fit font-mono text-xs">
          {{ rule.operation }}
        </Badge>
        <p class="break-all font-mono text-sm text-gray-600">
          {{ formatRequestPatchValueForDisplay(rule.value_json) }}
        </p>
        <p class="text-sm text-gray-500">
          {{ rule.description || "No description" }}
        </p>
        <div class="flex items-center">
          <Checkbox
            :model-value="rule.is_enabled"
            :disabled="isRuleBusy(rule.id)"
            @update:model-value="(checked) => handleToggleEnabled(rule, checked === true)"
          />
        </div>
        <div class="flex items-center justify-end gap-1">
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

    <Dialog :open="isEditorOpen" @update:open="handleEditorOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-2xl">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{ editorMode === "create" ? "Add Provider Request Patch" : "Edit Provider Request Patch" }}
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            Define a direct provider rule for headers, query parameters, or request body targets.
          </DialogDescription>
        </DialogHeader>

        <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label class="text-gray-700">
                Placement
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Select v-model="editorForm.placement">
                <SelectTrigger class="w-full">
                  <SelectValue placeholder="Select placement" />
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
                Operation
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Select v-model="editorForm.operation">
                <SelectTrigger class="w-full">
                  <SelectValue placeholder="Select operation" />
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
              Target
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
              Value JSON
              <span v-if="editorForm.operation === 'SET'" class="ml-0.5 text-red-500">*</span>
            </Label>
            <textarea
              v-model="editorForm.value_json_text"
              :disabled="editorForm.operation === 'REMOVE'"
              :placeholder="valueJsonPlaceholder"
              class="min-h-32 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200 disabled:cursor-not-allowed disabled:bg-gray-50 disabled:text-gray-400"
            />
            <p class="text-xs text-gray-500">
              HEADER and QUERY rules only accept JSON scalar values. BODY rules can use any JSON value.
            </p>
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">Description</Label>
            <textarea
              v-model="editorForm.description"
              placeholder="Optional note for operators and future debugging."
              class="min-h-24 w-full rounded-lg border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-300 focus:ring-2 focus:ring-gray-200"
            />
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <div>
              <p class="text-sm font-medium text-gray-900">Enabled</p>
              <p class="mt-1 text-xs text-gray-500">
                Disabled rules stay on the provider but are skipped at runtime.
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
            Cancel
          </Button>
          <Button
            variant="default"
            class="w-full sm:w-auto"
            :disabled="isSubmitting"
            @click="handleSaveRule"
          >
            <Loader2 v-if="isSubmitting" class="mr-1.5 h-4 w-4 animate-spin" />
            {{ editorMode === "create" ? "Create Rule" : "Save Changes" }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="isDangerDialogOpen" @update:open="handleDangerDialogOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-lg">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            Confirm Dangerous Target
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            This rule touches an upstream-sensitive target and needs an explicit confirmation.
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-4 px-4 py-4 sm:px-6">
          <div class="rounded-lg border border-gray-200 bg-gray-50/60 px-4 py-3">
            <p class="text-xs font-medium uppercase tracking-wider text-gray-500">
              Placement / Target
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
            Cancel
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
            Save Anyway
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </section>
</template>

<script setup lang="ts">
import { computed, ref } from "vue";
import { Api } from "@/services/request";
import { normalizeError } from "@/lib/error";
import {
  buildRequestPatchPayloadFromEditor,
  formatRequestPatchValueForDisplay,
  formatRequestPatchValueForEditor,
} from "@/lib/requestPatch";
import { toastController } from "@/lib/toastController";
import type {
  RequestPatchDangerousTargetConfirmation,
  RequestPatchOperation,
  RequestPatchPayload,
  RequestPatchPlacement,
  RequestPatchRule,
  RequestPatchUpdatePayload,
} from "@/store/types";
import type { EditingProviderData } from "./types";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
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
import { FileText, Loader2, Pencil, Plus, Trash2 } from "lucide-vue-next";

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

const editingData = defineModel<EditingProviderData>("editingData", {
  required: true,
});

const placementOptions: Array<{ value: RequestPatchPlacement; label: string }> = [
  { value: "HEADER", label: "HEADER" },
  { value: "QUERY", label: "QUERY" },
  { value: "BODY", label: "BODY" },
];

const operationOptions: Array<{ value: RequestPatchOperation; label: string }> = [
  { value: "SET", label: "SET" },
  { value: "REMOVE", label: "REMOVE" },
];

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
      return "authorization";
    case "QUERY":
      return "api-version";
    case "BODY":
      return "/generationConfig/temperature";
  }
});

const targetHelpText = computed(() => {
  switch (editorForm.value.placement) {
    case "HEADER":
      return "Use a header name. Reserved transport headers are rejected by the backend.";
    case "QUERY":
      return "Use a query parameter key without ?, &, =, or whitespace.";
    case "BODY":
      return "Use a JSON Pointer target, for example /model or /generationConfig/temperature.";
  }
});

const valueJsonPlaceholder = computed(() =>
  editorForm.value.operation === "REMOVE"
    ? "REMOVE does not accept value_json."
    : 'Use valid JSON, for example: "Bearer token", true, 1, null, {"key":"value"}',
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

function isRuleBusy(ruleId: number): boolean {
  return activeRuleId.value === ruleId;
}

async function refreshRules() {
  if (!editingData.value.id) {
    return;
  }

  const rules = await Api.listProviderRequestPatches(editingData.value.id);
  editingData.value.request_patches = rules;
}

function openCreateDialog() {
  if (!editingData.value.id) {
    toastController.warn("Save the provider before adding request patch rules.");
    return;
  }

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
  if (!editingData.value.id) {
    toastController.warn("Save the provider before editing request patch rules.");
    return;
  }

  const providerId = editingData.value.id;
  const outcome =
    mode === "create"
      ? await Api.createProviderRequestPatch(providerId, payload as RequestPatchPayload)
      : await Api.updateProviderRequestPatch(
          providerId,
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

  await refreshRules();
  isEditorOpen.value = false;
  pendingDangerousConfirmation.value = null;
  isDangerDialogOpen.value = false;
  editorForm.value = createEmptyForm();
  toastController.success(
    mode === "create"
      ? "Provider request patch created."
      : "Provider request patch updated.",
  );
}

async function handleSaveRule() {
  if (!editingData.value.id) {
    toastController.warn("Save the provider before editing request patch rules.");
    return;
  }

  try {
    isSubmitting.value = true;
    const payload = buildPayload(false);
    await submitRule(payload, editorMode.value, editorForm.value.id);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, "Failed to save request patch.");
    toastController.error(normalizedError.message);
  } finally {
    isSubmitting.value = false;
  }
}

async function confirmDangerousSubmission() {
  const pending = pendingDangerousConfirmation.value;
  if (!pending) {
    return;
  }

  try {
    isConfirmingDangerous.value = true;
    const payload = Object.assign({}, pending.payload, {
      confirm_dangerous_target: true,
    }) as unknown as RequestPatchPayload | RequestPatchUpdatePayload;
    await submitRule(payload, pending.mode, pending.ruleId);
  } catch (error: unknown) {
    const normalizedError = normalizeError(
      error,
      "Failed to confirm dangerous request patch target.",
    );
    toastController.error(normalizedError.message);
  } finally {
    isConfirmingDangerous.value = false;
  }
}

async function handleToggleEnabled(rule: RequestPatchRule, nextValue: boolean) {
  if (!editingData.value.id) {
    toastController.warn("Save the provider before editing request patch rules.");
    return;
  }

  try {
    activeRuleId.value = rule.id;
    await Api.updateProviderRequestPatch(editingData.value.id, rule.id, {
      is_enabled: nextValue,
    });
    await refreshRules();
    toastController.success(
      nextValue ? "Request patch enabled." : "Request patch disabled.",
    );
  } catch (error: unknown) {
    const normalizedError = normalizeError(
      error,
      "Failed to update request patch status.",
    );
    toastController.error(normalizedError.message);
  } finally {
    activeRuleId.value = null;
  }
}

async function handleDeleteRule(rule: RequestPatchRule) {
  if (!editingData.value.id) {
    toastController.warn("Save the provider before editing request patch rules.");
    return;
  }

  try {
    activeRuleId.value = rule.id;
    await Api.deleteProviderRequestPatch(editingData.value.id, rule.id);
    await refreshRules();
    toastController.success("Provider request patch deleted.");
  } catch (error: unknown) {
    const normalizedError = normalizeError(
      error,
      "Failed to delete provider request patch.",
    );
    toastController.error(normalizedError.message);
  } finally {
    activeRuleId.value = null;
  }
}
</script>
