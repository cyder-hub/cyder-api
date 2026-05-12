<script setup lang="ts">
import { AlertTriangle, FileText, HardDrive, Settings } from "lucide-vue-next";
import { useI18n } from "vue-i18n";

import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type {
  SystemConfigPersistenceHealthItem,
  SystemConfigPersistenceHealthReport,
  SystemConfigPersistenceHealthStatus,
  SystemConfigReportSummary,
} from "@/services/types";
import type { ConfigViewMode, SystemConfigSourceLayer } from "../types";
import {
  formatSystemConfigFileState,
  formatSystemConfigTimestamp,
  persistenceRowClass,
  persistenceStatusClass,
} from "../composables/useSystemConfigReport";

defineProps<{
  summary: SystemConfigReportSummary | null;
  isMultiInstance: boolean;
  governanceDisabled: boolean;
  sourceLayers: SystemConfigSourceLayer[];
  persistenceHealth: SystemConfigPersistenceHealthReport | null;
  persistenceHealthItems: SystemConfigPersistenceHealthItem[];
  persistenceIssueCount: number;
  configDocumentPath: string | undefined;
  configDocumentInvalidPaths: string[];
  configDocumentText: string;
}>();

const configViewMode = defineModel<ConfigViewMode>("configViewMode", {
  required: true,
});

const { t: $t } = useI18n();

function formatFileState(value: boolean | null | undefined): string {
  return formatSystemConfigFileState(value, {
    exists: $t("systemConfigPage.exists"),
    missing: $t("systemConfigPage.missing"),
  });
}

function formatHealthBoolean(value: boolean): string {
  return value
    ? $t("systemConfigPage.persistence.yes")
    : $t("systemConfigPage.persistence.no");
}

function persistenceStatusLabel(
  status: SystemConfigPersistenceHealthStatus,
): string {
  return $t(`systemConfigPage.persistence.status.${status}`);
}

function persistenceItemLabel(item: SystemConfigPersistenceHealthItem): string {
  return $t(`systemConfigPage.persistence.items.${item.key}`);
}

function sourceLabel(kind: string): string {
  return $t(`systemConfigPage.source.${kind}`);
}
</script>

