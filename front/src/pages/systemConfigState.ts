import type {
  JsonObject,
  JsonValue,
  SystemConfigChangeRequest,
  SystemConfigDiffItem,
  SystemConfigField,
  SystemConfigLayerKind,
  SystemConfigOverrideFileReport,
  SystemConfigPreviewResponse,
} from "@/store/types";

export const SYSTEM_CONFIG_ALL_FILTER = "__all__";

export type SystemConfigBooleanFilter = "all" | "yes" | "no";

export interface SystemConfigFilters {
  search: string;
  section: string;
  source: string;
  editable: SystemConfigBooleanFilter;
  hotReloadable: SystemConfigBooleanFilter;
  restartRequired: SystemConfigBooleanFilter;
  sensitive: SystemConfigBooleanFilter;
}

export interface SystemConfigValueDisplay {
  text: string;
  detail: string | null;
  redacted: boolean;
  configured: boolean | null;
}

export interface SystemConfigDiffDisplayItem {
  path: string;
  oldText: string;
  newText: string;
}

export interface SystemConfigPreviewApplyState {
  preview: SystemConfigPreviewResponse | null;
  previewPayload: SystemConfigChangeRequest | null;
  currentPayload: SystemConfigChangeRequest | null;
  reason: string;
  draftValidationError: string | null;
}

export function createDefaultSystemConfigFilters(): SystemConfigFilters {
  return {
    search: "",
    section: SYSTEM_CONFIG_ALL_FILTER,
    source: SYSTEM_CONFIG_ALL_FILTER,
    editable: "all",
    hotReloadable: "all",
    restartRequired: "all",
    sensitive: "all",
  };
}

export function filterSystemConfigFields(
  fields: SystemConfigField[],
  filters: SystemConfigFilters,
): SystemConfigField[] {
  const search = filters.search.trim().toLowerCase();

  return fields.filter((field) => {
    if (search) {
      const haystack = [
        field.path,
        field.section,
        field.description,
        field.source.source_name,
      ]
        .join(" ")
        .toLowerCase();
      if (!haystack.includes(search)) {
        return false;
      }
    }

    if (filters.section !== SYSTEM_CONFIG_ALL_FILTER && field.section !== filters.section) {
      return false;
    }

    if (filters.source !== SYSTEM_CONFIG_ALL_FILTER && field.source.kind !== filters.source) {
      return false;
    }

    return (
      matchesBooleanFilter(field.editable, filters.editable) &&
      matchesBooleanFilter(field.hot_reloadable, filters.hotReloadable) &&
      matchesBooleanFilter(field.restart_required, filters.restartRequired) &&
      matchesBooleanFilter(field.sensitive, filters.sensitive)
    );
  });
}

export function collectSystemConfigSections(fields: SystemConfigField[]): string[] {
  return collectSortedUnique(fields.map((field) => field.section));
}

export function collectSystemConfigSourceKinds(
  fields: SystemConfigField[],
): SystemConfigLayerKind[] {
  return collectSortedUnique(fields.map((field) => field.source.kind));
}

export function formatSystemConfigValue(field: SystemConfigField): SystemConfigValueDisplay {
  const redacted = readRedactedValue(field.value);
  if (redacted) {
    return redacted;
  }

  return {
    text: formatJsonValue(field.value),
    detail: null,
    redacted: false,
    configured: null,
  };
}

export function buildSystemConfigDiffDisplay(
  diff: SystemConfigDiffItem[],
): SystemConfigDiffDisplayItem[] {
  return diff.map((item) => ({
    path: item.path,
    oldText: formatJsonValue(item.old_value),
    newText: formatJsonValue(item.new_value),
  }));
}

export function buildSystemConfigHistoryDiffDisplay(
  diff: SystemConfigDiffItem[],
): SystemConfigDiffDisplayItem[] {
  return buildSystemConfigDiffDisplay(diff);
}

export function formatSystemConfigDocument(value: JsonValue): string {
  return formatJsonValue(value);
}

