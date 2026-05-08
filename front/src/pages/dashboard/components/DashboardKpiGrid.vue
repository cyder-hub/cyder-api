<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { AlertCircle, Loader2 } from "lucide-vue-next";

import { Card, CardContent } from "@/components/ui/card";
import type { DashboardKpiCardItem } from "../types";

const props = defineProps<{
  loading: boolean;
  error: string | null;
  cards: DashboardKpiCardItem[];
}>();

const { t: $t } = useI18n();
</script>

<template>
  <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-6">
    <template v-if="props.loading">
      <Card
        v-for="placeholder in 6"
        :key="`kpi-loading-${placeholder}`"
        class="border border-gray-200 shadow-none"
      >
        <CardContent class="flex items-center justify-center px-4 py-10">
          <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
          <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
        </CardContent>
      </Card>
    </template>

    <Card
      v-else-if="props.error"
      class="border border-red-200 bg-red-50 shadow-none md:col-span-2 xl:col-span-6"
    >
      <CardContent class="flex flex-col items-center justify-center px-4 py-10 text-center">
        <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
        <p class="text-sm font-medium text-red-500">
          {{ $t("dashboard.errorLoading", { error: props.error }) }}
        </p>
      </CardContent>
    </Card>

    <template v-else>
      <Card
        v-for="card in props.cards"
        :key="card.key"
        class="border border-gray-200 shadow-none"
      >
        <CardContent class="px-4 py-4">
          <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ card.label }}
          </p>
          <p class="mt-2 text-2xl font-semibold tracking-tight text-gray-900">
            {{ card.value }}
          </p>
          <p class="mt-1 text-xs text-gray-500">
            {{ card.description }}
          </p>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
