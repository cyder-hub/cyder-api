<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { AlertCircle, Loader2 } from "lucide-vue-next";

import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type {
  DashboardFormatDateTime,
  DashboardResourceItem,
  DashboardRuntimeBackendDisplayRow,
  DashboardRuntimeItem,
} from "../types";
import type { RuntimeStateBackendStatus } from "@/services/types";

const props = defineProps<{
  loading: boolean;
  error: string | null;
  resourceItems: DashboardResourceItem[];
  runtimeItems: DashboardRuntimeItem[];
  runtimeBackendStatus: RuntimeStateBackendStatus;
  runtimeBackendHeadline: string;
  runtimeBackendDetail: string;
  runtimeBackendRows: DashboardRuntimeBackendDisplayRow[];
  runtimeBackendBadgeLabel: string;
  runtimeBackendBadgeClass: string;
  runtimeBadgeClass: (key: string) => string;
  formatDateTime: DashboardFormatDateTime;
}>();

const emit = defineEmits<{
  viewRuntime: [];
  viewRecords: [];
}>();

const { t: $t } = useI18n();
</script>

<template>
  <div class="grid grid-cols-1 gap-4 xl:grid-cols-2">
    <section class="rounded-lg border border-gray-200 bg-white">
      <SectionHeader
        :title="$t('dashboard.sections.resources.title')"
        class="border-b border-gray-100 px-4 py-4 sm:px-6"
      />
      <div class="px-4 py-4 sm:px-6">
        <div
          v-if="props.loading"
          class="flex items-center justify-center py-12"
        >
          <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
          <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
        </div>
        <div
          v-else-if="props.error"
          class="flex flex-col items-center justify-center rounded-md bg-red-50 px-4 py-10 text-center"
        >
          <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
          <p class="text-sm font-medium text-red-500">
            {{ $t("dashboard.errorLoading", { error: props.error }) }}
          </p>
        </div>
        <ul v-else class="divide-y divide-gray-100">
          <li
            v-for="item in props.resourceItems"
            :key="item.key"
            class="flex items-start justify-between gap-4 py-3 first:pt-0 last:pb-0 sm:items-center"
          >
            <span class="text-sm text-gray-500">{{ item.label }}</span>
            <div class="text-right">
              <div class="font-mono text-base font-medium text-gray-900 sm:text-sm">
                {{ item.value }}
              </div>
              <div class="mt-0.5 text-xs text-gray-400">
                {{ item.description }}
              </div>
            </div>
          </li>
        </ul>
      </div>
    </section>

    <section class="rounded-lg border border-gray-200 bg-white">
      <SectionHeader
        :title="$t('dashboard.sections.runtime.title')"
        class="border-b border-gray-100 px-4 py-4 sm:px-6"
      />
      <div class="px-4 py-4 sm:px-6">
        <div
          v-if="props.loading"
          class="flex items-center justify-center py-12"
        >
          <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
          <span class="text-sm text-gray-500">{{ $t("common.loading") }}</span>
        </div>
        <div
          v-else-if="props.error"
          class="flex flex-col items-center justify-center rounded-md bg-red-50 px-4 py-10 text-center"
        >
          <AlertCircle class="mb-3 h-8 w-8 stroke-1 text-red-500" />
          <p class="text-sm font-medium text-red-500">
            {{ $t("dashboard.errorLoading", { error: props.error }) }}
          </p>
        </div>
        <template v-else>
          <div class="grid grid-cols-2 gap-px overflow-hidden rounded-lg border border-gray-200 bg-gray-100 sm:grid-cols-3">
            <div
              v-for="item in props.runtimeItems"
              :key="item.key"
              class="bg-white px-3 py-3"
            >
              <div class="flex items-center justify-between gap-2">
                <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ item.label }}
                </p>
                <Badge variant="outline" :class="props.runtimeBadgeClass(item.key)">
                  {{ item.value }}
                </Badge>
              </div>
              <p class="mt-2 text-xs text-gray-400">
                {{ item.description }}
              </p>
            </div>
          </div>

          <div class="mt-4 border-t border-gray-100 pt-4">
            <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0">
                <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ $t("dashboard.runtimeState.title") }}
                </p>
                <p class="mt-1 text-sm font-medium text-gray-900">
                  {{ props.runtimeBackendHeadline }}
                </p>
                <p class="mt-1 text-xs text-gray-500">
                  {{ props.runtimeBackendDetail }}
                </p>
                <dl class="mt-3 grid grid-cols-1 gap-x-4 gap-y-2 text-xs sm:grid-cols-2">
                  <div
                    v-for="row in props.runtimeBackendRows"
                    :key="`dashboard-backend-${row.key}`"
                    class="min-w-0"
                  >
                    <dt class="font-medium uppercase tracking-wide text-gray-400">
                      {{ row.label }}
                    </dt>
                    <dd class="mt-1 flex flex-wrap items-center gap-1.5 text-gray-600">
                      <span>{{ $t("dashboard.runtimeState.configured") }}</span>
                      <span class="font-mono font-medium text-gray-900">
                        {{ row.configured }}
                      </span>
                      <span class="text-gray-300">/</span>
                      <span>{{ $t("dashboard.runtimeState.effective") }}</span>
                      <span
                        class="font-mono font-medium"
                        :class="row.changed ? 'text-amber-700' : 'text-gray-900'"
                      >
                        {{ row.effective }}
                      </span>
                    </dd>
                  </div>
                </dl>
              </div>
              <Badge :class="props.runtimeBackendBadgeClass">
                {{ props.runtimeBackendBadgeLabel }}
              </Badge>
            </div>
            <p
              v-if="props.runtimeBackendStatus.last_error"
              class="mt-2 break-words text-xs text-red-600"
            >
              {{
                $t("dashboard.runtimeState.lastError", {
                  error: props.runtimeBackendStatus.last_error,
                })
              }}
            </p>
            <p class="mt-2 text-xs text-gray-400">
              {{
                $t("dashboard.runtimeState.lastChecked", {
                  time: props.formatDateTime(props.runtimeBackendStatus.last_checked_at),
                })
              }}
            </p>
          </div>

          <div class="mt-4 flex flex-col gap-2 border-t border-gray-100 pt-4 sm:flex-row">
            <Button variant="outline" class="w-full sm:w-auto" @click="emit('viewRuntime')">
              {{ $t("dashboard.actions.viewRuntime") }}
            </Button>
            <Button variant="outline" class="w-full sm:w-auto" @click="emit('viewRecords')">
              {{ $t("dashboard.actions.viewRecords") }}
            </Button>
          </div>
        </template>
      </div>
    </section>
  </div>
</template>
