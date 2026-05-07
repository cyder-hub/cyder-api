<script setup lang="ts">
import { computed, onMounted, reactive, ref, shallowRef, watch } from "vue";
import { useI18n } from "vue-i18n";
import {
  AlertTriangle,
  Check,
  Clock3,
  FileText,
  HardDrive,
  Loader2,
  Pencil,
  RefreshCcw,
  RotateCcw,
  Search,
  Settings,
  SlidersHorizontal,
  X,
} from "lucide-vue-next";

import { Api } from "@/services/request";
import type {
  SystemConfigBooleanFilter,
  SystemConfigFilters,
  SystemConfigValueDisplay,
} from "@/pages/systemConfigState";
import {
  SYSTEM_CONFIG_ALL_FILTER,
  collectSystemConfigSections,
  collectSystemConfigSourceKinds,
  createDefaultSystemConfigFilters,
  filterSystemConfigFields,
  formatSystemConfigValue,
  buildSystemConfigDiffDisplay,
  buildSystemConfigHistoryDiffDisplay,
  formatSystemConfigDocument,
  buildSystemConfigOverrideDocumentText,
  canApplySystemConfigPreview,
  countSystemConfigPersistenceIssues,
  sortSystemConfigPersistenceHealthItems,
} from "@/pages/systemConfigState";
import type {
  JsonValue,
  SystemConfigChangeRequest,
  SystemConfigField,
  SystemConfigHistoryItem,
  SystemConfigPersistenceHealthItem,
  SystemConfigPersistenceHealthStatus,
  SystemConfigPreviewResponse,
  SystemConfigReport,
  SystemConfigReportSummary,
} from "@/store/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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

type BooleanFilterKey =
  | "editable"
  | "hotReloadable"
  | "restartRequired"
  | "sensitive";

interface FieldBadge {
  key: string;
  label: string;
  class: string;
}

interface FieldRow {
  field: SystemConfigField;
  value: SystemConfigValueDisplay;
  badges: FieldBadge[];
}

interface EditDraft {
  raw: string;
  boolValue: boolean;
  isNull: boolean;
}

type DraftBuildResult =
  | { ok: true; value: JsonValue }
  | { ok: false; message: string };

type ConfigViewMode = "effective" | "override";

const { t: $t } = useI18n();

const report = shallowRef<SystemConfigReport | null>(null);
const isLoading = ref(false);
const isReloading = ref(false);
const errorMessage = ref<string | null>(null);
const filters = reactive(createDefaultSystemConfigFilters());
const isEditOpen = ref(false);
const selectedField = shallowRef<SystemConfigField | null>(null);
const editDraft = reactive<EditDraft>({
  raw: "",
  boolValue: false,
  isNull: false,
});
const editReason = ref("");
const editError = ref<string | null>(null);
const isPreviewing = ref(false);
const isApplying = ref(false);
const preview = shallowRef<SystemConfigPreviewResponse | null>(null);
const previewPayload = shallowRef<SystemConfigChangeRequest | null>(null);
const isResetOpen = ref(false);
const resetTargets = shallowRef<SystemConfigField[]>([]);
const resetReason = ref("");
const resetError = ref<string | null>(null);
const isResetting = ref(false);
const historyItems = shallowRef<SystemConfigHistoryItem[]>([]);
const isHistoryLoading = ref(false);
const historyError = ref<string | null>(null);
const historyLimit = 20;
const historyOffset = ref(0);
const hasMoreHistory = ref(false);
const configViewMode = ref<ConfigViewMode>("effective");

const fields = computed<SystemConfigField[]>(() => report.value?.fields ?? []);
const summary = computed<SystemConfigReportSummary | null>(
  () => report.value?.summary ?? null,
);
const isMultiInstance = computed(
  () => summary.value?.deployment_mode === "multi_instance",
);
const filteredFields = computed<SystemConfigField[]>(() =>
  filterSystemConfigFields(fields.value, currentFilters()),
);
const rows = computed<FieldRow[]>(() =>
  filteredFields.value.map((field) => ({
    field,
    value: formatSystemConfigValue(field),
    badges: buildFieldBadges(field),
  })),
);
const sectionOptions = computed(() => collectSystemConfigSections(fields.value));
const sourceOptions = computed(() => collectSystemConfigSourceKinds(fields.value));
const editableCount = computed(
  () => fields.value.filter((field) => field.editable).length,
);
const hotReloadableCount = computed(
  () => fields.value.filter((field) => field.hot_reloadable).length,
);
const overrideCount = computed(
  () => fields.value.filter((field) => field.source.kind === "override_file").length,
);
const governanceDisabled = computed(() => {
  const field = fields.value.find((item) => item.path === "provider_governance.enabled");
  return field?.value === false;
});
const isFilterActive = computed(() => {
  const defaults = createDefaultSystemConfigFilters();
  return (
    filters.search.trim() !== defaults.search ||
    filters.section !== defaults.section ||
    filters.source !== defaults.source ||
    filters.editable !== defaults.editable ||
    filters.hotReloadable !== defaults.hotReloadable ||
    filters.restartRequired !== defaults.restartRequired ||
    filters.sensitive !== defaults.sensitive
  );
});

