<template>
  <div class="space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <div class="space-y-1.5">
        <Label class="text-gray-700"
          >{{ $t("providerEditPage.labelName") }}
          <span class="text-red-500 ml-0.5">*</span></Label
        >
        <Input v-model="editingData.name" />
      </div>
      <div class="space-y-1.5">
        <Label class="text-gray-700"
          >{{ $t("providerEditPage.labelProviderKey") }}
          <span class="text-red-500 ml-0.5">*</span></Label
        >
        <Input
          v-model="editingData.provider_key"
          :disabled="!!editingData.id"
          class="font-mono text-sm"
        />
      </div>
      <div class="space-y-1.5">
        <Label class="text-gray-700"
          >{{ $t("providerEditPage.labelProviderType") }}
          <span class="text-red-500 ml-0.5">*</span></Label
        >
        <Select v-model="editingData.provider_type">
          <SelectTrigger class="w-full">
            <SelectValue
              :placeholder="$t('providerEditPage.placeholderProviderType')"
            />
          </SelectTrigger>
          <SelectContent>
            <SelectItem
              v-for="pt in providerTypes"
              :key="pt"
              :value="pt"
              >{{ pt }}</SelectItem
            >
          </SelectContent>
        </Select>
      </div>
      <div class="space-y-1.5">
        <Label class="text-gray-700"
          >{{ $t("providerEditPage.labelEndpoint") }}
          <span class="text-red-500 ml-0.5">*</span></Label
        >
        <Input v-model="editingData.endpoint" class="font-mono text-sm" />
      </div>
    </div>
    <div class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg">
      <Label for="use_proxy_checkbox" class="cursor-pointer text-gray-700"
        >{{ $t("providerEditPage.labelUseProxy") }}</Label
      >
      <Checkbox
        id="use_proxy_checkbox"
        :checked="editingData.use_proxy"
        @update:checked="(val: boolean) => (editingData.use_proxy = val)"
      />
    </div>

    <!-- Update Base Info Button -->
    <div class="mt-4">
      <Button variant="default" @click="handleUpdateProviderBaseInfo">
        {{
          editingData.id
            ? $t("providerEditPage.buttonUpdateBaseInfo")
            : $t("providerEditPage.buttonCreateBaseInfo")
        }}
      </Button>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { useProviderStore } from "@/store/providerStore";
import { toastController } from "@/lib/toastController";
import type { ProviderBase } from "@/store/types";
import type { EditingProviderData } from "./types";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

const providerTypes = [
  "OPENAI",
  "GEMINI",
  "GEMINI_OPENAI",
  "VERTEX",
  "VERTEX_OPENAI",
  "ANTHROPIC",
  "RESPONSES",
  "OLLAMA",
];

const { t: $t } = useI18n();
const providerStore = useProviderStore();

const editingData = defineModel<EditingProviderData>("editingData", { required: true });

const handleUpdateProviderBaseInfo = async () => {
  const data = editingData.value;
  if (!data) return;

  if (!data.name.trim()) {
    toastController.warn($t("providerEditPage.alert.nameRequired"));
    return;
  }
  if (!data.provider_key.trim()) {
    toastController.warn($t("providerEditPage.alert.providerKeyRequired"));
    return;
  }
  if (!data.endpoint.trim()) {
    toastController.warn($t("providerEditPage.alert.endpointRequired"));
    return;
  }

  const basePayload = {
    key: data.provider_key,
    name: data.name,
    endpoint: data.endpoint,
    use_proxy: data.use_proxy,
    provider_type: data.provider_type,
    omit_config: null,
    api_keys: [],
  };

  try {
    if (data.id) {
      await Api.updateProvider(data.id, basePayload);
      toastController.success($t("providerEditPage.alert.baseInfoUpdateSuccess"));
    } else {
      const newProvider = await Api.createProvider(basePayload) as ProviderBase;
      data.id = newProvider.id;
      toastController.success($t("providerEditPage.alert.createSuccess"));
    }
    void providerStore.fetchProviders().catch((error) => {
      console.error("Failed to refresh providers after save:", error);
    });
  } catch (error) {
    console.error("Failed to save provider base info:", error);
    toastController.error(
      $t("providerEditPage.alert.baseInfoSaveFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  }
};
</script>
