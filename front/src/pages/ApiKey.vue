<template>
  <CrudPageLayout
    :title="$t('apiKeyPage.title')"
    :description="$t('apiKeyPage.description')"
    :loading="loading"
    :error="error"
    :empty="!apiKeyStore.apiKeys.length"
    header-class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between"
    page-class="flex flex-col xl:h-full xl:min-h-0 xl:overflow-hidden"
    shell-class="flex flex-col xl:min-h-0 xl:flex-1"
    content-class="flex flex-col gap-4 sm:gap-5 xl:min-h-0 xl:flex-1"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" @click="handleRefresh">
        <RefreshCcw class="mr-1.5 h-4 w-4" />
        {{ $t("common.refresh") }}
      </Button>
      <Button variant="outline" class="w-full sm:w-auto" @click="handleStartEditing()">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("apiKeyPage.addApiKey") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex flex-col items-center justify-center py-20">
        <Loader2 class="mb-2 h-5 w-5 animate-spin text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ $t("apiKeyPage.loading") }}
        </span>
      </div>
    </template>

    <template #error="{ error: pageError }">
      <div class="rounded-lg border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ pageError }}
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20">
        <KeyRound class="mb-4 h-10 w-10 stroke-1 text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ $t("apiKeyPage.noData") }}
        </span>
      </div>
    </template>

    <div
      class="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-gray-200 bg-gray-100 sm:grid-cols-3 xl:grid-cols-5"
    >
      <div
        v-for="card in summaryCards"
        :key="card.key"
        class="bg-white px-4 py-3"
      >
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ card.label }}
        </p>
        <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
          {{ card.value }}
        </p>
      </div>
    </div>

    <button
      type="button"
      class="flex w-full items-start justify-between gap-3 rounded-xl border border-gray-200 bg-white px-4 py-3 text-left xl:hidden"
      @click="showMobileKeyPicker = true"
    >
      <div class="min-w-0 flex-1">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ $t("apiKeyPage.sections.listTitle") }}
        </p>
        <h2 class="mt-1 truncate text-sm font-semibold text-gray-900">
          {{ selectedListKey?.name || $t("apiKeyPage.sections.detailTitle") }}
        </h2>
        <p class="mt-1 line-clamp-1 text-xs text-gray-500">
          {{ selectedListKey?.description || $t("common.notAvailable") }}
        </p>
      </div>
      <div class="flex shrink-0 items-center gap-2 pt-0.5">
        <Badge
          v-if="selectedListKey"
          :class="statusBadgeClass(selectedListKey)"
          class="text-[11px]"
        >
          {{ lifecycleLabel(selectedListKey) }}
        </Badge>
        <ChevronDown class="h-4 w-4 text-gray-400" />
      </div>
    </button>

    <div class="grid grid-cols-1 gap-4 xl:min-h-0 xl:flex-1 xl:grid-cols-12">
      <div class="hidden xl:col-span-3 xl:block xl:min-h-0">
        <div class="rounded-xl border border-gray-200 bg-white xl:flex xl:h-full xl:min-h-0 xl:flex-col">
          <div class="border-b border-gray-100 px-4 py-3 sm:px-5">
            <h2 class="text-base font-semibold text-gray-900">
              {{ $t("apiKeyPage.sections.listTitle") }}
            </h2>
          </div>

          <div class="divide-y divide-gray-100 xl:min-h-0 xl:flex-1 xl:overflow-y-auto">
            <button
              v-for="key in enrichedApiKeys"
              :key="key.id"
              type="button"
              class="w-full px-4 py-3 text-left transition-colors sm:px-5"
              :class="
                selectedKeyId === key.id
                  ? 'bg-gray-50'
                  : 'bg-white hover:bg-gray-50/70'
              "
              @click="handleSelectKey(key.id)"
            >
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <h3 class="truncate text-sm font-semibold text-gray-900">
                    {{ key.name }}
                  </h3>
                  <p class="mt-1 line-clamp-1 text-xs text-gray-500">
                    {{ key.description || $t("common.notAvailable") }}
                  </p>
                </div>
                <Badge :class="statusBadgeClass(key)" class="shrink-0 text-[11px]">
                  {{ lifecycleLabel(key) }}
                </Badge>
              </div>
            </button>
          </div>
        </div>
      </div>

      <div class="xl:col-span-9 xl:min-h-0">
        <div class="rounded-xl border border-gray-200 bg-white xl:flex xl:h-full xl:min-h-0 xl:flex-col">
          <div class="border-b border-gray-100 px-4 py-4 sm:px-5">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0">
                <h2 class="text-base font-semibold text-gray-900">
                  {{
                    selectedDetail
                      ? selectedDetail.name
                      : $t("apiKeyPage.sections.detailTitle")
                  }}
                </h2>
                <p class="mt-1 text-sm text-gray-500">
                  {{
                    selectedDetail
                      ? maskedKey(selectedDetail)
                      : $t("apiKeyPage.sections.detailDescription")
                  }}
                </p>
              </div>

              <div
                v-if="selectedDetail"
                class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row"
              >
                <Button
                  variant="outline"
                  class="w-full sm:w-auto"
                  @click="handleRevealKey(selectedDetail.id)"
                >
                  <Eye class="mr-1.5 h-4 w-4" />
                  {{ $t("apiKeyPage.actions.reveal") }}
                </Button>
                <Button
                  variant="outline"
                  class="w-full sm:w-auto"
                  @click="handleRotateKey(selectedDetail.id)"
                >
                  <RotateCcw class="mr-1.5 h-4 w-4" />
                  {{ $t("apiKeyPage.actions.rotate") }}
                </Button>
                <Button
                  variant="outline"
                  class="w-full sm:w-auto"
                  @click="handleStartEditing(selectedDetail.id)"
                >
                  <Pencil class="mr-1.5 h-4 w-4" />
                  {{ $t("common.edit") }}
                </Button>
                <Button
                  variant="destructive"
                  class="w-full sm:w-auto"
                  @click="handleDeleteKey(selectedDetail.id)"
                >
                  <Trash2 class="mr-1.5 h-4 w-4" />
                  {{ $t("common.delete") }}
                </Button>
              </div>
            </div>
          </div>

          <div
            v-if="detailLoading"
            class="flex items-center justify-center px-4 py-16 xl:flex-1"
          >
            <Loader2 class="mr-2 h-5 w-5 animate-spin text-gray-400" />
            <span class="text-sm text-gray-500">
              {{ $t("apiKeyPage.loadingDetail") }}
            </span>
          </div>

          <div
            v-else-if="!selectedDetail"
            class="px-4 py-16 text-center text-sm text-gray-500 xl:flex-1"
          >
            {{ $t("apiKeyPage.noSelection") }}
          </div>

          <div
            v-else
            class="space-y-6 px-4 py-4 sm:px-5 xl:min-h-0 xl:flex-1 xl:overflow-y-auto"
          >
            <div
              v-if="secretReveal && secretReveal.id === selectedDetail.id"
              class="rounded-lg border border-gray-200 bg-gray-50 px-4 py-4"
            >
              <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div>
                  <h3 class="text-sm font-semibold text-gray-900">
                    {{ $t("apiKeyPage.secret.title") }}
                  </h3>
                  <p class="mt-1 text-sm text-gray-500">
                    {{ $t("apiKeyPage.secret.description") }}
                  </p>
                </div>
                <div class="flex gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    class="text-xs"
                    @click="copySecret(secretReveal.api_key)"
                  >
                    <Copy class="mr-1 h-3.5 w-3.5" />
                    {{ $t("apiKeyPage.actions.copySecret") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="text-xs text-gray-500"
                    @click="secretReveal = null"
                  >
                    {{ $t("common.close") }}
                  </Button>
                </div>
              </div>
              <textarea
                readonly
                rows="3"
                class="mt-3 flex w-full rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none"
                :value="secretReveal.api_key"
              />
            </div>

            <section class="space-y-3">
              <div class="flex items-center gap-2">
                <Shield class="h-4 w-4 text-gray-400" />
                <h3 class="text-base font-semibold text-gray-900">
                  {{ $t("apiKeyPage.sections.identityTitle") }}
                </h3>
              </div>
              <dl class="grid grid-cols-1 gap-3 sm:grid-cols-2">
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.table.status") }}
                  </dt>
                  <dd class="mt-1 text-sm font-medium text-gray-900">
                    {{ lifecycleLabel(selectedDetail) }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.table.defaultAction") }}
                  </dt>
                  <dd class="mt-1 text-sm font-medium text-gray-900">
                    {{ actionLabel(selectedDetail.default_action) }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.table.createdAt") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ formatTimestamp(selectedDetail.created_at) || $t("common.notAvailable") }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.table.updatedAt") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ formatTimestamp(selectedDetail.updated_at) || $t("common.notAvailable") }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3 sm:col-span-2">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.table.description") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ selectedDetail.description || $t("common.notAvailable") }}
                  </dd>
                </div>
              </dl>
            </section>

            <section class="space-y-3 border-t border-gray-100 pt-6">
              <div class="flex items-center gap-2">
                <Gauge class="h-4 w-4 text-gray-400" />
                <h3 class="text-base font-semibold text-gray-900">
                  {{ $t("apiKeyPage.sections.governanceTitle") }}
                </h3>
              </div>
              <dl class="grid grid-cols-1 gap-3 sm:grid-cols-2">
                <div
                  v-for="item in governanceItems(selectedDetail)"
                  :key="item.key"
                  class="rounded-lg border border-gray-200 px-3 py-3"
                >
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ item.label }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ item.value }}
                  </dd>
                </div>
              </dl>
            </section>

            <section class="space-y-3 border-t border-gray-100 pt-6">
              <div class="flex items-center gap-2">
                <Shield class="h-4 w-4 text-gray-400" />
                <h3 class="text-base font-semibold text-gray-900">
                  {{ $t("apiKeyPage.sections.aclTitle") }}
                </h3>
              </div>

              <div
                v-if="!selectedDetail.acl_rules.length"
                class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500"
              >
                {{ $t("apiKeyPage.noRules") }}
              </div>

              <div v-else class="space-y-3">
                <div
                  v-for="rule in selectedDetail.acl_rules"
                  :key="rule.id"
                  class="rounded-lg border border-gray-200 px-4 py-3"
                >
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="text-[11px]">
                      {{ actionLabel(rule.effect) }}
                    </Badge>
                    <Badge variant="secondary" class="text-[11px]">
                      {{ scopeLabel(rule.scope) }}
                    </Badge>
                    <Badge
                      :class="
                        rule.is_enabled
                          ? 'border border-gray-200 bg-white text-gray-600'
                          : 'border border-gray-200 bg-gray-100 text-gray-400'
                      "
                      class="text-[11px]"
                    >
                      {{
                        rule.is_enabled
                          ? $t("apiKeyPage.rule.enabled")
                          : $t("apiKeyPage.rule.disabled")
                      }}
                    </Badge>
                  </div>
                  <p class="mt-2 text-sm text-gray-900">
                    {{ aclRuleTarget(rule) }}
                  </p>
                  <p class="mt-1 text-xs text-gray-500">
                    {{ $t("apiKeyPage.rule.priority", { value: rule.priority }) }}
                  </p>
                  <p v-if="rule.description" class="mt-2 text-sm text-gray-500">
                    {{ rule.description }}
                  </p>
                </div>
              </div>
            </section>

            <section class="space-y-3 border-t border-gray-100 pt-6">
              <div class="flex items-center gap-2">
                <Activity class="h-4 w-4 text-gray-400" />
                <h3 class="text-base font-semibold text-gray-900">
                  {{ $t("apiKeyPage.sections.runtimeTitle") }}
                </h3>
              </div>

              <dl class="grid grid-cols-2 gap-3">
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.currentConcurrency") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ selectedRuntimeView.current_concurrency }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.currentMinuteRequests") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ selectedRuntimeView.current_minute_request_count }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.dailyRequests") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ selectedRuntimeView.daily_request_count }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.dailyTokens") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ selectedRuntimeView.daily_token_count }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3 sm:col-span-2">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.monthlyTokens") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ selectedRuntimeView.monthly_token_count }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.dailyBudgetUsage") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ billedAmountLabel(selectedRuntimeView.daily_billed_amounts) }}
                  </dd>
                </div>
                <div class="rounded-lg border border-gray-200 px-3 py-3">
                  <dt class="text-xs uppercase tracking-wide text-gray-500">
                    {{ $t("apiKeyPage.runtime.monthlyBudgetUsage") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{ billedAmountLabel(selectedRuntimeView.monthly_billed_amounts) }}
                  </dd>
                </div>
              </dl>
            </section>
          </div>
        </div>
      </div>
    </div>

    <template #modals>
      <Drawer v-model:open="showMobileKeyPicker">
        <DrawerContent class="gap-0 border border-gray-200 bg-white p-0 xl:hidden">
          <DrawerHeader class="border-b border-gray-100 pb-3">
            <DrawerTitle class="text-base text-gray-900">
              {{ $t("apiKeyPage.sections.listTitle") }}
            </DrawerTitle>
            <DrawerDescription class="text-sm text-gray-500">
              {{ selectedListKey?.name || $t("apiKeyPage.sections.detailTitle") }}
            </DrawerDescription>
          </DrawerHeader>

          <div class="min-h-0 flex-1 overflow-y-auto">
            <button
              v-for="key in enrichedApiKeys"
              :key="key.id"
              type="button"
              class="w-full border-b border-gray-100 px-4 py-3 text-left transition-colors last:border-b-0"
              :class="
                selectedKeyId === key.id
                  ? 'bg-gray-50'
                  : 'bg-white hover:bg-gray-50/70'
              "
              @click="handleSelectKey(key.id)"
            >
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0 flex-1">
                  <h3 class="truncate text-sm font-semibold text-gray-900">
                    {{ key.name }}
                  </h3>
                  <p class="mt-1 line-clamp-1 text-xs text-gray-500">
                    {{ key.description || $t("common.notAvailable") }}
                  </p>
                </div>
                <Badge :class="statusBadgeClass(key)" class="shrink-0 text-[11px]">
                  {{ lifecycleLabel(key) }}
                </Badge>
              </div>
            </button>
          </div>
        </DrawerContent>
      </Drawer>

      <ApiKeyEditModal
        v-model:isOpen="showEditModal"
        :initial-data="editingDetail"
        :providers="providerStore.providers"
        @save-success="handleSaveSuccess"
      />
    </template>
  </CrudPageLayout>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  Activity,
  ChevronDown,
  Copy,
  Eye,
  Gauge,
  KeyRound,
  Loader2,
  Pencil,
  Plus,
  RefreshCcw,
  RotateCcw,
  Shield,
  Trash2,
} from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import ApiKeyEditModal from "@/components/ApiKeyEditModal.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import { Api } from "@/services/request";
import { normalizeError } from "@/lib/error";
import { formatPriceInputFromNanos, formatTimestamp } from "@/lib/utils";
import { copyText } from "@/lib/clipboard";
import { toastController } from "@/lib/toastController";
import { confirm } from "@/lib/confirmController";
import { useApiKeyStore } from "@/store/apiKeyStore";
import { useProviderStore } from "@/store/providerStore";
import type {
  ApiKeyAclRule,
  ApiKeyAction,
  ApiKeyAclRuleScope,
  ApiKeyDetail,
  ApiKeyItem,
  ApiKeyReveal,
  ApiKeyRuntimeBilledAmount,
  ApiKeyRuntimeSnapshot,
} from "@/store/types";

