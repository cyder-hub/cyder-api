<template>
  <CrudPageLayout
    :title="$t('customFieldsPage.title')"
    :description="$t('customFieldsPage.description')"
    :loading="loading"
    :error="error"
    :empty="!store.customFields.length"
  >
    <template #actions>
      <Button @click="handleOpenAddModal" variant="outline" :disabled="loading">
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("customFieldsPage.addCustomField") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center py-16 text-gray-400">
        <Loader2 class="h-5 w-5 animate-spin mr-2" />
        <span class="text-sm">{{ $t("common.loading") }}</span>
      </div>
    </template>

    <template #error="{ error: pageError }">
      <div class="flex flex-col items-center justify-center py-20">
        <div
          class="text-red-600 bg-red-50 border border-red-200 rounded-lg p-4 max-w-lg text-sm"
        >
          {{ pageError }}
        </div>
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20 text-gray-400">
        <FileText class="h-10 w-10 mb-3 stroke-1" />
        <p class="text-sm font-medium text-gray-500">
          {{ $t("customFieldsPage.noData") }}
        </p>
      </div>
    </template>

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:hidden">
      <MobileCrudCard
        v-for="field in store.customFields"
        :key="field.id"
        :title="field.name || field.field_name"
        :description="field.description || field.field_name"
      >
        <template #header>
          <Badge variant="secondary" class="font-mono text-[11px]">
            {{ field.field_type }}
          </Badge>
        </template>

        <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("customFieldsPage.table.fieldName") }}</span>
            <span class="max-w-[12rem] truncate font-mono text-gray-700">
              {{ field.field_name }}
            </span>
          </div>
          <div class="flex items-center justify-between gap-3 rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("customFieldsPage.table.placement") }}</span>
            <Badge variant="outline" class="text-[11px]">
              {{ field.field_placement }}
            </Badge>
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("customFieldsPage.table.enabled") }}</span>
            <Checkbox
              :checked="field.is_enabled"
              @update:checked="() => handleToggleEnable(field)"
            />
          </div>
        </div>

        <template #actions>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-600 hover:text-gray-900"
            @click="handleOpenEditModal(field)"
          >
            <Pencil class="h-3.5 w-3.5 mr-1" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-400 hover:text-red-600"
            @click="handleDelete(field.id, field.name || field.field_name)"
          >
            <Trash2 class="h-3.5 w-3.5 mr-1" />
            {{ $t("common.delete") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden border border-gray-200 rounded-lg overflow-hidden md:block">
      <Table>
        <TableHeader>
          <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("customFieldsPage.table.name") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("customFieldsPage.table.fieldName") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("customFieldsPage.table.fieldType") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("customFieldsPage.table.placement") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("customFieldsPage.table.enabled") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
              >{{ $t("common.actions") }}</TableHead
            >
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="field in store.customFields" :key="field.id">
            <TableCell class="font-medium text-gray-900">{{
              field.name || "—"
            }}</TableCell>
            <TableCell class="font-mono text-xs text-gray-600">{{
              field.field_name
            }}</TableCell>
            <TableCell>
              <Badge variant="secondary" class="font-mono text-xs">{{
                field.field_type
              }}</Badge>
            </TableCell>
            <TableCell>
              <Badge variant="outline" class="text-xs">{{
                field.field_placement
              }}</Badge>
            </TableCell>
            <TableCell>
              <Checkbox
                :checked="field.is_enabled"
                @update:checked="() => handleToggleEnable(field)"
              />
            </TableCell>
            <TableCell class="text-right">
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-600 hover:text-gray-900"
                @click="handleOpenEditModal(field)"
              >
                <Pencil class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.edit") }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600"
                @click="handleDelete(field.id, field.name || field.field_name)"
              >
                <Trash2 class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.delete") }}
              </Button>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>

    <template #modals>
      <!-- Edit/Add Modal -->
      <Dialog :open="showEditModal" @update:open="setShowEditModal">
        <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-lg">
          <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
            <DialogTitle class="text-lg font-semibold text-gray-900">
              {{
                editingField.id
                  ? $t("customFieldsPage.modal.titleEdit")
                  : $t("customFieldsPage.modal.titleAdd")
              }}
            </DialogTitle>
          </DialogHeader>

          <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <!-- Field Name (Required) -->
            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ $t("customFieldsPage.modal.labelFieldName") }}
                <span class="text-red-500 ml-0.5">*</span>
              </Label>
              <Input
                v-model="editingField.field_name"
                :placeholder="$t('customFieldsPage.modal.placeholderFieldName')"
                class="font-mono text-sm"
              />
            </div>

            <!-- Name -->
            <div class="space-y-1.5">
              <Label class="text-gray-700">{{
                $t("customFieldsPage.modal.labelName")
              }}</Label>
              <Input
                v-model="editingField.name"
                :placeholder="$t('customFieldsPage.modal.placeholderName')"
              />
            </div>
          </div>

          <!-- Description -->
          <div class="space-y-1.5">
            <Label class="text-gray-700">{{
              $t("customFieldsPage.modal.labelDescription")
            }}</Label>
            <Input
              v-model="editingField.description"
              :placeholder="$t('customFieldsPage.modal.placeholderDescription')"
            />
          </div>

          <!-- Placement & Type row -->
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ $t("customFieldsPage.modal.labelPlacement") }}
                <span class="text-red-500 ml-0.5">*</span>
              </Label>
              <Select v-model="editingField.field_placement">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      $t('customFieldsPage.modal.placeholderPlacement')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="opt in fieldPlacements"
                    :key="opt"
                    :value="opt"
                    >{{ opt }}</SelectItem
                  >
                </SelectContent>
              </Select>
            </div>
            <div class="space-y-1.5">
              <Label class="text-gray-700">
                {{ $t("customFieldsPage.modal.labelFieldType") }}
                <span class="text-red-500 ml-0.5">*</span>
              </Label>
              <Select v-model="editingField.field_type">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      $t('customFieldsPage.modal.placeholderFieldType')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="opt in fieldTypes"
                    :key="opt"
                    :value="opt"
                    >{{ opt }}</SelectItem
                  >
                </SelectContent>
              </Select>
            </div>
          </div>

          <!-- Conditional Value Fields -->
          <template v-if="editingField.field_type !== 'UNSET'">
            <div class="pt-2">
              <hr class="border-gray-100" />
            </div>

            <div
              v-if="['STRING', 'JSON_STRING'].includes(editingField.field_type)"
              class="space-y-1.5"
            >
              <Label class="text-gray-700">{{
                $t("customFieldsPage.modal.labelValue")
              }}</Label>
              <Input
                v-model="editingField.string_value"
                :placeholder="
                  editingField.field_type === 'STRING'
                    ? $t('customFieldsPage.modal.placeholderStringValue')
                    : $t('customFieldsPage.modal.placeholderJsonStringValue')
                "
                :class="{
                  'font-mono text-sm':
                    editingField.field_type === 'JSON_STRING',
                }"
              />
            </div>

            <div
              v-else-if="editingField.field_type === 'INTEGER'"
              class="space-y-1.5"
            >
              <Label class="text-gray-700">{{
                $t("customFieldsPage.modal.labelValue")
              }}</Label>
              <Input
                type="number"
                v-model.number="editingField.integer_value"
                step="1"
                :placeholder="
                  $t('customFieldsPage.modal.placeholderIntegerValue')
                "
                class="font-mono text-sm"
              />
            </div>

            <div
              v-else-if="editingField.field_type === 'NUMBER'"
              class="space-y-1.5"
            >
              <Label class="text-gray-700">{{
                $t("customFieldsPage.modal.labelValue")
              }}</Label>
              <Input
                type="number"
                v-model.number="editingField.number_value"
                :placeholder="
                  $t('customFieldsPage.modal.placeholderNumberValue')
                "
                class="font-mono text-sm"
              />
            </div>

            <div
              v-else-if="editingField.field_type === 'BOOLEAN'"
              class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
            >
              <Label
                for="boolean_value_checkbox"
                class="text-gray-700 font-medium cursor-pointer"
                >{{ $t("customFieldsPage.modal.labelValue") }}</Label
              >
              <Checkbox
                id="boolean_value_checkbox"
                :checked="editingField.boolean_value"
                @update:checked="
                  (val: boolean) => (editingField.boolean_value = val)
                "
              />
            </div>
          </template>

          <div class="pt-2">
            <hr class="border-gray-100" />
          </div>

          <!-- Enabled -->
          <div
            class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
          >
            <Label
              for="is_enabled_checkbox"
              class="text-gray-700 font-medium cursor-pointer"
              >{{ $t("customFieldsPage.modal.labelEnabled") }}</Label
            >
            <Checkbox
              id="is_enabled_checkbox"
              :checked="editingField.is_enabled"
              @update:checked="
                (val: boolean) => (editingField.is_enabled = val)
              "
            />
          </div>
        </div>

          <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
            <Button
              @click="handleCloseModal"
              variant="ghost"
              class="w-full text-gray-600 sm:w-auto"
              >{{ $t("common.cancel") }}</Button
            >
            <Button @click="handleSave" variant="default" class="w-full sm:w-auto">{{
              $t("common.save")
            }}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </template>
  </CrudPageLayout>
