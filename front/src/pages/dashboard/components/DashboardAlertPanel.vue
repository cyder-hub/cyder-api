<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { AlertCircle, Loader2 } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import type {
  DashboardAlertsSection,
  DashboardProviderAlertItem,
} from "@/services/types";
import type {
  DashboardFormatCostEntries,
  DashboardFormatCount,
  DashboardFormatDateTime,
  DashboardFormatLatency,
  DashboardFormatPercentage,
  DashboardRuntimeLevelClass,
  DashboardRuntimeLevelLabel,
} from "../types";

const props = defineProps<{
  loading: boolean;
  error: string | null;
  alertsSection: DashboardAlertsSection;
  unstableProviders: DashboardProviderAlertItem[];
  showCostHotspots: boolean;
  formatCount: DashboardFormatCount;
  formatPercentage: DashboardFormatPercentage;
  formatLatency: DashboardFormatLatency;
  formatDateTime: DashboardFormatDateTime;
  formatCostEntries: DashboardFormatCostEntries;
  runtimeLevelBadgeClass: DashboardRuntimeLevelClass;
  runtimeLevelLabel: DashboardRuntimeLevelLabel;
}>();

const emit = defineEmits<{
  viewRuntime: [];
  viewRecords: [];
  editProvider: [providerId: number];
  editModel: [modelId: number];
}>();

const { t: $t } = useI18n();
</script>

