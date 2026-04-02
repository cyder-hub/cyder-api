<template>
  <div class="p-6 space-y-6 max-w-5xl mx-auto">
    <!-- 页面头部 -->
    <div class="flex justify-between items-start">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">
          {{
            providerId
              ? $t("providerEditPage.titleEdit")
              : $t("providerEditPage.titleAdd")
          }}
        </h1>
        <p class="mt-1 text-sm text-gray-500">
          {{
            providerId
              ? $t("providerEditPage.titleEdit")
              : $t("providerEditPage.titleAdd")
          }}
        </p>
      </div>
      <Button variant="outline" @click="router.push('/provider')">
        <ArrowLeft class="h-4 w-4 mr-1.5" />
        {{ $t("providerEditPage.buttonBackToList") }}
      </Button>
    </div>

    <!-- Loading -->
    <div v-if="isLoading" class="flex items-center justify-center py-16">
      <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
      <span class="text-sm font-medium text-gray-500">{{
        $t("providerEditPage.loadingData")
      }}</span>
    </div>

    <!-- Error -->
    <div
      v-else-if="errorMsg"
      class="flex flex-col items-center justify-center py-20"
    >
      <AlertCircle class="h-10 w-10 stroke-1 text-red-500 mb-2" />
      <span class="text-sm font-medium text-red-500">{{ errorMsg }}</span>
    </div>

    <!-- Main Content -->
    <template v-else-if="editingData">
      <div class="bg-white border border-gray-200 rounded-lg p-6 space-y-8">
        
        <ProviderBaseInfoForm v-model:editingData="editingData" />

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

        <!-- Back to list -->
        <div class="mt-6 flex justify-end space-x-2 pt-4 border-t">
          <Button variant="secondary" @click="router.push('/provider')">{{
            $t("providerEditPage.buttonBackToList")
          }}</Button>
        </div>
      </div>

      <!-- ============ Model Select Modal ============ -->
      <Dialog
        :open="isModelSelectModalOpen"
        @update:open="(v: boolean) => (isModelSelectModalOpen = v)"
      >
        <DialogContent class="max-w-md bg-white border border-gray-200">
          <DialogHeader>
            <DialogTitle class="text-lg font-semibold text-gray-900">{{
              $t("providerEditPage.modalSelectModel.title")
            }}</DialogTitle>
          </DialogHeader>
          <div class="py-4 space-y-4">
            <p class="text-sm text-gray-500">
              {{ $t("providerEditPage.modalSelectModel.description") }}
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
          <DialogFooter>
            <Button
              variant="ghost"
              class="text-gray-600"
              @click="isModelSelectModalOpen = false"
              >{{ $t("common.cancel") }}</Button
            >
            <Button
              variant="default"
              @click="handleConfirmModelSelection"
              :disabled="modelIndexToUseStr === null"
              >{{ $t("common.check") }}</Button
            >
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <!-- ============ API Key Select Modal ============ -->
      <Dialog
        :open="isApiKeySelectModalOpen"
        @update:open="(v: boolean) => (isApiKeySelectModalOpen = v)"
      >
        <DialogContent class="max-w-md bg-white border border-gray-200">
          <DialogHeader>
            <DialogTitle class="text-lg font-semibold text-gray-900">{{
              $t("providerEditPage.modalSelectApiKey.title")
            }}</DialogTitle>
          </DialogHeader>
          <div class="py-4 space-y-4">
            <p class="text-sm text-gray-500">
              {{ $t("providerEditPage.modalSelectApiKey.description") }}
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
          <DialogFooter>
            <Button
              variant="ghost"
              class="text-gray-600"
              @click="isApiKeySelectModalOpen = false"
              >{{ $t("common.cancel") }}</Button
            >
            <Button
              variant="default"
              @click="handleConfirmApiKeySelection"
              :disabled="apiKeyIndexToUseStr === null"
              >{{ $t("common.check") }}</Button
            >
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </template>
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, onMounted } from "vue";
import { useI18n } from "vue-i18n";
import { useRoute, useRouter } from "vue-router";
import { Api } from "@/services/request";
import type { CustomFieldType, CustomFieldItem } from "@/store/types";
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

const {
  isModelSelectModalOpen,
  isApiKeySelectModalOpen,
  modelIndexToUseStr,
  apiKeyIndexToUseStr,
  modelOptionsForSelect,
  apiKeyOptionsForSelect,
  handleCheck,
  handleBatchCheck,
  handleConfirmModelSelection,
  handleConfirmApiKeySelection,
} = useProviderCheck(editingData);

const fetchAllCustomFields = async (): Promise<CustomFieldItem[]> => {
  try {
    const response = await Api.getCustomFieldList(1000);
    if (response && response.list) {
      return response.list.map((f: any) => ({
        id: f.id,
        field_name: f.field_name,
        field_value:
          (f.string_value ??
            f.integer_value?.toString() ??
            f.number_value?.toString() ??
            f.boolean_value?.toString()) ||
          "",
        description: f.description,
        field_type: (f.field_type?.toLowerCase() as CustomFieldType) || "unset",
      }));
    }
    return [];
  } catch (error) {
    console.error("Failed to fetch all custom fields", error);
    toastController.error("Failed to fetch all custom fields");
    return [];
  }
};

const fetchProviderDetail = async (id: number) => {
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
  id: null,
  name: "",
  provider_key: "",
  provider_type: "OPENAI",
  endpoint: "",
  use_proxy: false,
  models: [],
  provider_keys: [],
  custom_fields: [],
});

onMounted(async () => {
  isLoading.value = true;
  errorMsg.value = null;

  const fields = await fetchAllCustomFields();
  allCustomFields.value = fields;

  if (providerId.value) {
    const detail = await fetchProviderDetail(providerId.value);
    if (detail) {
      editingData.value = reactive({
        id: detail.provider.id,
        name: detail.provider.name,
        provider_key: detail.provider.provider_key,
        provider_type: detail.provider.provider_type || "OPENAI",
        endpoint: detail.provider.endpoint,
        use_proxy: detail.provider.use_proxy,
        models: detail.models.map((m: any) => ({
          id: m.model.id,
          model_name: m.model.model_name,
          real_model_name: m.model.real_model_name ?? null,
          isEditing: false,
          checkStatus: "unchecked" as const,
        })),
        provider_keys: detail.provider_keys.map((k: any) => ({
          id: k.id,
          api_key: k.api_key,
          description: k.description ?? null,
          isEditing: false,
          checkStatus: "unchecked" as const,
        })),
        custom_fields: (detail.custom_fields || []).map((f: any) => ({
          id: f.id,
          field_name: f.field_name,
          field_value:
            (f.string_value ??
              f.integer_value?.toString() ??
              f.number_value?.toString() ??
              f.boolean_value?.toString()) ||
            "",
          description: f.description,
          field_type:
            (f.field_type?.toLowerCase() as CustomFieldType) || "unset",
        })),
      });
    } else {
      errorMsg.value = $t("providerEditPage.alert.loadDataFailed", {
        providerId: providerId.value,
      });
    }
  } else {
    editingData.value = reactive(getEmptyProvider());
  }
  isLoading.value = false;
});
</script>
