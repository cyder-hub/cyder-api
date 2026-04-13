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
import type { CatalogDraft } from "./types";

defineProps<{
  open: boolean;
  draft: CatalogDraft;
  isSaving: boolean;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "save"): void;
}>();
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-lg">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{
            draft.id === null
              ? $t("costPage.catalogs.modal.titleAdd")
              : $t("costPage.catalogs.modal.titleEdit")
          }}
        </DialogTitle>
      </DialogHeader>
      <div class="space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
        <div class="space-y-1.5">
          <Label for="catalog-name">{{ $t("costPage.catalogs.modal.name") }}</Label>
          <Input id="catalog-name" v-model="draft.name" />
        </div>
        <div class="space-y-1.5">
          <Label for="catalog-description">{{ $t("costPage.catalogs.modal.description") }}</Label>
          <textarea
            id="catalog-description"
            v-model="draft.description"
            rows="4"
            class="flex min-h-[112px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
          />
        </div>
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
