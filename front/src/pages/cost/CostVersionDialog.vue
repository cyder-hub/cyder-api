<script setup lang="ts">
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { VersionDraft } from "./types";

defineProps<{
  open: boolean;
  draft: VersionDraft;
  isSaving: boolean;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "save"): void;
}>();
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-2xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{ $t("costPage.versions.modal.titleAdd") }}
        </DialogTitle>
      </DialogHeader>
      <div class="grid grid-cols-1 gap-4 overflow-y-auto px-4 py-4 sm:grid-cols-2 sm:px-6">
        <div class="space-y-1.5">
          <Label for="version-name">{{ $t("costPage.versions.modal.version") }}</Label>
          <Input id="version-name" v-model="draft.version" />
        </div>
        <div class="space-y-1.5">
          <Label for="version-currency">{{ $t("costPage.versions.modal.currency") }}</Label>
          <Input id="version-currency" v-model="draft.currency" />
        </div>
        <div class="space-y-1.5 sm:col-span-2">
          <Label for="version-source">{{ $t("costPage.versions.modal.source") }}</Label>
          <Input id="version-source" v-model="draft.source" />
        </div>
        <div class="space-y-1.5">
          <Label for="version-effective-from">{{ $t("costPage.versions.modal.effectiveFrom") }}</Label>
          <Input id="version-effective-from" v-model="draft.effective_from" type="datetime-local" />
        </div>
        <div class="space-y-1.5">
          <Label for="version-effective-until">{{ $t("costPage.versions.modal.effectiveUntil") }}</Label>
          <Input id="version-effective-until" v-model="draft.effective_until" type="datetime-local" />
        </div>
        <label
          class="flex items-center justify-between rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3 sm:col-span-2"
        >
          <div>
            <div class="text-sm font-medium text-gray-900">
              {{ $t("costPage.versions.modal.enabled") }}
            </div>
            <div class="mt-1 text-sm text-gray-500">
              {{ $t("costPage.versions.modal.enabledDescription") }}
            </div>
          </div>
          <input v-model="draft.is_enabled" type="checkbox" class="h-4 w-4" />
        </label>
      </div>
      <DialogFooter
        class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6"
        :show-close-button="true"
      >
        <Button :disabled="isSaving" @click="emit('save')">
          {{ isSaving ? $t("common.saving") : $t("common.save") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