export function buildSystemConfigOverrideDocumentText(
  overrideFile: SystemConfigOverrideFileReport,
): string {
  if (overrideFile.invalid_paths.length) {
    return [
      "# Override file is invalid.",
      "# Invalid paths:",
      ...overrideFile.invalid_paths.map((path) => `# - ${path}`),
    ].join("\n");
  }
  return overrideFile.yaml || "{}";
}

export function systemConfigPayloadsMatch(
  left: SystemConfigChangeRequest | null,
  right: SystemConfigChangeRequest | null,
): boolean {
  if (!left || !right) {
    return false;
  }
  return canonicalizeSystemConfigChanges(left) === canonicalizeSystemConfigChanges(right);
}

export function canApplySystemConfigPreview(state: SystemConfigPreviewApplyState): boolean {
  return (
    !!state.preview &&
    state.preview.validation.valid &&
    state.preview.diff.length > 0 &&
    !state.preview.write_disabled_reason &&
    state.reason.trim().length > 0 &&
    !state.draftValidationError &&
    systemConfigPayloadsMatch(state.previewPayload, state.currentPayload)
  );
}

function matchesBooleanFilter(value: boolean, filter: SystemConfigBooleanFilter): boolean {
  if (filter === "all") {
    return true;
  }
  return filter === "yes" ? value : !value;
}

function collectSortedUnique<T extends string>(items: T[]): T[] {
  return Array.from(new Set(items)).sort((left, right) => left.localeCompare(right));
}

function canonicalizeSystemConfigChanges(payload: SystemConfigChangeRequest): string {
  return JSON.stringify(sortJsonValue(payload.changes));
}

function sortJsonValue(
  value: JsonValue | Record<string, JsonValue>,
): JsonValue | Record<string, JsonValue> {
  if (Array.isArray(value)) {
    return value.map((item) => sortJsonValue(item) as JsonValue);
  }
  if (isJsonObject(value)) {
    return Object.fromEntries(
      Object.keys(value)
        .sort((left, right) => left.localeCompare(right))
        .map((key) => [key, sortJsonValue(value[key]) as JsonValue]),
    );
  }
  return value;
}

function formatJsonValue(value: JsonValue): string {
  const redacted = readRedactedValue(value);
  if (redacted) {
    return redactedSummary(redacted);
  }

  if (typeof value === "string") {
    return value.length ? value : '""';
  }
  if (value === null || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(collapseRedactedValues(value), null, 2);
}

function readRedactedValue(value: JsonValue): SystemConfigValueDisplay | null {
  if (!isJsonObject(value) || value.redacted !== true) {
    return null;
  }

  const configured = typeof value.configured === "boolean" ? value.configured : null;
  const display = typeof value.display === "string" ? value.display : "";
  const sha256Prefix =
    typeof value.sha256_prefix === "string" ? value.sha256_prefix : null;
  const length = typeof value.length === "number" ? value.length : null;
  const detail = [
    sha256Prefix ? `sha256:${sha256Prefix}` : null,
    length !== null ? `${length} chars` : null,
  ]
    .filter((item): item is string => item !== null)
    .join(" · ");

  return {
    text: display,
    detail: detail || null,
    redacted: true,
    configured,
  };
}

function collapseRedactedValues(value: JsonValue): JsonValue {
  const redacted = readRedactedValue(value);
  if (redacted) {
    return redactedSummary(redacted);
  }
  if (Array.isArray(value)) {
    return value.map(collapseRedactedValues);
  }
  if (isJsonObject(value)) {
    return Object.fromEntries(
      Object.entries(value).map(([key, child]) => [key, collapseRedactedValues(child)]),
    );
  }
  return value;
}

function redactedSummary(display: SystemConfigValueDisplay): string {
  const base = display.configured === false ? "<redacted:not-configured>" : "<redacted>";
  return display.detail ? `${base} ${display.detail}` : base;
}

function isJsonObject(value: JsonValue): value is JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