const summaryCards = computed(() => [
  {
    key: "version",
    label: $t("systemConfigPage.summary.version"),
    value: summary.value ? `v${summary.value.version}` : "-",
  },
  {
    key: "fields",
    label: $t("systemConfigPage.summary.fields"),
    value: fields.value.length,
  },
  {
    key: "editable",
    label: $t("systemConfigPage.summary.editable"),
    value: editableCount.value,
  },
  {
    key: "hotReloadable",
    label: $t("systemConfigPage.summary.hotReloadable"),
    value: hotReloadableCount.value,
  },
  {
    key: "override",
    label: $t("systemConfigPage.summary.overrideFields"),
    value: overrideCount.value,
  },
]);

const booleanFilterOptions = computed(() => [
  { value: "all" as const, label: $t("systemConfigPage.filters.all") },
  { value: "yes" as const, label: $t("systemConfigPage.filters.yes") },
  { value: "no" as const, label: $t("systemConfigPage.filters.no") },
]);
const previewDiffRows = computed(() =>
  buildSystemConfigDiffDisplay(preview.value?.diff ?? []),
);
const previewWarningRows = computed(() => preview.value?.validation.warnings ?? []);
const historyRows = computed(() =>
  historyItems.value.map((item) => ({
    item,
    diff: buildSystemConfigHistoryDiffDisplay(item.diff),
  })),
);
const configDocumentText = computed(() => {
  if (!report.value) {
    return "";
  }
  if (configViewMode.value === "override") {
    return buildSystemConfigOverrideDocumentText(report.value.override_file);
  }
  return formatSystemConfigDocument(report.value.effective);
});
const configDocumentInvalidPaths = computed(
  () => report.value?.override_file.invalid_paths ?? [],
);
const persistenceHealth = computed(() => report.value?.persistence_health ?? null);
const persistenceHealthItems = computed<SystemConfigPersistenceHealthItem[]>(() =>
  sortSystemConfigPersistenceHealthItems(persistenceHealth.value?.items ?? []),
);
const persistenceIssueCount = computed(() =>
  countSystemConfigPersistenceIssues(persistenceHealthItems.value),
);
const configDocumentPath = computed(() =>
  configViewMode.value === "override"
    ? report.value?.override_file.path
    : $t("systemConfigPage.configView.effectivePath"),
);
const runtimeActionLabels = computed(() => {
  const actions = preview.value?.runtime_actions;
  if (!actions) {
    return [];
  }
  const labels: string[] = [];
  if (actions.update_runtime_snapshot) {
    labels.push($t("systemConfigPage.preview.runtimeSnapshot"));
  }
  if (actions.update_log_level) {
    labels.push($t("systemConfigPage.preview.logLevel"));
  }
  if (actions.rebuild_http_client) {
    labels.push($t("systemConfigPage.preview.httpClient"));
  }
  if (actions.hot_reloadable_paths.length) {
    labels.push(
      $t("systemConfigPage.preview.hotPaths", {
        count: actions.hot_reloadable_paths.length,
      }),
    );
  }
  return labels;
});
const draftValidationError = computed(() => {
  if (!selectedField.value) {
    return null;
  }
  const result = buildDraftValue(selectedField.value);
  return result.ok ? null : result.message;
});
const currentChangePayload = computed<SystemConfigChangeRequest | null>(() =>
  buildValidChangePayload(),
);
const canApplyPreview = computed(() =>
  canApplySystemConfigPreview({
    preview: preview.value,
    previewPayload: previewPayload.value,
    currentPayload: currentChangePayload.value,
    reason: editReason.value,
    draftValidationError: draftValidationError.value,
  }),
);
const resetTargetPaths = computed<string[]>(() =>
  resetTargets.value.map((field) => field.path),
);

function currentFilters(): SystemConfigFilters {
  return {
    search: filters.search,
    section: filters.section,
    source: filters.source,
    editable: filters.editable,
    hotReloadable: filters.hotReloadable,
    restartRequired: filters.restartRequired,
    sensitive: filters.sensitive,
  };
}

function formatTimestamp(value: number | null | undefined): string {
  if (!value) {
    return "-";
  }
  return new Date(value).toLocaleString();
}

function formatFileState(value: boolean | null | undefined): string {
  return value ? $t("systemConfigPage.exists") : $t("systemConfigPage.missing");
}

