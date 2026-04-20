<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import {
  Activity,
  Inbox,
  Loader2,
  Plus,
  RefreshCcw,
  Trash2,
  Server,
  Layers3,
} from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { confirm } from "@/lib/confirmController";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { Api } from "@/services/request";
import { useProviderStore } from "@/store/providerStore";
import type { ProviderRuntimeLevel, ProviderSummaryItem } from "@/store/types";

const { t: $t } = useI18n();
const router = useRouter();
const store = useProviderStore();

const isLoading = ref(true);
const error = ref<string | null>(null);
const providerRuntimeLevelMap = ref<Record<number, ProviderRuntimeLevel>>({});

const summaryCards = computed(() => {
  const total = store.providers.length;
  const enabled = store.providers.filter((item) => item.is_enabled).length;
  const disabled = total - enabled;
  const runtimeIssues = Object.values(providerRuntimeLevelMap.value).filter(
    (level) => level === "open" || level === "half_open" || level === "degraded",
  ).length;

  return [
    { key: "total", label: $t("providerPage.summary.total"), value: total },
    { key: "enabled", label: $t("providerPage.summary.enabled"), value: enabled },
    { key: "disabled", label: $t("providerPage.summary.disabled"), value: disabled },
    { key: "runtime", label: $t("providerPage.summary.runtimeIssues"), value: runtimeIssues },
  ];
});

const runtimeLevelLabel = (level: ProviderRuntimeLevel) =>
  $t(`providerRuntimePage.status.${level}`);

const runtimeBadgeClass = (level: ProviderRuntimeLevel) => {
  switch (level) {
    case "open":
      return "border-red-200 bg-red-50 text-red-700 hover:bg-red-50";
    case "half_open":
      return "border-amber-200 bg-amber-50 text-amber-700 hover:bg-amber-50";
    case "degraded":
      return "border-orange-200 bg-orange-50 text-orange-700 hover:bg-orange-50";
    case "healthy":
      return "border-emerald-200 bg-emerald-50 text-emerald-700 hover:bg-emerald-50";
    case "no_traffic":
      return "border-gray-200 bg-gray-100 text-gray-600 hover:bg-gray-100";
  }
};

const providerStateLabel = (provider: ProviderSummaryItem) =>
  provider.is_enabled ? $t("providerPage.state.enabled") : $t("providerPage.state.disabled");

const providerStateClass = (provider: ProviderSummaryItem) =>
  provider.is_enabled
    ? "border-emerald-200 bg-emerald-50 text-emerald-700"
    : "border-gray-200 bg-gray-100 text-gray-500";

const loadRuntimeLevels = async () => {
  try {
    const runtimeItems = await Api.getProviderRuntimeList({
      window: "1h",
      only_enabled: false,
    });
    providerRuntimeLevelMap.value = Object.fromEntries(
      runtimeItems.map((item) => [item.provider_id, item.runtime_level]),
    );
  } catch (err) {
    console.error("Failed to fetch provider runtime levels:", err);
    providerRuntimeLevelMap.value = {};
  }
};

const loadData = async () => {
  isLoading.value = true;
  error.value = null;
  try {
    await store.fetchProviders();
    await loadRuntimeLevels();
  } catch (err: unknown) {
    error.value = normalizeError(err, $t("common.unknownError")).message;
  } finally {
    isLoading.value = false;
  }
};

const handleRefresh = async () => {
  await loadData();
};

const handleDeleteProvider = async (provider: ProviderSummaryItem) => {
  await confirm($t("providerPage.confirmDelete", { name: provider.name }));
  try {
    await Api.deleteProvider(provider.id);
    toastController.success($t("providerPage.deleteSuccess"));
    await loadData();
  } catch (err: unknown) {
    console.error("Failed to delete provider:", err);
    const errorMessage = normalizeError(err, $t("common.unknownError")).message;
    toastController.error($t("providerPage.deleteFailed", { error: errorMessage }));
  }
};

onMounted(() => {
  void loadData();
});
</script>

