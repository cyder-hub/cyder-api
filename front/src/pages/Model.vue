<script setup lang="ts">
import { computed, onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import { useRouter } from "vue-router";
import {
  ArrowLeftRight,
  Loader2,
  Pencil,
  RefreshCcw,
  Search,
} from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { buildModelPageState } from "@/pages/modelViewModel";
import { useModelStore } from "@/store/modelStore";

const { t: $t } = useI18n();
const router = useRouter();
const modelStore = useModelStore();

const query = ref("");

const modelPageState = computed(() => buildModelPageState(modelStore.models, query.value));

const summaryCards = computed(() => {
  const total = modelStore.models.length;
  const enabled = modelStore.models.filter((item) => item.is_enabled).length;
  const providers = new Set(modelStore.models.map((item) => item.provider_id)).size;
  const mapped = modelStore.models.filter((item) => item.real_model_name).length;

  return [
    { key: "total", label: $t("modelPage.summary.total"), value: total },
    { key: "enabled", label: $t("modelPage.summary.enabled"), value: enabled },
    { key: "providers", label: $t("modelPage.summary.providers"), value: providers },
    { key: "mapped", label: $t("modelPage.summary.mapped"), value: mapped },
  ];
});

const loadData = async () => {
  try {
    await modelStore.fetchModels();
  } catch (err: unknown) {
    console.error("Failed to load model summaries:", err);
  }
};

const handleRefresh = async () => {
  await loadData();
};

const handleOpenModel = (id: number) => {
  router.push(`/model/edit/${id}`);
};

onMounted(() => {
  void loadData();
});
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
      <Button variant="outline" class="w-full sm:w-auto" @click="handleRefresh">
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
      <div class="grid grid-cols-1 gap-3 md:hidden">
        <MobileCrudCard
          v-for="model in modelPageState.filteredItems"
          :key="model.id"
          :title="model.model_name"
          :description="model.real_model_name || $t('modelPage.noMappedModel')"
        >
          <template #header>
            <Badge :variant="model.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
              {{ model.is_enabled ? $t("common.yes") : $t("common.no") }}
            </Badge>
          </template>

          <div class="grid grid-cols-1 gap-2 text-xs text-gray-500">
            <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("modelPage.table.provider") }}</span>
              <span class="max-w-[12rem] truncate font-mono text-gray-700">
                {{ model.provider_name }} / {{ model.provider_key }}
              </span>
            </div>
            <div class="flex items-center justify-between rounded-lg border border-gray-100 px-3 py-2.5">
              <span>{{ $t("modelPage.table.realModel") }}</span>
              <span class="max-w-[12rem] truncate font-mono text-gray-700">
                {{ model.real_model_name || $t("common.notAvailable") }}
              </span>
            </div>
          </div>

          <template #actions>
            <Button variant="ghost" size="sm" class="w-full justify-center" @click="handleOpenModel(model.id)">
              <Pencil class="mr-1.5 h-3.5 w-3.5" />
              {{ $t("common.edit") }}
            </Button>
          </template>
        </MobileCrudCard>
      </div>

      <div class="hidden overflow-hidden rounded-xl border border-gray-200 bg-white md:block">
        <Table>
          <TableHeader>
            <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("modelPage.table.provider") }}
              </TableHead>
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("modelPage.table.model") }}
              </TableHead>
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("modelPage.table.realModel") }}
              </TableHead>
              <TableHead class="text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("modelPage.table.enabled") }}
              </TableHead>
              <TableHead class="text-right text-xs font-medium uppercase tracking-wider text-gray-500">
                {{ $t("common.actions") }}
              </TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            <TableRow v-for="model in modelPageState.filteredItems" :key="model.id">
              <TableCell>
                <div class="min-w-0">
                  <div class="font-medium text-gray-900">{{ model.provider_name }}</div>
                  <div class="mt-0.5 font-mono text-xs text-gray-500">
                    {{ model.provider_key }}
                  </div>
                </div>
              </TableCell>
              <TableCell class="font-mono text-sm text-gray-800">
                {{ model.model_name }}
              </TableCell>
              <TableCell class="font-mono text-sm text-gray-700">
                {{ model.real_model_name || $t("common.notAvailable") }}
              </TableCell>
              <TableCell>
                <Badge :variant="model.is_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
                  {{ model.is_enabled ? $t("common.yes") : $t("common.no") }}
                </Badge>
              </TableCell>
              <TableCell class="text-right">
                <Button variant="ghost" size="sm" @click="handleOpenModel(model.id)">
                  <Pencil class="mr-1.5 h-3.5 w-3.5" />
                  {{ $t("common.edit") }}
                </Button>
              </TableCell>
            </TableRow>
          </TableBody>
        </Table>
      </div>
    </template>
  </CrudPageLayout>
</template>