<template>
  <div v-if="props.loading" class="grid grid-cols-1 gap-4 xl:grid-cols-3">
    <Card
      v-for="placeholder in 3"
      :key="`alerts-loading-${placeholder}`"
      class="border border-gray-200 shadow-none"
    >
      <CardContent class="flex items-center justify-center px-4 py-16">
        <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
        <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
      </CardContent>
    </Card>
  </div>

  <Card v-else-if="props.error" class="border border-red-200 bg-red-50 shadow-none">
    <CardContent class="flex flex-col items-center justify-center px-4 py-16 text-center">
      <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
      <p class="text-sm font-medium text-red-500">
        {{ $t("dashboard.errorLoading", { error: props.error }) }}
      </p>
    </CardContent>
  </Card>

  <div v-else class="grid grid-cols-1 gap-4 xl:grid-cols-3">
    <Card class="border border-gray-200 shadow-none">
      <CardHeader class="px-4 pb-4 sm:px-6">
        <CardTitle class="text-base">
          {{ $t("dashboard.sections.alerts.title") }}
        </CardTitle>
      </CardHeader>
      <CardContent class="app-stack-sm px-4 sm:px-6">
        <div class="rounded-lg border border-gray-100">
          <div class="border-b border-gray-100 px-4 py-3">
            <p class="text-sm font-medium text-gray-900">
              {{ $t("dashboard.alertGroups.unstable") }}
            </p>
          </div>
          <ul v-if="props.unstableProviders.length" class="divide-y divide-gray-100">
            <li
              v-for="item in props.unstableProviders"
              :key="`unstable-${item.provider_id}-${item.runtime_level}`"
              class="px-4 py-3"
            >
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <p class="truncate text-sm font-medium text-gray-900">
                    {{ item.provider_name || item.provider_key }}
                  </p>
                  <p class="mt-1 font-mono text-xs text-gray-500">
                    {{ item.provider_key }}
                  </p>
                </div>
                <Badge :class="props.runtimeLevelBadgeClass(item.runtime_level)">
                  {{ props.runtimeLevelLabel(item.runtime_level) }}
                </Badge>
              </div>
              <div class="mt-3">
                <Button variant="outline" size="sm" @click="emit('viewRuntime')">
                  {{ $t("dashboard.actions.viewRuntime") }}
                </Button>
              </div>
            </li>
          </ul>
          <div v-else class="px-4 py-5 text-sm text-gray-500">
            {{ $t("dashboard.empty.noUnstableProviders") }}
          </div>
        </div>

        <div class="rounded-lg border border-gray-100">
          <div class="border-b border-gray-100 px-4 py-3">
            <p class="text-sm font-medium text-gray-900">
              {{ $t("dashboard.alertGroups.topErrors") }}
            </p>
          </div>
          <ul
            v-if="props.alertsSection.alerts.top_error_providers.length"
            class="divide-y divide-gray-100"
          >
            <li
              v-for="item in props.alertsSection.alerts.top_error_providers"
              :key="`error-${item.provider_id}`"
              class="px-4 py-3"
            >
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <p class="truncate text-sm font-medium text-gray-900">
                    {{ item.provider_name || item.provider_key }}
                  </p>
                  <p class="mt-1 text-xs text-gray-500">
                    {{ $t("dashboard.metrics.errors") }}
                    {{ props.formatCount(item.error_count) }}
                    · {{ $t("dashboard.metrics.successRate") }}
                    {{ props.formatPercentage(item.success_rate) }}
                  </p>
                  <p class="mt-1 text-xs text-gray-400">
                    {{ $t("dashboard.metrics.lastError") }}
                    {{ props.formatDateTime(item.last_error_at) }}
                  </p>
                </div>
                <Button variant="outline" size="sm" @click="emit('viewRecords')">
                  {{ $t("dashboard.actions.viewRecords") }}
                </Button>
              </div>
            </li>
          </ul>
          <div v-else class="px-4 py-5 text-sm text-gray-500">
            {{ $t("dashboard.empty.noErrors") }}
          </div>
        </div>

        <div class="rounded-lg border border-gray-100">
          <div class="border-b border-gray-100 px-4 py-3">
            <p class="text-sm font-medium text-gray-900">
              {{ $t("dashboard.alertGroups.costHotspots") }}
            </p>
          </div>
          <div v-if="props.showCostHotspots" class="divide-y divide-gray-100">
            <div class="px-4 py-3">
              <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("dashboard.alertGroups.costProviders") }}
              </p>
              <ul
                v-if="props.alertsSection.alerts.top_cost_providers.length"
                class="mt-3 divide-y divide-gray-100"
              >
                <li
                  v-for="item in props.alertsSection.alerts.top_cost_providers"
                  :key="`cost-provider-${item.provider_id}`"
                  class="py-3 first:pt-0 last:pb-0"
                >
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-sm font-medium text-gray-900">
                        {{ item.provider_name || item.provider_key }}
                      </p>
                      <p class="mt-1 font-mono text-xs text-gray-500">
                        {{ item.provider_key }}
                      </p>
                      <p class="mt-1 text-xs text-gray-500">
                        {{ $t("dashboard.metrics.requests") }}
                        {{ props.formatCount(item.request_count) }}
                        · {{ $t("dashboard.metrics.successRate") }}
                        {{ props.formatPercentage(item.success_rate) }}
                      </p>
                    </div>
                    <div class="text-right">
                      <div
                        v-for="cost in props.formatCostEntries(item.total_cost)"
                        :key="cost"
                        class="text-xs font-medium text-gray-900"
                      >
                        {{ cost }}
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        class="mt-3"
                        @click="emit('editProvider', item.provider_id)"
                      >
                        {{ $t("common.edit") }}
                      </Button>
                    </div>
                  </div>
                </li>
              </ul>
              <p v-else class="mt-3 text-sm text-gray-500">
                {{ $t("dashboard.empty.noCostProviders") }}
              </p>
            </div>

            <div class="px-4 py-3">
              <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("dashboard.alertGroups.costModels") }}
              </p>
              <ul
                v-if="props.alertsSection.alerts.top_cost_models.length"
                class="mt-3 divide-y divide-gray-100"
              >
                <li
                  v-for="item in props.alertsSection.alerts.top_cost_models"
                  :key="`cost-model-${item.provider_id}-${item.model_id}`"
                  class="py-3 first:pt-0 last:pb-0"
                >
                  <div class="flex items-start justify-between gap-3">
                    <div class="min-w-0">
                      <p class="truncate text-sm font-medium text-gray-900">
                        {{ item.model_name || item.real_model_name || $t("common.notAvailable") }}
                      </p>
                      <p class="mt-1 font-mono text-xs text-gray-500">
                        {{ item.provider_key }} / {{ item.real_model_name || item.model_name }}
                      </p>
                      <p class="mt-1 text-xs text-gray-500">
                        {{ $t("dashboard.metrics.requests") }}
                        {{ props.formatCount(item.request_count) }}
                        · {{ $t("dashboard.metrics.totalTokens") }}
                        {{ props.formatCount(item.total_tokens) }}
                      </p>
                    </div>
                    <div class="text-right">
                      <div
                        v-for="cost in props.formatCostEntries(item.total_cost)"
                        :key="cost"
                        class="text-xs font-medium text-gray-900"
                      >
                        {{ cost }}
                      </div>
                      <Button
                        variant="outline"
                        size="sm"
                        class="mt-3"
                        @click="emit('editModel', item.model_id)"
                      >
                        {{ $t("common.edit") }}
                      </Button>
                    </div>
                  </div>
                </li>
              </ul>
              <p v-else class="mt-3 text-sm text-gray-500">
                {{ $t("dashboard.empty.noCostModels") }}
              </p>
            </div>
          </div>
          <div v-else class="px-4 py-5 text-sm text-gray-500">
            {{ $t("dashboard.empty.noCostHotspots") }}
          </div>
        </div>
      </CardContent>
    </Card>

    <Card class="border border-gray-200 shadow-none">
      <CardHeader class="px-4 pb-4 sm:px-6">
        <CardTitle class="text-base">
          {{ $t("dashboard.sections.topProviders.title") }}
        </CardTitle>
      </CardHeader>
      <CardContent class="px-4 sm:px-6">
        <ul v-if="props.alertsSection.top_providers.length" class="divide-y divide-gray-100">
          <li
            v-for="item in props.alertsSection.top_providers"
            :key="item.provider_id"
            class="py-3 first:pt-0 last:pb-0"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="truncate text-sm font-medium text-gray-900">
                  {{ item.provider_name || item.provider_key }}
                </p>
                <p class="mt-1 font-mono text-xs text-gray-500">
                  {{ item.provider_key }}
                </p>
                <p class="mt-1 text-xs text-gray-500">
                  {{ $t("dashboard.metrics.requests") }}
                  {{ props.formatCount(item.request_count) }}
                  · {{ $t("dashboard.metrics.successRate") }}
                  {{ props.formatPercentage(item.success_rate) }}
                </p>
              </div>
              <div class="text-right">
                <div
                  v-for="cost in props.formatCostEntries(item.total_cost)"
                  :key="cost"
                  class="text-xs font-medium text-gray-900"
                >
                  {{ cost }}
                </div>
                <div class="mt-1 text-xs text-gray-400">
                  {{ props.formatLatency(item.avg_total_latency_ms) }}
                </div>
              </div>
            </div>
          </li>
        </ul>
        <div v-else class="py-12 text-center text-sm text-gray-500">
          {{ $t("dashboard.empty.noTopProviders") }}
        </div>
      </CardContent>
    </Card>

    <Card class="border border-gray-200 shadow-none">
      <CardHeader class="px-4 pb-4 sm:px-6">
        <CardTitle class="text-base">
          {{ $t("dashboard.sections.topModels.title") }}
        </CardTitle>
      </CardHeader>
      <CardContent class="px-4 sm:px-6">
        <ul v-if="props.alertsSection.top_models.length" class="divide-y divide-gray-100">
          <li
            v-for="item in props.alertsSection.top_models"
            :key="`${item.provider_id}-${item.model_id}`"
            class="py-3 first:pt-0 last:pb-0"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <p class="truncate text-sm font-medium text-gray-900">
                  {{ item.model_name || item.real_model_name || $t("common.notAvailable") }}
                </p>
                <p class="mt-1 font-mono text-xs text-gray-500">
                  {{ item.provider_key }} / {{ item.real_model_name || item.model_name }}
                </p>
                <p class="mt-1 text-xs text-gray-500">
                  {{ $t("dashboard.metrics.requests") }}
                  {{ props.formatCount(item.request_count) }}
                  · {{ $t("dashboard.metrics.totalTokens") }}
                  {{ props.formatCount(item.total_tokens) }}
                </p>
              </div>
              <div class="text-right">
                <div
                  v-for="cost in props.formatCostEntries(item.total_cost)"
                  :key="cost"
                  class="text-xs font-medium text-gray-900"
                >
                  {{ cost }}
                </div>
              </div>
            </div>
          </li>
        </ul>
        <div v-else class="py-12 text-center text-sm text-gray-500">
          {{ $t("dashboard.empty.noTopModels") }}
        </div>
      </CardContent>
    </Card>
  </div>
</template>