function formatHealthBoolean(value: boolean): string {
  return value ? $t("systemConfigPage.persistence.yes") : $t("systemConfigPage.persistence.no");
}

function persistenceStatusLabel(status: SystemConfigPersistenceHealthStatus): string {
  return $t(`systemConfigPage.persistence.status.${status}`);
}

function persistenceItemLabel(item: SystemConfigPersistenceHealthItem): string {
  return $t(`systemConfigPage.persistence.items.${item.key}`);
}

function persistenceStatusClass(status: SystemConfigPersistenceHealthStatus): string {
  switch (status) {
    case "error":
      return "border-red-200 bg-red-50 text-red-700";
    case "warning":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "ok":
      return "border-gray-200 bg-gray-50 text-gray-700";
    case "skipped":
      return "border-gray-200 bg-white text-gray-500";
  }
}

function persistenceRowClass(status: SystemConfigPersistenceHealthStatus): string {
  switch (status) {
    case "error":
      return "bg-red-50/50";
    case "warning":
      return "bg-amber-50/50";
    default:
      return "bg-white";
  }
}

function sourceLabel(kind: string): string {
  return $t(`systemConfigPage.source.${kind}`);
}

function valuePrimary(display: SystemConfigValueDisplay): string {
  if (!display.redacted) {
    return display.text;
  }
  if (display.configured === false) {
    return $t("systemConfigPage.redactedMissing");
  }
  const configured = $t("systemConfigPage.redactedConfigured");
  return display.text ? `${configured} · ${display.text}` : configured;
}

function toErrorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

function toSelectValue(value: unknown): string {
  return typeof value === "string" ? value : SYSTEM_CONFIG_ALL_FILTER;
}

function isBooleanFilter(value: string): value is SystemConfigBooleanFilter {
  return value === "all" || value === "yes" || value === "no";
}

function setSectionFilter(value: unknown): void {
  filters.section = toSelectValue(value);
}

function setSourceFilter(value: unknown): void {
  filters.source = toSelectValue(value);
}

function setBooleanFilter(key: BooleanFilterKey, value: unknown): void {
  const next = toSelectValue(value);
  if (isBooleanFilter(next)) {
    filters[key] = next;
  }
}

function resetFilters(): void {
  Object.assign(filters, createDefaultSystemConfigFilters());
}

function canEditField(field: SystemConfigField): boolean {
  return field.editable && field.hot_reloadable && !isMultiInstance.value;
}

function enumOptionsForField(field: SystemConfigField): string[] {
  if (field.path === "log_level") {
    return ["trace", "debug", "info", "warn", "error"];
  }
  if (field.path === "deployment.mode") {
    return ["single_instance", "multi_instance"];
  }
  const firstConstraint = field.constraints.find((constraint) =>
    constraint.includes(" or "),
  );
  if (!firstConstraint) {
    return [];
  }
  return firstConstraint
    .split(/\s+or\s+|,\s*/)
    .map((item) => item.trim())
    .filter((item) => /^[a-z0-9_]+$/i.test(item));
}

function openEditDialog(field: SystemConfigField): void {
  if (!canEditField(field)) {
    return;
  }
  selectedField.value = field;
  const value = field.value;
  editDraft.isNull = value === null;
  editDraft.boolValue = typeof value === "boolean" ? value : false;
  editDraft.raw = value === null ? "" : valueToDraftString(value);
  editReason.value = "";
  editError.value = null;
  preview.value = null;
  previewPayload.value = null;
  isEditOpen.value = true;
}

function handleEditOpenChange(open: boolean): void {
  isEditOpen.value = open;
  if (!open) {
    selectedField.value = null;
    editError.value = null;
    preview.value = null;
    previewPayload.value = null;
  }
}

function openSingleResetDialog(field: SystemConfigField): void {
  if (!canEditField(field)) {
    return;
  }
  resetTargets.value = [field];
  resetReason.value = "";
  resetError.value = null;
  isResetOpen.value = true;
}

function openAllOverridesResetDialog(): void {
  if (isMultiInstance.value) {
    return;
  }
  const targets = fields.value.filter(
    (field) => field.editable && field.source.kind === "override_file",
  );
  if (!targets.length) {
    return;
  }
  resetTargets.value = targets;
  resetReason.value = "";
  resetError.value = null;
  isResetOpen.value = true;
}

function handleResetOpenChange(open: boolean): void {
  isResetOpen.value = open;
  if (!open) {
    resetTargets.value = [];
    resetError.value = null;
  }
}

function valueToDraftString(value: JsonValue): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value, null, 2);
}

