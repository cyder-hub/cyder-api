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
        <!-- Base Info -->
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
                    :placeholder="
                      $t('providerEditPage.placeholderProviderType')
                    "
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
          <div
            class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
          >
            <Label
              for="use_proxy_checkbox"
              class="cursor-pointer text-gray-700"
              >{{ $t("providerEditPage.labelUseProxy") }}</Label
            >
            <Checkbox
              id="use_proxy_checkbox"
              :checked="editingData.use_proxy"
              @update:checked="(val: boolean) => (editingData!.use_proxy = val)"
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

        <!-- ============ Models Section ============ -->
        <div class="space-y-4 pt-4 border-t border-gray-100">
          <div class="flex justify-between items-center">
            <h3 class="text-lg font-semibold text-gray-900">
              {{ $t("providerEditPage.sectionModels") }}
            </h3>
            <div class="space-x-2">
              <Button
                variant="outline"
                size="sm"
                @click="handleBatchCheck('models')"
                :disabled="!editingData.id || editingData.models.length === 0"
              >
                <Check class="h-4 w-4 mr-1.5" />
                {{ $t("providerEditPage.alert.buttonCheckAll") }}
              </Button>
              <Button
                variant="outline"
                size="sm"
                @click="handleFetchRemoteModels"
                :disabled="!editingData.id"
              >
                <CloudDownload class="h-4 w-4 mr-1.5" />
                {{ $t("providerEditPage.buttonFetchRemote") }}
              </Button>
            </div>
          </div>

          <div class="border border-gray-200 rounded-lg overflow-hidden">
            <!-- Header -->
            <div
              class="grid grid-cols-[1fr_1fr_auto] gap-4 items-center px-4 py-3 bg-gray-50/80 border-b border-gray-200"
            >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderModelId") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderMappedModelId") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >操作</span
              >
            </div>

            <div
              v-if="editingData.models.length === 0"
              class="flex flex-col items-center justify-center py-10"
            >
              <Box class="h-10 w-10 stroke-1 text-gray-400 mb-2" />
              <span class="text-sm font-medium text-gray-500">暂无模型</span>
            </div>

            <!-- Models rows -->
            <div
              v-for="(model, index) in editingData.models"
              :key="index"
              class="grid grid-cols-[1fr_1fr_auto] gap-4 items-center px-4 py-3 border-b border-gray-100 last:border-0 hover:bg-gray-50/50 transition-colors"
            >
              <Input
                v-model="model.model_name"
                :disabled="!!model.id"
                :placeholder="$t('providerEditPage.placeholderModelId')"
                class="font-mono text-sm h-8"
              />
              <Input
                :model-value="model.real_model_name ?? ''"
                @update:model-value="
                  (v: string | number) =>
                    (model.real_model_name = String(v) || null)
                "
                :disabled="!!model.id"
                :placeholder="$t('providerEditPage.placeholderMappedModelId')"
                class="font-mono text-sm h-8"
              />
              <div class="flex items-center space-x-1 justify-end">
                <template v-if="!model.id && editingData!.id">
                  <Button
                    variant="default"
                    size="sm"
                    class="h-8"
                    @click="handleSaveSingleModel(index)"
                  >
                    {{ $t("providerEditPage.buttonSaveModel") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    :title="model.checkMessage"
                    @click="handleCheck('model', index)"
                  >
                    <Loader2
                      v-if="model.checkStatus === 'checking'"
                      class="h-4 w-4 animate-spin text-blue-500"
                    />
                    <AlertCircle
                      v-else-if="model.checkStatus === 'error'"
                      class="h-4 w-4 text-red-500"
                    />
                    <Check
                      v-else-if="model.checkStatus === 'success'"
                      class="h-4 w-4 text-green-500"
                    />
                    <Check v-else class="h-4 w-4" />
                  </Button>
                </template>
                <template v-if="model.id">
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    :title="model.checkMessage"
                    @click="handleCheck('model', index)"
                  >
                    <Loader2
                      v-if="model.checkStatus === 'checking'"
                      class="h-4 w-4 animate-spin text-blue-500"
                    />
                    <AlertCircle
                      v-else-if="model.checkStatus === 'error'"
                      class="h-4 w-4 text-red-500"
                    />
                    <Check
                      v-else-if="model.checkStatus === 'success'"
                      class="h-4 w-4 text-green-500"
                    />
                    <Check v-else class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    @click="router.push(`/model/edit/${model.id}`)"
                  >
                    <Edit2 class="h-4 w-4" />
                  </Button>
                </template>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-8 text-gray-400 hover:text-red-600 px-2"
                  @click="handleDeleteModel(index)"
                >
                  <Trash2 class="h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>

          <!-- Model action buttons -->
          <div class="flex items-center gap-2 pt-2">
            <Button variant="outline" size="sm" @click="addModel">
              <Plus class="h-4 w-4 mr-1.5" />
              {{ $t("providerEditPage.buttonAddModel") }}
            </Button>
            <Button
              v-if="hasUncommittedModels"
              variant="outline"
              size="sm"
              class="text-red-600 hover:text-red-700 hover:bg-red-50 border-red-200"
              @click="handleClearUncommittedModels"
            >
              <Trash2 class="h-4 w-4 mr-1.5" />
              {{ $t("providerEditPage.buttonClearUncommitted") }}
            </Button>
          </div>
        </div>

        <!-- ============ API Keys Section ============ -->
        <div class="space-y-4 pt-4 border-t border-gray-100">
          <div class="flex justify-between items-center">
            <h3 class="text-lg font-semibold text-gray-900">
              {{ $t("providerEditPage.sectionApiKeys") }}
            </h3>
            <Button
              variant="outline"
              size="sm"
              @click="handleBatchCheck('api_keys')"
              :disabled="
                !editingData.id || editingData.provider_keys.length === 0
              "
            >
              <Check class="h-4 w-4 mr-1.5" />
              {{ $t("providerEditPage.alert.buttonCheckAll") }}
            </Button>
          </div>

          <div class="border border-gray-200 rounded-lg overflow-hidden">
            <!-- Header -->
            <div
              class="grid grid-cols-[2fr_1fr_auto] gap-4 items-center px-4 py-3 bg-gray-50/80 border-b border-gray-200"
            >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderApiKey") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderDescription") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >操作</span
              >
            </div>

            <div
              v-if="editingData.provider_keys.length === 0"
              class="flex flex-col items-center justify-center py-10"
            >
              <Key class="h-10 w-10 stroke-1 text-gray-400 mb-2" />
              <span class="text-sm font-medium text-gray-500"
                >暂无 API Key</span
              >
            </div>

            <!-- API Key rows -->
            <div
              v-for="(keyItem, index) in editingData.provider_keys"
              :key="index"
              class="grid grid-cols-[2fr_1fr_auto] gap-4 items-center px-4 py-3 border-b border-gray-100 last:border-0 hover:bg-gray-50/50 transition-colors"
            >
              <Input
                v-model="keyItem.api_key"
                :disabled="!!keyItem.id"
                :placeholder="$t('providerEditPage.placeholderApiKey')"
                :type="
                  editingData!.provider_type === 'VERTEX' || !!keyItem.id
                    ? 'text'
                    : 'password'
                "
                class="font-mono text-sm h-8"
              />
              <Input
                :model-value="keyItem.description ?? ''"
                @update:model-value="
                  (v: string | number) =>
                    (keyItem.description = String(v) || null)
                "
                :disabled="!!keyItem.id && !keyItem.isEditing"
                :placeholder="$t('providerEditPage.placeholderDescription')"
                class="text-sm h-8"
              />
              <div class="flex items-center space-x-1 justify-end">
                <template v-if="!keyItem.id && editingData!.id">
                  <Button
                    variant="default"
                    size="sm"
                    class="h-8"
                    @click="handleSaveSingleApiKey(index)"
                  >
                    {{ $t("providerEditPage.buttonSaveThisKey") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    :title="keyItem.checkMessage"
                    @click="handleCheck('apiKey', index)"
                  >
                    <Loader2
                      v-if="keyItem.checkStatus === 'checking'"
                      class="h-4 w-4 animate-spin text-blue-500"
                    />
                    <AlertCircle
                      v-else-if="keyItem.checkStatus === 'error'"
                      class="h-4 w-4 text-red-500"
                    />
                    <Check
                      v-else-if="keyItem.checkStatus === 'success'"
                      class="h-4 w-4 text-green-500"
                    />
                    <Check v-else class="h-4 w-4" />
                  </Button>
                </template>
                <template v-if="keyItem.id && !keyItem.isEditing">
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    :title="keyItem.checkMessage"
                    @click="handleCheck('apiKey', index)"
                  >
                    <Loader2
                      v-if="keyItem.checkStatus === 'checking'"
                      class="h-4 w-4 animate-spin text-blue-500"
                    />
                    <AlertCircle
                      v-else-if="keyItem.checkStatus === 'error'"
                      class="h-4 w-4 text-red-500"
                    />
                    <Check
                      v-else-if="keyItem.checkStatus === 'success'"
                      class="h-4 w-4 text-green-500"
                    />
                    <Check v-else class="h-4 w-4" />
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    @click="keyItem.isEditing = true"
                  >
                    <Edit2 class="h-4 w-4" />
                  </Button>
                </template>
                <template v-if="keyItem.id && keyItem.isEditing">
                  <Button
                    variant="default"
                    size="sm"
                    class="h-8"
                    @click="handleSaveSingleApiKey(index)"
                  >
                    {{ $t("common.save") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-8 text-gray-600 px-2"
                    @click="keyItem.isEditing = false"
                  >
                    <X class="h-4 w-4" />
                  </Button>
                </template>
                <Button
                  variant="ghost"
                  size="sm"
                  class="h-8 text-gray-400 hover:text-red-600 px-2"
                  @click="handleDeleteApiKey(index)"
                >
                  <Trash2 class="h-4 w-4" />
                </Button>
              </div>
            </div>
          </div>
          <div class="pt-2">
            <Button variant="outline" size="sm" @click="addApiKey">
              <Plus class="h-4 w-4 mr-1.5" />
              {{ $t("providerEditPage.buttonAddApiKey") }}
            </Button>
          </div>
        </div>

        <!-- ============ Custom Fields Section ============ -->
        <div class="space-y-4 pt-4 border-t border-gray-100">
          <h3 class="text-lg font-semibold text-gray-900">
            {{ $t("providerEditPage.sectionCustomFields") }}
          </h3>

          <div class="border border-gray-200 rounded-lg overflow-hidden">
            <!-- Header -->
            <div
              class="grid grid-cols-[1fr_1fr_1fr_1fr_auto] gap-4 items-center px-4 py-3 bg-gray-50/80 border-b border-gray-200"
            >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderFieldName") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderFieldValue") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderDescription") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                >{{ $t("providerEditPage.tableHeaderFieldType") }}</span
              >
              <span
                class="text-xs font-medium text-gray-500 uppercase tracking-wider text-right"
                >操作</span
              >
            </div>

            <div
              v-if="editingData.custom_fields.length === 0"
              class="flex flex-col items-center justify-center py-10"
            >
              <FileText class="h-10 w-10 stroke-1 text-gray-400 mb-2" />
              <span class="text-sm font-medium text-gray-500"
                >暂无自定义字段</span
              >
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
                  :placeholder="
                    $t('modelEditPage.placeholderSelectCustomField')
                  "
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
import { useProviderStore } from "@/store/providerStore";
import type { CustomFieldType, CustomFieldItem } from "@/store/types";
import { toastController } from "@/lib/toastController";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
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
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
} from "@/components/ui/dialog";
import {
  ArrowLeft,
  Plus,
  Check,
  Trash2,
  Edit2,
  X,
  CloudDownload,
  Box,
  Key,
  FileText,
  Loader2,
  AlertCircle,
} from "lucide-vue-next";

// ========== Types ==========
interface LocalProviderApiKeyItem {
  id: number | null;
  api_key: string;
  description: string | null;
  isEditing: boolean;
  checkStatus: "unchecked" | "checking" | "success" | "error";
  checkMessage?: string;
}

interface LocalEditableModelItem {
  id: number | null;
  model_name: string;
  real_model_name: string | null;
  isEditing: boolean;
  checkStatus: "unchecked" | "checking" | "success" | "error";
  checkMessage?: string;
}

interface EditingProviderData {
  id: number | null;
  name: string;
  provider_key: string;
  provider_type: string;
  endpoint: string;
  use_proxy: boolean;
  models: LocalEditableModelItem[];
  provider_keys: LocalProviderApiKeyItem[];
  custom_fields: CustomFieldItem[];
}

// ========== Constants ==========
const providerTypes = ["OPENAI", "GEMINI", "VERTEX", "VERTEX_OPENAI", "OLLAMA"];

// ========== Composables ==========
const { t: $t } = useI18n();
const route = useRoute();
const router = useRouter();
const providerStore = useProviderStore();

// ========== Route params ==========
const providerId = computed(() => {
  const id = route.params.id;
  if (id) {
    const num = parseInt(id as string, 10);
    return isNaN(num) ? null : num;
  }
  return null;
});

// ========== State ==========
const isLoading = ref(true);
const errorMsg = ref<string | null>(null);
const editingData = ref<EditingProviderData | null>(null);
const allCustomFields = ref<CustomFieldItem[]>([]);
const selectedCustomFieldId = ref<string | null>(null);

// Modal states
const isModelSelectModalOpen = ref(false);
const isApiKeySelectModalOpen = ref(false);
const apiKeyIndexToCheck = ref<number | null>(null);
const modelIndexToUseStr = ref<string | null>(null);
const isBatchCheckingApiKeys = ref(false);
const modelIndexToCheck = ref<number | null>(null);
const apiKeyIndexToUseStr = ref<string | null>(null);
const isBatchCheckingModels = ref(false);

// ========== Computed ==========
const hasUncommittedModels = computed(() => {
  if (!editingData.value) return false;
  return editingData.value.models.some((m) => m.id === null);
});

const modelOptionsForSelect = computed(() => {
  if (!editingData.value?.models) return [];
  return editingData.value.models.map((m, i) => ({
    value: i,
    label: m.model_name,
  }));
});

const apiKeyOptionsForSelect = computed(() => {
  if (!editingData.value?.provider_keys) return [];
  return editingData.value.provider_keys.map((k, i) => ({
    value: i,
    label:
      k.description ||
      $t("providerEditPage.alert.apiKeyNameFallback", {
        lastKeyChars: k.api_key.slice(-4),
      }),
  }));
});

const availableCustomFields = computed(() => {
  if (!editingData.value) return [];
  const linkedIds = new Set(editingData.value.custom_fields.map((f) => f.id));
  return allCustomFields.value.filter((f) => f.id && !linkedIds.has(f.id));
});

// ========== Data fetching ==========
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

// ========== Base Info ==========
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
      toastController.success(
        $t("providerEditPage.alert.baseInfoUpdateSuccess"),
      );
    } else {
      const newProvider: any = await Api.createProvider(basePayload);
      data.id = newProvider.id;
      toastController.success($t("providerEditPage.alert.createSuccess"));
    }
    providerStore.refetchProviders();
  } catch (error) {
    console.error("Failed to save provider base info:", error);
    toastController.error(
      $t("providerEditPage.alert.baseInfoSaveFailed", {
        error: (error as Error).message || $t("unknownError"),
      }),
    );
  }
};

