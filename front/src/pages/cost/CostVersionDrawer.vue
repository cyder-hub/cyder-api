<script setup lang="ts">
import { Button } from "@/components/ui/button";
import {
  Drawer,
  DrawerContent,
  DrawerFooter,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
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
  <Drawer :open="open" direction="right" @update:open="(val) => emit('update:open', val)">
    <DrawerContent class="flex h-full w-full flex-col rounded-none rounded-l-2xl border-none bg-background sm:max-w-[600px] lg:max-w-[800px] xl:max-w-[1000px] right-0 left-auto mt-0 top-0">
      <DrawerHeader class="border-b border-gray-100 px-6 py-4 text-left">
        <DrawerTitle class="text-lg font-semibold text-gray-900">
          {{ $t("costPage.versions.modal.titleAdd") }}
        </DrawerTitle>
      </DrawerHeader>
      <div class="flex-1 overflow-y-auto px-6 py-6 grid grid-cols-1 gap-4">
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
        <div
          class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5 sm:col-span-2"
        >
          <div>
            <Label for="version-enabled" class="text-sm font-medium text-gray-900">
              {{ $t("costPage.versions.modal.enabled") }}
            </Label>
            <div class="mt-1 text-xs leading-5 text-gray-500">
              {{ $t("costPage.versions.modal.enabledDescription") }}
            </div>
          </div>
          <Checkbox id="version-enabled" v-model="draft.is_enabled" />
        </div>
      </div>
      <DrawerFooter class="border-t border-gray-100 px-6 py-4">
        <div class="flex w-full justify-end gap-2">
          <Button variant="outline" @click="emit('update:open', false)">
            {{ $t("common.cancel") }}
          </Button>
          <Button :disabled="isSaving" @click="emit('save')">
            {{ isSaving ? $t("common.saving") : $t("common.save") }}
          </Button>
        </div>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>
</template>