function buildDraftValue(field: SystemConfigField): DraftBuildResult {
  if (field.value_kind === "bool") {
    return { ok: true, value: editDraft.boolValue };
  }
  if (field.value_kind === "nullable_string") {
    return { ok: true, value: editDraft.isNull ? null : editDraft.raw };
  }
  if (field.value_kind === "string" || field.value_kind === "enum") {
    return { ok: true, value: editDraft.raw };
  }
  if (field.value_kind === "nullable_u64") {
    if (editDraft.isNull) {
      return { ok: true, value: null };
    }
    return parseUnsignedInteger(field.value_kind);
  }
  if (
    field.value_kind === "u16" ||
    field.value_kind === "u32" ||
    field.value_kind === "u64" ||
    field.value_kind === "usize"
  ) {
    return parseUnsignedInteger(field.value_kind);
  }
  try {
    return { ok: true, value: JSON.parse(editDraft.raw) as JsonValue };
  } catch {
    return {
      ok: false,
      message: $t("systemConfigPage.edit.invalidJson"),
    };
  }
}

function parseUnsignedInteger(kind: SystemConfigField["value_kind"]): DraftBuildResult {
  const trimmed = editDraft.raw.trim();
  if (!/^\d+$/.test(trimmed)) {
    return {
      ok: false,
      message: $t("systemConfigPage.edit.invalidNumber"),
    };
  }
  const value = Number(trimmed);
  if (!Number.isSafeInteger(value)) {
    return {
      ok: false,
      message: $t("systemConfigPage.edit.invalidNumber"),
    };
  }
  if (kind === "u16" && value > 65535) {
    return {
      ok: false,
      message: $t("systemConfigPage.edit.invalidU16"),
    };
  }
  if (kind === "u32" && value > 4294967295) {
    return {
      ok: false,
      message: $t("systemConfigPage.edit.invalidU32"),
    };
  }
  return { ok: true, value };
}

function clearEditPreview(): void {
  preview.value = null;
  previewPayload.value = null;
}

function buildValidChangePayload(): SystemConfigChangeRequest | null {
  const field = selectedField.value;
  if (!field) {
    return null;
  }
  const result = buildDraftValue(field);
  if (!result.ok) {
    return null;
  }
  return {
    changes: {
      [field.path]: result.value,
    },
  };
}

function buildChangePayload(): SystemConfigChangeRequest | null {
  const payload = buildValidChangePayload();
  if (payload) {
    return payload;
  }
  if (selectedField.value) {
    const result = buildDraftValue(selectedField.value);
    if (!result.ok) {
      editError.value = result.message;
    }
  }
  return null;
}

async function loadConfig(): Promise<void> {
  isLoading.value = true;
  errorMessage.value = null;
  try {
    report.value = await Api.getSystemConfig();
  } catch (err: unknown) {
    errorMessage.value = toErrorMessage(err);
  } finally {
    isLoading.value = false;
  }
}

async function loadHistory(reset = false): Promise<void> {
  if (reset) {
    historyOffset.value = 0;
    historyItems.value = [];
  }
  isHistoryLoading.value = true;
  historyError.value = null;
  try {
    const items = await Api.getSystemConfigHistory({
      limit: historyLimit,
      offset: historyOffset.value,
    });
    historyItems.value = reset ? items : [...historyItems.value, ...items];
    historyOffset.value += items.length;
    hasMoreHistory.value = items.length === historyLimit;
  } catch (err: unknown) {
    historyError.value = toErrorMessage(err);
  } finally {
    isHistoryLoading.value = false;
  }
}

async function reloadOverride(): Promise<void> {
  if (isMultiInstance.value) {
    return;
  }
  isReloading.value = true;
  errorMessage.value = null;
  try {
    report.value = await Api.reloadSystemConfig();
    void loadHistory(true);
  } catch (err: unknown) {
    errorMessage.value = toErrorMessage(err);
  } finally {
    isReloading.value = false;
  }
}

async function previewEdit(): Promise<void> {
  const payload = buildChangePayload();
  if (!payload) {
    return;
  }
  isPreviewing.value = true;
  editError.value = null;
  preview.value = null;
  previewPayload.value = null;
  try {
    const response = await Api.previewSystemConfig(payload);
    preview.value = response;
    previewPayload.value = payload;
  } catch (err: unknown) {
    editError.value = toErrorMessage(err);
  } finally {
    isPreviewing.value = false;
  }
}

async function applyEdit(): Promise<void> {
  const payload = buildChangePayload();
  if (!payload || !selectedField.value || !canApplyPreview.value) {
    return;
  }
  isApplying.value = true;
  editError.value = null;
  try {
    report.value = await Api.applySystemConfig({
      ...payload,
      reason: editReason.value.trim(),
    });
    void loadHistory(true);
    handleEditOpenChange(false);
  } catch (err: unknown) {
    editError.value = toErrorMessage(err);
  } finally {
    isApplying.value = false;
  }
}

