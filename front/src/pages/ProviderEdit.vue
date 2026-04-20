<template>
  <div class="app-page">
    <div class="app-page-shell app-page-shell--narrow">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold text-gray-900 tracking-tight sm:text-xl">
            {{ pageTitle }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ pageDescription }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto">
          <Button variant="outline" @click="router.push('/provider')">
            <ArrowLeft class="h-4 w-4 mr-1.5" />
            {{ $t("providerEditPage.buttonBackToList") }}
          </Button>
        </div>
      </div>

      <div v-if="isLoading" class="flex items-center justify-center py-16">
        <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
        <span class="text-sm font-medium text-gray-500">{{
          $t("providerEditPage.loadingData")
        }}</span>
      </div>

      <div
        v-else-if="errorMsg"
        class="flex flex-col items-center justify-center py-20"
      >
        <AlertCircle class="h-10 w-10 stroke-1 text-red-500 mb-2" />
        <span class="text-sm font-medium text-red-500">{{ errorMsg }}</span>
      </div>

      <template v-else-if="editingData">
        <div class="space-y-5 sm:space-y-6">
          <ProviderBaseInfoForm v-model:editingData="editingData" />
          <div class="space-y-3 pt-1">
            <div class="border-t border-gray-200 pt-5">
              <h2 class="text-base font-semibold text-gray-900">
                {{ $t("providerEditPage.sections.advanced.title") }}
              </h2>
              <p class="mt-1 text-sm text-gray-500">
                {{ $t("providerEditPage.sections.advanced.description") }}
              </p>
            </div>
          </div>
          <ProviderModelList 
            v-model:editingData="editingData"
            @check-single="(index) => handleCheck('model', index)"
            @check-batch="() => handleBatchCheck('models')"
          />

          <ProviderApiKeyList 
            v-model:editingData="editingData"
            @check-single="(index) => handleCheck('apiKey', index)"
            @check-batch="() => handleBatchCheck('api_keys')"
          />

          <ProviderCustomFieldList 
            v-model:editingData="editingData"
            :all-custom-fields="allCustomFields"
          />

          <div class="flex flex-col gap-2 rounded-xl border border-gray-200 bg-white p-4 sm:flex-row sm:justify-end">
            <Button variant="secondary" class="w-full sm:w-auto" @click="router.push('/provider')">{{
              $t("providerEditPage.buttonBackToList")
            }}</Button>
          </div>
        </div>

        <Dialog
          :open="isModelSelectModalOpen"
          @update:open="(v: boolean) => (isModelSelectModalOpen = v)"
        >
          <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-md">
            <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
              <DialogTitle class="text-lg font-semibold text-gray-900">{{
                $t("providerEditPage.modalSelectModel.title")
              }}</DialogTitle>
            </DialogHeader>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
              <p class="text-sm text-gray-500">
                {{ $t("providerEditPage.modalSelectModel.description") }}
              </p>
              <p class="font-mono text-xs text-gray-600">
                {{ $t("providerEditPage.modalSelectModel.target", { target: selectedApiKeyCheckTargetLabel }) }}
              </p>
              <Select v-model="modelIndexToUseStr">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      $t('providerEditPage.modalSelectModel.selectPlaceholder')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="opt in modelOptionsForSelect"
                    :key="opt.value"
                    :value="String(opt.value)"
                  >
                    {{ opt.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
              <Button
                variant="ghost"
                class="w-full text-gray-600 sm:w-auto"
                @click="isModelSelectModalOpen = false"
                >{{ $t("common.cancel") }}</Button
              >
              <Button
                variant="default"
                class="w-full sm:w-auto"
                @click="handleConfirmModelSelection"
                :disabled="modelIndexToUseStr === null"
                >{{ $t("common.check") }}</Button
              >
            </DialogFooter>
          </DialogContent>
        </Dialog>

        <Dialog
          :open="isApiKeySelectModalOpen"
          @update:open="(v: boolean) => (isApiKeySelectModalOpen = v)"
        >
          <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-md">
            <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
              <DialogTitle class="text-lg font-semibold text-gray-900">{{
                $t("providerEditPage.modalSelectApiKey.title")
              }}</DialogTitle>
            </DialogHeader>
            <div class="flex-1 space-y-4 overflow-y-auto px-4 py-4 sm:px-6 sm:pt-4">
              <p class="text-sm text-gray-500">
                {{ $t("providerEditPage.modalSelectApiKey.description") }}
              </p>
              <p class="font-mono text-xs text-gray-600">
                {{ $t("providerEditPage.modalSelectApiKey.target", { target: selectedModelCheckTargetLabel }) }}
              </p>
              <Select v-model="apiKeyIndexToUseStr">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      $t('providerEditPage.modalSelectApiKey.selectPlaceholder')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="opt in apiKeyOptionsForSelect"
                    :key="opt.value"
                    :value="String(opt.value)"
                  >
                    {{ opt.label }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
              <Button
                variant="ghost"
                class="w-full text-gray-600 sm:w-auto"
                @click="isApiKeySelectModalOpen = false"
                >{{ $t("common.cancel") }}</Button
              >
              <Button
                variant="default"
                class="w-full sm:w-auto"
                @click="handleConfirmApiKeySelection"
                :disabled="apiKeyIndexToUseStr === null"
                >{{ $t("common.check") }}</Button
              >
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </template>
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { Api } from "@/services/request";
import type {
  CustomFieldType,
  CustomFieldItem,
  CustomFieldDefinition,
  ProviderListItem,
} from "@/store/types";
import { toastController } from "@/lib/toastController";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  ArrowLeft,
  Loader2,
  AlertCircle,
} from "lucide-vue-next";

// Import types and composable
import type { EditingProviderData } from "@/components/provider/types";
import { createEmptyEditingProviderData } from "@/pages/providerEditState";
import { useProviderCheck } from "@/composables/useProviderCheck";

// Import components
import ProviderBaseInfoForm from "@/components/provider/ProviderBaseInfoForm.vue";
import ProviderModelList from "@/components/provider/ProviderModelList.vue";
import ProviderApiKeyList from "@/components/provider/ProviderApiKeyList.vue";
import ProviderCustomFieldList from "@/components/provider/ProviderCustomFieldList.vue";

const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();

const providerId = computed(() => {
  const id = route.params.id;
  if (id) {
    const num = parseInt(id as string, 10);
    return isNaN(num) ? null : num;
  }
  return null;
});

const isLoading = ref(true);
const errorMsg = ref<string | null>(null);
const editingData = ref<EditingProviderData | null>(null);
const allCustomFields = ref<CustomFieldItem[]>([]);

const pageTitle = computed(() =>
  providerId.value || editingData.value?.id
    ? $t("providerEditPage.titleEdit")
    : $t("providerEditPage.titleAdd"),
);

const pageDescription = computed(() =>
  providerId.value || editingData.value?.id
    ? $t("providerEditPage.descriptionEdit")
    : $t("providerEditPage.descriptionAdd"),
);

const {
  isModelSelectModalOpen,
  isApiKeySelectModalOpen,
  modelIndexToUseStr,
  apiKeyIndexToUseStr,
  modelOptionsForSelect,
  apiKeyOptionsForSelect,
  selectedModelCheckTargetLabel,
  selectedApiKeyCheckTargetLabel,
  handleCheck,
  handleBatchCheck,
  handleConfirmModelSelection,
  handleConfirmApiKeySelection,
} = useProviderCheck(editingData);

const toEditableCustomField = (
  field: Pick<
    CustomFieldDefinition,
    | "id"
    | "name"
    | "field_name"
    | "string_value"
    | "integer_value"
    | "number_value"
    | "boolean_value"
    | "description"
    | "field_type"
  >,
): CustomFieldItem => ({
  id: field.id,
  name: field.name,
  field_name: field.field_name,
  field_value:
    (field.string_value ??
      field.integer_value?.toString() ??
      field.number_value?.toString() ??
      field.boolean_value?.toString()) ||
    "",
  description: field.description,
  field_type: (field.field_type?.toLowerCase() as CustomFieldType) || "unset",
});

const fetchAllCustomFields = async (): Promise<CustomFieldItem[]> => {
  try {
    const response = await Api.getCustomFieldList(1000);
    if (response && response.list) {
      return response.list.map(toEditableCustomField);
    }
    return [];
  } catch (error) {
    console.error("Failed to fetch all custom fields", error);
    toastController.error("Failed to fetch all custom fields");
    return [];
  }
};

const fetchProviderDetail = async (
  id: number,
): Promise<ProviderListItem | null> => {
  try {
    const response = await Api.getProviderDetail(id);
    return response || null;
  } catch (error) {
    console.error(
      $t("providerEditPage.alert.fetchDetailFailed", { providerId: id }),
      error,
    );
    toastController.error(
      $t("providerEditPage.alert.fetchDetailFailed", { providerId: id }),
    );
    return null;
  }
};

const getEmptyProvider = (): EditingProviderData => ({
  ...createEmptyEditingProviderData(),
});

onMounted(async () => {
  isLoading.value = true;
  errorMsg.value = null;

  const fields = await fetchAllCustomFields();
  allCustomFields.value = fields;

  if (providerId.value) {
      const detail = await fetchProviderDetail(providerId.value);
      if (detail) {
        editingData.value = {
          id: detail.provider.id,
          name: detail.provider.name,
          provider_key: detail.provider.provider_key,
          provider_type: detail.provider.provider_type || "OPENAI",
          endpoint: detail.provider.endpoint,
          use_proxy: detail.provider.use_proxy,
          models: detail.models.map((m) => ({
            id: m.model.id,
            model_name: m.model.model_name,
            real_model_name: m.model.real_model_name ?? null,
            is_enabled: m.model.is_enabled,
            isEditing: false,
            checkStatus: "unchecked" as const,
          })),
          provider_keys: detail.provider_keys.map((k) => ({
            id: k.id,
            api_key: k.api_key,
            description: k.description ?? null,
            isEditing: false,
            checkStatus: "unchecked" as const,
          })),
          custom_fields: (detail.custom_fields || []).map(toEditableCustomField),
        };
      } else {
        errorMsg.value = $t("providerEditPage.alert.loadDataFailed", {
          providerId: providerId.value,
      });
    }
  } else {
    editingData.value = getEmptyProvider();
  }
  isLoading.value = false;
});
</script>