// ========== Models ==========
const addModel = () => {
  editingData.value?.models.push({
    id: null,
    model_name: "",
    real_model_name: null,
    isEditing: false,
    checkStatus: "unchecked",
  });
};

const handleSaveSingleModel = async (index: number) => {
  const data = editingData.value;
  if (!data || !data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForModel"));
    return;
  }

  const modelItem = data.models[index];
  if (!modelItem.model_name.trim()) {
    toastController.warn(
      $t("providerEditPage.alert.modelIdRequiredWithIndex", {
        index: index + 1,
      }),
    );
    return;
  }

  try {
    const savedModel: any = await Api.createModel({
      provider_id: data.id,
      model_name: modelItem.model_name,
      real_model_name: modelItem.real_model_name || null,
      is_enabled: true,
    });
    modelItem.id = savedModel.id;
    modelItem.model_name = savedModel.model_name;
    modelItem.real_model_name = savedModel.real_model_name ?? null;
    modelItem.isEditing = false;
    toastController.success($t("providerEditPage.alert.modelSaveSuccess"));
  } catch (error) {
    console.error("Failed to save model:", error);
    toastController.error(
      $t("providerEditPage.alert.saveModelFailed", {
        error: (error as Error).message || $t("unknownError"),
      }),
    );
  }
};

