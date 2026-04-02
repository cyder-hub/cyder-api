<template>
  <div class="space-y-4 pt-4 border-t border-gray-100">
    <h3 class="text-lg font-semibold text-gray-900">
      {{ $t("providerEditPage.sectionCustomFields") }}
    </h3>

    <div class="border border-gray-200 rounded-lg overflow-hidden">
      <!-- Header -->
      <div class="grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-4 items-center px-4 py-3 bg-gray-50/80 border-b border-gray-200">
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">{{ $t("providerEditPage.tableHeaderFieldName") }}</span>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">{{ $t("providerEditPage.tableHeaderFieldValue") }}</span>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">{{ $t("providerEditPage.tableHeaderDescription") }}</span>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider">{{ $t("providerEditPage.tableHeaderFieldType") }}</span>
        <span class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right">{{ $t('common.actions') }}</span>
      </div>

      <div v-if="editingData.custom_fields.length === 0" class="flex flex-col items-center justify-center py-10">
        <FileText class="h-10 w-10 stroke-1 text-gray-400 mb-2" />
        <span class="text-sm font-medium text-gray-500">{{ $t('providerEditPage.alert.noCustomFields') }}</span>
      </div>

      <!-- Custom field rows -->
      <div
        v-for="(field, index) in editingData.custom_fields"
        :key="field.id"
        class="grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-4 items-center px-4 py-3 border-b border-gray-100 last:border-0 hover:bg-gray-50/50 transition-colors"
      >
        <Input
          :model-value="field.field_name"
          disabled
          class="font-mono text-sm h-8"
        />
        <Input
          :model-value="field.field_value"
          disabled
          class="font-mono text-sm h-8"
        />
        <Input
          :model-value="field.description ?? ''"
          disabled
          class="text-sm h-8"
        />
        <Badge variant="secondary" class="font-mono text-xs w-fit">{{
          field.field_type
        }}</Badge>
        <div class="flex justify-end">
          <Button
            variant="ghost"
            size="sm"
            class="h-8 text-gray-400 hover:text-red-600 px-2"
            @click="handleUnlinkCustomField(field.id!, index)"
          >
            <Trash2 class="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>

    <!-- Add custom field -->
    <div class="flex items-center gap-4 pt-2">
      <Select v-model="selectedCustomFieldId">
        <SelectTrigger class="w-64">
          <SelectValue
            :placeholder="$t('modelEditPage.placeholderSelectCustomField')"
          />
        </SelectTrigger>
        <SelectContent>
          <SelectItem
            v-for="f in availableCustomFields"
            :key="f.id"
            :value="String(f.id)"
          >
            {{ f.field_name }}
          </SelectItem>
        </SelectContent>
      </Select>
      <Button
        variant="outline"
        @click="handleLinkCustomField"
        :disabled="!selectedCustomFieldId"
      >
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("providerEditPage.buttonAddCustomField") }}
      </Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { toastController } from "@/lib/toastController";
import type { CustomFieldItem } from "@/store/types";
import type { EditingProviderData } from "./types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { FileText, Trash2, Plus } from "lucide-vue-next";

const { t: $t } = useI18n();

const props = defineProps<{
  allCustomFields: CustomFieldItem[];
}>();

const editingData = defineModel<EditingProviderData>("editingData", { required: true });

const selectedCustomFieldId = ref<string | null>(null);

const availableCustomFields = computed(() => {
  if (!editingData.value) return [];
  const linkedIds = new Set(editingData.value.custom_fields.map((f) => f.id));
  return props.allCustomFields.filter((f) => f.id && !linkedIds.has(f.id));
});

const handleLinkCustomField = async () => {
  const fieldIdStr = selectedCustomFieldId.value;
  const pId = editingData.value.id;

  if (!fieldIdStr) {
    toastController.warn($t("providerEditPage.alert.selectCustomField"));
    return;
  }
  if (!pId) {
    toastController.warn($t("providerEditPage.alert.saveProviderBeforeLink"));
    return;
  }

  const fieldId = Number(fieldIdStr);
  try {
    await Api.linkCustomField({
      custom_field_definition_id: fieldId,
      provider_id: pId,
      is_enabled: true,
    });

    const fieldToAdd = props.allCustomFields.find((f) => f.id === fieldId);
    if (fieldToAdd && editingData.value) {
      editingData.value.custom_fields.push({ ...fieldToAdd });
    }
    selectedCustomFieldId.value = null;
    toastController.success(
      $t("providerEditPage.alert.linkCustomFieldSuccess"),
    );
  } catch (error) {
    console.error("Failed to link custom field:", error);
    toastController.error(
      $t("providerEditPage.alert.linkCustomFieldFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  }
};

const handleUnlinkCustomField = async (fieldId: number, index: number) => {
  const pId = editingData.value.id;
  if (!pId) {
    toastController.warn($t("providerEditPage.alert.providerIdNotFound"));
    return;
  }

  try {
    await Api.unlinkCustomField({
      custom_field_definition_id: fieldId,
      provider_id: pId,
    });
    editingData.value.custom_fields.splice(index, 1);
    toastController.success(
      $t("providerEditPage.alert.unlinkCustomFieldSuccess"),
    );
  } catch (error) {
    console.error("Failed to unlink custom field:", error);
    toastController.error(
      $t("providerEditPage.alert.unlinkedCustomFieldFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  }
};
</script>
