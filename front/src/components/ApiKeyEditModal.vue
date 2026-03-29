<script setup lang="ts">
import { ref, computed, watch } from "vue";
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
import type { ApiKeyItem } from "@/store/types";
// Assuming a Pinia store is set up
// import { useAccessControlStore } from '../store/accessControlStore';

// Mock store for now
const useAccessControlStore = () => ({
  policies: ref([
    { id: 1, name: "Admin Policy" },
    { id: 2, name: "User Policy" },
  ]),
});

const accessControlStore = useAccessControlStore();
const policies = accessControlStore.policies;

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
  const policiesList = (policies.value || []).map((p) => ({
    value: p.id,
    label: p.name,
  }));
  return [noPolicy, ...policiesList];
});

watch(
  () => props.isOpen,
  (newVal) => {
    if (newVal) {
      if (props.initialData) {
        editingData.value = {
          id: props.initialData.id,
          name: props.initialData.name,
          description: props.initialData.description,
          is_enabled: props.initialData.is_enabled,
          access_control_policy_id:
            (props.initialData as any).access_control_policy_id ?? null,
        };
      } else {
        editingData.value = getEmptyEditingData();
      }
    }
  },
);

const handleCommit = async () => {
  const currentFormState = editingData.value;

  if (!currentFormState) {
    alert(t("apiKeyEditModal.alert.formDataError"));
    return;
  }

  if (!currentFormState.name.trim()) {
    alert(t("apiKeyEditModal.alert.nameRequired"));
    return;
  }

  const payload: any = {
    name: currentFormState.name,
    description: currentFormState.description,
    is_enabled: currentFormState.is_enabled,
    access_control_policy_id: currentFormState.access_control_policy_id,
  };

  try {
    if (currentFormState.id) {
      await Api.updateApiKey(currentFormState.id, payload);
    } else {
      await Api.createApiKey(payload);
    }
    emit("saveSuccess");
    emit("update:isOpen", false);
  } catch (error) {
    console.error("Failed to commit API key:", error);
    alert(
      t("apiKeyEditModal.alert.saveFailed", {
        error: (error as Error).message || t("unknownError"),
      }),
    );
  }
};

const handleOpenChange = (open: boolean) => {
  emit("update:isOpen", open);
};
</script>

<template>
  <Dialog :open="props.isOpen" @update:open="handleOpenChange">
    <DialogContent class="max-w-lg max-h-[90vh] flex flex-col">
      <DialogHeader>
        <DialogTitle class="text-lg font-semibold text-gray-900">{{
          editingData?.id
            ? t("apiKeyEditModal.titleEdit")
            : t("apiKeyEditModal.titleAdd")
        }}</DialogTitle>
      </DialogHeader>
      <div class="overflow-y-auto space-y-4 pr-2">
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
          <Select v-model="editingData.access_control_policy_id">
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
                :value="policy.value"
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
      <DialogFooter class="border-t border-gray-100 pt-4 mt-2">
        <Button
          variant="ghost"
          class="text-gray-600"
          @click="handleOpenChange(false)"
          >{{ t("common.cancel") }}</Button
        >
        <Button variant="default" @click="handleCommit">{{
          t("common.save")
        }}</Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
