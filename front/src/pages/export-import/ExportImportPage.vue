<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import {
  AlertTriangle,
  CheckCircle2,
  Download,
  ExternalLink,
  FileUp,
  KeyRound,
  Loader2,
  RefreshCcw,
  ShieldCheck,
  Upload,
} from "lucide-vue-next";

import CrudPageLayout from "@/components/CrudPageLayout.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  ConflictStrategy,
  FileProtectionMode,
  PortableApplyModuleStatus,
  PortableModuleId,
  PortableModuleSummary,
  PortableSubrangeId,
} from "@/services/types";
import { usePortableExport } from "./composables/usePortableExport";
import { usePortableImport } from "./composables/usePortableImport";

type ActiveTab = "export" | "import";

const { t } = useI18n();
const activeTab = ref<ActiveTab>("export");

const {
  fileProtection,
  password: exportPassword,
  autoGeneratePassword,
  isLoadingModules,
  isExporting,
  error: exportError,
  exportResult,
  downloadedFilename,
  moduleRows: exportModuleRows,
  selectedModulesContainSecrets,
  canExport,
  loadModules,
  setFileProtection,
  toggleModule: toggleExportModule,
  toggleSubrange: toggleExportSubrange,
  runExport,
} = usePortableExport({ t });

const {
  fileName: importFileName,
  password: importPassword,
  preview,
  selectedModules: importSelectedModules,
  conflictStrategy,
  reason: importReason,
  dangerousPatchConfirmations,
  applyResult,
  isReadingFile,
  isPreviewing,
  isApplying,
  error: importError,
  previewModuleRows,
  blockingIssues,
  hasBlockingState,
  canPreview,
  canApply,
  applyDisabledReason,
  applySummaryText,
  readFile,
  runPreview,
  toggleModule: toggleImportModule,
  setDangerousPatchConfirmation,
  runApply,
} = usePortableImport({ t });

const conflictStrategies: ConflictStrategy[] = [
  "fail_on_conflict",
  "skip_existing",
  "overwrite_existing",
];

const fileProtectionModes: FileProtectionMode[] = [
  "password_encrypted",
  "plaintext",
];

const hasImportSelection = computed(() => importSelectedModules.value.length > 0);

function handleImportFileChange(event: Event) {
  const input = event.target as HTMLInputElement;
  void readFile(input.files?.[0] ?? null);
}

function handleFileProtectionChange(value: unknown) {
  if (value === "plaintext" || value === "password_encrypted") {
    setFileProtection(value);
  }
}

function handleConflictStrategyChange(value: unknown) {
  if (
    value === "fail_on_conflict" ||
    value === "skip_existing" ||
    value === "overwrite_existing"
  ) {
    conflictStrategy.value = value;
  }
}

function handleExportModuleToggle(moduleId: PortableModuleId, checked: boolean | "indeterminate") {
  toggleExportModule(moduleId, checked === true);
}

function handleExportSubrangeToggle(
  moduleId: PortableModuleId,
  subrangeId: PortableSubrangeId,
  checked: boolean | "indeterminate",
) {
  toggleExportSubrange(moduleId, subrangeId, checked === true);
}

function handleImportModuleToggle(moduleId: PortableModuleId, checked: boolean | "indeterminate") {
  toggleImportModule(moduleId, checked === true);
}

function moduleLabel(moduleId: PortableModuleId, fallback: string): string {
  return translatedFallback(`portableConfigPage.modules.${moduleId}`, fallback);
}

function subrangeLabel(subrangeId: PortableSubrangeId, fallback: string): string {
  return translatedFallback(`portableConfigPage.subranges.${subrangeId}`, fallback);
}

function translatedFallback(key: string, fallback: string): string {
  const translated = t(key);
  return translated === key ? fallback : translated;
}

function summaryRows(summary: PortableModuleSummary) {
  return [
    ["total", summary.total],
    ["create", summary.create],
    ["update", summary.update],
    ["skip", summary.skip],
    ["blocked", summary.blocked],
    ["conflict", summary.conflict],
  ] as const;
}

function formatDateTime(value: number): string {
  if (!value) {
    return t("common.notAvailable");
  }
  return new Date(value).toLocaleString();
}

function fileProtectionLabel(mode: FileProtectionMode): string {
  return t(`portableConfigPage.fileProtection.${mode}`);
}

