<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div>
      <h3 class="text-lg font-semibold text-gray-900">
        {{ $t("providerEditPage.sectionCustomFields") }}
      </h3>
      <p class="mt-1 text-sm text-gray-500">
        {{ editingData.custom_fields.length }} items
      </p>
    </div>

    <div v-if="editingData.custom_fields.length === 0" class="flex flex-col items-center justify-center rounded-xl border border-dashed border-gray-200 py-10">
      <FileText class="mb-2 h-10 w-10 stroke-1 text-gray-400" />
      <span class="text-sm font-medium text-gray-500">{{ $t('providerEditPage.alert.noCustomFields') }}</span>
    </div>

    <div v-else class="space-y-3 md:hidden">
      <MobileCrudCard
        v-for="(field, index) in editingData.custom_fields"
        :key="field.id"
        :title="field.field_name"
        :description="field.description || '-'"
      >
        <div class="grid grid-cols-1 gap-3">
          <div class="space-y-1">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerEditPage.tableHeaderFieldValue") }}
            </p>
            <p class="break-all font-mono text-sm text-gray-700">
              {{ field.field_value }}
            </p>
          </div>
          <div class="space-y-1">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ $t("providerEditPage.tableHeaderFieldType") }}
            </p>
            <div>
              <Badge variant="secondary" class="w-fit font-mono text-xs">
                {{ field.field_type }}
              </Badge>
            </div>
          </div>
        </div>

        <template #actions>
          <Button
            variant="ghost"
            size="sm"
            class="w-full text-red-600 hover:bg-red-50 hover:text-red-700"
            @click="handleUnlinkCustomField(field.id!, index)"
          >
            <Trash2 class="mr-1.5 h-4 w-4" />
            {{ $t("common.delete") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
      <div class="grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-4 items-center border-b border-gray-200 bg-gray-50/80 px-4 py-3">
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t("providerEditPage.tableHeaderFieldName") }}</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t("providerEditPage.tableHeaderFieldValue") }}</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t("providerEditPage.tableHeaderDescription") }}</span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t("providerEditPage.tableHeaderFieldType") }}</span>
        <span class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">{{ $t('common.actions') }}</span>
      </div>

      <div
        v-for="(field, index) in editingData.custom_fields"
        :key="field.id"
        class="grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-4 items-center border-b border-gray-100 px-4 py-3 last:border-0 hover:bg-gray-50/50 transition-colors"
      >
        <Input
          :model-value="field.field_name"
          disabled
          class="h-8 font-mono text-sm"
        />
        <Input
          :model-value="field.field_value"
          disabled
          class="h-8 font-mono text-sm"
        />
        <Input
          :model-value="field.description ?? ''"
          disabled
          class="h-8 text-sm"
        />
        <Badge variant="secondary" class="w-fit font-mono text-xs">{{
          field.field_type
        }}</Badge>
        <div class="flex justify-end">
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-400 hover:text-red-600"
            @click="handleUnlinkCustomField(field.id!, index)"
          >
            <Trash2 class="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>

    <div class="flex flex-col gap-3 border-t border-gray-100 pt-3 sm:flex-row sm:items-center">
      <Select v-model="selectedCustomFieldId">
        <SelectTrigger class="w-full sm:w-64">
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
        class="w-full sm:w-auto"
        @click="handleLinkCustomField"
        :disabled="!selectedCustomFieldId"
      >
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("providerEditPage.buttonAddCustomField") }}
      </Button>
    </div>
  </section>
</template>

<script setup lang="ts">
import { ref, computed } from "vue";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { toastController } from "@/lib/toastController";
import type { CustomFieldItem } from "@/store/types";
import type { EditingProviderData } from "./types";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
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