const handleDeleteModel = async (index: number) => {
  const data = editingData.value;
  if (!data) return;

  const modelItem = data.models[index];

  if (modelItem.id) {
    if (!data.id) {
      toastController.warn(
        $t("providerEditPage.alert.providerNotSavedForModelDelete"),
      );
      return;
    }
    try {
      await Api.deleteModel(modelItem.id);
      data.models.splice(index, 1);
      toastController.success($t("providerEditPage.alert.modelDeleteSuccess"));
    } catch (error) {
      console.error("Failed to delete model:", error);
      toastController.error(
        $t("providerEditPage.alert.deleteModelFailed", {
          error: (error as Error).message || $t("unknownError"),
        }),
      );
    }
  } else {
    data.models.splice(index, 1);
  }
};

const handleFetchRemoteModels = async () => {
  const data = editingData.value;
  if (!data || !data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForModel"));
    return;
  }

  try {
    const response: any = await Api.getProviderRemoteModels(data.id);

    let remoteModels: any[] = [];
    let isGeminiLike = false;
    if (response) {
      if (Array.isArray(response.data)) {
        remoteModels = response.data;
      } else if (Array.isArray(response.models)) {
        remoteModels = response.models;
        isGeminiLike = true;
      } else if (Array.isArray(response)) {
        remoteModels = response;
      }
    }

    if (!remoteModels || remoteModels.length === 0) {
      toastController.warn($t("providerEditPage.alert.noRemoteModels"));
      return;
    }

    const existingModelNames = new Set<string>();
    data.models.forEach((m) => {
      existingModelNames.add(m.model_name);
      if (m.real_model_name) existingModelNames.add(m.real_model_name);
    });

    const newModels: LocalEditableModelItem[] = [];
    remoteModels.forEach((item) => {
      let model_name = item.id || item.name;
      const providerType = data.provider_type;
      const isGoogleOwned = item.owned_by === "google";
      const isGeminiProvider =
        providerType === "GEMINI" || providerType === "VERTEX";

      if (
        (isGeminiProvider || isGeminiLike || isGoogleOwned) &&
        model_name &&
        model_name.startsWith("models/")
      ) {
        model_name = model_name.substring("models/".length);
      }

      if (model_name && !existingModelNames.has(model_name)) {
        newModels.push({
          id: null,
          model_name,
          real_model_name: null,
          isEditing: false,
          checkStatus: "unchecked",
        });
        existingModelNames.add(model_name);
      }
    });

    if (newModels.length > 0) {
      data.models.push(...newModels);
      toastController.success(
        $t("providerEditPage.alert.newModelsAdded", {
          count: newModels.length,
        }),
      );
    } else {
      toastController.info($t("providerEditPage.alert.noNewModels"));
    }
  } catch (error) {
    console.error("Failed to fetch remote models:", error);
    toastController.error(
      $t("providerEditPage.alert.fetchRemoteModelsFailed", {
        error: (error as Error).message || $t("unknownError"),
      }),
    );
  }
};

