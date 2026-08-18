<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader
      :title="
        editingData.id
          ? $t('providerEditPage.sections.logicalProvider.title')
          : $t('providerEditPage.sections.quickStart.title')
      "
      :help="
        editingData.id
          ? $t('providerEditPage.sections.logicalProvider.description')
          : $t('providerEditPage.sections.quickStart.description')
      "
      :help-label="
        editingData.id
          ? $t('providerEditPage.sections.logicalProvider.title')
          : $t('providerEditPage.sections.quickStart.title')
      "
    />

    <div class="space-y-4 rounded-lg border border-gray-200 bg-gray-50/40 p-3.5 sm:p-4">
      <div>
        <h3 class="text-sm font-semibold text-gray-900">
          {{ $t("providerEditPage.sections.logicalProvider.title") }}
        </h3>
        <p class="mt-1 text-xs leading-5 text-gray-500">
          {{ $t("providerEditPage.sections.logicalProvider.description") }}
        </p>
      </div>

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div class="space-y-1.5">
          <Label class="text-gray-700">{{ $t("providerEditPage.labelName") }}</Label>
          <Input
            v-model="quickStart.provider_name"
            class="font-mono text-sm"
            :placeholder="$t('providerEditPage.quickStart.placeholderProviderName')"
          />
        </div>

        <div class="space-y-1.5">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.labelProviderKey") }}
            <span v-if="!editingData.id" class="ml-0.5 text-red-500">*</span>
          </Label>
          <Input
            v-model="quickStart.provider_key"
            class="font-mono text-sm"
            :disabled="!!editingData.id"
            :placeholder="$t('providerEditPage.quickStart.placeholderProviderKey')"
          />
        </div>

        <div
          v-if="editingData.id"
          class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5"
        >
          <Label for="provider_enabled" class="cursor-pointer text-gray-700">
            {{ $t("providerEditPage.labelEnabled") }}
          </Label>
          <Checkbox id="provider_enabled" v-model="editingData.is_enabled" />
        </div>

        <div v-if="editingData.id" class="space-y-1.5">
          <Label class="text-gray-700">{{ $t("providerEditPage.labelApiKeyMode") }}</Label>
          <Select v-model="editingData.provider_api_key_mode">
            <SelectTrigger class="w-full">
              <SelectValue :placeholder="$t('providerEditPage.placeholderApiKeyMode')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="mode in providerApiKeyModes" :key="mode" :value="mode">
                {{ $t(`providerEditPage.apiKeyModes.${mode}`) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="!editingData.id" class="space-y-1.5">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.quickStart.labelApiKey") }}
            <span class="ml-0.5 text-red-500">*</span>
          </Label>
          <Input
            v-model="quickStart.api_key"
            type="password"
            class="font-mono text-sm"
            :placeholder="$t('providerEditPage.placeholderApiKey')"
          />
        </div>

        <div v-if="!editingData.id" class="space-y-1.5">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.quickStart.labelModelName") }}
            <span class="ml-0.5 text-red-500">*</span>
          </Label>
          <Input v-model="quickStart.model_name" class="font-mono text-sm" />
        </div>

        <div v-if="!editingData.id" class="space-y-1.5">
          <Label class="text-gray-700">
            {{ $t("modelEditPage.labelModelKind") }}
            <span class="ml-0.5 text-red-500">*</span>
          </Label>
          <Select v-model="quickStart.model_kind">
            <SelectTrigger class="w-full"><SelectValue /></SelectTrigger>
            <SelectContent>
              <SelectItem v-for="kind in ['CHAT', 'EMBEDDING', 'RERANK']" :key="kind" :value="kind">
                {{ $t(`modelKinds.${kind}`) }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="!editingData.id" class="space-y-1.5 sm:col-span-2">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.quickStart.labelApiKeyDescription") }}
          </Label>
          <Input
            v-model="quickStart.api_key_description"
            :placeholder="$t('providerEditPage.quickStart.placeholderApiKeyDescription')"
          />
          <p class="text-xs leading-5 text-gray-500">
            {{ $t("providerEditPage.credentials.providerOwnership") }}
          </p>
        </div>
      </div>
    </div>

    <div v-if="!editingData.id" class="space-y-4 rounded-lg border border-gray-200 p-3.5 sm:p-4">
      <div>
        <h3 class="text-sm font-semibold text-gray-900">
          {{ $t("providerEditPage.sections.initialSource.title") }}
        </h3>
        <p class="mt-1 text-xs leading-5 text-gray-500">
          {{ $t("providerEditPage.sections.initialSource.description") }}
        </p>
      </div>

      <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div class="space-y-1.5">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.labelProfileType") }}
            <span class="ml-0.5 text-red-500">*</span>
          </Label>
          <Select v-model="quickStart.profile_type">
            <SelectTrigger class="w-full">
              <SelectValue :placeholder="$t('providerEditPage.placeholderProfileType')" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="profile in providerProfileTypes" :key="profile" :value="profile">
                {{ profile }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div class="space-y-1.5 sm:col-span-2">
          <Label class="text-gray-700">
            {{ $t("providerEditPage.labelBaseUrl") }}
            <span v-if="!sourceBaseUrlMayBeEmpty(quickStart.profile_type)" class="ml-0.5 text-red-500">*</span>
          </Label>
          <Input
            v-model="quickStart.base_url"
            class="font-mono text-sm"
            :placeholder="sourceBaseUrlMayBeEmpty(quickStart.profile_type) ? $t('providerEditPage.sources.officialDefaultPlaceholder') : ''"
          />
          <p v-if="sourceBaseUrlMayBeEmpty(quickStart.profile_type)" class="text-xs leading-5 text-gray-500">
            {{ $t("providerEditPage.sources.officialDefaultHelp") }}
          </p>
        </div>

        <div
          v-for="operation in operationRows"
          :key="operation.key"
          class="space-y-3 rounded-lg border border-gray-200 bg-gray-50/40 p-3.5 sm:col-span-2"
        >
          <div class="flex items-center justify-between gap-4">
            <div>
              <Label :for="`quick-source-${operation.key}`" class="cursor-pointer text-gray-700">
                {{ $t(`providerEditPage.sources.operations.${operation.key}.label`) }}
              </Label>
              <p class="mt-1 text-xs leading-5 text-gray-500">
                {{ $t(`providerEditPage.sources.operations.${operation.key}.help`) }}
              </p>
            </div>
            <Checkbox :id="`quick-source-${operation.key}`" v-model="quickStart[operation.enabledKey]" />
          </div>
          <Input
            v-model="quickStart[operation.pathKey]"
            class="font-mono text-sm"
            :placeholder="$t('providerEditPage.sources.operationPathPlaceholder')"
          />
        </div>
      </div>

      <div class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5">
        <Label for="provider_quick_start_proxy" class="cursor-pointer text-gray-700">
          {{ $t("providerEditPage.labelUseProxy") }}
        </Label>
        <Checkbox id="provider_quick_start_proxy" v-model="quickStart.use_proxy" />
      </div>
    </div>

    <div class="border-t border-gray-100 pt-4">
      <div class="flex flex-col gap-1 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h3 class="text-sm font-semibold text-gray-900">
            {{ $t("providerEditPage.preview.title") }}
          </h3>
        </div>
        <Badge variant="outline" class="w-fit font-mono text-[10px] uppercase tracking-wider">
          {{ $t("providerEditPage.preview.finalValues") }}
        </Badge>
      </div>
      <dl class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
        <div class="space-y-1">
          <dt class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
            {{ $t("providerEditPage.preview.providerName") }}
          </dt>
          <dd class="break-all font-mono text-sm text-gray-800">
            {{ preview.provider_name }}
          </dd>
        </div>
        <div class="space-y-1">
          <dt class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
            {{ $t("providerEditPage.preview.providerKey") }}
          </dt>
          <dd class="break-all font-mono text-sm text-gray-800">
            {{ preview.provider_key }}
          </dd>
        </div>
      </dl>
    </div>

    <div class="flex flex-col gap-2 border-t border-gray-100 pt-4 sm:flex-row sm:justify-end">
      <template v-if="editingData.id">
        <Button
          variant="default"
          class="w-full sm:w-auto"
          :disabled="isSubmitting"
          @click="handleUpdateProvider()"
        >
          <Loader2 v-if="pendingAction === 'save'" class="mr-1.5 h-4 w-4 animate-spin" />
          <span v-else>{{ $t("providerEditPage.buttonUpdateBaseInfo") }}</span>
          <span v-if="pendingAction === 'save'">{{ $t("common.saving") }}</span>
        </Button>
      </template>
      <template v-else>
        <Button
          variant="outline"
          class="w-full sm:w-auto"
          :disabled="isSubmitting"
          @click="handleBootstrap(false)"
        >
          <Loader2 v-if="pendingAction === 'save'" class="mr-1.5 h-4 w-4 animate-spin" />
          <span v-else>{{ $t("providerEditPage.buttonSaveOnly") }}</span>
          <span v-if="pendingAction === 'save'">{{ $t("common.saving") }}</span>
        </Button>
        <Button
          variant="default"
          class="w-full sm:w-auto"
          :disabled="isSubmitting"
          @click="handleBootstrap(true)"
        >
          <Loader2 v-if="pendingAction === 'test'" class="mr-1.5 h-4 w-4 animate-spin" />
          <span v-else>{{ $t("providerEditPage.buttonSaveAndTest") }}</span>
          <span v-if="pendingAction === 'test'">{{ $t("common.saving") }}</span>
        </Button>
      </template>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import * as providerService from "@/services/providers";
import { useProviderStore } from "@/store/providerStore";
import { useAuthStore } from "@/store/authStore";
import { toastController } from "@/services/uiFeedback";
import SectionHeader from "@/components/SectionHeader.vue";
import type { EditingProviderData } from "../types";
import type { ProviderBootstrapResponse } from "@/services/types";
import type { ProviderBootstrapFormState } from "../composables/providerEditState";
import {
  buildProviderUpdatePayload,
  createProviderBootstrapFormState,
  buildProviderBootstrapPayload,
  buildProviderBootstrapPreview,
  hydrateEditingProviderDataFromBootstrap,
  normalizeBootstrapCheckResult,
  syncProviderBootstrapFormState,
} from "../composables/providerEditState";
import { Badge } from "@/components/ui/badge";
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
import { Loader2 } from "lucide-vue-next";
import {
  applySourceProfileDefaults,
  providerProfileTypes,
  sourceBaseUrlMayBeEmpty,
  sourceSupportsOperation,
} from "../composables/sourceProfileContract";

const providerApiKeyModes = ["QUEUE", "RANDOM"];

const { t: $t } = useI18n();
const providerStore = useProviderStore();
const authStore = useAuthStore();
const editingData = defineModel<EditingProviderData>("editingData", {
  required: true,
});

const quickStart = reactive<ProviderBootstrapFormState>(
  createProviderBootstrapFormState(editingData.value),
);

const isSubmitting = ref(false);
const pendingAction = ref<"save" | "test" | null>(null);
const operationRows = computed(() =>
  [
    { key: "chatCompletions", operation: "chat_completions", enabledKey: "chat_completions_enabled", pathKey: "chat_completions_path_override" },
    { key: "embeddings", operation: "embeddings", enabledKey: "embeddings_enabled", pathKey: "embeddings_path_override" },
    { key: "rerank", operation: "rerank", enabledKey: "rerank_enabled", pathKey: "rerank_path_override" },
  ].filter((item) => sourceSupportsOperation(
    quickStart.profile_type,
    item.operation as "chat_completions" | "embeddings" | "rerank",
  )) as Array<{
    key: string;
    enabledKey: "chat_completions_enabled" | "embeddings_enabled" | "rerank_enabled";
    pathKey: "chat_completions_path_override" | "embeddings_path_override" | "rerank_path_override";
  }>,
);

watch(
  () => quickStart.profile_type,
  (profile, previous) => {
    if (!editingData.value.id && profile !== previous) {
      applySourceProfileDefaults(quickStart, profile);
    }
  },
);

watch(
  editingData,
  (data) => {
    syncProviderBootstrapFormState(quickStart, data);
  },
  { immediate: true },
);

const preview = computed(() =>
  buildProviderBootstrapPreview(quickStart, {
    provider_name: quickStart.provider_name,
    provider_key: quickStart.provider_key,
  }),
);

const syncHydratedIdentity = (response: ProviderBootstrapResponse) => {
  const data = editingData.value;
  if (!data) return;

  syncProviderBootstrapFormState(quickStart, data);
  quickStart.provider_name = response.provider_name || data.name || quickStart.provider_name;
  quickStart.provider_key = response.provider_key || data.provider_key || quickStart.provider_key;
};

const handleBootstrap = async (saveAndTest: boolean) => {
  if (isSubmitting.value) return;

  const data = editingData.value;
  if (!data) return;

  if (!quickStart.profile_type.trim()) {
    toastController.warn($t("providerEditPage.alert.profileTypeRequired"));
    return;
  }
  if (!sourceBaseUrlMayBeEmpty(quickStart.profile_type) && !quickStart.base_url.trim()) {
    toastController.warn($t("providerEditPage.alert.baseUrlRequired"));
    return;
  }
  if (!quickStart.provider_key.trim()) {
    toastController.warn($t("providerEditPage.alert.providerKeyRequired"));
    return;
  }
  if (!quickStart.api_key.trim()) {
    toastController.warn($t("providerEditPage.alert.apiKeyRequired"));
    return;
  }
  if (!quickStart.model_name.trim()) {
    toastController.warn($t("providerEditPage.alert.modelNameRequired"));
    return;
  }

  const payload = buildProviderBootstrapPayload(quickStart, saveAndTest);

  isSubmitting.value = true;
  pendingAction.value = saveAndTest ? "test" : "save";

  try {
    const response = await providerService.bootstrapProvider(payload);
    hydrateEditingProviderDataFromBootstrap(data, response);
    syncHydratedIdentity(response);

    const checkResult = normalizeBootstrapCheckResult(response.check_result);
    void providerStore.fetchProviders().catch(() => undefined);

    if (saveAndTest && response.check_result?.status === "check_skipped") {
      toastController.warn(
        $t("providerEditPage.alert.bootstrapSaveAndTestSkipped", {
          reason: response.check_result.message,
        }),
      );
      return;
    }

    if (saveAndTest && checkResult && !checkResult.ok) {
      toastController.error(
        $t("providerEditPage.alert.bootstrapSaveAndTestFailed", {
          error: checkResult.message || $t("common.unknownError"),
        }),
      );
      return;
    }

    if (saveAndTest) {
      toastController.success($t("providerEditPage.alert.bootstrapSaveAndTestSuccess"));
    } else {
      toastController.success($t("providerEditPage.alert.bootstrapSaveSuccess"));
    }
  } catch (error) {
    toastController.error(
      $t("providerEditPage.alert.bootstrapFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  } finally {
    payload.api_key = "";
    quickStart.api_key = "";
    isSubmitting.value = false;
    pendingAction.value = null;
  }
};

const handleUpdateProvider = async () => {
  const data = editingData.value;
  if (!data?.id || isSubmitting.value) return;
  const providerName = quickStart.provider_name?.trim() ?? "";

  if (!providerName) {
    toastController.warn($t("providerEditPage.alert.nameRequired"));
    return;
  }

  isSubmitting.value = true;
  pendingAction.value = "save";

  try {
    const updated = await providerService.updateProvider(
      data.id,
      buildProviderUpdatePayload(data, quickStart),
    );
    data.name = updated.name;
    data.provider_key = updated.provider_key;
    data.is_enabled = updated.is_enabled;
    data.provider_api_key_mode = updated.provider_api_key_mode;
    data.upstream_sources = updated.upstream_sources.map((source) => ({ ...source }));
    syncProviderBootstrapFormState(quickStart, data);

    void providerStore.fetchProviders().catch(() => undefined);
    toastController.success($t("providerEditPage.alert.baseInfoUpdateSuccess"));
  } catch (error) {
    toastController.error(
      $t("providerEditPage.alert.baseInfoSaveFailed", {
        error: (error as Error).message || $t("common.unknownError"),
      }),
    );
  } finally {
    isSubmitting.value = false;
    pendingAction.value = null;
  }
};

const clearBootstrapSecret = () => {
  quickStart.api_key = "";
};

watch(
  () => editingData.value.id,
  () => clearBootstrapSecret(),
);
watch(
  () => authStore.lifecycle,
  (lifecycle) => {
    if (lifecycle === "anonymous") clearBootstrapSecret();
  },
);
onBeforeUnmount(clearBootstrapSecret);

</script>