const { t: $t } = useI18n();

const apiKeyStore = useApiKeyStore();
const providerStore = useProviderStore();

const loading = ref(true);
const error = ref<string | null>(null);
const detailLoading = ref(false);
const selectedKeyId = ref<number | null>(null);
const selectedDetail = ref<ApiKeyDetail | null>(null);
const selectedRuntime = ref<ApiKeyRuntimeSnapshot | null>(null);
const showEditModal = ref(false);
const showMobileKeyPicker = ref(false);
const editingDetail = ref<ApiKeyDetail | null>(null);
const secretReveal = ref<ApiKeyReveal | null>(null);

function emptyRuntimeSnapshot(apiKeyId: number): ApiKeyRuntimeSnapshot {
  return {
    api_key_id: apiKeyId,
    current_concurrency: 0,
    current_minute_bucket: null,
    current_minute_request_count: 0,
    day_bucket: null,
    daily_request_count: 0,
    daily_token_count: 0,
    month_bucket: null,
    monthly_token_count: 0,
    daily_billed_amounts: [],
    monthly_billed_amounts: [],
  };
}

const runtimeById = computed(() => {
  const map = new Map<number, ApiKeyRuntimeSnapshot>();
  for (const snapshot of apiKeyStore.runtimeSnapshots) {
    map.set(snapshot.api_key_id, snapshot);
  }
  return map;
});