const handleClearUncommittedModels = () => {
  if (!editingData.value) return;
  const originalCount = editingData.value.models.length;
  editingData.value.models = editingData.value.models.filter(
    (m) => m.id !== null,
  );
  if (editingData.value.models.length < originalCount) {
    toastController.info($t("providerEditPage.alert.uncommittedCleared"));
  } else {
    toastController.info($t("providerEditPage.alert.noUncommittedToClear"));
  }
};

// ========== API Keys ==========
const addApiKey = () => {
  editingData.value?.provider_keys.push({
    id: null,
    api_key: "",
    description: null,
    isEditing: false,
    checkStatus: "unchecked",
  });
};

const handleSaveSingleApiKey = async (index: number) => {
  const data = editingData.value;
  if (!data || !data.id) {
    toastController.warn(
      $t("providerEditPage.alert.providerNotSavedForApiKey"),
    );
    return;
  }

  const keyItem = data.provider_keys[index];
  if (!keyItem.api_key.trim()) {
    toastController.warn(
      $t("providerEditPage.alert.apiKeyRequiredWithIndex", {
        index: index + 1,
      }),
    );
    return;
  }

  if (data.provider_type === "VERTEX") {
    try {
      const parsedKey = JSON.parse(keyItem.api_key);
      const requiredFields = [
        "client_email",
        "private_key",
        "private_key_id",
        "token_uri",
      ];
      const missingFields = requiredFields.filter(
        (field) => !(field in parsedKey) || !parsedKey[field],
      );
      if (missingFields.length > 0) {
        toastController.warn(
          $t("providerEditPage.alert.vertexApiKeyMissingFields", {
            index: index + 1,
            fields: missingFields.join(", "),
          }),
        );
        return;
      }
    } catch {
      toastController.warn(
        $t("providerEditPage.alert.vertexApiKeyInvalidJson", {
          index: index + 1,
        }),
      );
      return;
    }
  }

  try {
    const savedKey: any = await Api.createProviderKey(data.id, {
      api_key: keyItem.api_key,
      description: keyItem.description,
    });
    keyItem.id = savedKey.id;
    keyItem.api_key = savedKey.api_key;
    keyItem.description = savedKey.description ?? null;
    keyItem.isEditing = false;
    toastController.success($t("providerEditPage.alert.apiKeySaveSuccess"));
  } catch (error) {
    console.error("Failed to save API key:", error);
    toastController.error(
      $t("providerEditPage.alert.saveApiKeyFailed", {
        error: (error as Error).message || $t("unknownError"),
      }),
    );
  }
};

