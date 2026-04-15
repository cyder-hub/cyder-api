import type {
  ProviderRuntimeListParams,
  ProviderRuntimeSortField,
  ProviderRuntimeStatusFilter,
  ProviderRuntimeWindow,
  SortDirection,
} from "./types";

export type ProviderRuntimeFilters = Required<ProviderRuntimeListParams>;

export const DEFAULT_PROVIDER_RUNTIME_FILTERS: ProviderRuntimeFilters = {
  window: "1h",
  status: "all",
  search: "",
  sort: "health",
  direction: "desc",
  only_enabled: true,
};

const VALID_WINDOWS: ProviderRuntimeWindow[] = ["15m", "1h", "6h", "24h"];
const VALID_STATUS_FILTERS: ProviderRuntimeStatusFilter[] = [
  "all",
  "healthy",
  "degraded",
  "open",
  "half_open",
  "no_traffic",
];
const VALID_SORT_FIELDS: ProviderRuntimeSortField[] = [
  "health",
  "error_rate",
  "latency",
  "last_error_at",
  "request_count",
];
const VALID_DIRECTIONS: SortDirection[] = ["asc", "desc"];

export function parseProviderRuntimeBoolean(
  value: unknown,
  fallback: boolean,
): boolean {
  if (typeof value === "boolean") {
    return value;
  }
  if (typeof value !== "string") {
    return fallback;
  }

  const normalized = value.trim().toLowerCase();
  if (normalized === "true" || normalized === "1") {
    return true;
  }
  if (normalized === "false" || normalized === "0") {
    return false;
  }
  return fallback;
}

export function getProviderRuntimeSingleQueryValue(
  value: string | null | Array<string | null> | undefined,
): string | undefined {
  if (Array.isArray(value)) {
    return value[0] ?? undefined;
  }
  return value ?? undefined;
}

export function normalizeProviderRuntimeWindow(value: unknown): ProviderRuntimeWindow {
  return typeof value === "string" && VALID_WINDOWS.includes(value as ProviderRuntimeWindow)
    ? (value as ProviderRuntimeWindow)
    : DEFAULT_PROVIDER_RUNTIME_FILTERS.window;
}

export function normalizeProviderRuntimeStatusFilter(
  value: unknown,
): ProviderRuntimeStatusFilter {
  return typeof value === "string" &&
      VALID_STATUS_FILTERS.includes(value as ProviderRuntimeStatusFilter)
    ? (value as ProviderRuntimeStatusFilter)
    : DEFAULT_PROVIDER_RUNTIME_FILTERS.status;
}

export function normalizeProviderRuntimeSortField(
  value: unknown,
): ProviderRuntimeSortField {
  return typeof value === "string" &&
      VALID_SORT_FIELDS.includes(value as ProviderRuntimeSortField)
    ? (value as ProviderRuntimeSortField)
    : DEFAULT_PROVIDER_RUNTIME_FILTERS.sort;
}

export function normalizeProviderRuntimeDirection(value: unknown): SortDirection {
  return typeof value === "string" && VALID_DIRECTIONS.includes(value as SortDirection)
    ? (value as SortDirection)
    : DEFAULT_PROVIDER_RUNTIME_FILTERS.direction;
}

export function normalizeProviderRuntimeSearch(value: unknown): string {
  return typeof value === "string" ? value.trim() : DEFAULT_PROVIDER_RUNTIME_FILTERS.search;
}

export function buildProviderRuntimeApiParamsFromFilters(
  filters: ProviderRuntimeFilters,
): ProviderRuntimeListParams {
  return {
    window: filters.window,
    status: filters.status,
    search: filters.search || undefined,
    sort: filters.sort,
    direction: filters.direction,
    only_enabled: filters.only_enabled,
  };
}

export function buildProviderRuntimeRouteQueryFromFilters(
  filters: ProviderRuntimeFilters,
): Record<string, string> {
  const query: Record<string, string> = {};

  if (filters.window !== DEFAULT_PROVIDER_RUNTIME_FILTERS.window) {
    query.window = filters.window;
  }
  if (filters.status !== DEFAULT_PROVIDER_RUNTIME_FILTERS.status) {
    query.status = filters.status;
  }
  if (filters.search) {
    query.search = filters.search;
  }
  if (filters.sort !== DEFAULT_PROVIDER_RUNTIME_FILTERS.sort) {
    query.sort = filters.sort;
  }
  if (filters.direction !== DEFAULT_PROVIDER_RUNTIME_FILTERS.direction) {
    query.direction = filters.direction;
  }
  if (filters.only_enabled !== DEFAULT_PROVIDER_RUNTIME_FILTERS.only_enabled) {
    query.only_enabled = String(filters.only_enabled);
  }

  return query;
}

export function buildProviderRuntimeFiltersFromQuery(
  query: Record<string, string | null | Array<string | null> | undefined>,
): ProviderRuntimeFilters {
  return {
    window: normalizeProviderRuntimeWindow(
      getProviderRuntimeSingleQueryValue(query.window),
    ),
    status: normalizeProviderRuntimeStatusFilter(
      getProviderRuntimeSingleQueryValue(query.status),
    ),
    search: normalizeProviderRuntimeSearch(
      getProviderRuntimeSingleQueryValue(query.search),
    ),
    sort: normalizeProviderRuntimeSortField(
      getProviderRuntimeSingleQueryValue(query.sort),
    ),
    direction: normalizeProviderRuntimeDirection(
      getProviderRuntimeSingleQueryValue(query.direction),
    ),
    only_enabled: parseProviderRuntimeBoolean(
      getProviderRuntimeSingleQueryValue(query.only_enabled),
      DEFAULT_PROVIDER_RUNTIME_FILTERS.only_enabled,
    ),
  };
}