const providerNameById = computed(() => {
  const map = new Map<number, string>();
  for (const item of providerStore.providers) {
    map.set(item.provider.id, `${item.provider.name} (${item.provider.provider_key})`);
  }
  return map;
});

const modelNameById = computed(() => {
  const map = new Map<number, string>();
  for (const item of providerStore.providers) {
    for (const modelItem of item.models) {
      map.set(
        modelItem.model.id,
        `${item.provider.provider_key} / ${modelItem.model.model_name}`,
      );
    }
  }
  return map;
});

const enrichedApiKeys = computed(() => apiKeyStore.apiKeys);

const selectedListKey = computed(
  () => enrichedApiKeys.value.find((key) => key.id === selectedKeyId.value) ?? null,
);

const summaryCards = computed(() => {
  const total = apiKeyStore.apiKeys.length;
  const enabled = apiKeyStore.apiKeys.filter((key) => key.is_enabled).length;
  const governed = apiKeyStore.apiKeys.filter((key) =>
    [
      key.rate_limit_rpm,
      key.max_concurrent_requests,
      key.quota_daily_requests,
      key.quota_daily_tokens,
      key.quota_monthly_tokens,
      key.budget_daily_nanos,
      key.budget_monthly_nanos,
    ].some((value) => value !== null),
  ).length;
  const expiringSoon = apiKeyStore.apiKeys.filter((key) => isExpiringSoon(key.expires_at)).length;
  const currentConcurrency = apiKeyStore.runtimeSnapshots.reduce(
    (sum, item) => sum + item.current_concurrency,
    0,
  );

  return [
    { key: "total", label: $t("apiKeyPage.summary.total"), value: total },
    { key: "enabled", label: $t("apiKeyPage.summary.enabled"), value: enabled },
    { key: "governed", label: $t("apiKeyPage.summary.governed"), value: governed },
    {
      key: "concurrency",
      label: $t("apiKeyPage.summary.activeConcurrency"),
      value: currentConcurrency,
    },
    {
      key: "expiring",
      label: $t("apiKeyPage.summary.expiringSoon"),
      value: expiringSoon,
    },
  ];
});

