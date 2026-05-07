import { computed, reactive, ref, shallowRef } from "vue";
import { useI18n } from "vue-i18n";

import * as systemConfigService from "@/services/systemConfig";
import type {
  SystemConfigField,
  SystemConfigLayerKind,
  SystemConfigPersistenceHealthItem,
  SystemConfigPersistenceHealthStatus,
  SystemConfigReport,
  SystemConfigReportSummary,
} from "@/services/types";
import type {
  BooleanFilterKey,
  ConfigViewMode,
  FieldBadge,
  FieldRow,
  SystemConfigSourceLayer,
  SystemConfigSummaryCard,
} from "../types";
import {
  SYSTEM_CONFIG_ALL_FILTER,
  collectSystemConfigSections,
  collectSystemConfigSourceKinds,
  countSystemConfigPersistenceIssues,
  createDefaultSystemConfigFilters,
  filterSystemConfigFields,
  formatSystemConfigDocument,
  formatSystemConfigValue,
  buildSystemConfigOverrideDocumentText,
  sortSystemConfigPersistenceHealthItems,
  type SystemConfigBooleanFilter,
  type SystemConfigFilters,
  type SystemConfigValueDisplay,
} from "./systemConfigState";

const SYSTEM_CONFIG_SOURCE_PRIORITY: SystemConfigLayerKind[] = [
  "program_default",
  "default_file",
  "user_file",
  "environment",
  "override_file",
  "derived",
];

interface UseSystemConfigReportOptions {
  afterReload?: () => void;
}

