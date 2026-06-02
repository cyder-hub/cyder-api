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
  <Drawer :open="open" direction="right" @update:open="(val) => emit('update:open', val)">
    <DrawerContent class="flex h-full w-full flex-col rounded-none rounded-l-2xl border-none bg-background sm:max-w-[600px] lg:max-w-[800px] xl:max-w-[1000px] right-0 left-auto mt-0 top-0">
      <DrawerHeader class="border-b border-gray-100 px-6 py-4 text-left">
        <DrawerTitle class="text-lg font-semibold text-gray-900">
          {{
            draft.id === null
              ? $t("costPage.catalogs.modal.titleAdd")
              : $t("costPage.catalogs.modal.titleEdit")
          }}
        </DrawerTitle>
      </DrawerHeader>
      <div class="flex-1 overflow-y-auto px-6 py-6">
        <div class="space-y-1.5">
          <Label for="catalog-name">{{ $t("costPage.catalogs.modal.name") }}</Label>
          <Input id="catalog-name" v-model="draft.name" />
        </div>
        <div class="space-y-1.5 pt-4">
          <Label for="catalog-description">{{ $t("costPage.catalogs.modal.description") }}</Label>
          <textarea
            id="catalog-description"
            v-model="draft.description"
            rows="4"
            class="flex min-h-[112px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm shadow-xs outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
          />
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