const selectedRuntimeView = computed(() => {
  if (selectedRuntime.value) {
    return selectedRuntime.value;
  }
  if (selectedKeyId.value != null) {
    return runtimeById.value.get(selectedKeyId.value) ?? emptyRuntimeSnapshot(selectedKeyId.value);
  }
  return emptyRuntimeSnapshot(0);
});

function maskedKey(key: Pick<ApiKeyItem, "key_prefix" | "key_last4">) {
  return `${key.key_prefix}...${key.key_last4}`;
}

function actionLabel(action: ApiKeyAction) {
  return $t(`apiKeyEditModal.action.${action}`);
}

function scopeLabel(scope: ApiKeyAclRuleScope) {
  return $t(`apiKeyEditModal.scope.${scope}`);
}

function lifecycleLabel(key: Pick<ApiKeyItem, "is_enabled" | "expires_at">) {
  if (!key.is_enabled) {
    return $t("apiKeyPage.status.disabled");
  }
  if (key.expires_at && key.expires_at <= Date.now()) {
    return $t("apiKeyPage.status.expired");
  }
  if (isExpiringSoon(key.expires_at)) {
    return $t("apiKeyPage.status.expiringSoon");
  }
  return $t("apiKeyPage.status.active");
}

function statusBadgeClass(key: Pick<ApiKeyItem, "is_enabled" | "expires_at">) {
  if (!key.is_enabled || (key.expires_at && key.expires_at <= Date.now())) {
    return "border border-gray-200 bg-gray-100 text-gray-500";
  }
  if (isExpiringSoon(key.expires_at)) {
    return "border border-gray-200 bg-white text-gray-700";
  }
  return "border border-gray-900 bg-gray-900 text-white";
}

