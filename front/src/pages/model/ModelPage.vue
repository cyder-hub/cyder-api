<script setup lang="ts">
import { useRouter } from "vue-router";
import { ArrowLeftRight, Loader2, RefreshCcw, Search } from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import ModelCards from "./components/ModelCards.vue";
import ModelTable from "./components/ModelTable.vue";
import { useModelList } from "./composables/useModelList";

const router = useRouter();

const {
  query,
  modelStore,
  modelPageState,
  summaryCards,
  capabilityItems,
  loadData,
} = useModelList();

const openModel = (id: number) => {
  void router.push(`/model/edit/${id}`);
};
</script>

<template>
  <CrudPageLayout
    :title="$t('modelPage.title')"
    :description="$t('modelPage.description')"
    :loading="modelStore.loading"
    :error="modelStore.error"
    :empty="modelPageState.isPageEmpty"
    content-class="space-y-4"
  >
    <template #actions>
      <Button variant="outline" class="w-full sm:w-auto" @click="loadData">
        <RefreshCcw class="mr-1.5 h-4 w-4" />
        {{ $t("common.refresh") }}
      </Button>
      <Button variant="outline" class="w-full sm:w-auto" @click="router.push('/provider')">
        <ArrowLeftRight class="mr-1.5 h-4 w-4" />
        {{ $t("modelPage.actions.backToProviders") }}
      </Button>
    </template>

    <template #loading>
      <div class="flex items-center justify-center py-16 text-gray-400">
        <Loader2 class="mr-2 h-5 w-5 animate-spin" />
        <span class="text-sm">{{ $t("modelPage.loading") }}</span>
      </div>
    </template>

    <template #error="{ error: pageError }">
      <div class="rounded-xl border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600">
        {{ pageError }}
      </div>
    </template>

    <template #empty>
      <div class="flex flex-col items-center justify-center py-20 text-gray-500">
        <ArrowLeftRight class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
        <p class="text-sm font-medium">{{ $t("modelPage.noData") }}</p>
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

    <div class="relative">
      <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
      <Input
        v-model="query"
        :placeholder="$t('modelPage.searchPlaceholder')"
        class="pl-9"
      />
    </div>

    <div
      v-if="modelPageState.isSearchEmpty"
      class="flex flex-col items-center justify-center rounded-xl border border-gray-200 bg-white py-16 text-center text-gray-500"
    >
      <Search class="mb-3 h-10 w-10 stroke-1 text-gray-400" />
      <p class="text-sm font-medium">{{ $t("modelPage.noSearchResults") }}</p>
    </div>

    <template v-else>
      <ModelCards
        :models="modelPageState.filteredItems"
        :capability-items="capabilityItems"
        @open="openModel"
      />
      <ModelTable
        :models="modelPageState.filteredItems"
        :capability-items="capabilityItems"
        @open="openModel"
      />
    </template>
  </CrudPageLayout>
</template>
