import { computed, reactive, ref } from "vue";
import { defineStore } from "pinia";
import type { LocationQuery, LocationQueryRaw } from "vue-router";

import { normalizeError } from "@/lib/error";
import { Api } from "@/services/request";
import type {
  ProviderRuntimeItem,
  ProviderRuntimeListParams,
  ProviderRuntimeSummary,
  ProviderRuntimeWindow,
} from "./types";
import {
  DEFAULT_PROVIDER_RUNTIME_FILTERS as DEFAULT_FILTERS,
  buildProviderRuntimeApiParamsFromFilters,
  buildProviderRuntimeFiltersFromQuery,
  buildProviderRuntimeRouteQueryFromFilters,
  normalizeProviderRuntimeDirection,
  normalizeProviderRuntimeSearch,
  normalizeProviderRuntimeSortField,
  normalizeProviderRuntimeStatusFilter,
  normalizeProviderRuntimeWindow,
  type ProviderRuntimeFilters,
} from "./providerRuntimeQuery";

export const useProviderRuntimeStore = defineStore("providerRuntime", () => {
  const items = ref<ProviderRuntimeItem[]>([]);
  const summary = ref<ProviderRuntimeSummary | null>(null);
  const loadingList = ref(false);
  const loadingSummary = ref(false);
  const error = ref<string | null>(null);
  const filters = reactive<ProviderRuntimeFilters>({ ...DEFAULT_FILTERS });

  const isLoading = computed(() => loadingList.value || loadingSummary.value);
  const hasActiveFilters = computed(
    () =>
      filters.window !== DEFAULT_FILTERS.window ||
      filters.status !== DEFAULT_FILTERS.status ||
      filters.search !== DEFAULT_FILTERS.search ||
      filters.sort !== DEFAULT_FILTERS.sort ||
      filters.direction !== DEFAULT_FILTERS.direction ||
      filters.only_enabled !== DEFAULT_FILTERS.only_enabled,
  );

  function setFilters(next: Partial<ProviderRuntimeFilters>) {
    if (next.window !== undefined) {
      filters.window = normalizeProviderRuntimeWindow(next.window);
    }
    if (next.status !== undefined) {
      filters.status = normalizeProviderRuntimeStatusFilter(next.status);
    }
    if (next.search !== undefined) {
      filters.search = normalizeProviderRuntimeSearch(next.search);
    }
    if (next.sort !== undefined) {
      filters.sort = normalizeProviderRuntimeSortField(next.sort);
    }
    if (next.direction !== undefined) {
      filters.direction = normalizeProviderRuntimeDirection(next.direction);
    }
    if (next.only_enabled !== undefined) {
      filters.only_enabled = Boolean(next.only_enabled);
    }
  }

  function resetFilters() {
    Object.assign(filters, DEFAULT_FILTERS);
  }

  function applyQuery(query: LocationQuery) {
    Object.assign(filters, buildProviderRuntimeFiltersFromQuery(query));
  }

  function toRouteQuery(): LocationQueryRaw {
    return buildProviderRuntimeRouteQueryFromFilters(filters);
  }

  function toApiParams(): ProviderRuntimeListParams {
    return buildProviderRuntimeApiParamsFromFilters(filters);
  }

  async function fetchList(params?: Partial<ProviderRuntimeFilters>) {
    if (params) {
      setFilters(params);
    }

    loadingList.value = true;
    error.value = null;
    try {
      const data = await Api.getProviderRuntimeList(toApiParams());
      items.value = data || [];
      return items.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch provider runtime list:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loadingList.value = false;
    }
  }

  async function fetchSummary(window?: ProviderRuntimeWindow) {
    if (window) {
      filters.window = normalizeProviderRuntimeWindow(window);
    }

    loadingSummary.value = true;
    error.value = null;
    try {
      summary.value = await Api.getProviderRuntimeSummary(filters.window);
      return summary.value;
    } catch (err) {
      const normalizedError = normalizeError(err);
      console.error("Failed to fetch provider runtime summary:", normalizedError);
      error.value = normalizedError.message;
      throw normalizedError;
    } finally {
      loadingSummary.value = false;
    }
  }

  async function refresh(params?: Partial<ProviderRuntimeFilters>) {
    if (params) {
      setFilters(params);
    }

    await Promise.all([fetchSummary(filters.window), fetchList()]);
  }

  return {
    items,
    summary,
    loadingList,
    loadingSummary,
    isLoading,
    error,
    filters,
    hasActiveFilters,
    setFilters,
    resetFilters,
    applyQuery,
    toRouteQuery,
    toApiParams,
    fetchList,
    fetchSummary,
    refresh,
  };
});