function isExpiringSoon(expiresAt: number | null) {
  if (!expiresAt || expiresAt <= Date.now()) {
    return false;
  }
  return expiresAt - Date.now() <= 7 * 24 * 60 * 60 * 1000;
}

function formatExpiry(expiresAt: number | null) {
  return expiresAt ? formatTimestamp(expiresAt) : $t("apiKeyPage.neverExpires");
}

function limitLabel(value: number | null) {
  return value == null ? $t("apiKeyPage.unlimited") : String(value);
}

function formatBudgetAmount(nanos: number | null | undefined, currency?: string | null) {
  if (nanos === null || nanos === undefined) {
    return $t("common.notAvailable");
  }

  const amount = formatPriceInputFromNanos(nanos, currency);
  const normalizedCurrency = currency?.toUpperCase();
  if (normalizedCurrency === "CNY") {
    return `${amount} ${$t("apiKeyPage.currencyUnit.cny")}`;
  }
  if (normalizedCurrency === "USD") {
    return `${amount} ${$t("apiKeyPage.currencyUnit.usd")}`;
  }

  return normalizedCurrency ? `${amount} ${normalizedCurrency}` : amount;
}

function billedAmountLabel(items: ApiKeyRuntimeBilledAmount[]) {
  if (!items.length) {
    return $t("common.notAvailable");
  }
  return items.map((item) => formatBudgetAmount(item.amount_nanos, item.currency)).join(" / ");
}

