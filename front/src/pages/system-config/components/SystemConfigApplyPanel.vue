<script setup lang="ts">
import { Check, Loader2, X } from "lucide-vue-next";

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
import type {
  SystemConfigField,
  SystemConfigPreviewResponse,
  SystemConfigValidationIssue,
} from "@/services/types";
import type { EditDraft, FieldBadge } from "../types";
import { toSelectValue } from "../composables/useSystemConfigReport";
import SystemConfigDiffPanel from "./SystemConfigDiffPanel.vue";

defineProps<{
  isEditOpen: boolean;
  selectedField: SystemConfigField | null;
  editError: string | null;
  isPreviewing: boolean;
  isApplying: boolean;
  preview: SystemConfigPreviewResponse | null;
  previewDiffRows: Array<{
    path: string;
    oldText: string;
    newText: string;
  }>;
  previewWarningRows: SystemConfigValidationIssue[];
  runtimeActionLabels: string[];
  draftValidationError: string | null;
  canApplyPreview: boolean;
  isResetOpen: boolean;
  resetError: string | null;
  resetTargetPaths: string[];
  isResetting: boolean;
  buildFieldBadges: (field: SystemConfigField) => FieldBadge[];
  enumOptionsForField: (field: SystemConfigField) => string[];
  writeDisabledReasonLabel: (reason: string) => string;
}>();

defineEmits<{
  editOpenChange: [open: boolean];
  resetOpenChange: [open: boolean];
  previewEdit: [];
  applyEdit: [];
  resetSelectedFields: [];
}>();

const editDraft = defineModel<EditDraft>("editDraft", { required: true });
const editReason = defineModel<string>("editReason", { required: true });
const resetReason = defineModel<string>("resetReason", { required: true });
</script>