function conflictStrategyLabel(strategy: ConflictStrategy): string {
  return t(`portableConfigPage.conflictStrategy.${strategy}`);
}

function applyStatusLabel(status: PortableApplyModuleStatus): string {
  return t(`portableConfigPage.applyStatus.${status}`);
}

function applyStatusClass(status: PortableApplyModuleStatus): string {
  switch (status) {
    case "applied":
      return "border-gray-900 bg-gray-900 text-white";
    case "skipped":
      return "border-gray-200 bg-gray-100 text-gray-600";
    case "blocked":
    case "failed":
      return "border-red-200 bg-red-50 text-red-700";
  }
}
</script>

<template>
  <CrudPageLayout
    :title="t('portableConfigPage.title')"
    :description="t('portableConfigPage.description')"
    content-class="space-y-5"
  >
    <div class="flex flex-col gap-2 sm:w-fit sm:flex-row">
      <Button
        :variant="activeTab === 'export' ? 'default' : 'outline'"
        class="w-full sm:w-auto"
        @click="activeTab = 'export'"
      >
        <Download class="mr-1.5 h-4 w-4" />
        {{ t("portableConfigPage.tabs.export") }}
      </Button>
      <Button
        :variant="activeTab === 'import' ? 'default' : 'outline'"
        class="w-full sm:w-auto"
        @click="activeTab = 'import'"
      >
        <Upload class="mr-1.5 h-4 w-4" />
        {{ t("portableConfigPage.tabs.import") }}
      </Button>
    </div>

    <section v-if="activeTab === 'export'" class="rounded-xl border border-gray-200 bg-white">
      <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-4 sm:flex-row sm:items-start sm:justify-between sm:px-5">
        <div class="min-w-0">
          <h2 class="text-base font-semibold text-gray-900">
            {{ t("portableConfigPage.export.title") }}
          </h2>
          <p class="mt-1 text-sm text-gray-500">
            {{ t("portableConfigPage.export.description") }}
          </p>
        </div>
        <Button
          variant="outline"
          class="w-full sm:w-auto"
          :disabled="isLoadingModules"
          @click="loadModules"
        >
          <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isLoadingModules }" />
          {{ t("common.refresh") }}
        </Button>
      </div>

      <div class="space-y-5 px-4 py-4 sm:px-5">
        <div v-if="exportError" class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
          {{ exportError }}
        </div>

        <div v-if="isLoadingModules" class="flex items-center justify-center py-12 text-gray-500">
          <Loader2 class="mr-2 h-5 w-5 animate-spin" />
          <span class="text-sm">{{ t("portableConfigPage.export.loadingModules") }}</span>
        </div>

        <div v-else class="divide-y divide-gray-100 overflow-hidden rounded-lg border border-gray-200">
          <div
            v-for="row in exportModuleRows"
            :key="row.module.module_id"
            class="px-4 py-4"
            :class="row.disabledReason ? 'bg-gray-50/70' : 'bg-white'"
          >
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <label class="flex min-w-0 items-start gap-3">
                <Checkbox
                  class="mt-0.5"
                  :checked="row.checked"
                  :disabled="!!row.disabledReason && !row.checked"
                  @update:checked="handleExportModuleToggle(row.module.module_id, $event)"
                />
                <span class="min-w-0">
                  <span class="flex flex-wrap items-center gap-2">
                    <span class="text-sm font-medium text-gray-900">
                      {{ moduleLabel(row.module.module_id, row.module.label) }}
                    </span>
                    <Badge v-if="row.module.contains_secrets" variant="outline" class="text-xs">
                      {{ t("portableConfigPage.badges.secrets") }}
                    </Badge>
                    <Badge v-if="row.module.deferred" variant="secondary" class="text-xs">
                      {{ t("portableConfigPage.badges.deferred") }}
                    </Badge>
                  </span>
                  <span class="mt-1 block text-sm text-gray-500">
                    {{ row.module.description }}
                  </span>
                </span>
              </label>
              <p v-if="row.disabledReason" class="text-sm text-gray-500 sm:max-w-xs sm:text-right">
                {{ row.disabledReason }}
              </p>
            </div>

            <div v-if="row.module.subranges.length" class="mt-3 grid grid-cols-1 gap-2 sm:grid-cols-2 xl:grid-cols-3">
              <label
                v-for="subrange in row.module.subranges"
                :key="subrange.subrange_id"
                class="flex items-center justify-between gap-3 rounded-lg border border-gray-200 px-3 py-2.5"
                :class="!row.checked || subrange.deferred ? 'bg-gray-50 text-gray-400' : 'bg-white text-gray-700'"
              >
                <span class="min-w-0">
                  <span class="block truncate text-sm font-medium">
                    {{ subrangeLabel(subrange.subrange_id, subrange.label) }}
                  </span>
                  <span v-if="subrange.required" class="mt-0.5 block text-xs text-gray-400">
                    {{ t("portableConfigPage.badges.required") }}
                  </span>
                </span>
                <Checkbox
                  :checked="row.selectedSubrangeIds.has(subrange.subrange_id)"
                  :disabled="!row.checked || subrange.required || subrange.deferred"
                  @update:checked="handleExportSubrangeToggle(row.module.module_id, subrange.subrange_id, $event)"
                />
              </label>
            </div>
          </div>
        </div>

        <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <div class="rounded-lg border border-gray-200 p-4">
            <div class="flex items-center gap-2">
              <ShieldCheck class="h-4 w-4 text-gray-500" />
              <h3 class="text-sm font-semibold text-gray-900">
                {{ t("portableConfigPage.export.protection") }}
              </h3>
            </div>
            <div class="mt-4 space-y-3">
              <Select :model-value="fileProtection" @update:model-value="handleFileProtectionChange">
                <SelectTrigger class="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent :body-lock="false">
                  <SelectItem v-for="mode in fileProtectionModes" :key="mode" :value="mode">
                    {{ fileProtectionLabel(mode) }}
                  </SelectItem>
                </SelectContent>
              </Select>

              <div
                v-if="fileProtection === 'plaintext' && selectedModulesContainSecrets"
                class="rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-sm text-amber-800"
              >
                {{ t("portableConfigPage.export.plaintextWarning") }}
              </div>

              <div v-if="fileProtection === 'password_encrypted'" class="space-y-2">
                <Input
                  v-model="exportPassword"
                  type="password"
                  autocomplete="new-password"
                  :placeholder="t('portableConfigPage.export.passwordPlaceholder')"
                  :disabled="autoGeneratePassword"
                />
                <label class="flex items-center justify-between gap-3 rounded-lg border border-gray-200 px-3 py-2.5">
                  <span class="text-sm text-gray-700">
                    {{ t("portableConfigPage.export.autoGeneratePassword") }}
                  </span>
                  <Checkbox v-model:checked="autoGeneratePassword" />
                </label>
              </div>
            </div>
          </div>

          <div class="rounded-lg border border-gray-200 p-4">
            <div class="flex items-center gap-2">
              <KeyRound class="h-4 w-4 text-gray-500" />
              <h3 class="text-sm font-semibold text-gray-900">
                {{ t("portableConfigPage.export.result") }}
              </h3>
            </div>
            <div class="mt-4 space-y-2 text-sm">
              <div class="flex items-center justify-between gap-3">
                <span class="text-gray-500">{{ t("portableConfigPage.fields.filename") }}</span>
                <span class="min-w-0 truncate font-mono text-xs text-gray-700">
                  {{ downloadedFilename || t("common.notAvailable") }}
                </span>
              </div>
              <div class="flex items-center justify-between gap-3">
                <span class="text-gray-500">{{ t("portableConfigPage.fields.digest") }}</span>
                <span class="min-w-0 truncate font-mono text-xs text-gray-700">
                  {{ exportResult?.bundle_digest || t("common.notAvailable") }}
                </span>
              </div>
              <div v-if="exportResult?.generated_password" class="rounded-lg border border-gray-200 bg-gray-50 px-3 py-2">
                <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
                  {{ t("portableConfigPage.export.generatedPassword") }}
                </p>
                <p class="mt-1 break-all font-mono text-sm text-gray-900">
                  {{ exportResult.generated_password }}
                </p>
              </div>
            </div>
          </div>
        </div>

        <div class="flex flex-col gap-2 sm:flex-row sm:justify-end">
          <Button
            class="w-full sm:w-auto"
            :disabled="!canExport || isExporting"
            @click="runExport"
          >
            <Loader2 v-if="isExporting" class="mr-1.5 h-4 w-4 animate-spin" />
            <Download v-else class="mr-1.5 h-4 w-4" />
            {{ isExporting ? t("portableConfigPage.export.exporting") : t("portableConfigPage.export.download") }}
          </Button>
        </div>
      </div>
    </section>

    <section v-else class="rounded-xl border border-gray-200 bg-white">
      <div class="border-b border-gray-100 px-4 py-4 sm:px-5">
        <h2 class="text-base font-semibold text-gray-900">
          {{ t("portableConfigPage.import.title") }}
        </h2>
        <p class="mt-1 text-sm text-gray-500">
          {{ t("portableConfigPage.import.description") }}
        </p>
      </div>

      <div class="space-y-5 px-4 py-4 sm:px-5">
        <div v-if="importError" class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
          {{ importError }}
        </div>

        <div class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <div class="rounded-lg border border-gray-200 p-4">
            <div class="flex items-center gap-2">
              <FileUp class="h-4 w-4 text-gray-500" />
              <h3 class="text-sm font-semibold text-gray-900">
                {{ t("portableConfigPage.import.bundleFile") }}
              </h3>
            </div>
            <div class="mt-4 space-y-3">
              <input
                type="file"
                accept=".cyd,application/json,text/plain"
                class="block w-full text-sm text-gray-600 file:mr-3 file:rounded-md file:border file:border-gray-200 file:bg-white file:px-3 file:py-2 file:text-sm file:font-medium file:text-gray-700 hover:file:bg-gray-50"
                @change="handleImportFileChange"
              />
              <div class="flex items-center justify-between gap-3 text-sm">
                <span class="text-gray-500">{{ t("portableConfigPage.fields.filename") }}</span>
                <span class="min-w-0 truncate font-mono text-xs text-gray-700">
                  {{ importFileName || t("common.notAvailable") }}
                </span>
              </div>
              <Input
                v-model="importPassword"
                type="password"
                autocomplete="current-password"
                :placeholder="t('portableConfigPage.import.passwordPlaceholder')"
              />
              <Button
                variant="outline"
                class="w-full"
                :disabled="!canPreview || isReadingFile || isPreviewing"
                @click="runPreview"
              >
                <Loader2 v-if="isReadingFile || isPreviewing" class="mr-1.5 h-4 w-4 animate-spin" />
                <ShieldCheck v-else class="mr-1.5 h-4 w-4" />
                {{ t("portableConfigPage.import.preview") }}
              </Button>
            </div>
          </div>

          <div class="rounded-lg border border-gray-200 p-4">
            <div class="flex items-center gap-2">
              <CheckCircle2 class="h-4 w-4 text-gray-500" />
              <h3 class="text-sm font-semibold text-gray-900">
                {{ t("portableConfigPage.import.previewSummary") }}
              </h3>
            </div>
            <dl class="mt-4 space-y-2 text-sm">
              <div class="flex items-center justify-between gap-3">
                <dt class="text-gray-500">{{ t("portableConfigPage.fields.exportedAt") }}</dt>
                <dd class="text-right text-gray-900">
                  {{ preview ? formatDateTime(preview.exported_at) : t("common.notAvailable") }}
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3">
                <dt class="text-gray-500">{{ t("portableConfigPage.fields.protection") }}</dt>
                <dd class="text-right text-gray-900">
                  {{ preview ? fileProtectionLabel(preview.file_protection.mode) : t("common.notAvailable") }}
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3">
                <dt class="text-gray-500">{{ t("portableConfigPage.fields.digest") }}</dt>
                <dd class="min-w-0 truncate text-right font-mono text-xs text-gray-700">
                  {{ preview?.bundle_digest || t("common.notAvailable") }}
                </dd>
              </div>
              <div class="flex items-center justify-between gap-3">
                <dt class="text-gray-500">{{ t("portableConfigPage.fields.blocking") }}</dt>
                <dd>
                  <Badge :variant="hasBlockingState ? 'outline' : 'secondary'" :class="hasBlockingState ? 'border-red-200 bg-red-50 text-red-700' : ''">
                    {{ hasBlockingState ? t("portableConfigPage.state.blocked") : t("portableConfigPage.state.clear") }}
                  </Badge>
                </dd>
              </div>
            </dl>
          </div>
        </div>

        <div v-if="blockingIssues.length" class="rounded-lg border border-red-200 bg-red-50 px-4 py-3">
          <div class="flex items-center gap-2 text-sm font-medium text-red-800">
            <AlertTriangle class="h-4 w-4" />
            {{ t("portableConfigPage.import.blockingIssues") }}
          </div>
          <ul class="mt-2 space-y-1 text-sm text-red-700">
            <li v-for="issue in blockingIssues" :key="`${issue.path}:${issue.code}:${issue.target || ''}`">
              <span class="font-mono text-xs">{{ issue.path }}</span>
              <span class="mx-1">-</span>
              <span>{{ issue.message }}</span>
            </li>
          </ul>
        </div>

        <div v-if="preview" class="divide-y divide-gray-100 overflow-hidden rounded-lg border border-gray-200">
          <div
            v-for="row in previewModuleRows"
            :key="row.module.module_id"
            class="bg-white px-4 py-4"
          >
            <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
              <label class="flex min-w-0 items-start gap-3">
                <Checkbox
                  class="mt-0.5"
                  :checked="row.checked"
                  :disabled="!row.module.supported || !row.module.available || row.module.deferred"
                  @update:checked="handleImportModuleToggle(row.module.module_id, $event)"
                />
                <span class="min-w-0">
                  <span class="flex flex-wrap items-center gap-2">
                    <span class="text-sm font-medium text-gray-900">
                      {{ moduleLabel(row.module.module_id, row.module.label) }}
                    </span>
                    <Badge v-if="row.module.contains_secrets" variant="outline" class="text-xs">
                      {{ t("portableConfigPage.badges.secrets") }}
                    </Badge>
                    <Badge v-if="row.module.summary.blocked" variant="outline" class="border-red-200 bg-red-50 text-xs text-red-700">
                      {{ t("portableConfigPage.state.blocked") }}
                    </Badge>
                    <Badge v-if="row.module.summary.conflict" variant="outline" class="border-amber-200 bg-amber-50 text-xs text-amber-800">
                      {{ t("portableConfigPage.state.conflict") }}
                    </Badge>
                  </span>
                  <span class="mt-1 block text-sm text-gray-500">
                    {{ row.module.subranges.join(", ") || t("common.notAvailable") }}
                  </span>
                </span>
              </label>
              <div class="grid grid-cols-3 gap-2 sm:grid-cols-6">
                <div
                  v-for="[key, value] in summaryRows(row.module.summary)"
                  :key="key"
                  class="min-w-16 rounded-md bg-gray-50 px-2 py-1.5 text-center"
                >
                  <p class="text-[10px] font-medium uppercase tracking-wide text-gray-500">
                    {{ t(`portableConfigPage.summary.${key}`) }}
                  </p>
                  <p class="font-mono text-sm font-semibold text-gray-900">
                    {{ value }}
                  </p>
                </div>
              </div>
            </div>
          </div>
        </div>

        <div v-if="preview" class="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <div class="rounded-lg border border-gray-200 p-4">
            <h3 class="text-sm font-semibold text-gray-900">
              {{ t("portableConfigPage.import.applyOptions") }}
            </h3>
            <div class="mt-4 space-y-3">
              <Select :model-value="conflictStrategy" @update:model-value="handleConflictStrategyChange">
                <SelectTrigger class="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent :body-lock="false">
                  <SelectItem v-for="strategy in conflictStrategies" :key="strategy" :value="strategy">
                    {{ conflictStrategyLabel(strategy) }}
                  </SelectItem>
                </SelectContent>
              </Select>
              <textarea
                v-model="importReason"
                rows="3"
                class="min-h-24 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition focus:border-gray-400 focus:ring-2 focus:ring-gray-200"
                :placeholder="t('portableConfigPage.import.reasonPlaceholder')"
              />
            </div>
          </div>

          <div class="rounded-lg border border-gray-200 p-4">
            <h3 class="text-sm font-semibold text-gray-900">
              {{ t("portableConfigPage.import.dangerousPatchConfirmations") }}
            </h3>
            <div v-if="dangerousPatchConfirmations.length" class="mt-4 space-y-2">
              <label
                v-for="confirmation in dangerousPatchConfirmations"
                :key="`${confirmation.path}:${confirmation.target}`"
                class="flex items-start justify-between gap-3 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2.5"
              >
                <span class="min-w-0 text-sm text-amber-900">
                  <span class="block font-mono text-xs">{{ confirmation.path }}</span>
                  <span class="mt-1 block break-all">{{ confirmation.target }}</span>
                </span>
                <Checkbox
                  :checked="confirmation.confirmed"
                  @update:checked="setDangerousPatchConfirmation(confirmation.path, confirmation.target, $event === true)"
                />
              </label>
            </div>
            <p v-else class="mt-4 text-sm text-gray-500">
              {{ t("portableConfigPage.import.noDangerousPatchConfirmations") }}
            </p>
          </div>
        </div>

        <div v-if="preview" class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <p
            class="text-sm"
            :class="applyDisabledReason ? 'text-amber-700' : 'text-gray-500'"
          >
            {{ applyDisabledReason || (hasImportSelection ? t("portableConfigPage.import.readyToApply") : t("portableConfigPage.import.selectModule")) }}
          </p>
          <Button
            class="w-full sm:w-auto"
            :disabled="!canApply || isApplying"
            @click="runApply"
          >
            <Loader2 v-if="isApplying" class="mr-1.5 h-4 w-4 animate-spin" />
            <Upload v-else class="mr-1.5 h-4 w-4" />
            {{ isApplying ? t("portableConfigPage.import.applying") : t("portableConfigPage.import.apply") }}
          </Button>
        </div>

        <div v-if="applyResult" class="rounded-lg border border-gray-200 p-4">
          <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
            <div>
              <h3 class="text-sm font-semibold text-gray-900">
                {{ t("portableConfigPage.import.applyResult") }}
              </h3>
              <p class="mt-1 font-mono text-xs text-gray-500">
                {{ applySummaryText }}
              </p>
            </div>
            <div class="flex flex-col gap-2 sm:flex-row">
              <Button as-child variant="outline" class="w-full sm:w-auto">
                <RouterLink to="/provider">
                  <ExternalLink class="mr-1.5 h-4 w-4" />
                  {{ t("sidebar.provider") }}
                </RouterLink>
              </Button>
              <Button as-child variant="outline" class="w-full sm:w-auto">
                <RouterLink to="/model">
                  <ExternalLink class="mr-1.5 h-4 w-4" />
                  {{ t("sidebar.model") }}
                </RouterLink>
              </Button>
              <Button as-child variant="outline" class="w-full sm:w-auto">
                <RouterLink to="/api_key">
                  <ExternalLink class="mr-1.5 h-4 w-4" />
                  {{ t("sidebar.apiKey") }}
                </RouterLink>
              </Button>
              <Button as-child variant="outline" class="w-full sm:w-auto">
                <RouterLink to="/cost">
                  <ExternalLink class="mr-1.5 h-4 w-4" />
                  {{ t("sidebar.cost") }}
                </RouterLink>
              </Button>
            </div>
          </div>

          <div class="mt-4 divide-y divide-gray-100 rounded-lg border border-gray-200">
            <div
              v-for="module in applyResult.modules"
              :key="module.module_id"
              class="flex flex-col gap-3 px-3 py-3 sm:flex-row sm:items-start sm:justify-between"
            >
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-sm font-medium text-gray-900">
                    {{ moduleLabel(module.module_id, module.module_id) }}
                  </span>
                  <Badge variant="outline" :class="applyStatusClass(module.status)">
                    {{ applyStatusLabel(module.status) }}
                  </Badge>
                </div>
                <p v-if="module.messages.length" class="mt-1 text-sm text-gray-500">
                  {{ module.messages.join("; ") }}
                </p>
              </div>
              <div class="grid grid-cols-3 gap-2 sm:grid-cols-6">
                <div
                  v-for="[key, value] in summaryRows(module.summary)"
                  :key="key"
                  class="rounded-md bg-gray-50 px-2 py-1.5 text-center"
                >
                  <p class="text-[10px] font-medium uppercase tracking-wide text-gray-500">
                    {{ t(`portableConfigPage.summary.${key}`) }}
                  </p>
                  <p class="font-mono text-sm font-semibold text-gray-900">
                    {{ value }}
                  </p>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  </CrudPageLayout>
</template>
