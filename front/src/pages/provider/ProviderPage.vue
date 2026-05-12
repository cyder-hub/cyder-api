<script setup lang="ts">
import { useRouter } from "vue-router";
import { Activity, Inbox, Layers3, Loader2, Plus, RefreshCcw } from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import StatsStrip from "@/components/StatsStrip.vue";
import { Button } from "@/components/ui/button";
import ProviderCards from "./components/ProviderCards.vue";
import ProviderTable from "./components/ProviderTable.vue";
import { useProviderList } from "./composables/useProviderList";
import type { ProviderSummaryItem } from "@/services/types";

const router = useRouter();

const {
  providers,
  isLoading,
  error,
  providerRuntimeLevelMap,
  summaryCards,
  runtimeLevelLabel,
  runtimeBadgeClass,
  providerStateLabel,
  providerStateClass,
  loadData,
  deleteProvider,
} = useProviderList();

const openProviderEdit = (provider: ProviderSummaryItem) => {
  void router.push(`/provider/edit/${provider.id}`);
};

const openProviderRuntime = (provider: ProviderSummaryItem) => {
  void router.push({
    path: "/provider/runtime",
    query: { search: provider.provider_key },
  });
};
</script>

<template>
  <CrudPageLayout
    :title="$t('providerPage.title')"
    :loading="isLoading"
    :error="error"
    :empty="!providers.length"
    content-class="space-y-4"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" @click="loadData">
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

    <StatsStrip :items="summaryCards" grid-class="grid-cols-2 sm:grid-cols-4" />

    <ProviderCards
      :providers="providers"
      :runtime-levels="providerRuntimeLevelMap"
      :provider-state-label="providerStateLabel"
      :provider-state-class="providerStateClass"
      :runtime-level-label="runtimeLevelLabel"
      :runtime-badge-class="runtimeBadgeClass"
      @edit="openProviderEdit"
      @runtime="openProviderRuntime"
      @delete="deleteProvider"
    />

    <ProviderTable
      :providers="providers"
      :runtime-levels="providerRuntimeLevelMap"
      :provider-state-label="providerStateLabel"
      :provider-state-class="providerStateClass"
      :runtime-level-label="runtimeLevelLabel"
      :runtime-badge-class="runtimeBadgeClass"
      @edit="openProviderEdit"
      @runtime="openProviderRuntime"
      @delete="deleteProvider"
    />
  </CrudPageLayout>
</template>