const handleDeleteApiKey = async (index: number) => {
  const data = editingData.value;
  if (!data) return;

  const keyItem = data.provider_keys[index];
  if (keyItem.id && data.id) {
    try {
      await Api.deleteProviderKey(data.id, keyItem.id);
      data.provider_keys.splice(index, 1);
      toastController.success($t("providerEditPage.alert.apiKeyDeleteSuccess"));
    } catch (error) {
      console.error("Failed to delete API key:", error);
      toastController.error(
        $t("providerEditPage.alert.deleteApiKeyFailed", {
          error: (error as Error).message || $t("unknownError"),
        }),
      );
    }
  } else {
    data.provider_keys.splice(index, 1);
  }
};

// ========== Custom Fields ==========
const handleLinkCustomField = async () => {
  const fieldIdStr = selectedCustomFieldId.value;
  const pId = editingData.value?.id;

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

    const fieldToAdd = allCustomFields.value.find((f) => f.id === fieldId);
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
        error: (error as Error).message || $t("unknownError"),
      }),
    );
  }
};

const handleUnlinkCustomField = async (fieldId: number, index: number) => {
  const pId = editingData.value?.id;
  if (!pId) {
    toastController.warn($t("providerEditPage.alert.providerIdNotFound"));
    return;
  }

  try {
    await Api.unlinkCustomField({
      custom_field_definition_id: fieldId,
      provider_id: pId,
    });
    editingData.value!.custom_fields.splice(index, 1);
    toastController.success(
      $t("providerEditPage.alert.unlinkCustomFieldSuccess"),
    );
  } catch (error) {
    console.error("Failed to unlink custom field:", error);
    toastController.error(
      $t("providerEditPage.alert.unlinkedCustomFieldFailed", {
        error: (error as Error).message || $t("unknownError"),
      }),
    );
  }
};