function governanceItems(detail: ApiKeyDetail) {
  return [
    {
      key: "expires_at",
      label: $t("apiKeyPage.table.expiresAt"),
      value: formatExpiry(detail.expires_at),
    },
    {
      key: "rate_limit_rpm",
      label: $t("apiKeyPage.table.rateLimitRpm"),
      value: limitLabel(detail.rate_limit_rpm),
    },
    {
      key: "max_concurrent_requests",
      label: $t("apiKeyPage.table.maxConcurrency"),
      value: limitLabel(detail.max_concurrent_requests),
    },
    {
      key: "quota_daily_requests",
      label: $t("apiKeyPage.table.quotaDailyRequests"),
      value: limitLabel(detail.quota_daily_requests),
    },
    {
      key: "quota_daily_tokens",
      label: $t("apiKeyPage.table.quotaDailyTokens"),
      value: limitLabel(detail.quota_daily_tokens),
    },
    {
      key: "quota_monthly_tokens",
      label: $t("apiKeyPage.table.quotaMonthlyTokens"),
      value: limitLabel(detail.quota_monthly_tokens),
    },
    {
      key: "budget_daily",
      label: $t("apiKeyPage.table.budgetDaily"),
      value:
        detail.budget_daily_nanos == null
          ? $t("apiKeyPage.unlimited")
          : formatBudgetAmount(detail.budget_daily_nanos, detail.budget_daily_currency),
    },
    {
      key: "budget_monthly",
      label: $t("apiKeyPage.table.budgetMonthly"),
      value:
        detail.budget_monthly_nanos == null
          ? $t("apiKeyPage.unlimited")
          : formatBudgetAmount(detail.budget_monthly_nanos, detail.budget_monthly_currency),
    },
  ];
}

function aclRuleTarget(rule: ApiKeyAclRule) {
  if (rule.scope === "PROVIDER") {
    return providerNameById.value.get(rule.provider_id ?? -1) ?? $t("common.notAvailable");
  }
  return modelNameById.value.get(rule.model_id ?? -1) ?? $t("common.notAvailable");
}

