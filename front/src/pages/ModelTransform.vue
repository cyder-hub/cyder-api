<template>
  <CrudPageLayout
    :title="$t('modelAliasPage.title')"
    :description="$t('modelAliasPage.description')"
    :loading="loading"
    :error="error"
    :empty="!transforms.length"
  >
    <template #actions>
      <Button @click="handleOpenAddModal" variant="outline" :disabled="loading">
        <Plus class="h-4 w-4 mr-1.5" />
        {{ $t("modelAliasPage.addModelAlias") }}
      </Button>
    </template>

    <template #loading>
      <div
        class="flex items-center justify-center py-16 border border-gray-200 rounded-lg bg-white"
      >
        <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
        <span class="text-sm text-gray-500">{{
          $t("modelAliasPage.loading")
        }}</span>
      </div>
    </template>

    <template #error="{ error }">
      <div
        class="flex flex-col items-center justify-center py-20 border border-gray-200 rounded-lg bg-white"
      >
        <AlertCircle class="h-10 w-10 stroke-1 text-red-400 mb-4" />
        <span class="text-sm font-medium text-red-500"
          >{{ $t("modelAliasPage.errorPrefix") }} {{ error }}</span
        >
      </div>
    </template>

    <template #empty>
      <div
        class="flex flex-col items-center justify-center py-20 border border-gray-200 rounded-lg bg-white"
      >
        <Inbox class="h-10 w-10 stroke-1 text-gray-400 mb-4" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("modelAliasPage.noData")
        }}</span>
      </div>
    </template>

    <div class="grid grid-cols-1 gap-3 sm:grid-cols-2 md:hidden">
      <MobileCrudCard
        v-for="transform in transforms"
        :key="transform.id"
        :title="transform.alias_name"
        :description="`${transform.provider_key}/${transform.model_name}`"
      >
        <template #header>
          <Badge
            :variant="transform.is_enabled ? 'secondary' : 'outline'"
            class="font-mono text-[11px]"
          >
            {{ transform.is_enabled ? $t("common.yes") : $t("common.no") }}
          </Badge>
        </template>

        <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("modelAliasPage.table.targetModelName") }}</span>
            <span class="max-w-[13rem] truncate text-right text-gray-700">
              {{ transform.provider_key }}/{{ transform.model_name }}
            </span>
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("modelAliasPage.table.enabled") }}</span>
            <Checkbox
              :checked="transform.is_enabled"
              @update:checked="
                (val: boolean) => handleToggleEnable(transform, val)
              "
            />
          </div>
        </div>

        <template #actions>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center"
            @click="handleOpenEditModal(transform.id)"
          >
            <Pencil class="h-3.5 w-3.5 mr-1" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-400 hover:text-red-600"
            @click="
              handleDeleteTransform(transform.id, transform.alias_name)
            "
          >
            <Trash2 class="h-3.5 w-3.5 mr-1" />
            {{ $t("common.delete") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden border border-gray-200 rounded-lg overflow-hidden bg-white md:block">
      <Table>
        <TableHeader>
          <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("modelAliasPage.table.aliasName") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("modelAliasPage.table.targetModelName") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider"
              >{{ $t("modelAliasPage.table.enabled") }}</TableHead
            >
            <TableHead
              class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
              >{{ $t("common.actions") }}</TableHead
            >
          </TableRow>
        </TableHeader>
        <TableBody>
          <TableRow v-for="transform in transforms" :key="transform.id">
            <TableCell class="font-medium text-gray-900">{{
              transform.alias_name
            }}</TableCell>
            <TableCell class="text-sm"
              >{{ transform.provider_key }}/{{ transform.model_name }}</TableCell
            >
            <TableCell>
              <Checkbox
                :checked="transform.is_enabled"
                @update:checked="
                  (val: boolean) => handleToggleEnable(transform, val)
                "
              />
            </TableCell>
            <TableCell class="text-right">
              <Button
                variant="ghost"
                size="sm"
                @click="handleOpenEditModal(transform.id)"
              >
                <Pencil class="h-3.5 w-3.5 mr-1" />
                {{ $t("common.edit") }}
              </Button>
              <Button
                variant="ghost"
                size="sm"
                class="text-gray-400 hover:text-red-600"
                @click="
                  handleDeleteTransform(transform.id, transform.alias_name)
                "
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
                editingTransform.id
                  ? $t("modelAliasPage.modal.titleEdit")
                  : $t("modelAliasPage.modal.titleAdd")
              }}
            </DialogTitle>
          </DialogHeader>
          <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("modelAliasPage.modal.labelAliasName")
              }}<span class="text-red-500 ml-0.5">*</span>
            </Label>
            <Input
              v-model="editingTransform.alias_name"
              :placeholder="$t('modelAliasPage.modal.placeholderAliasName')"
              class="font-mono text-sm"
            />
          </div>

          <div class="space-y-1.5">
            <Label class="text-gray-700">
              {{ $t("modelAliasPage.modal.labelTargetProvider")
              }}<span class="text-red-500 ml-0.5">*</span>
            </Label>
            <Select
              v-model="editingTransform.provider_id"
              @update:model-value="onProviderChange"
            >
              <SelectTrigger class="w-full">
                <SelectValue
                  :placeholder="$t('modelAliasPage.modal.placeholderProvider')"
                />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="opt in providerOptions"
                  :key="opt.value"
                  :value="String(opt.value)"
                >
                  {{ opt.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div class="space-y-1.5" v-if="editingTransform.provider_id !== null">
            <Label class="text-gray-700">
              {{ $t("modelAliasPage.modal.labelTargetModel")
              }}<span class="text-red-500 ml-0.5">*</span>
            </Label>
            <Select
              v-model="editingTransform.target_model_id"
              :disabled="editingTransform.provider_id === null"
            >
              <SelectTrigger class="w-full">
                <SelectValue
                  :placeholder="$t('modelAliasPage.modal.placeholderModel')"
                />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="opt in modelOptions"
                  :key="opt.value"
                  :value="String(opt.value)"
                >
                  {{ opt.label }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div
            class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
          >
            <Label
              for="is_enabled_checkbox"
              class="cursor-pointer text-gray-700"
            >
              {{ $t("modelAliasPage.modal.labelEnabled") }}
            </Label>
            <Checkbox
              id="is_enabled_checkbox"
              :checked="editingTransform.is_enabled"
              @update:checked="
                (val: boolean) => (editingTransform.is_enabled = val)
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
            <Button @click="handleSaveTransform" variant="default" class="w-full sm:w-auto">{{
              $t("common.save")
            }}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </template>
  </CrudPageLayout>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import { useProviderStore } from "@/store/providerStore";
import type { ModelAliasListItem, EditingModelAlias } from "@/store/types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
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
import { Badge } from "@/components/ui/badge";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import {
  Plus,
  Pencil,
  Trash2,
  Loader2,
  Inbox,
  AlertCircle,
} from "lucide-vue-next";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { confirm } from "@/lib/confirmController";
import { toastController } from "@/lib/toastController";

const { t: $t } = useI18n();
const providerStore = useProviderStore();

const newModelAliasTemplate = (): EditingModelAlias => ({
  id: null,
  alias_name: "",
  provider_id: null,
  target_model_id: null,
  is_enabled: true,
});

const transforms = ref<ModelAliasListItem[]>([]);
const loading = ref(true);
const error = ref<string | null>(null);
const showEditModal = ref(false);
const editingTransform = ref<EditingModelAlias>(newModelAliasTemplate());

const setShowEditModal = (val: boolean) => {
  showEditModal.value = val;
};

// Data Options
const providerOptions = computed(() => {
  return providerStore.providers.map((p) => ({
    value: p.provider.id,
    label: p.provider.name,
  }));
});

const modelOptions = computed(() => {
  const pid = Number(editingTransform.value.provider_id);
  if (!pid) return [];
  const pItem = providerStore.providers.find((p) => p.provider.id === pid);
  if (!pItem) return [];
  return (pItem.models || []).map((m) => ({
    value: m.model.id,
    label: m.model.model_name,
  }));
});

const onProviderChange = () => {
  editingTransform.value.target_model_id = null;
};

const findProviderForModel = (modelId: number | null): number | null => {
  if (modelId === null) return null;
  for (const pItem of providerStore.providers) {
    if (pItem.models && pItem.models.some((m) => m.model.id === modelId)) {
      return pItem.provider.id;
    }
  }
  return null;
};

// API Fetching
const fetchModelAliassAPI = async () => {
  loading.value = true;
  try {
    const response = await Api.getModelAliasList();
    const rawAliases = (response as any) || [];
    transforms.value = rawAliases.map((item: any) => ({
      id: item.id,
      alias_name: item.alias_name,
      provider_key: item.provider_key,
      model_name: item.model_name,
      target_model_id: item.target_model_id,
      is_enabled: item.is_enabled,
      description: item.description,
      priority: item.priority,
    }));
  } catch (err: any) {
    error.value = err.message || $t("common.unknownError");
  } finally {
    loading.value = false;
  }
};

onMounted(async () => {
  try {
    await providerStore.fetchProviders();
    fetchModelAliassAPI();
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
    loading.value = false;
  }
});

const handleOpenAddModal = () => {
  editingTransform.value = newModelAliasTemplate();
  showEditModal.value = true;
};

const handleOpenEditModal = async (id: number) => {
  try {
    const detail: any = await Api.getModelAliasDetail(id);
    if (detail && detail.target_model_id) {
      const providerId = findProviderForModel(detail.target_model_id);
      editingTransform.value = {
        id: detail.id,
        alias_name: detail.alias_name,
        provider_id: providerId ? String(providerId) : null,
        target_model_id: String(detail.target_model_id),
        is_enabled: detail.is_enabled,
      };
      showEditModal.value = true;
    } else {
      toastController.error($t("modelAliasPage.alert.loadDetailFailed"));
    }
  } catch (err: any) {
    console.error("Failed to fetch model alias detail:", err);
    toastController.error($t("modelAliasPage.alert.loadDetailFailed"));
  }
};

const handleCloseModal = () => {
  showEditModal.value = false;
};

const handleSaveTransform = async () => {
  const transform = editingTransform.value;
  if (!transform.alias_name.trim() || !transform.target_model_id) {
    toastController.error($t("modelAliasPage.alert.nameAndTargetRequired"));
    return;
  }

  const payload = {
    alias_name: transform.alias_name,
    target_model_id: Number(transform.target_model_id),
    is_enabled: transform.is_enabled,
  };

  try {
    if (transform.id) {
      await Api.updateModelAlias(transform.id, payload);
    } else {
      await Api.createModelAlias(payload);
    }
    showEditModal.value = false;
    fetchModelAliassAPI();
  } catch (err: any) {
    console.error("Failed to save model transform:", err);
    toastController.error(
      $t("modelAliasPage.alert.saveFailed", {
        error: err.message || $t("common.unknownError"),
      }),
    );
  }
};

const handleToggleEnable = async (
  transform: ModelAliasListItem,
  updatedIsEnabled: boolean,
) => {
  const payload = {
    alias_name: transform.alias_name,
    target_model_id: transform.target_model_id,
    is_enabled: updatedIsEnabled,
  };

  try {
    await Api.updateModelAlias(transform.id, payload);
    fetchModelAliassAPI();
  } catch (err: any) {
    console.error("Failed to toggle status:", err);
    toastController.error(
      $t("modelAliasPage.alert.toggleFailed", {
        error: err.message || $t("common.unknownError"),
      }),
    );
  }
};

const handleDeleteTransform = async (id: number, name: string) => {
  if (
    await confirm({
      title: $t("modelAliasPage.confirmDelete", { name: name }),
    })
  ) {
    try {
      await Api.deleteModelAlias(id);
      fetchModelAliassAPI();
    } catch (err: any) {
      console.error("Failed to delete model transform:", err);
      toastController.error(
        $t("modelAliasPage.alert.deleteFailed", {
          error: err.message || $t("common.unknownError"),
        }),
      );
    }
  }
};
</script>