// ========== Connection Check ==========
const performCheck = async (modelIndex: number, apiKeyIndex: number) => {
  const data = editingData.value;
  if (!data || !data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForCheck"));
    return;
  }

  data.models[modelIndex].checkStatus = "checking";
  data.models[modelIndex].checkMessage = undefined;
  data.provider_keys[apiKeyIndex].checkStatus = "checking";
  data.provider_keys[apiKeyIndex].checkMessage = undefined;

  const modelItem = data.models[modelIndex];
  const keyItem = data.provider_keys[apiKeyIndex];

  const payload: Record<string, any> = {
    ...(modelItem.id
      ? { model_id: modelItem.id }
      : { model_name: modelItem.real_model_name || modelItem.model_name }),
    ...(keyItem.id
      ? { provider_api_key_id: keyItem.id }
      : { provider_api_key: keyItem.api_key }),
  };

  try {
    await Api.checkProviderConnection(data.id, payload);
    data.models[modelIndex].checkStatus = "success";
    data.provider_keys[apiKeyIndex].checkStatus = "success";
  } catch (error) {
    const errMsg = (error as Error).message || $t("unknownError");
    data.models[modelIndex].checkStatus = "error";
    data.models[modelIndex].checkMessage = errMsg;
    data.provider_keys[apiKeyIndex].checkStatus = "error";
    data.provider_keys[apiKeyIndex].checkMessage = errMsg;
  }
};