async function fetchData() {
  loading.value = true;
  error.value = null;
  try {
    const currentSelectedId = selectedKeyId.value;
    await Promise.all([
      apiKeyStore.fetchApiKeys(),
      apiKeyStore.fetchRuntimeSnapshots(),
      providerStore.fetchProviders(),
    ]);

    if (!apiKeyStore.apiKeys.length) {
      selectedKeyId.value = null;
      selectedDetail.value = null;
      selectedRuntime.value = null;
      secretReveal.value = null;
      return;
    }

    const nextSelectedId =
      currentSelectedId &&
      apiKeyStore.apiKeys.some((key) => key.id === currentSelectedId)
        ? currentSelectedId
        : apiKeyStore.apiKeys[0].id;

    selectedKeyId.value = nextSelectedId;
    await loadSelectedKey(nextSelectedId);
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    loading.value = false;
  }
}

async function loadSelectedKey(id: number | null) {
  if (id == null) {
    selectedDetail.value = null;
    selectedRuntime.value = null;
    return;
  }

  detailLoading.value = true;
  try {
    const [detail, runtime] = await Promise.all([
      Api.getApiKeyDetail(id),
      Api.getApiKeyRuntime(id),
    ]);
    selectedDetail.value = detail;
    selectedRuntime.value = runtime;
    if (secretReveal.value && secretReveal.value.id !== id) {
      secretReveal.value = null;
    }
  } catch (err: unknown) {
    toastController.error(
      $t("apiKeyPage.loadDetailFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  } finally {
    detailLoading.value = false;
  }
}

function handleSelectKey(id: number) {
  showMobileKeyPicker.value = false;
  if (selectedKeyId.value === id && selectedDetail.value) {
    return;
  }
  selectedKeyId.value = id;
  void loadSelectedKey(id);
}

async function handleRefresh() {
  await fetchData();
}

async function handleStartEditing(id?: number) {
  if (!id) {
    editingDetail.value = null;
    showEditModal.value = true;
    return;
  }

  try {
    editingDetail.value =
      selectedDetail.value?.id === id ? selectedDetail.value : await Api.getApiKeyDetail(id);
    showEditModal.value = true;
  } catch (err: unknown) {
    toastController.error(
      $t("apiKeyPage.loadEditFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
}

async function handleSaveSuccess(payload: { detail: ApiKeyDetail; reveal?: ApiKeyReveal }) {
  selectedKeyId.value = payload.detail.id;
  selectedDetail.value = payload.detail;
  editingDetail.value = null;
  showEditModal.value = false;
  if (payload.reveal) {
    secretReveal.value = payload.reveal;
  }
  await fetchData();
}

async function handleRevealKey(id: number) {
  try {
    secretReveal.value = await Api.revealApiKey(id);
    if (selectedKeyId.value !== id) {
      selectedKeyId.value = id;
      await loadSelectedKey(id);
    }
  } catch (err: unknown) {
    toastController.error(
      $t("apiKeyPage.revealFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
}

async function handleRotateKey(id: number) {
  const target = apiKeyStore.apiKeys.find((item) => item.id === id);
  if (
    !(await confirm(
      $t("apiKeyPage.confirmRotate", { name: target?.name ?? String(id) }),
    ))
  ) {
    return;
  }

  try {
    secretReveal.value = await Api.rotateApiKey(id);
    await fetchData();
  } catch (err: unknown) {
    toastController.error(
      $t("apiKeyPage.rotateFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
}

async function handleDeleteKey(id: number) {
  const target = apiKeyStore.apiKeys.find((item) => item.id === id);
  if (
    !(await confirm(
      $t("apiKeyPage.confirmDelete", { name: target?.name ?? String(id) }),
    ))
  ) {
    return;
  }

  try {
    await Api.deleteApiKey(id);
    if (selectedKeyId.value === id) {
      selectedKeyId.value = null;
      selectedDetail.value = null;
      selectedRuntime.value = null;
      secretReveal.value = null;
    }
    await fetchData();
  } catch (err: unknown) {
    toastController.error(
      $t("apiKeyPage.deleteFailed", {
        error: normalizeError(err, $t("common.unknownError")).message,
      }),
    );
  }
}

async function copySecret(secret: string) {
  const copied = await copyText(secret);
  if (!copied) {
    toastController.error($t("apiKeyPage.copyFailed"));
    return;
  }
  toastController.success($t("apiKeyPage.secret.copied"));
}

onMounted(() => {
  void fetchData();
});
</script>
