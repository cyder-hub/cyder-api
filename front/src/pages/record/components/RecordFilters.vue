<template>
  <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 border-b border-gray-100 pb-4 md:flex-row md:items-start md:justify-between">
      <div class="min-w-0">
        <h2 class="text-base font-semibold text-gray-900">
          {{ $t("recordPage.filter.title") }}
        </h2>
        <p class="mt-1 text-xs leading-5 text-gray-500">{{ filterSummary }}</p>
      </div>
      <div class="flex w-full flex-col gap-2 sm:flex-row md:w-auto md:items-center">
        <Button
          variant="outline"
          class="w-full justify-between md:hidden"
          @click="$emit('toggleFilterPanel')"
        >
          <span class="flex items-center gap-2">
            <SlidersHorizontal class="h-4 w-4" />
            {{
              isFilterPanelOpen
                ? $t("recordPage.filter.hideFilters")
                : $t("recordPage.filter.showFilters")
            }}
          </span>
          <ChevronDown
            class="h-4 w-4 transition-transform"
            :class="{ 'rotate-180': isFilterPanelOpen }"
          />
        </Button>
        <Button
          v-if="hasActiveFilters"
          variant="outline"
          class="hidden md:inline-flex"
          @click="$emit('reset')"
        >
          {{ $t("recordPage.filter.resetButton") }}
        </Button>
      </div>
    </div>

    <div
      :class="[
        'mt-4 flex-col gap-4 md:flex',
        isFilterPanelOpen ? 'flex' : 'hidden md:flex',
      ]"
    >
      <div class="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-12">
        <FilterSelect
          class="xl:col-span-2"
          :label="$t('recordPage.filter.apiKeyLabel')"
          :model-value="String(filters.api_key_id)"
          :options="apiKeyOptions"
          @update:model-value="$emit('updateNumberFilter', 'api_key_id', $event)"
        />
        <FilterSelect
          class="xl:col-span-2"
          :label="$t('recordPage.filter.providerLabel')"
          :model-value="String(filters.provider_id)"
          :options="providerOptions"
          @update:model-value="$emit('updateNumberFilter', 'provider_id', $event)"
        />
        <FilterSelect
          class="xl:col-span-3"
          :label="$t('recordPage.filter.modelLabel')"
          :model-value="String(filters.model_id)"
          :options="modelOptions"
          @update:model-value="$emit('updateNumberFilter', 'model_id', $event)"
        />
        <FilterSelect
          class="xl:col-span-2"
          :label="$t('recordPage.filter.statusLabel')"
          :model-value="filters.status"
          :options="statusOptions"
          @update:model-value="$emit('updateStatusFilter', $event)"
        />
        <div class="flex flex-col gap-1.5 xl:col-span-3">
          <span class="text-xs font-medium uppercase tracking-wide text-gray-500">
            {{ $t("recordPage.filter.searchLabel") }}
          </span>
          <div class="relative">
            <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
            <Input
              :model-value="searchInput"
              :placeholder="$t('recordPage.filter.searchPlaceholder')"
              class="w-full pl-9 pr-9"
              @update:model-value="$emit('update:searchInput', String($event))"
              @keydown.enter="$emit('apply')"
            />
            <button
              v-if="searchInput"
              type="button"
              :aria-label="$t('recordPage.filter.clearSearch')"
              class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 transition-colors hover:text-gray-600"
              @click="$emit('clearSearch')"
            >
              <X class="h-4 w-4" />
            </button>
          </div>
        </div>
      </div>

      <div class="border-t border-gray-100 pt-3">
        <button
          type="button"
          class="flex w-full items-center justify-between gap-3 rounded-md px-1 py-2 text-left text-sm font-medium text-gray-700 transition-colors hover:text-gray-900"
          @click="$emit('toggleAdvancedFilters')"
        >
          <span class="flex min-w-0 items-center gap-2">
            <SlidersHorizontal class="h-4 w-4 flex-shrink-0 text-gray-400" />
            <span>{{ $t("recordPage.filter.advancedFilters") }}</span>
            <Badge
              v-if="advancedActiveFilterCount > 0"
              variant="outline"
              class="font-mono text-[11px]"
            >
              {{ advancedActiveFilterCount }}
            </Badge>
          </span>
          <span class="flex flex-shrink-0 items-center gap-2 text-xs text-gray-500">
            {{
              isAdvancedFilterOpen
                ? $t("recordPage.filter.collapse")
                : $t("recordPage.filter.expand")
            }}
            <ChevronDown
              class="h-4 w-4 transition-transform"
              :class="{ 'rotate-180': isAdvancedFilterOpen }"
            />
          </span>
        </button>

        <div
          v-if="isAdvancedFilterOpen"
          class="mt-3 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-12"
        >
          <FilterSelect
            class="xl:col-span-3"
            :label="$t('recordPage.filter.userApiLabel')"
            :model-value="filters.user_api_type"
            :options="userApiTypeOptions"
            @update:model-value="$emit('updateFilter', 'user_api_type', $event)"
          />
          <FilterSelect
            class="xl:col-span-3"
            :label="$t('recordPage.filter.resolvedScopeLabel')"
            :model-value="filters.resolved_name_scope"
            :options="resolvedScopeOptions"
            @update:model-value="$emit('updateFilter', 'resolved_name_scope', $event)"
          />
          <FilterSelect
            class="xl:col-span-2"
            :label="$t('recordPage.filter.retryLabel')"
            :model-value="filters.has_retry"
            :options="booleanOptions"
            @update:model-value="$emit('updateFilter', 'has_retry', $event)"
          />
          <FilterSelect
            class="xl:col-span-2"
            :label="$t('recordPage.filter.fallbackLabel')"
            :model-value="filters.has_fallback"
            :options="booleanOptions"
            @update:model-value="$emit('updateFilter', 'has_fallback', $event)"
          />
          <FilterSelect
            class="xl:col-span-2"
            :label="$t('recordPage.filter.diagnosticsLabel')"
            :model-value="filters.has_transform_diagnostics"
            :options="booleanOptions"
            @update:model-value="$emit('updateFilter', 'has_transform_diagnostics', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.errorCodeLabel')"
            :model-value="filters.final_error_code"
            @update:model-value="$emit('updateFilter', 'final_error_code', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.latencyMinLabel')"
            :model-value="filters.latency_ms_min"
            inputmode="numeric"
            :placeholder="$t('recordPage.filter.msPlaceholder')"
            @update:model-value="$emit('updateFilter', 'latency_ms_min', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.latencyMaxLabel')"
            :model-value="filters.latency_ms_max"
            inputmode="numeric"
            :placeholder="$t('recordPage.filter.msPlaceholder')"
            @update:model-value="$emit('updateFilter', 'latency_ms_max', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.tokensMinLabel')"
            :model-value="filters.total_tokens_min"
            inputmode="numeric"
            @update:model-value="$emit('updateFilter', 'total_tokens_min', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.tokensMaxLabel')"
            :model-value="filters.total_tokens_max"
            inputmode="numeric"
            @update:model-value="$emit('updateFilter', 'total_tokens_max', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.costMinLabel')"
            :model-value="filters.estimated_cost_nanos_min"
            inputmode="numeric"
            :placeholder="$t('recordPage.filter.nanosPlaceholder')"
            @update:model-value="$emit('updateFilter', 'estimated_cost_nanos_min', $event)"
          />
          <FilterInput
            class="xl:col-span-2"
            :label="$t('recordPage.filter.costMaxLabel')"
            :model-value="filters.estimated_cost_nanos_max"
            inputmode="numeric"
            :placeholder="$t('recordPage.filter.nanosPlaceholder')"
            @update:model-value="$emit('updateFilter', 'estimated_cost_nanos_max', $event)"
          />
          <FilterInput
            class="xl:col-span-3"
            :label="$t('recordPage.filter.startTimeLabel')"
            :model-value="filters.start_time"
            type="datetime-local"
            @update:model-value="$emit('updateFilter', 'start_time', $event)"
          />
          <FilterInput
            class="xl:col-span-3"
            :label="$t('recordPage.filter.endTimeLabel')"
            :model-value="filters.end_time"
            type="datetime-local"
            @update:model-value="$emit('updateFilter', 'end_time', $event)"
          />
        </div>
      </div>

      <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
        <p class="text-xs text-gray-500">
          {{ $t("recordPage.filter.helpText") }}
        </p>
        <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
          <Button class="w-full sm:w-auto" @click="$emit('apply')">
            {{ $t("recordPage.filter.applyButton") }}
          </Button>
          <Button
            v-if="hasActiveFilters"
            variant="outline"
            class="w-full md:hidden sm:w-auto"
            @click="$emit('reset')"
          >
            {{ $t("recordPage.filter.resetButton") }}
          </Button>
        </div>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { defineComponent, h } from "vue";
