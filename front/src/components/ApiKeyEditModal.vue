<script setup lang="ts">
import { ref, computed, watch, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Api } from "@/services/request";
import type { ApiKeyCreatePayload, ApiKeyItem, ApiKeyUpdatePayload } from "@/store/types";
import { useAccessControlStore } from "@/store/accessControlStore";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";

export interface EditingApiKeyData {
  id: number | null;
  name: string;
  description: string;
  is_enabled: boolean;
  access_control_policy_id: number | null;
}

interface ApiKeyEditModalProps {
  isOpen: boolean;
  initialData: ApiKeyItem | null;
}

const props = defineProps<ApiKeyEditModalProps>();
const emit = defineEmits(["update:isOpen", "saveSuccess"]);

const { t } = useI18n();
const accessControlStore = useAccessControlStore();
const isSubmitting = ref(false);
const isLoadingPolicies = ref(false);

const getEmptyEditingData = (): EditingApiKeyData => ({
  id: null,
  name: "",
  description: "",
  is_enabled: true,
  access_control_policy_id: null,
});

const editingData = ref<EditingApiKeyData>(getEmptyEditingData());

const policyOptions = computed(() => {
  const noPolicy = { value: null, label: t("apiKeyEditModal.noPolicy") };
  const policiesList = (accessControlStore.policies || []).map((p) => ({
    value: p.id,
    label: p.name,
  }));
  return [noPolicy, ...policiesList];
});

const loadPolicies = async () => {
  if (accessControlStore.policies.length > 0) return;

  isLoadingPolicies.value = true;
  try {
    await accessControlStore.fetchPolicies();
  } catch (error: unknown) {
    toastController.error(
      t("apiKeyEditModal.alert.saveFailed", {
        error: normalizeError(error, t("common.unknownError")).message,
      }),
    );
  } finally {
    isLoadingPolicies.value = false;
  }
};

watch(
  () => props.isOpen,
  async (newVal) => {
    if (newVal) {
      await loadPolicies();
      if (props.initialData) {
        editingData.value = {
          id: props.initialData.id,
          name: props.initialData.name,
          description: props.initialData.description,
          is_enabled: props.initialData.is_enabled,
          access_control_policy_id: props.initialData.access_control_policy_id ?? null,
        };
      } else {
        editingData.value = getEmptyEditingData();
      }
    }
  },
);

onMounted(() => {
  void loadPolicies();
});

const handleCommit = async () => {
  const currentFormState = editingData.value;

  if (!currentFormState) {
    toastController.error(t("apiKeyEditModal.alert.formDataError"));
    return;
  }

  if (!currentFormState.name.trim()) {
    toastController.error(t("apiKeyEditModal.alert.nameRequired"));
    return;
  }

  const payload: ApiKeyCreatePayload | ApiKeyUpdatePayload = {
    name: currentFormState.name.trim(),
    description: currentFormState.description.trim(),
    is_enabled: currentFormState.is_enabled,
    access_control_policy_id: currentFormState.access_control_policy_id,
  };

  try {
    isSubmitting.value = true;
    if (currentFormState.id) {
      await Api.updateApiKey(currentFormState.id, payload);
    } else {
      await Api.createApiKey(payload);
    }
    emit("saveSuccess");
    emit("update:isOpen", false);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    console.error("Failed to commit API key:", normalizedError);
    toastController.error(
      t("apiKeyEditModal.alert.saveFailed", {
        error: normalizedError.message,
      }),
    );
  } finally {
    isSubmitting.value = false;
  }
};

const handleOpenChange = (open: boolean) => {
  emit("update:isOpen", open);
};
</script>

<template>
  <Dialog :open="props.isOpen" @update:open="handleOpenChange">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-lg">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle class="text-lg font-semibold text-gray-900">{{
          editingData?.id
            ? t("apiKeyEditModal.titleEdit")
            : t("apiKeyEditModal.titleAdd")
        }}</DialogTitle>
      </DialogHeader>
      <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
        <div class="space-y-1.5">
          <Label for="apiKeyName" class="text-gray-700"
            >{{ t("apiKeyEditModal.labelName")
            }}<span class="text-red-500 ml-0.5">*</span></Label
          >
          <Input id="apiKeyName" v-model="editingData.name" />
        </div>

        <div class="space-y-1.5">
          <Label for="apiKeyDescription" class="text-gray-700">{{
            t("apiKeyEditModal.labelDescription")
          }}</Label>
          <Input id="apiKeyDescription" v-model="editingData.description" />
        </div>

        <div class="space-y-1.5">
          <Label class="text-gray-700">{{
            t("apiKeyEditModal.labelAccessControlPolicy")
          }}</Label>
          <Select
            :model-value="
              editingData.access_control_policy_id == null
                ? 'none'
                : String(editingData.access_control_policy_id)
            "
            @update:model-value="
              (value) =>
                (editingData.access_control_policy_id =
                  value === 'none' ? null : Number(value))
            "
            :disabled="isLoadingPolicies"
          >
            <SelectTrigger class="w-full">
              <SelectValue
                :placeholder="
                  t('apiKeyEditModal.placeholderAccessControlPolicy')
                "
              />
            </SelectTrigger>
            <SelectContent>
              <SelectItem
                v-for="policy in policyOptions"
                :key="policy.value || 'none'"
                :value="policy.value == null ? 'none' : String(policy.value)"
              >
                {{ policy.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div
          class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
        >
          <Label
            for="isEnabledCheckbox"
            class="text-sm font-medium leading-none cursor-pointer"
            >{{ t("apiKeyEditModal.labelEnabled") }}</Label
          >
          <Checkbox
            id="isEnabledCheckbox"
            :checked="editingData.is_enabled"
            @update:checked="(val: boolean) => (editingData.is_enabled = !!val)"
          />
        </div>
      </div>
      <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
        <Button
          variant="ghost"
          class="w-full text-gray-600 sm:w-auto"
          @click="handleOpenChange(false)"
          :disabled="isSubmitting"
          >{{ t("common.cancel") }}</Button
        >
        <Button
          variant="default"
          class="w-full sm:w-auto"
          @click="handleCommit"
          :disabled="isSubmitting || isLoadingPolicies"
        >{{
          t("common.save")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