<template>
  <CrudPageLayout
    :title="$t('providerPage.title')"
    :description="$t('providerPage.description')"
    :loading="isLoading"
    :error="error"
    :empty="!store.providers.length"
    content-class="space-y-4"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" @click="handleRefresh">
        <RefreshCcw class="mr-1.5 h-4 w-4" />
        {{ $t("common.refresh") }}
      </Button>
      <Button variant="outline" class="w-full sm:w-auto" @click="router.push('/model')">
        <Layers3 class="mr-1.5 h-4 w-4" />
        {{ $t("providerPage.viewModels") }}
      </Button>
      <Button variant="outline" class="w-full sm:w-auto" @click="router.push('/provider/runtime')">
        <Activity class="mr-1.5 h-4 w-4" />
        {{ $t("providerPage.viewRuntime") }}
      </Button>
      <Button variant="default" class="w-full sm:w-auto" @click="router.push('/provider/new')">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("providerPage.addProvider") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center py-16 text-gray-400">
        <Loader2 class="mr-2 h-5 w-5 animate-spin" />
        <span class="text-sm">{{ $t("providerPage.loading") }}</span>
      </div>
    </template>

    <template #error="{ error: pageError }">
      <div class="rounded-xl border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ pageError }}
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20 text-gray-500">
        <Inbox class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
        <p class="text-sm font-medium">{{ $t("providerPage.noData") }}</p>
      </div>
    </template>

    <div class="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-gray-200 bg-gray-100 sm:grid-cols-4">
      <div v-for="card in summaryCards" :key="card.key" class="bg-white px-4 py-3">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ card.label }}
        </p>
        <p class="mt-1 text-lg font-semibold tracking-tight text-gray-900">
          {{ card.value }}
        </p>
      </div>
    </div>

    <div class="grid grid-cols-1 gap-3 md:hidden">
      <MobileCrudCard
        v-for="provider in store.providers"
        :key="provider.id"
        :title="provider.name"
        :description="provider.provider_key"
      >
        <template #header>
          <div class="flex items-center gap-2">
            <Badge :class="providerStateClass(provider)" class="font-mono text-[11px]">
              {{ providerStateLabel(provider) }}
            </Badge>
            <Badge
              v-if="providerRuntimeLevelMap[provider.id]"
              :class="runtimeBadgeClass(providerRuntimeLevelMap[provider.id])"
              class="font-mono text-[11px]"
            >
              {{ runtimeLevelLabel(providerRuntimeLevelMap[provider.id]) }}
            </Badge>
          </div>
        </template>

        <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("providerPage.table.name") }}</span>
            <span class="max-w-[12rem] truncate font-mono text-gray-700">
              {{ provider.name }}
            </span>
          </div>
          <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
            <span>{{ $t("providerPage.table.key") }}</span>
            <span class="max-w-[12rem] truncate font-mono text-gray-700">
              {{ provider.provider_key }}
            </span>
          </div>
        </div>

        <template #actions>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center"
            @click="router.push(`/provider/edit/${provider.id}`)"
          >
            <Server class="mr-1.5 h-3.5 w-3.5" />
            {{ $t("common.edit") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-600 hover:text-gray-900"
            @click="router.push({ path: '/provider/runtime', query: { search: provider.provider_key } })"
          >
            <Activity class="mr-1.5 h-3.5 w-3.5" />
            {{ $t("providerPage.viewRuntime") }}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-400 hover:text-red-600"
            @click="handleDeleteProvider(provider)"
          >
            <Trash2 class="mr-1.5 h-3.5 w-3.5" />
            {{ $t("common.delete") }}
          </Button>
        </template>
      </MobileCrudCard>
    </div>

    <div class="hidden overflow-hidden rounded-xl border border-gray-200 bg-white md:block">
      <div
        class="grid grid-cols-[1.3fr_1fr_1fr_auto] items-center gap-4 border-b border-gray-200 bg-gray-50/80 px-4 py-3"
      >
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerPage.table.name") }}
        </span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerPage.table.key") }}
        </span>
        <span class="text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("providerPage.table.status") }}
        </span>
        <span class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
          {{ $t("common.actions") }}
        </span>
      </div>

      <div
        v-for="provider in store.providers"
        :key="provider.id"
        class="grid grid-cols-[1.3fr_1fr_1fr_auto] items-center gap-4 border-b border-gray-100 px-4 py-3 last:border-0 transition-colors hover:bg-gray-50/50"
      >
        <div>
          <div class="font-medium text-gray-900">{{ provider.name }}</div>
          <div class="mt-0.5 text-xs text-gray-500">
            {{ $t("providerPage.subtitle") }}
          </div>
        </div>
        <div class="font-mono text-sm text-gray-700">{{ provider.provider_key }}</div>
        <div class="flex flex-wrap items-center gap-2">
          <Badge :class="providerStateClass(provider)" class="font-mono text-[11px]">
            {{ providerStateLabel(provider) }}
          </Badge>
          <Badge
            v-if="providerRuntimeLevelMap[provider.id]"
            :class="runtimeBadgeClass(providerRuntimeLevelMap[provider.id])"
            class="font-mono text-[11px]"
          >
            {{ runtimeLevelLabel(providerRuntimeLevelMap[provider.id]) }}
          </Badge>
        </div>
        <div class="flex items-center justify-end gap-1">
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-600"
            @click="router.push(`/provider/edit/${provider.id}`)"
          >
            <Server class="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-600"
            @click="router.push({ path: '/provider/runtime', query: { search: provider.provider_key } })"
          >
            <Activity class="h-4 w-4" />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            class="h-8 px-2 text-gray-400 hover:text-red-600"
            @click="handleDeleteProvider(provider)"
          >
            <Trash2 class="h-4 w-4" />
          </Button>
        </div>
      </div>
    </div>
  </CrudPageLayout>
</template>