async function resetSelectedFields(): Promise<void> {
  const reason = resetReason.value.trim();
  if (!resetTargetPaths.value.length || !reason) {
    return;
  }
  isResetting.value = true;
  resetError.value = null;
  try {
    report.value = await Api.resetSystemConfig({
      paths: resetTargetPaths.value,
      reason,
    });
    void loadHistory(true);
    handleResetOpenChange(false);
  } catch (err: unknown) {
    resetError.value = toErrorMessage(err);
  } finally {
    isResetting.value = false;
  }
}

function buildFieldBadges(field: SystemConfigField): FieldBadge[] {
  const badges: FieldBadge[] = [];
  if (field.source.kind === "override_file") {
    badges.push({
      key: "override",
      label: $t("systemConfigPage.status.override"),
      class: "border-gray-300 bg-gray-900 text-white",
    });
  }
  if (field.source.kind === "environment") {
    badges.push({
      key: "environment",
      label: $t("systemConfigPage.status.environment"),
      class: "border-gray-300 bg-gray-100 text-gray-800",
    });
  }
  if (field.hot_reloadable) {
    badges.push({
      key: "hot",
      label: $t("systemConfigPage.status.hotReload"),
      class: "border-emerald-200 bg-emerald-50 text-emerald-700",
    });
  }
  if (field.restart_required) {
    badges.push({
      key: "restart",
      label: $t("systemConfigPage.status.restartRequired"),
      class: "border-amber-200 bg-amber-50 text-amber-700",
    });
  }
  if (!field.editable) {
    badges.push({
      key: "readonly",
      label: $t("systemConfigPage.status.readonly"),
      class: "border-gray-200 bg-white text-gray-500",
    });
  }
  if (field.sensitive) {
    badges.push({
      key: "sensitive",
      label: $t("systemConfigPage.status.sensitive"),
      class: "border-red-200 bg-red-50 text-red-700",
    });
  }
  return badges;
}

onMounted(() => {
  void loadConfig();
  void loadHistory(true);
});

watch(
  () => [
    selectedField.value?.path ?? "",
    editDraft.raw,
    editDraft.boolValue,
    editDraft.isNull,
  ],
  () => {
    if (preview.value || previewPayload.value) {
      clearEditPreview();
    }
  },
);

function historyOperationLabel(operation: SystemConfigHistoryItem["operation"]): string {
  return $t(`systemConfigPage.history.operation.${operation}`);
}