const performBatchModelCheck = async (apiKeyIndex: number) => {
  const data = editingData.value;
  if (!data || !data.id) return;

  const translatedType = $t("providerEditPage.alert.checkTypeModels");
  toastController.info(
    $t("providerEditPage.alert.batchChecking", { type: translatedType }),
  );

  const key = data.provider_keys[apiKeyIndex];
  data.models.forEach((m) => {
    m.checkStatus = "checking";
    m.checkMessage = undefined;
  });

  let successCount = 0;
  for (const [index, model] of data.models.entries()) {
    const payload: Record<string, any> = {
      ...(model.id
        ? { model_id: model.id }
        : { model_name: model.real_model_name || model.model_name }),
      ...(key.id
        ? { provider_api_key_id: key.id }
        : { provider_api_key: key.api_key }),
    };
    try {
      await Api.checkProviderConnection(data.id!, payload);
      successCount++;
      data.models[index].checkStatus = "success";
    } catch (error) {
      const errMsg = (error as Error).message || $t("unknownError");
      data.models[index].checkStatus = "error";
      data.models[index].checkMessage = errMsg;
    }
  }
  toastController.info(
    $t("providerEditPage.alert.batchCheckComplete", {
      success: successCount,
      total: data.models.length,
      type: translatedType,
    }),
  );
};

const performBatchApiKeyCheck = async (modelIndex: number) => {
  const data = editingData.value;
  if (!data || !data.id) return;

  const translatedType = $t("providerEditPage.alert.checkTypeApiKeys");
  toastController.info(
    $t("providerEditPage.alert.batchChecking", { type: translatedType }),
  );

  const model = data.models[modelIndex];
  data.provider_keys.forEach((k) => {
    k.checkStatus = "checking";
    k.checkMessage = undefined;
  });

  let successCount = 0;
  for (const [index, key] of data.provider_keys.entries()) {
    const payload: Record<string, any> = {
      ...(model.id
        ? { model_id: model.id }
        : { model_name: model.real_model_name || model.model_name }),
      ...(key.id
        ? { provider_api_key_id: key.id }
        : { provider_api_key: key.api_key }),
    };
    try {
      await Api.checkProviderConnection(data.id!, payload);
      successCount++;
      data.provider_keys[index].checkStatus = "success";
    } catch (error) {
      const errMsg = (error as Error).message || $t("unknownError");
      data.provider_keys[index].checkStatus = "error";
      data.provider_keys[index].checkMessage = errMsg;
    }
  }
  toastController.info(
    $t("providerEditPage.alert.batchCheckComplete", {
      success: successCount,
      total: data.provider_keys.length,
      type: translatedType,
    }),
  );
};