<template>
  <div class="space-y-4">
    <div
      v-if="isMultiInstance"
      class="rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
    >
      <div class="flex gap-2">
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
        <div class="min-w-0">
          <p class="font-medium">{{ $t("systemConfigPage.multiInstanceTitle") }}</p>
          <p class="mt-1 text-amber-700">
            {{ $t("systemConfigPage.multiInstanceDescription") }}
          </p>
        </div>
      </div>
    </div>

    <div
      v-if="governanceDisabled"
      class="rounded-xl border border-amber-200 bg-amber-50 px-4 py-3 text-sm text-amber-800"
    >
      <div class="flex gap-2">
        <AlertTriangle class="mt-0.5 h-4 w-4 shrink-0" />
        <p class="min-w-0">
          {{ $t("systemConfigPage.providerGovernanceDisabled") }}
        </p>
      </div>
    </div>

    <div class="rounded-xl border border-gray-200 bg-white">
      <div class="flex items-center gap-2 border-b border-gray-100 px-4 py-3">
        <Settings class="h-4 w-4 text-gray-400" />
        <h2 class="text-base font-semibold text-gray-900">
          {{ $t("systemConfigPage.summaryTitle") }}
        </h2>
      </div>
      <dl class="grid grid-cols-1 divide-y divide-gray-100 text-sm md:grid-cols-2 md:divide-x md:divide-y-0">
        <div class="space-y-3 px-4 py-4">
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("systemConfigPage.deploymentMode") }}
            </dt>
            <dd class="mt-1 font-mono text-sm text-gray-900">
              {{ summary?.deployment_mode ?? "-" }}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("systemConfigPage.lastLoaded") }}
            </dt>
            <dd class="mt-1 font-mono text-sm text-gray-900">
              {{ formatSystemConfigTimestamp(summary?.loaded_at) }}
            </dd>
          </div>
          <div v-if="summary?.last_error">
            <dt class="text-xs font-medium uppercase tracking-wide text-red-500">
              {{ $t("systemConfigPage.lastError") }}
            </dt>
            <dd class="mt-1 break-words text-sm text-red-600">
              {{ summary.last_error }}
            </dd>
          </div>
        </div>

        <div class="space-y-3 px-4 py-4">
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("systemConfigPage.overridePath") }}
            </dt>
            <dd class="mt-1 break-all font-mono text-xs text-gray-700">
              {{ summary?.override_path ?? "-" }}
            </dd>
            <dd class="mt-1 text-xs text-gray-500">
              {{ formatFileState(summary?.override_exists) }}
            </dd>
          </div>
          <div>
            <dt class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("systemConfigPage.historyPath") }}
            </dt>
            <dd class="mt-1 break-all font-mono text-xs text-gray-700">
              {{ summary?.history_path ?? "-" }}
            </dd>
            <dd class="mt-1 text-xs text-gray-500">
              {{ formatFileState(summary?.history_exists) }}
            </dd>
          </div>
        </div>
      </dl>
    </div>

    <div class="rounded-xl border border-gray-200 bg-white">
      <SectionHeader
        :title="$t('systemConfigPage.sourcePriority.title')"
        :help="$t('systemConfigPage.sourcePriority.description')"
        :help-label="$t('systemConfigPage.sourcePriority.title')"
        class="border-b border-gray-100 px-4 py-3"
      />
      <div class="grid grid-cols-1 divide-y divide-gray-100 sm:grid-cols-2 sm:divide-x sm:divide-y-0 lg:grid-cols-6">
        <div v-for="layer in sourceLayers" :key="layer.kind" class="px-4 py-3">
          <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ sourceLabel(layer.kind) }}
          </p>
          <p class="mt-1 font-mono text-lg font-semibold text-gray-900">
            {{ layer.count }}
          </p>
          <p class="mt-1 text-xs text-gray-500">
            {{ $t("systemConfigPage.sourcePriority.configured", { count: layer.configured }) }}
          </p>
        </div>
      </div>
    </div>

    <div class="rounded-xl border border-gray-200 bg-white">
      <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <HardDrive class="h-4 w-4 text-gray-400" />
            <h2 class="text-base font-semibold text-gray-900">
              {{ $t("systemConfigPage.persistence.title") }}
            </h2>
          </div>
          <p class="mt-1 text-xs leading-5 text-gray-500">
            {{ $t("systemConfigPage.persistence.description") }}
          </p>
        </div>
        <div class="flex shrink-0 items-center gap-2">
          <Badge
            variant="outline"
            :class="persistenceStatusClass(persistenceHealth?.status ?? 'skipped')"
          >
            {{ persistenceStatusLabel(persistenceHealth?.status ?? "skipped") }}
          </Badge>
          <span class="text-xs text-gray-500">
            {{
              persistenceIssueCount
                ? $t("systemConfigPage.persistence.issueCount", { count: persistenceIssueCount })
                : $t("systemConfigPage.persistence.noIssues")
            }}
          </span>
        </div>
      </div>

      <div v-if="persistenceHealthItems.length" class="divide-y divide-gray-100">
        <article
          v-for="item in persistenceHealthItems"
          :key="item.key"
          :class="['px-4 py-4', persistenceRowClass(item.status)]"
        >
          <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <h3 class="text-sm font-medium text-gray-900">
                  {{ persistenceItemLabel(item) }}
                </h3>
                <Badge variant="outline" :class="persistenceStatusClass(item.status)">
                  {{ persistenceStatusLabel(item.status) }}
                </Badge>
              </div>
              <p v-if="item.path" class="mt-1 break-all font-mono text-xs text-gray-500">
                {{ item.path }}
              </p>
              <p class="mt-2 break-words text-sm text-gray-600">
                {{ item.message }}
              </p>
            </div>

            <dl class="grid grid-cols-3 gap-2 text-xs text-gray-500 sm:min-w-64">
              <div>
                <dt class="font-medium uppercase tracking-wide">
                  {{ $t("systemConfigPage.persistence.exists") }}
                </dt>
                <dd class="mt-1 font-mono text-gray-800">
                  {{ formatHealthBoolean(item.exists) }}
                </dd>
              </div>
              <div>
                <dt class="font-medium uppercase tracking-wide">
                  {{ $t("systemConfigPage.persistence.readable") }}
                </dt>
                <dd class="mt-1 font-mono text-gray-800">
                  {{ formatHealthBoolean(item.readable) }}
                </dd>
              </div>
              <div>
                <dt class="font-medium uppercase tracking-wide">
                  {{ $t("systemConfigPage.persistence.writable") }}
                </dt>
                <dd class="mt-1 font-mono text-gray-800">
                  {{ formatHealthBoolean(item.writable) }}
                </dd>
              </div>
            </dl>
          </div>
        </article>
      </div>
      <div v-else class="px-4 py-4 text-sm text-gray-500">
        {{ $t("systemConfigPage.persistence.empty") }}
      </div>
    </div>

    <div class="rounded-xl border border-gray-200 bg-white">
      <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <div class="flex items-center gap-2">
            <FileText class="h-4 w-4 text-gray-400" />
            <h2 class="text-base font-semibold text-gray-900">
              {{ $t("systemConfigPage.configView.title") }}
            </h2>
          </div>
          <p class="mt-1 break-all font-mono text-xs text-gray-500">
            {{ configDocumentPath }}
          </p>
        </div>
        <div class="grid grid-cols-2 gap-2 sm:flex sm:w-auto">
          <Button
            :variant="configViewMode === 'effective' ? 'default' : 'outline'"
            size="sm"
            @click="configViewMode = 'effective'"
          >
            {{ $t("systemConfigPage.configView.effective") }}
          </Button>
          <Button
            :variant="configViewMode === 'override' ? 'default' : 'outline'"
            size="sm"
            @click="configViewMode = 'override'"
          >
            {{ $t("systemConfigPage.configView.override") }}
          </Button>
        </div>
      </div>
      <div
        v-if="configViewMode === 'override' && configDocumentInvalidPaths.length"
        class="border-b border-amber-100 bg-amber-50 px-4 py-3 text-sm text-amber-800"
      >
        {{ $t("systemConfigPage.configView.invalidOverride") }}
        <span class="break-all font-mono">{{ configDocumentInvalidPaths.join(", ") }}</span>
      </div>
      <pre class="max-h-[32rem] overflow-auto whitespace-pre-wrap break-all px-4 py-4 font-mono text-xs text-gray-700">{{ configDocumentText }}</pre>
    </div>
  </div>
</template>