function writeDisabledReasonLabel(reason: string): string {
  return $t(`systemConfigPage.preview.writeDisabled.${reason}`);
}
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold tracking-tight text-gray-900 sm:text-xl">
            {{ $t("systemConfigPage.title") }}
          </h1>
          <p class="mt-1 text-sm text-gray-500">
            {{ $t("systemConfigPage.description") }}
          </p>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
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
        </div>
      </div>

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
        <div class="grid grid-cols-2 gap-px overflow-hidden rounded-xl border border-gray-200 bg-gray-100 md:grid-cols-5">
          <div v-for="card in summaryCards" :key="card.key" class="bg-white px-4 py-3">
            <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
              {{ card.label }}
            </p>
            <p class="mt-1 font-mono text-lg font-semibold tracking-tight text-gray-900">
              {{ card.value }}
            </p>
          </div>
        </div>

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
                  {{ formatTimestamp(summary?.loaded_at) }}
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
          <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-3 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <HardDrive class="h-4 w-4 text-gray-400" />
                <h2 class="text-base font-semibold text-gray-900">
                  {{ $t("systemConfigPage.persistence.title") }}
                </h2>
              </div>
              <p class="mt-1 text-sm text-gray-500">
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

        <div class="rounded-xl border border-gray-200 bg-white">
          <div class="flex flex-col gap-3 border-b border-gray-100 px-4 py-3 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <Clock3 class="h-4 w-4 text-gray-400" />
                <h2 class="text-base font-semibold text-gray-900">
                  {{ $t("systemConfigPage.history.title") }}
                </h2>
              </div>
              <p class="mt-1 text-sm text-gray-500">
                {{ $t("systemConfigPage.history.description") }}
              </p>
            </div>
            <Button
              variant="outline"
              class="w-full sm:w-auto"
              :disabled="isHistoryLoading"
              @click="loadHistory(true)"
            >
              <RefreshCcw class="mr-1.5 h-4 w-4" :class="{ 'animate-spin': isHistoryLoading }" />
              {{ $t("systemConfigPage.refresh") }}
            </Button>
          </div>

          <div
            v-if="historyError"
            class="border-b border-red-100 bg-red-50 px-4 py-3 text-sm text-red-600"
          >
            {{ historyError }}
          </div>

          <div v-if="historyRows.length" class="divide-y divide-gray-100">
            <article v-for="row in historyRows" :key="`${row.item.changed_at}-${row.item.version_after}`" class="px-4 py-4">
              <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-2">
                    <Badge variant="outline" class="font-mono text-xs">
                      {{ historyOperationLabel(row.item.operation) }}
                    </Badge>
                    <span class="font-mono text-xs text-gray-500">
                      v{{ row.item.version_before }} -> v{{ row.item.version_after }}
                    </span>
                  </div>
                  <p class="mt-2 break-words text-sm text-gray-700">
                    {{ row.item.reason || $t("systemConfigPage.history.noReason") }}
                  </p>
                </div>
                <p class="font-mono text-xs text-gray-500">
                  {{ formatTimestamp(row.item.changed_at) }}
                </p>
              </div>
              <div class="mt-3 flex flex-wrap gap-1.5">
                <Badge
                  v-for="path in row.item.changed_paths"
                  :key="`${row.item.changed_at}-${path}`"
                  variant="outline"
                  class="max-w-full break-all font-mono text-xs text-gray-500"
                >
                  {{ path }}
                </Badge>
              </div>
              <div v-if="row.diff.length" class="mt-3 overflow-hidden rounded-lg border border-gray-200">
                <div class="grid grid-cols-1 divide-y divide-gray-100 md:grid-cols-3 md:divide-x md:divide-y-0">
                  <div v-for="diff in row.diff" :key="`${row.item.changed_at}-${diff.path}`" class="contents">
                    <div class="px-3 py-2 font-mono text-xs font-medium text-gray-900">
                      {{ diff.path }}
                    </div>
                    <pre class="max-h-24 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-500">{{ diff.oldText }}</pre>
                    <pre class="max-h-24 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-900">{{ diff.newText }}</pre>
                  </div>
                </div>
              </div>
            </article>
          </div>
          <div v-else class="px-4 py-8 text-center text-sm text-gray-500">
            {{ isHistoryLoading ? $t("systemConfigPage.history.loading") : $t("systemConfigPage.history.empty") }}
          </div>
          <div class="border-t border-gray-100 px-4 py-3">
            <Button
              variant="outline"
              class="w-full"
              :disabled="isHistoryLoading || !hasMoreHistory"
              @click="loadHistory(false)"
            >
              <Loader2 v-if="isHistoryLoading" class="mr-1.5 h-4 w-4 animate-spin" />
              {{ hasMoreHistory ? $t("systemConfigPage.history.loadMore") : $t("systemConfigPage.history.noMore") }}
            </Button>
          </div>
        </div>

        <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
          <div class="flex flex-col gap-3 border-b border-gray-100 pb-4 sm:flex-row sm:items-start sm:justify-between">
            <div class="min-w-0">
              <div class="flex items-center gap-2">
                <SlidersHorizontal class="h-4 w-4 text-gray-400" />
                <h2 class="text-base font-semibold text-gray-900">
                  {{ $t("systemConfigPage.filters.title") }}
                </h2>
              </div>
              <p class="mt-1 text-sm text-gray-500">
                {{
                  $t("systemConfigPage.filters.activeSummary", {
                    shown: rows.length,
                    total: fields.length,
                  })
                }}
              </p>
            </div>
            <Button
              variant="outline"
              class="w-full sm:w-auto"
              :disabled="!isFilterActive"
              @click="resetFilters"
            >
              <X class="mr-1.5 h-4 w-4" />
              {{ $t("systemConfigPage.resetFilters") }}
            </Button>
          </div>

          <div class="mt-4 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
            <div class="md:col-span-2">
              <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("systemConfigPage.filters.search") }}
              </span>
              <div class="relative">
                <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
                <Input
                  v-model="filters.search"
                  class="w-full pl-9"
                  :placeholder="$t('systemConfigPage.filters.searchPlaceholder')"
                />
              </div>
            </div>

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
                    <pre class="max-h-28 whitespace-pre-wrap break-all font-mono text-xs text-gray-700">{{ valuePrimary(row.value) }}</pre>
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
            <pre class="mt-3 max-h-32 overflow-auto whitespace-pre-wrap break-all rounded-md bg-gray-50 px-3 py-2 font-mono text-xs text-gray-700">{{ valuePrimary(row.value) }}</pre>
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

    <Dialog :open="isEditOpen" @update:open="handleEditOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-4xl">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{ $t("systemConfigPage.edit.title") }}
          </DialogTitle>
          <DialogDescription v-if="selectedField" class="break-all font-mono text-xs text-gray-500">
            {{ selectedField.path }}
          </DialogDescription>
        </DialogHeader>

        <div v-if="selectedField" class="space-y-5 overflow-y-auto px-4 py-4 sm:px-6">
          <div
            v-if="editError"
            class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600"
          >
            {{ editError }}
          </div>

          <div class="rounded-lg border border-gray-200 bg-gray-50/60 p-3">
            <p class="text-sm text-gray-700">{{ selectedField.description }}</p>
            <div class="mt-2 flex flex-wrap gap-1.5">
              <Badge
                v-for="badge in buildFieldBadges(selectedField)"
                :key="`dialog-${selectedField.path}-${badge.key}`"
                variant="outline"
                :class="badge.class"
              >
                {{ badge.label }}
              </Badge>
            </div>
          </div>

          <div class="space-y-2">
            <Label>{{ $t("systemConfigPage.edit.value") }}</Label>

            <label
              v-if="selectedField.value_kind === 'bool'"
              class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5"
            >
              <span class="text-sm font-medium text-gray-700">
                {{ $t("systemConfigPage.edit.booleanValue") }}
              </span>
              <Checkbox v-model="editDraft.boolValue" />
            </label>

            <Select
              v-else-if="enumOptionsForField(selectedField).length"
              :model-value="editDraft.raw"
              @update:model-value="(value) => (editDraft.raw = toSelectValue(value))"
            >
              <SelectTrigger class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent :body-lock="false">
                <SelectItem
                  v-for="option in enumOptionsForField(selectedField)"
                  :key="option"
                  :value="option"
                >
                  {{ option }}
                </SelectItem>
              </SelectContent>
            </Select>

            <div
              v-else-if="
                selectedField.value_kind === 'nullable_string' ||
                selectedField.value_kind === 'nullable_u64'
              "
              class="space-y-3"
            >
              <label class="flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5">
                <span class="text-sm font-medium text-gray-700">
                  {{ $t("systemConfigPage.edit.setNull") }}
                </span>
                <Checkbox v-model="editDraft.isNull" />
              </label>
              <Input
                v-model="editDraft.raw"
                :disabled="editDraft.isNull"
                :inputmode="selectedField.value_kind === 'nullable_u64' ? 'numeric' : 'text'"
              />
            </div>

            <Input
              v-else-if="
                selectedField.value_kind === 'u16' ||
                selectedField.value_kind === 'u32' ||
                selectedField.value_kind === 'u64' ||
                selectedField.value_kind === 'usize'
              "
              v-model="editDraft.raw"
              inputmode="numeric"
            />

            <Input
              v-else-if="selectedField.value_kind === 'string'"
              v-model="editDraft.raw"
            />

            <textarea
              v-else
              v-model="editDraft.raw"
              class="min-h-32 w-full rounded-md border border-gray-200 bg-white px-3 py-2 font-mono text-sm text-gray-900 outline-none focus:border-gray-400"
            />

            <p v-if="draftValidationError" class="text-sm text-red-600">
              {{ draftValidationError }}
            </p>
          </div>

          <div class="space-y-2">
            <Label for="system-config-edit-reason">
              {{ $t("systemConfigPage.edit.reason") }}
            </Label>
            <Input
              id="system-config-edit-reason"
              v-model="editReason"
              :placeholder="$t('systemConfigPage.edit.reasonPlaceholder')"
            />
          </div>

          <div v-if="preview" class="space-y-4 rounded-lg border border-gray-200 bg-white p-4">
            <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
              <div>
                <h3 class="text-base font-semibold text-gray-900">
                  {{ $t("systemConfigPage.preview.title") }}
                </h3>
                <p class="mt-1 text-sm text-gray-500">
                  {{
                    $t("systemConfigPage.preview.diffSummary", {
                      count: preview.diff.length,
                    })
                  }}
                </p>
              </div>
              <Badge
                variant="outline"
                :class="
                  preview.validation.valid
                    ? 'border-emerald-200 bg-emerald-50 text-emerald-700'
                    : 'border-red-200 bg-red-50 text-red-700'
                "
              >
                {{
                  preview.validation.valid
                    ? $t("systemConfigPage.preview.valid")
                    : $t("systemConfigPage.preview.invalid")
                }}
              </Badge>
            </div>

            <div v-if="preview.validation.errors.length" class="space-y-1">
              <p
                v-for="issue in preview.validation.errors"
                :key="`${issue.path}-${issue.code}`"
                class="break-words text-sm text-red-600"
              >
                {{ issue.path }}: {{ issue.message }}
              </p>
            </div>

            <div v-if="previewWarningRows.length || preview.write_disabled_reason" class="space-y-1 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2">
              <p
                v-if="preview.write_disabled_reason"
                class="break-words text-sm text-amber-800"
              >
                {{ writeDisabledReasonLabel(preview.write_disabled_reason) }}
              </p>
              <p
                v-for="issue in previewWarningRows"
                :key="`warning-${issue.path}-${issue.code}`"
                class="break-words text-sm text-amber-800"
              >
                {{ issue.path }}: {{ issue.message }}
              </p>
            </div>

            <div v-if="runtimeActionLabels.length" class="flex flex-wrap gap-1.5">
              <Badge
                v-for="label in runtimeActionLabels"
                :key="label"
                variant="outline"
                class="border-gray-200 bg-gray-50 text-gray-700"
              >
                {{ label }}
              </Badge>
            </div>

            <div v-if="previewDiffRows.length" class="overflow-hidden rounded-lg border border-gray-200">
              <div class="grid grid-cols-1 divide-y divide-gray-100 md:grid-cols-3 md:divide-x md:divide-y-0">
                <div
                  v-for="diff in previewDiffRows"
                  :key="diff.path"
                  class="contents"
                >
                  <div class="px-3 py-2 font-mono text-xs font-medium text-gray-900">
                    {{ diff.path }}
                  </div>
                  <pre class="max-h-28 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-500">{{ diff.oldText }}</pre>
                  <pre class="max-h-28 overflow-auto whitespace-pre-wrap break-all px-3 py-2 font-mono text-xs text-gray-900">{{ diff.newText }}</pre>
                </div>
              </div>
            </div>
            <p v-else class="text-sm text-gray-500">
              {{ $t("systemConfigPage.preview.noChanges") }}
            </p>

            <div>
              <Label>{{ $t("systemConfigPage.preview.nextOverride") }}</Label>
              <pre class="mt-2 max-h-56 overflow-auto rounded-md bg-gray-950 px-3 py-2 font-mono text-xs text-gray-100">{{ preview.next_override_yaml }}</pre>
            </div>
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
          <Button variant="ghost" class="text-gray-600" @click="handleEditOpenChange(false)">
            {{ $t("common.cancel") }}
          </Button>
          <Button
            variant="outline"
            :disabled="isPreviewing || isApplying || !!draftValidationError"
            @click="previewEdit"
          >
            <Loader2 v-if="isPreviewing" class="mr-1.5 h-4 w-4 animate-spin" />
            {{ $t("systemConfigPage.actions.preview") }}
          </Button>
          <Button :disabled="isApplying || !canApplyPreview" @click="applyEdit">
            <Check v-if="!isApplying" class="mr-1.5 h-4 w-4" />
            <Loader2 v-else class="mr-1.5 h-4 w-4 animate-spin" />
            {{ $t("systemConfigPage.actions.apply") }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>

    <Dialog :open="isResetOpen" @update:open="handleResetOpenChange">
      <DialogContent class="flex max-h-[92dvh] flex-col border border-gray-200 bg-white p-0 sm:max-w-lg">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
          <DialogTitle class="text-lg font-semibold text-gray-900">
            {{ $t("systemConfigPage.reset.title") }}
          </DialogTitle>
          <DialogDescription class="text-sm text-gray-500">
            {{ $t("systemConfigPage.reset.description") }}
          </DialogDescription>
        </DialogHeader>

        <div class="space-y-4 overflow-y-auto px-4 py-4 sm:px-6">
          <div
            v-if="resetError"
            class="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-600"
          >
            {{ resetError }}
          </div>

          <div class="rounded-lg border border-gray-200 bg-gray-50/60 p-3">
            <p class="text-xs font-medium uppercase tracking-wide text-gray-500">
              {{ $t("systemConfigPage.reset.paths") }}
            </p>
            <div class="mt-2 flex max-h-40 flex-wrap gap-1.5 overflow-y-auto">
              <Badge
                v-for="path in resetTargetPaths"
                :key="path"
                variant="outline"
                class="font-mono text-xs"
              >
                {{ path }}
              </Badge>
            </div>
          </div>

          <div class="space-y-2">
            <Label for="system-config-reset-reason">
              {{ $t("systemConfigPage.reset.reason") }}
            </Label>
            <Input
              id="system-config-reset-reason"
              v-model="resetReason"
              :placeholder="$t('systemConfigPage.reset.reasonPlaceholder')"
            />
          </div>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6">
          <Button variant="ghost" class="text-gray-600" @click="handleResetOpenChange(false)">
            {{ $t("common.cancel") }}
          </Button>
          <Button
            :disabled="isResetting || !resetReason.trim() || !resetTargetPaths.length"
            @click="resetSelectedFields"
          >
            <Loader2 v-if="isResetting" class="mr-1.5 h-4 w-4 animate-spin" />
            <X v-else class="mr-1.5 h-4 w-4" />
            {{ $t("systemConfigPage.actions.reset") }}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  </div>
</template>
