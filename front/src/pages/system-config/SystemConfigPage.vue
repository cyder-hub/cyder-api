<script setup lang="ts">
import { onMounted, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  AlertCircle,
  Loader2,
  Pencil,
  RefreshCcw,
  RotateCcw,
  Search,
  SlidersHorizontal,
  X,
} from "lucide-vue-next";

import PageHeader from "@/components/PageHeader.vue";
import StatsStrip from "@/components/StatsStrip.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import type { SystemConfigHistoryItem } from "@/services/types";
import SystemConfigApplyPanel from "./components/SystemConfigApplyPanel.vue";
import SystemConfigHistoryTable from "./components/SystemConfigHistoryTable.vue";
import SystemConfigSourcePanel from "./components/SystemConfigSourcePanel.vue";
import { useSystemConfigHistory } from "./composables/useSystemConfigHistory";
import { useSystemConfigPreview } from "./composables/useSystemConfigPreview";
import {
  valuePrimary,
  useSystemConfigReport,
} from "./composables/useSystemConfigReport";
import { SYSTEM_CONFIG_ALL_FILTER } from "./composables/systemConfigState";

const { t: $t } = useI18n();

const {
  historyRows,
  isHistoryLoading,
  historyError,
  hasMoreHistory,
  loadHistory,
} = useSystemConfigHistory();

const {
  report,
  isLoading,
  isReloading,
  errorMessage,
  filters,
  configViewMode,
  fields,
  summary,
  isMultiInstance,
  rows,
  sectionOptions,
  sourceOptions,
  overrideCount,
  governanceDisabled,
  isFilterActive,
  summaryCards,
  booleanFilterOptions,
  configDocumentText,
  configDocumentInvalidPaths,
  persistenceHealth,
  persistenceHealthItems,
  persistenceIssueCount,
  configDocumentPath,
  sourceLayers,
  loadConfig,
  reloadOverride,
  setReport,
  setSectionFilter,
  setSourceFilter,
  setBooleanFilter,
  resetFilters,
  buildFieldBadges,
} = useSystemConfigReport({
  afterReload: () => {
    void loadHistory(true);
  },
});

const {
  isEditOpen,
  selectedField,
  editDraft,
  editReason,
  editError,
  isPreviewing,
  isApplying,
  preview,
  isResetOpen,
  resetReason,
  resetError,
  isResetting,
  previewDiffRows,
  previewWarningRows,
  runtimeActionLabels,
  draftValidationError,
  canApplyPreview,
  resetTargetPaths,
  canEditField,
  enumOptionsForField,
  openEditDialog,
  handleEditOpenChange,
  openSingleResetDialog,
  openAllOverridesResetDialog,
  handleResetOpenChange,
  previewEdit,
  applyEdit,
  resetSelectedFields,
} = useSystemConfigPreview({
  fields,
  isMultiInstance,
  setReport,
  afterMutation: () => {
    void loadHistory(true);
  },
});

const activeTab = ref<"current" | "source" | "history">("current");
const isFiltersExpanded = ref(false);

onMounted(() => {
  void loadConfig();
  void loadHistory(true);
});

function sourceLabel(kind: string): string {
  return $t(`systemConfigPage.source.${kind}`);
}

function historyOperationLabel(
  operation: SystemConfigHistoryItem["operation"],
): string {
  return $t(`systemConfigPage.history.operation.${operation}`);
}

function writeDisabledReasonLabel(reason: string): string {
  return $t(`systemConfigPage.preview.writeDisabled.${reason}`);
}