</template>

<script setup lang="ts">
import { ref, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Plus, Pencil, Trash2, Loader2, FileText } from "lucide-vue-next";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import { useCustomFieldStore } from "@/store/customFieldStore";
import type { CustomFieldDefinition } from "@/store/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { confirm } from "@/lib/confirmController";
import { toastController } from "@/lib/toastController";

const { t: $t } = useI18n();
const store = useCustomFieldStore();

const loading = ref(true);
const error = ref<string | null>(null);
const showEditModal = ref(false);

const setShowEditModal = (val: boolean) => {
  showEditModal.value = val;
};

type CustomFieldType =
  | "STRING"
  | "INTEGER"
  | "NUMBER"
  | "BOOLEAN"
  | "JSON_STRING"
  | "UNSET";
const fieldPlacements = ["HEADER", "QUERY", "BODY"];
const fieldTypes: CustomFieldType[] = [
  "STRING",
  "INTEGER",
  "NUMBER",
  "BOOLEAN",
  "JSON_STRING",
  "UNSET",
];

interface EditingCustomField {
  id: number | null;
  name: string | undefined;
  description: string | undefined;
  field_name: string;
  field_placement: string;
  field_type: CustomFieldType | string;
  string_value: string | undefined;
  integer_value: number | undefined;
  number_value: number | undefined;
  boolean_value: boolean;
  is_enabled: boolean;
}