export function useSystemConfigReport(
  options: UseSystemConfigReportOptions = {},
) {
  const { t } = useI18n();

  const report = shallowRef<SystemConfigReport | null>(null);
  const isLoading = ref(false);
  const isReloading = ref(false);
  const errorMessage = ref<string | null>(null);
  const filters = reactive(createDefaultSystemConfigFilters());
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
  const sourceOptions = computed(() =>
    collectSystemConfigSourceKinds(fields.value),
  );
  const editableCount = computed(
    () => fields.value.filter((field) => field.editable).length,
  );
  const hotReloadableCount = computed(
    () => fields.value.filter((field) => field.hot_reloadable).length,
  );
  const overrideCount = computed(
    () =>
      fields.value.filter((field) => field.source.kind === "override_file")
        .length,
  );
  const governanceDisabled = computed(() => {
    const field = fields.value.find(
      (item) => item.path === "provider_governance.enabled",
    );
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

  const summaryCards = computed<SystemConfigSummaryCard[]>(() => [
    {
      key: "version",
      label: t("systemConfigPage.summary.version"),
      value: summary.value ? `v${summary.value.version}` : "-",
    },
    {
      key: "fields",
      label: t("systemConfigPage.summary.fields"),
      value: fields.value.length,
    },
    {
      key: "editable",
      label: t("systemConfigPage.summary.editable"),
      value: editableCount.value,
    },
    {
      key: "hotReloadable",
      label: t("systemConfigPage.summary.hotReloadable"),
      value: hotReloadableCount.value,
    },
    {
      key: "override",
      label: t("systemConfigPage.summary.overrideFields"),
      value: overrideCount.value,
    },
  ]);

  const booleanFilterOptions = computed(() => [
    { value: "all" as const, label: t("systemConfigPage.filters.all") },
    { value: "yes" as const, label: t("systemConfigPage.filters.yes") },
    { value: "no" as const, label: t("systemConfigPage.filters.no") },
  ]);

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
  const persistenceHealth = computed(
    () => report.value?.persistence_health ?? null,
  );
  const persistenceHealthItems = computed<SystemConfigPersistenceHealthItem[]>(
    () =>
      sortSystemConfigPersistenceHealthItems(
        persistenceHealth.value?.items ?? [],
      ),
  );
  const persistenceIssueCount = computed(() =>
    countSystemConfigPersistenceIssues(persistenceHealthItems.value),
  );
  const configDocumentPath = computed(() =>
    configViewMode.value === "override"
      ? report.value?.override_file.path
      : t("systemConfigPage.configView.effectivePath"),
  );
  const sourceLayers = computed<SystemConfigSourceLayer[]>(() => {
    return SYSTEM_CONFIG_SOURCE_PRIORITY.map((kind) => {
      const layerFields = fields.value.filter((field) => field.source.kind === kind);
      return {
        kind,
        count: layerFields.length,
        configured: layerFields.filter((field) => field.source.configured).length,
      };
    });
  });

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

  async function loadConfig(): Promise<void> {
    isLoading.value = true;
    errorMessage.value = null;
    try {
      report.value = await systemConfigService.getSystemConfig();
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isLoading.value = false;
    }
  }

  async function reloadOverride(): Promise<void> {
    if (isMultiInstance.value) {
      return;
    }
    isReloading.value = true;
    errorMessage.value = null;
    try {
      report.value = await systemConfigService.reloadSystemConfig();
      options.afterReload?.();
    } catch (err: unknown) {
      errorMessage.value = toErrorMessage(err);
    } finally {
      isReloading.value = false;
    }
  }

  function setReport(nextReport: SystemConfigReport): void {
    report.value = nextReport;
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

  function buildFieldBadges(field: SystemConfigField): FieldBadge[] {
    const badges: FieldBadge[] = [];
    if (field.source.kind === "override_file") {
      badges.push({
        key: "override",
        label: t("systemConfigPage.status.override"),
        class: "border-gray-300 bg-gray-900 text-white",
      });
    }
    if (field.source.kind === "environment") {
      badges.push({
        key: "environment",
        label: t("systemConfigPage.status.environment"),
        class: "border-gray-300 bg-gray-100 text-gray-800",
      });
    }
    if (field.hot_reloadable) {
      badges.push({
        key: "hot",
        label: t("systemConfigPage.status.hotReload"),
        class: "border-emerald-200 bg-emerald-50 text-emerald-700",
      });
    }
    if (field.restart_required) {
      badges.push({
        key: "restart",
        label: t("systemConfigPage.status.restartRequired"),
        class: "border-amber-200 bg-amber-50 text-amber-700",
      });
    }
    if (!field.editable) {
      badges.push({
        key: "readonly",
        label: t("systemConfigPage.status.readonly"),
        class: "border-gray-200 bg-white text-gray-500",
      });
    }
    if (field.sensitive) {
      badges.push({
        key: "sensitive",
        label: t("systemConfigPage.status.sensitive"),
        class: "border-red-200 bg-red-50 text-red-700",
      });
    }
    return badges;
  }

  return {
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
    editableCount,
    hotReloadableCount,
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
    currentFilters,
    loadConfig,
    reloadOverride,
    setReport,
    setSectionFilter,
    setSourceFilter,
    setBooleanFilter,
    resetFilters,
    buildFieldBadges,
  };
}

export function formatSystemConfigTimestamp(
  value: number | null | undefined,
): string {
  if (!value) {
    return "-";
  }
  return new Date(value).toLocaleString();
}

export function formatSystemConfigFileState(
  value: boolean | null | undefined,
  labels: { exists: string; missing: string },
): string {
  return value ? labels.exists : labels.missing;
}

export function sourceLayerOrder(): SystemConfigLayerKind[] {
  return [...SYSTEM_CONFIG_SOURCE_PRIORITY];
}

export function persistenceStatusClass(
  status: SystemConfigPersistenceHealthStatus,
): string {
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

export function persistenceRowClass(
  status: SystemConfigPersistenceHealthStatus,
): string {
  switch (status) {
    case "error":
      return "bg-red-50/50";
    case "warning":
      return "bg-amber-50/50";
    default:
      return "bg-white";
  }
}

export function valuePrimary(
  display: SystemConfigValueDisplay,
  labels: { redactedMissing: string; redactedConfigured: string },
): string {
  if (!display.redacted) {
    return display.text;
  }
  if (display.configured === false) {
    return labels.redactedMissing;
  }
  return display.text
    ? `${labels.redactedConfigured} · ${display.text}`
    : labels.redactedConfigured;
}

export function toErrorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

export function toSelectValue(value: unknown): string {
  return typeof value === "string" ? value : SYSTEM_CONFIG_ALL_FILTER;
}

function isBooleanFilter(value: string): value is SystemConfigBooleanFilter {
  return value === "all" || value === "yes" || value === "no";
}
