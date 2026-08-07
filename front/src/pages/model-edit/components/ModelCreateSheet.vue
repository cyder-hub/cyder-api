<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Loader2 } from "lucide-vue-next";

import * as modelService from "@/services/models";
import { toastController } from "@/services/uiFeedback";
import type { UpstreamSource } from "@/services/types";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Drawer, DrawerContent, DrawerDescription, DrawerFooter, DrawerHeader, DrawerTitle } from "@/components/ui/drawer";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import ModelSourceConfigEditor from "@/components/model-source-config/ModelSourceConfigEditor.vue";
import {
  createSourceConfigDraft,
  toSourceConfigPayload,
  type SourceConfigDraft,
} from "@/components/model-source-config/sourceConfigViewModel";
import type { EditingModelData } from "../types";

const props = defineProps<{
  open: boolean;
  providerId: number | null;
  sources: UpstreamSource[];
}>();

const emit = defineEmits<{
  "update:open": [value: boolean];
  saved: [model: EditingModelData];
}>();

const { t } = useI18n();
const isOpen = computed({
  get: () => props.open,
  set: (value: boolean) => emit("update:open", value),
});
const modelName = ref("");
const realModelName = ref("");
const isEnabled = ref(true);
const sourceConfigDraft = ref<SourceConfigDraft>(createSourceConfigDraft());
const isSaving = ref(false);
const error = ref<string | null>(null);

const resetDraft = () => {
  modelName.value = "";
  realModelName.value = "";
  isEnabled.value = true;
  sourceConfigDraft.value = createSourceConfigDraft(null, props.sources);
  error.value = null;
};

watch(
  () => props.open,
  (open) => {
    if (open) resetDraft();
  },
);

const handleSave = async () => {
  if (isSaving.value) return;
  const normalizedName = modelName.value.trim();
  if (!normalizedName) {
    toastController.warn(t("modelEditPage.alert.nameRequired"));
    return;
  }
  if (props.providerId === null) {
    error.value = t("providerEditPage.alert.providerNotSavedForModel");
    return;
  }

  isSaving.value = true;
  error.value = null;
  try {
    const saved = await modelService.createModel({
      provider_id: props.providerId,
      model_name: normalizedName,
      real_model_name: realModelName.value.trim() || null,
      is_enabled: isEnabled.value,
      source_config: toSourceConfigPayload(sourceConfigDraft.value),
    });
    toastController.success(t("modelEditPage.alert.createSuccess"));
    emit("saved", {
      id: saved.id,
      provider_id: props.providerId,
      cost_catalog_id: null,
      model_name: saved.model_name,
      real_model_name: saved.real_model_name ?? "",
      is_enabled: saved.is_enabled,
      request_patches: [],
      source_config: saved.source_config,
    });
    isOpen.value = false;
  } catch (caught: unknown) {
    const message = caught instanceof Error ? caught.message : t("common.unknownError");
    error.value = message;
    toastController.error(t("modelEditPage.alert.createFailed", { error: message }));
  } finally {
    isSaving.value = false;
  }
};
</script>

<template>
  <Drawer direction="right" v-model:open="isOpen">
    <DrawerContent class="flex flex-col p-0 outline-none">
      <DrawerHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DrawerTitle class="pr-8 text-base font-semibold text-gray-900 sm:text-lg">
          {{ t("modelEditPage.createTitle") }}
        </DrawerTitle>
        <DrawerDescription class="text-sm leading-5 text-gray-500">
          {{ t("modelEditPage.createDescription") }}
        </DrawerDescription>
      </DrawerHeader>

      <div class="min-h-0 flex-1 space-y-6 overflow-y-auto px-4 py-4 sm:px-6">
        <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div class="grid gap-1.5">
              <Label for="create-model-name" class="text-gray-700">
                {{ t("modelEditPage.labelModelName") }}
                <span class="ml-0.5 text-red-500">*</span>
              </Label>
              <Input id="create-model-name" v-model="modelName" autofocus />
            </div>
            <div class="grid gap-1.5">
              <Label for="create-real-model-name" class="text-gray-700">
                {{ t("modelEditPage.labelRealModelName") }}
              </Label>
              <Input id="create-real-model-name" v-model="realModelName" />
            </div>
          </div>

          <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
            <Label for="create-model-enabled" class="cursor-pointer text-gray-700">
              {{ t("modelEditPage.labelEnabled") }}
            </Label>
            <Checkbox id="create-model-enabled" v-model="isEnabled" />
          </div>
        </section>

        <ModelSourceConfigEditor
          v-model="sourceConfigDraft"
          :sources="sources"
          :show-save="false"
          :show-explain="false"
        />

        <div
          v-if="error"
          class="rounded-lg border border-red-200 bg-red-50 px-3.5 py-3 text-sm text-red-700"
        >
          {{ error }}
        </div>
      </div>

      <DrawerFooter class="border-t border-gray-100 sm:flex-row sm:justify-end">
        <Button variant="outline" class="w-full sm:w-auto" :disabled="isSaving" @click="isOpen = false">
          {{ t("common.cancel") }}
        </Button>
        <Button class="w-full sm:w-auto" :disabled="isSaving" @click="handleSave">
          <Loader2 v-if="isSaving" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ t("common.save") }}
        </Button>
      </DrawerFooter>
    </DrawerContent>
  </Drawer>
</template>