// ========== Check Handlers (single + batch with modal selection) ==========
const handleCheck = async (type: "model" | "apiKey", index: number) => {
  const data = editingData.value;
  if (!data || !data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForCheck"));
    return;
  }

  if (type === "model") {
    const apiKeys = data.provider_keys;
    if (apiKeys.length === 0) {
      toastController.warn($t("providerEditPage.alert.noApiKeyForCheck"));
      data.models[index].checkStatus = "error";
      data.models[index].checkMessage = $t(
        "providerEditPage.alert.noApiKeyForCheck",
      );
      return;
    }
    if (apiKeys.length === 1) {
      await performCheck(index, 0);
    } else {
      modelIndexToCheck.value = index;
      apiKeyIndexToUseStr.value = "0";
      isApiKeySelectModalOpen.value = true;
    }
  } else {
    const models = data.models;
    if (models.length === 0) {
      toastController.warn($t("providerEditPage.alert.noModelForCheck"));
      data.provider_keys[index].checkStatus = "error";
      data.provider_keys[index].checkMessage = $t(
        "providerEditPage.alert.noModelForCheck",
      );
      return;
    }
    if (models.length === 1) {
      await performCheck(0, index);
    } else {
      apiKeyIndexToCheck.value = index;
      modelIndexToUseStr.value = "0";
      isModelSelectModalOpen.value = true;
    }
  }
};

const handleBatchCheck = async (type: "models" | "api_keys") => {
  const data = editingData.value;
  if (!data || !data.id) {
    toastController.warn($t("providerEditPage.alert.providerNotSavedForCheck"));
    return;
  }

  if (type === "models") {
    if (data.models.length === 0) {
      toastController.info($t("providerEditPage.alert.noModelsToCheck"));
      return;
    }
    if (data.provider_keys.length === 0) {
      toastController.warn($t("providerEditPage.alert.noApiKeyForCheck"));
      return;
    }
    if (data.provider_keys.length === 1) {
      await performBatchModelCheck(0);
    } else {
      isBatchCheckingModels.value = true;
      apiKeyIndexToUseStr.value = "0";
      isApiKeySelectModalOpen.value = true;
    }
  } else {
    if (data.provider_keys.length === 0) {
      toastController.info($t("providerEditPage.alert.noApiKeysToCheck"));
      return;
    }
    if (data.models.length === 0) {
      toastController.warn($t("providerEditPage.alert.noModelForCheck"));
      return;
    }
    if (data.models.length === 1) {
      await performBatchApiKeyCheck(0);
    } else {
      isBatchCheckingApiKeys.value = true;
      modelIndexToUseStr.value = "0";
      isModelSelectModalOpen.value = true;
    }
  }
};

const handleConfirmModelSelection = () => {
  const akIndex = apiKeyIndexToCheck.value;
  const mIndex =
    modelIndexToUseStr.value !== null ? Number(modelIndexToUseStr.value) : null;

  isModelSelectModalOpen.value = false;

  if (mIndex !== null) {
    if (isBatchCheckingApiKeys.value) {
      performBatchApiKeyCheck(mIndex);
    } else if (akIndex !== null) {
      performCheck(mIndex, akIndex);
    }
  }

  apiKeyIndexToCheck.value = null;
  modelIndexToUseStr.value = null;
  isBatchCheckingApiKeys.value = false;
};

const handleConfirmApiKeySelection = () => {
  const mIndex = modelIndexToCheck.value;
  const akIndex =
    apiKeyIndexToUseStr.value !== null
      ? Number(apiKeyIndexToUseStr.value)
      : null;

  isApiKeySelectModalOpen.value = false;

  if (akIndex !== null) {
    if (isBatchCheckingModels.value) {
      performBatchModelCheck(akIndex);
    } else if (mIndex !== null) {
      performCheck(mIndex, akIndex);
    }
  }

  modelIndexToCheck.value = null;
  apiKeyIndexToUseStr.value = null;
  isBatchCheckingModels.value = false;
};
</script>