import { ChevronDown, Search, SlidersHorizontal, X } from "lucide-vue-next";
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
import type { FilterOption, RecordFilters } from "../types";

defineProps<{
  filters: RecordFilters;
  searchInput: string;
  filterSummary: string;
  isFilterPanelOpen: boolean;
  isAdvancedFilterOpen: boolean;
  hasActiveFilters: boolean;
  advancedActiveFilterCount: number;
  apiKeyOptions: FilterOption[];
  providerOptions: FilterOption[];
  modelOptions: FilterOption[];
  statusOptions: FilterOption[];
  userApiTypeOptions: FilterOption[];
  resolvedScopeOptions: FilterOption[];
  booleanOptions: FilterOption[];
}>();

defineEmits<{
  "update:searchInput": [value: string];
  updateFilter: [key: keyof RecordFilters, value: string];
  updateNumberFilter: [key: "api_key_id" | "provider_id" | "model_id", value: string];
  updateStatusFilter: [value: string];
  toggleFilterPanel: [];
  toggleAdvancedFilters: [];
  apply: [];
  clearSearch: [];
  reset: [];
}>();

const FilterSelect = defineComponent({
  props: {
    label: { type: String, required: true },
    modelValue: { type: String, required: true },
    options: {
      type: Array as () => FilterOption[],
      required: true,
    },
  },
  emits: ["update:modelValue"],
  setup(props, { emit, attrs }) {
    return () =>
      h("div", { class: ["flex flex-col gap-1.5", attrs.class] }, [
        h(
          "span",
          { class: "text-xs font-medium uppercase tracking-wide text-gray-500" },
          props.label,
        ),
        h(
          Select,
          {
            modelValue: props.modelValue,
            "onUpdate:modelValue": (value: unknown) =>
              emit("update:modelValue", String(value)),
          },
          () => [
            h(SelectTrigger, { class: "w-full" }, () => h(SelectValue)),
            h(
              SelectContent,
              { bodyLock: false },
              () =>
                props.options.map((option) =>
                  h(
                    SelectItem,
                    { key: option.value, value: option.value },
                    () => option.label,
                  ),
                ),
            ),
          ],
        ),
      ]);
  },
});

const FilterInput = defineComponent({
  props: {
    label: { type: String, required: true },
    modelValue: { type: String, required: true },
    type: { type: String, default: "text" },
    inputmode: { type: String, default: undefined },
    placeholder: { type: String, default: "" },
  },
  emits: ["update:modelValue"],
  setup(props, { emit, attrs }) {
    return () =>
      h("div", { class: ["flex flex-col gap-1.5", attrs.class] }, [
        h(
          "span",
          { class: "text-xs font-medium uppercase tracking-wide text-gray-500" },
          props.label,
        ),
        h(Input, {
          modelValue: props.modelValue,
          type: props.type,
          inputmode: props.inputmode,
          placeholder: props.placeholder,
          "onUpdate:modelValue": (value: string | number) =>
            emit("update:modelValue", String(value)),
        }),
      ]);
  },
});
</script>
