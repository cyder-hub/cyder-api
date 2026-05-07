import { computed, ref, type Ref } from "vue";
import type { LocationQuery, LocationQueryRaw, Router } from "vue-router";
import type {
  ProviderRuntimeListParams,
  ProviderRuntimeSortField,
  ProviderRuntimeStatusFilter,
  ProviderRuntimeWindow,
  SortDirection,
} from "@/services/types";
import type { ProviderRuntimeFilters } from "../types";

export type ProviderRuntimeQueryValue =
  | string
  | null
  | Array<string | null>
  | undefined;

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
  value: ProviderRuntimeQueryValue,
): string | undefined {
  if (Array.isArray(value)) {
    return value[0] ?? undefined;
  }
  return value ?? undefined;
}

export function normalizeProviderRuntimeWindow(
  value: unknown,
): ProviderRuntimeWindow {
  return typeof value === "string" &&
    VALID_WINDOWS.includes(value as ProviderRuntimeWindow)
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
  return typeof value === "string"
    ? value.trim()
    : DEFAULT_PROVIDER_RUNTIME_FILTERS.search;
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
  query: Record<string, ProviderRuntimeQueryValue>,
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

export function isProviderRuntimeDefaultFilters(filters: ProviderRuntimeFilters) {
  return (
    filters.window === DEFAULT_PROVIDER_RUNTIME_FILTERS.window &&
    filters.status === DEFAULT_PROVIDER_RUNTIME_FILTERS.status &&
    filters.search === DEFAULT_PROVIDER_RUNTIME_FILTERS.search &&
    filters.sort === DEFAULT_PROVIDER_RUNTIME_FILTERS.sort &&
    filters.direction === DEFAULT_PROVIDER_RUNTIME_FILTERS.direction &&
    filters.only_enabled === DEFAULT_PROVIDER_RUNTIME_FILTERS.only_enabled
  );
}

function isSameQuery(
  currentQuery: LocationQuery | Record<string, ProviderRuntimeQueryValue>,
  nextQuery: LocationQuery | Record<string, ProviderRuntimeQueryValue>,
): boolean {
  const currentEntries = Object.entries(currentQuery)
    .map(([key, value]) => [key, getProviderRuntimeSingleQueryValue(value)] as const)
    .filter(([, value]) => value !== undefined)
    .sort(([left], [right]) => left.localeCompare(right));
  const nextEntries = Object.entries(nextQuery)
    .map(([key, value]) => [key, getProviderRuntimeSingleQueryValue(value)] as const)
    .filter(([, value]) => value !== undefined)
    .sort(([left], [right]) => left.localeCompare(right));

  if (currentEntries.length !== nextEntries.length) {
    return false;
  }

  return currentEntries.every(([key, value], index) => {
    const [nextKey, nextValue] = nextEntries[index];
    return key === nextKey && value === nextValue;
  });
}

export interface UseProviderRuntimeFiltersOptions {
  routeQuery: Ref<LocationQuery>;
  router: Router;
}

export function useProviderRuntimeFilters({
  routeQuery,
  router,
}: UseProviderRuntimeFiltersOptions) {
  const filters = ref<ProviderRuntimeFilters>({
    ...DEFAULT_PROVIDER_RUNTIME_FILTERS,
  });
  const searchInput = ref("");

  const hasActiveFilters = computed(() =>
    !isProviderRuntimeDefaultFilters(filters.value),
  );

  function applyRouteQuery() {
    filters.value = buildProviderRuntimeFiltersFromQuery(routeQuery.value);
    searchInput.value = filters.value.search;
  }

  function setFilters(next: Partial<ProviderRuntimeFilters>) {
    filters.value = {
      ...filters.value,
      ...(next.window !== undefined
        ? { window: normalizeProviderRuntimeWindow(next.window) }
        : {}),
      ...(next.status !== undefined
        ? { status: normalizeProviderRuntimeStatusFilter(next.status) }
        : {}),
      ...(next.search !== undefined
        ? { search: normalizeProviderRuntimeSearch(next.search) }
        : {}),
      ...(next.sort !== undefined
        ? { sort: normalizeProviderRuntimeSortField(next.sort) }
        : {}),
      ...(next.direction !== undefined
        ? { direction: normalizeProviderRuntimeDirection(next.direction) }
        : {}),
      ...(next.only_enabled !== undefined
        ? { only_enabled: Boolean(next.only_enabled) }
        : {}),
    };
  }

  function resetFilters() {
    filters.value = { ...DEFAULT_PROVIDER_RUNTIME_FILTERS };
    searchInput.value = filters.value.search;
  }

  function toRouteQuery(): LocationQueryRaw {
    return buildProviderRuntimeRouteQueryFromFilters(filters.value);
  }

  function toApiParams(): ProviderRuntimeListParams {
    return buildProviderRuntimeApiParamsFromFilters(filters.value);
  }

  async function syncRouteWithFilters() {
    const nextQuery = toRouteQuery() as Record<string, ProviderRuntimeQueryValue>;
    if (isSameQuery(routeQuery.value, nextQuery)) {
      return false;
    }
    await router.replace({ query: nextQuery });
    return true;
  }

  return {
    applyRouteQuery,
    filters,
    hasActiveFilters,
    resetFilters,
    searchInput,
    setFilters,
    syncRouteWithFilters,
    toApiParams,
    toRouteQuery,
  };
}