const newCustomFieldTemplate = (): EditingCustomField => ({
  id: null,
  name: "",
  description: "",
  field_name: "",
  field_placement: "",
  field_type: "UNSET",
  string_value: undefined,
  integer_value: undefined,
  number_value: undefined,
  boolean_value: false,
  is_enabled: true,
});

const editingField = ref<EditingCustomField>(newCustomFieldTemplate());

onMounted(async () => {
  loading.value = true;
  error.value = null;
  try {
    await store.fetchCustomFields();
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    loading.value = false;
  }
});

const fetchCustomFieldDetailAPI = async (
  id: number,
): Promise<CustomFieldDefinition | null> => {
  try {
    return await Api.getCustomFieldDetail(id);
  } catch (error) {
    console.error(`Failed to fetch custom field detail for id ${id}:`, error);
    return null;
  }
};

const handleOpenAddModal = () => {
  editingField.value = newCustomFieldTemplate();
  showEditModal.value = true;
};

const handleOpenEditModal = async (field: CustomFieldDefinition) => {
  const detail = await fetchCustomFieldDetailAPI(field.id);
  if (detail) {
    editingField.value = {
      id: detail.id,
      name: detail.name ?? undefined,
      description: detail.description ?? undefined,
      field_name: detail.field_name,
      field_placement: detail.field_placement,
      field_type: detail.field_type as CustomFieldType,
      string_value: detail.string_value ?? undefined,
      integer_value: detail.integer_value ?? undefined,
      number_value: detail.number_value ?? undefined,
      boolean_value: detail.boolean_value ?? false,
      is_enabled: detail.is_enabled,
    };
    showEditModal.value = true;
  } else {
    toastController.error($t("customFieldsPage.alert.loadDetailFailed"));
  }
};

const handleCloseModal = () => {
  showEditModal.value = false;
};

const handleSave = async () => {
  const field = editingField.value;
  if (!field.field_name?.trim()) {
    toastController.error($t("customFieldsPage.alert.nameAndTypeRequired"));
    return;
  }

  const payload = {
    name: field.name,
    description: field.description,
    field_name: field.field_name,
    field_placement: field.field_placement,
    field_type: field.field_type,
    string_value:
      field.field_type === "STRING" || field.field_type === "JSON_STRING"
        ? field.string_value
        : null,
    integer_value: field.field_type === "INTEGER" ? field.integer_value : null,
    number_value: field.field_type === "NUMBER" ? field.number_value : null,
    boolean_value: field.field_type === "BOOLEAN" ? field.boolean_value : null,
    is_enabled: field.is_enabled,
  };

  try {
    if (field.id) {
      await Api.updateCustomField(field.id, payload);
    } else {
      await Api.createCustomField(payload);
    }
    showEditModal.value = false;
    await store.fetchCustomFields();
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, $t("common.unknownError"));
    toastController.error(
      $t("customFieldsPage.alert.saveFailed", {
        error: normalizedError.message,
      }),
    );
  }
};

const handleToggleEnable = async (field: CustomFieldDefinition) => {
  const updatedField = {
    ...field,
    is_enabled: !field.is_enabled,
  };

  const payload = {
    name: updatedField.name,
    description: updatedField.description,
    field_name: updatedField.field_name,
    field_placement: updatedField.field_placement,
    field_type: updatedField.field_type,
    string_value:
      updatedField.field_type === "STRING" ||
      updatedField.field_type === "JSON_STRING"
        ? updatedField.string_value
        : null,
    integer_value:
      updatedField.field_type === "INTEGER" ? updatedField.integer_value : null,
    number_value:
      updatedField.field_type === "NUMBER" ? updatedField.number_value : null,
    boolean_value:
      updatedField.field_type === "BOOLEAN" ? updatedField.boolean_value : null,
    is_enabled: updatedField.is_enabled,
  };

  try {
    await Api.updateCustomField(updatedField.id, payload);
    await store.fetchCustomFields();
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, $t("common.unknownError"));
    toastController.error(
      $t("customFieldsPage.alert.toggleFailed", {
        error: normalizedError.message,
      }),
    );
  }
};

const handleDelete = async (id: number, name: string) => {
  if (
    await confirm({
      title: $t("customFieldsPage.confirmDelete", { name: name }),
    })
  ) {
    try {
      await Api.deleteCustomField(id);
      await store.fetchCustomFields();
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, $t("common.unknownError"));
      toastController.error(
        $t("customFieldsPage.alert.deleteFailed", {
          error: normalizedError.message,
        }),
      );
    }
  }
};
</script>