function valueText(rowValue: Parameters<typeof valuePrimary>[0]): string {
  return valuePrimary(rowValue, {
    redactedMissing: $t("systemConfigPage.redactedMissing"),
    redactedConfigured: $t("systemConfigPage.redactedConfigured"),
  });
}
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <PageHeader :title="$t('systemConfigPage.title')">
        <template #actions>
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="isLoading || isReloading || !overrideCount || isMultiInstance"
            @click="openAllOverridesResetDialog"
          >
            <X class="mr-1.5 h-4 w-4" />
            {{ $t("systemConfigPage.resetOverrides") }}
          </Button>
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="isLoading || isReloading || !report || isMultiInstance"
            @click="reloadOverride"
          >
            <RotateCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isReloading }" />
            {{ isReloading ? $t("systemConfigPage.reloading") : $t("systemConfigPage.reload") }}
          </Button>
          <Button
            variant="outline"
            class="w-full sm:w-auto"
            :disabled="isLoading || isReloading"
            @click="loadConfig"
          >
            <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isLoading }" />
            {{ $t("systemConfigPage.refresh") }}
          </Button>
        </template>
      </PageHeader>

      <div
        v-if="errorMessage"
        class="rounded-xl border border-red-200 bg-red-50 px-4 py-4 text-sm text-red-600"
      >
        {{ $t("systemConfigPage.loadFailed", { error: errorMessage }) }}
      </div>

      <div
        v-if="isLoading && !report"
        class="flex items-center justify-center rounded-xl border border-gray-200 bg-white py-16 text-gray-400"
      >
        <Loader2 class="mr-2 h-5 w-5 animate-spin" />
        <span class="text-sm">{{ $t("systemConfigPage.loading") }}</span>
      </div>

      <template v-else-if="report">
        <StatsStrip
          :items="summaryCards.map((card) => ({ ...card, mono: true }))"
          grid-class="grid-cols-2 md:grid-cols-5"
        />

        <div class="mt-6 border-b border-gray-200 app-scroll-x">
          <div class="flex min-w-max gap-1">
            <button
              v-for="tab in [
                { id: 'current', label: $t('systemConfigPage.tabs.current') },
                { id: 'source', label: $t('systemConfigPage.tabs.source') },
                { id: 'history', label: $t('systemConfigPage.tabs.history') }
              ]"
              :key="tab.id"
              type="button"
              class="border-b-2 px-4 py-2.5 text-sm font-medium transition-colors"
              :class="
                activeTab === tab.id
                  ? 'border-gray-900 text-gray-900'
                  : 'border-transparent text-gray-500 hover:text-gray-900 hover:border-gray-300'
              "
              @click="activeTab = tab.id as any"
            >
              {{ tab.label }}
            </button>
          </div>
        </div>

        <div class="mt-4 flex flex-col gap-4">
          <template v-if="activeTab === 'source'">
            <SystemConfigSourcePanel
          v-model:config-view-mode="configViewMode"
          :summary="summary"
          :is-multi-instance="isMultiInstance"
          :governance-disabled="governanceDisabled"
          :source-layers="sourceLayers"
          :persistence-health="persistenceHealth"
          :persistence-health-items="persistenceHealthItems"
          :persistence-issue-count="persistenceIssueCount"
          :config-document-path="configDocumentPath"
          :config-document-invalid-paths="configDocumentInvalidPaths"
          :config-document-text="configDocumentText"
        />
          </template>

          <template v-else-if="activeTab === 'history'">
            <SystemConfigHistoryTable
          :history-rows="historyRows"
          :is-history-loading="isHistoryLoading"
          :history-error="historyError"
          :has-more-history="hasMoreHistory"
          :history-operation-label="historyOperationLabel"
          @refresh="loadHistory(true)"
          @load-more="loadHistory(false)"
        />
          </template>

          <template v-else-if="activeTab === 'current'">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div class="flex items-center gap-2">
                <Button variant="outline" size="sm" @click="isFiltersExpanded = !isFiltersExpanded">
                  <SlidersHorizontal class="mr-1.5 h-4 w-4" :class="isFiltersExpanded ? 'text-gray-900' : 'text-gray-500'" />
                  {{ $t("systemConfigPage.filters.title") }}
                  <Badge v-if="isFilterActive" variant="secondary" class="ml-1.5 h-5 px-1.5 rounded-full font-mono text-xs">!</Badge>
                </Button>
                <div class="text-sm text-gray-500">
                  {{ $t("systemConfigPage.filters.activeSummary", { shown: rows.length, total: fields.length }) }}
                </div>
              </div>

              <div class="relative w-full sm:w-64">
                <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
                <Input
                  v-model="filters.search"
                  class="h-9 w-full pl-9"
                  :placeholder="$t('systemConfigPage.filters.searchPlaceholder')"
                />
              </div>
            </div>

            <div v-show="isFiltersExpanded" class="rounded-xl border border-gray-200 bg-white p-4">
              <div class="mb-4 flex items-center justify-between border-b border-gray-100 pb-3">
                <h3 class="text-sm font-medium text-gray-700">{{ $t("systemConfigPage.filters.title") }}</h3>
                <Button variant="ghost" size="sm" :disabled="!isFilterActive" @click="resetFilters">
                  <X class="mr-1.5 h-4 w-4" />
                  {{ $t("systemConfigPage.resetFilters") }}
                </Button>
              </div>

              <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
                <div>
                  <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("systemConfigPage.filters.section") }}
                  </span>
                  <Select :model-value="filters.section" @update:model-value="setSectionFilter">
                    <SelectTrigger class="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :body-lock="false">
                      <SelectItem :value="SYSTEM_CONFIG_ALL_FILTER">
                        {{ $t("systemConfigPage.filters.allSections") }}
                      </SelectItem>
                      <SelectItem
                        v-for="section in sectionOptions"
                        :key="section"
                        :value="section"
                      >
                        {{ section }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div>
                  <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("systemConfigPage.filters.source") }}
                  </span>
                  <Select :model-value="filters.source" @update:model-value="setSourceFilter">
                    <SelectTrigger class="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :body-lock="false">
                      <SelectItem :value="SYSTEM_CONFIG_ALL_FILTER">
                        {{ $t("systemConfigPage.filters.allSources") }}
                      </SelectItem>
                      <SelectItem v-for="source in sourceOptions" :key="source" :value="source">
                        {{ sourceLabel(source) }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div>
                  <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("systemConfigPage.filters.editable") }}
                  </span>
                  <Select
                    :model-value="filters.editable"
                    @update:model-value="(value) => setBooleanFilter('editable', value)"
                  >
                    <SelectTrigger class="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :body-lock="false">
                      <SelectItem
                        v-for="option in booleanFilterOptions"
                        :key="`editable-${option.value}`"
                        :value="option.value"
                      >
                        {{ option.label }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div>
                  <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("systemConfigPage.filters.hotReloadable") }}
                  </span>
                  <Select
                    :model-value="filters.hotReloadable"
                    @update:model-value="(value) => setBooleanFilter('hotReloadable', value)"
                  >
                    <SelectTrigger class="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :body-lock="false">
                      <SelectItem
                        v-for="option in booleanFilterOptions"
                        :key="`hot-${option.value}`"
                        :value="option.value"
                      >
                        {{ option.label }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div>
                  <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("systemConfigPage.filters.restartRequired") }}
                  </span>
                  <Select
                    :model-value="filters.restartRequired"
                    @update:model-value="(value) => setBooleanFilter('restartRequired', value)"
                  >
                    <SelectTrigger class="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :body-lock="false">
                      <SelectItem
                        v-for="option in booleanFilterOptions"
                        :key="`restart-${option.value}`"
                        :value="option.value"
                      >
                        {{ option.label }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>

                <div>
                  <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                    {{ $t("systemConfigPage.filters.sensitive") }}
                  </span>
                  <Select
                    :model-value="filters.sensitive"
                    @update:model-value="(value) => setBooleanFilter('sensitive', value)"
                  >
                    <SelectTrigger class="w-full">
                      <SelectValue />
                    </SelectTrigger>
                    <SelectContent :body-lock="false">
                      <SelectItem
                        v-for="option in booleanFilterOptions"
                        :key="`sensitive-${option.value}`"
                        :value="option.value"
                      >
                        {{ option.label }}
                      </SelectItem>
                    </SelectContent>
                  </Select>
                </div>
              </div>
            </div>

            <div v-if="rows.length === 0" class="rounded-xl border border-gray-200 bg-white py-14 text-center text-sm text-gray-500">
          {{ fields.length === 0 ? $t("systemConfigPage.noFields") : $t("systemConfigPage.noMatchingFields") }}
        </div>

        <div v-else class="hidden overflow-hidden rounded-xl border border-gray-200 bg-white md:block">
          <div class="app-scroll-x">
            <Table>
              <TableHeader>
                <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                  <TableHead>{{ $t("systemConfigPage.table.path") }}</TableHead>
                  <TableHead>{{ $t("systemConfigPage.table.value") }}</TableHead>
                  <TableHead>{{ $t("systemConfigPage.table.source") }}</TableHead>
                  <TableHead>{{ $t("systemConfigPage.table.status") }}</TableHead>
                  <TableHead>{{ $t("systemConfigPage.table.description") }}</TableHead>
                  <TableHead class="text-right">{{ $t("systemConfigPage.table.actions") }}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow v-for="row in rows" :key="row.field.path">
                  <TableCell class="align-top">
                    <div class="font-mono text-xs font-medium text-gray-900">
                      {{ row.field.path }}
                    </div>
                    <div class="mt-1 text-xs text-gray-500">
                      {{ row.field.section }}
                    </div>
                  </TableCell>
                  <TableCell class="max-w-[320px] align-top">
                    <pre class="max-h-28 whitespace-pre-wrap break-all font-mono text-xs text-gray-700">{{ valueText(row.value) }}</pre>
                    <p v-if="row.value.detail" class="mt-1 break-all font-mono text-[11px] text-gray-400">
                      {{ row.value.detail }}
                    </p>
                  </TableCell>
                  <TableCell class="align-top">
                    <Badge variant="outline" class="font-mono text-xs">
                      {{ sourceLabel(row.field.source.kind) }}
                    </Badge>
                    <p class="mt-1 max-w-[240px] break-all font-mono text-[11px] text-gray-500">
                      {{ row.field.source.source_name }}
                    </p>
                  </TableCell>
                  <TableCell class="align-top">
                    <div class="flex max-w-[260px] flex-wrap gap-1.5">
                      <Badge
                        v-for="badge in row.badges"
                        :key="`${row.field.path}-${badge.key}`"
                        variant="outline"
                        :class="badge.class"
                      >
                        {{ badge.label }}
                      </Badge>
                    </div>
                  </TableCell>
                  <TableCell class="max-w-[360px] align-top">
                    <p class="text-sm text-gray-600">
                      {{ row.field.description }}
                    </p>
                    <div v-if="row.field.constraints.length" class="mt-2 flex flex-wrap gap-1.5">
                      <Badge
                        v-for="constraint in row.field.constraints"
                        :key="`${row.field.path}-${constraint}`"
                        variant="outline"
                        class="max-w-full whitespace-normal break-words text-xs text-gray-500"
                      >
                        {{ constraint }}
                      </Badge>
                    </div>
                    <p
                      v-if="row.field.source.warnings.length"
                      class="mt-2 break-words text-xs text-amber-700"
                    >
                      {{ row.field.source.warnings.join(" · ") }}
                    </p>
                  </TableCell>
                  <TableCell class="align-top text-right">
                    <div v-if="canEditField(row.field)" class="flex justify-end gap-1.5">
                      <Button variant="ghost" size="sm" @click="openEditDialog(row.field)">
                        <Pencil class="mr-1 h-3.5 w-3.5" />
                        {{ $t("systemConfigPage.actions.edit") }}
                      </Button>
                      <Button
                        v-if="row.field.source.kind === 'override_file'"
                        variant="ghost"
                        size="sm"
                        class="text-gray-500 hover:text-red-600"
                        @click="openSingleResetDialog(row.field)"
                      >
                        <X class="mr-1 h-3.5 w-3.5" />
                        {{ $t("systemConfigPage.actions.reset") }}
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>
        </div>

        <div v-if="rows.length > 0" class="space-y-3 md:hidden">
          <article
            v-for="row in rows"
            :key="`mobile-${row.field.path}`"
            class="rounded-xl border border-gray-200 bg-white p-4"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0">
                <h2 class="break-all font-mono text-sm font-semibold text-gray-900">
                  {{ row.field.path }}
                </h2>
                <p class="mt-1 text-xs text-gray-500">{{ row.field.section }}</p>
              </div>
              <Badge variant="outline" class="font-mono text-xs">
                {{ sourceLabel(row.field.source.kind) }}
              </Badge>
            </div>
            <pre class="mt-3 max-h-32 overflow-auto whitespace-pre-wrap break-all rounded-md bg-gray-50 px-3 py-2 font-mono text-xs text-gray-700">{{ valueText(row.value) }}</pre>
            <p v-if="row.value.detail" class="mt-1 break-all font-mono text-[11px] text-gray-400">
              {{ row.value.detail }}
            </p>
            <div class="mt-3 flex flex-wrap gap-1.5">
              <Badge
                v-for="badge in row.badges"
                :key="`mobile-${row.field.path}-${badge.key}`"
                variant="outline"
                :class="badge.class"
              >
                {{ badge.label }}
              </Badge>
            </div>
            <p class="mt-3 text-sm text-gray-600">
              {{ row.field.description }}
            </p>
            <p class="mt-2 break-all font-mono text-[11px] text-gray-500">
              {{ row.field.source.source_name }}
            </p>
            <div v-if="row.field.constraints.length" class="mt-3 flex flex-wrap gap-1.5">
              <Badge
                v-for="constraint in row.field.constraints"
                :key="`mobile-${row.field.path}-${constraint}`"
                variant="outline"
                class="max-w-full whitespace-normal break-words text-xs text-gray-500"
              >
                {{ constraint }}
              </Badge>
            </div>
            <div v-if="canEditField(row.field)" class="mt-4 grid grid-cols-1 gap-2 sm:grid-cols-2">
              <Button variant="outline" size="sm" @click="openEditDialog(row.field)">
                <Pencil class="mr-1 h-3.5 w-3.5" />
                {{ $t("systemConfigPage.actions.edit") }}
              </Button>
              <Button
                v-if="row.field.source.kind === 'override_file'"
                variant="outline"
                size="sm"
                class="text-gray-500 hover:text-red-600"
                @click="openSingleResetDialog(row.field)"
              >
                <X class="mr-1 h-3.5 w-3.5" />
                {{ $t("systemConfigPage.actions.reset") }}
              </Button>
            </div>
          </article>
        </div>
          </template>
        </div>
      </template>
    </div>

    <div v-if="!isLoading && !report && !errorMessage" class="app-page-shell">
      <div class="flex flex-col items-center justify-center rounded-xl border border-gray-200 bg-white py-20">
        <AlertCircle class="mb-4 h-10 w-10 stroke-1 text-gray-400" />
        <span class="text-sm font-medium text-gray-500">
          {{ $t("systemConfigPage.noFields") }}
        </span>
      </div>
    </div>

    <SystemConfigApplyPanel
      v-model:edit-draft="editDraft"
      v-model:edit-reason="editReason"
      v-model:reset-reason="resetReason"
      :is-edit-open="isEditOpen"
      :selected-field="selectedField"
      :edit-error="editError"
      :is-previewing="isPreviewing"
      :is-applying="isApplying"
      :preview="preview"
      :preview-diff-rows="previewDiffRows"
      :preview-warning-rows="previewWarningRows"
      :runtime-action-labels="runtimeActionLabels"
      :draft-validation-error="draftValidationError"
      :can-apply-preview="canApplyPreview"
      :is-reset-open="isResetOpen"
      :reset-error="resetError"
      :reset-target-paths="resetTargetPaths"
      :is-resetting="isResetting"
      :build-field-badges="buildFieldBadges"
      :enum-options-for-field="enumOptionsForField"
      :write-disabled-reason-label="writeDisabledReasonLabel"
      @edit-open-change="handleEditOpenChange"
      @reset-open-change="handleResetOpenChange"
      @preview-edit="previewEdit"
      @apply-edit="applyEdit"
      @reset-selected-fields="resetSelectedFields"
    />
  </div>
</template>