<template>
  <Dialog :open="isEditOpen" @update:open="$emit('editOpenChange', $event)">
    <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-4xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ $t("systemConfigPage.edit.title") }}
        </DialogTitle>
        <DialogDescription v-if="selectedField" class="break-all font-mono text-xs text-gray-500">
          {{ selectedField.path }}
        </DialogDescription>
      </DialogHeader>

      <div v-if="selectedField" class="space-y-5 overflow-y-auto px-4 py-4 sm:px-6">
        <div
          v-if="editError"
          class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600"
        >
          {{ editError }}
        </div>

        <div class="rounded-lg border border-gray-200 bg-gray-50/60 p-3">
          <p class="text-sm text-gray-700">{{ selectedField.description }}</p>
          <div class="mt-2 flex flex-wrap gap-1.5">
            <Badge
              v-for="badge in buildFieldBadges(selectedField)"
              :key="`dialog-${selectedField.path}-${badge.key}`"
              variant="outline"
              :class="badge.class"
            >
              {{ badge.label }}
            </Badge>
          </div>
        </div>

        <div class="space-y-2">
          <Label>{{ $t("systemConfigPage.edit.value") }}</Label>

          <label
            v-if="selectedField.value_kind === 'bool'"
            class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5"
          >
            <span class="text-sm font-medium text-gray-700">
              {{ $t("systemConfigPage.edit.booleanValue") }}
            </span>
            <Checkbox v-model="editDraft.boolValue" />
          </label>

          <Select
            v-else-if="enumOptionsForField(selectedField).length"
            :model-value="editDraft.raw"
            @update:model-value="(value) => (editDraft.raw = toSelectValue(value))"
          >
            <SelectTrigger class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent :body-lock="false">
              <SelectItem
                v-for="option in enumOptionsForField(selectedField)"
                :key="option"
                :value="option"
              >
                {{ option }}
              </SelectItem>
            </SelectContent>
          </Select>

          <div
            v-else-if="
              selectedField.value_kind === 'nullable_string' ||
              selectedField.value_kind === 'nullable_u64'
            "
            class="space-y-3"
          >
            <label class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5">
              <span class="text-sm font-medium text-gray-700">
                {{ $t("systemConfigPage.edit.setNull") }}
              </span>
              <Checkbox v-model="editDraft.isNull" />
            </label>
            <Input
              v-model="editDraft.raw"
              :disabled="editDraft.isNull"
              :inputmode="selectedField.value_kind === 'nullable_u64' ? 'numeric' : 'text'"
            />
          </div>

          <Input
            v-else-if="
              selectedField.value_kind === 'u16' ||
              selectedField.value_kind === 'u32' ||
              selectedField.value_kind === 'u64' ||
              selectedField.value_kind === 'usize'
            "
            v-model="editDraft.raw"
            inputmode="numeric"
          />

          <Input
            v-else-if="selectedField.value_kind === 'string'"
            v-model="editDraft.raw"
          />

          <textarea
            v-else
            v-model="editDraft.raw"
            class="min-h-32 w-full rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none focus:border-gray-400"
          />

          <p v-if="draftValidationError" class="text-sm text-red-600">
            {{ draftValidationError }}
          </p>
        </div>

        <div class="space-y-2">
          <Label for="system-config-edit-reason">
            {{ $t("systemConfigPage.edit.reason") }}
          </Label>
          <Input
            id="system-config-edit-reason"
            v-model="editReason"
            :placeholder="$t('systemConfigPage.edit.reasonPlaceholder')"
          />
        </div>

        <SystemConfigDiffPanel
          v-if="preview"
          :preview="preview"
          :diff-rows="previewDiffRows"
          :warning-rows="previewWarningRows"
          :runtime-action-labels="runtimeActionLabels"
          :write-disabled-reason-label="writeDisabledReasonLabel"
        />
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="text-gray-600" @click="$emit('editOpenChange', false)">
          {{ $t("common.cancel") }}
        </Button>
        <Button
          variant="outline"
          :disabled="isPreviewing || isApplying || !!draftValidationError"
          @click="$emit('previewEdit')"
        >
          <Loader2 v-if="isPreviewing" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ $t("systemConfigPage.actions.preview") }}
        </Button>
        <Button :disabled="isApplying || !canApplyPreview" @click="$emit('applyEdit')">
          <Check v-if="!isApplying" class="mr-1.5 h-4 w-4" />
          <Loader2 v-else class="mr-1.5 h-4 w-4 animate-spin" />
          {{ $t("systemConfigPage.actions.apply") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>

  <Dialog :open="isResetOpen" @update:open="$emit('resetOpenChange', $event)">
    <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-lg">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ $t("systemConfigPage.reset.title") }}
        </DialogTitle>
        <DialogDescription class="text-sm text-gray-500">
          {{ $t("systemConfigPage.reset.description") }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
        <div
          v-if="resetError"
          class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600"
        >
          {{ resetError }}
        </div>

        <div class="rounded-lg border border-gray-200 bg-gray-50/60 p-3">
          <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("systemConfigPage.reset.paths") }}
          </p>
          <div class="mt-2 flex max-h-40 flex-wrap gap-1.5 overflow-y-auto">
            <Badge
              v-for="path in resetTargetPaths"
              :key="path"
              variant="outline"
              class="font-mono text-xs"
            >
              {{ path }}
            </Badge>
          </div>
        </div>

        <div class="space-y-2">
          <Label for="system-config-reset-reason">
            {{ $t("systemConfigPage.reset.reason") }}
          </Label>
          <Input
            id="system-config-reset-reason"
            v-model="resetReason"
            :placeholder="$t('systemConfigPage.reset.reasonPlaceholder')"
          />
        </div>
      </div>

      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button variant="ghost" class="text-gray-600" @click="$emit('resetOpenChange', false)">
          {{ $t("common.cancel") }}
        </Button>
        <Button
          :disabled="isResetting || !resetReason.trim() || !resetTargetPaths.length"
          @click="$emit('resetSelectedFields')"
        >
          <Loader2 v-if="isResetting" class="mr-1.5 h-4 w-4 animate-spin" />
          <X v-else class="mr-1.5 h-4 w-4" />
          {{ $t("systemConfigPage.actions.reset") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
