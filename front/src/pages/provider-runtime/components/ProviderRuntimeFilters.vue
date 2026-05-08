<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { ArrowUpDown, Search, X } from "lucide-vue-next";

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
  ProviderRuntimeSortField,
  ProviderRuntimeStatusFilter,
  ProviderRuntimeWindow,
} from "@/services/types";
import type { ProviderRuntimeFilters, ProviderRuntimeOption } from "../types";

const props = defineProps<{
  filters: ProviderRuntimeFilters;
  searchInput: string;
  activeFilterSummary: string;
  windowOptions: ProviderRuntimeOption<ProviderRuntimeWindow>[];
  statusOptions: ProviderRuntimeOption<ProviderRuntimeStatusFilter>[];
  sortOptions: ProviderRuntimeOption<ProviderRuntimeSortField>[];
}>();

const emit = defineEmits<{
  "update:searchInput": [value: string];
  applySearch: [];
  clearSearch: [];
  selectWindow: [value: ProviderRuntimeWindow];
  selectStatus: [value: ProviderRuntimeStatusFilter];
  selectSort: [value: ProviderRuntimeSortField];
  toggleDirection: [];
  updateOnlyEnabled: [value: boolean];
}>();

const { t: $t } = useI18n();

function handleStatusChange(value: unknown) {
  if (typeof value === "string") {
    emit("selectStatus", value as ProviderRuntimeStatusFilter);
  }
}

function handleSortChange(value: unknown) {
  if (typeof value === "string") {
    emit("selectSort", value as ProviderRuntimeSortField);
  }
}

function handleOnlyEnabledChange(value: boolean | "indeterminate") {
  emit("updateOnlyEnabled", value === true);
}
</script>

<template>
  <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 border-b border-gray-100 pb-4 md:flex-row md:items-start md:justify-between">
      <div class="min-w-0">
        <h2 class="text-base font-semibold text-gray-900">
          {{ $t("providerRuntimePage.filter.title") }}
        </h2>
        <p class="mt-1 text-sm text-gray-500">
          {{ props.activeFilterSummary }}
        </p>
      </div>
      <div class="flex flex-wrap gap-2">
        <Button
          v-for="windowOption in props.windowOptions"
          :key="windowOption.value"
          :variant="props.filters.window === windowOption.value ? 'default' : 'outline'"
          size="sm"
          @click="emit('selectWindow', windowOption.value)"
        >
          {{ windowOption.label }}
        </Button>
      </div>
    </div>

    <div class="mt-4 grid grid-cols-1 gap-3 lg:grid-cols-12">
      <div class="lg:col-span-4">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("providerRuntimePage.searchPlaceholder") }}
        </span>
        <div class="relative">
          <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
          <Input
            :model-value="props.searchInput"
            class="w-full pl-9 pr-9"
            :placeholder="$t('providerRuntimePage.searchPlaceholder')"
            @update:model-value="emit('update:searchInput', String($event))"
            @keydown.enter="emit('applySearch')"
          />
          <button
            v-if="props.searchInput"
            type="button"
            class="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-gray-400 transition-colors hover:text-gray-600"
            @click="emit('clearSearch')"
          >
            <X class="h-4 w-4" />
          </button>
        </div>
      </div>

      <div class="lg:col-span-3">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("providerRuntimePage.filter.status") }}
        </span>
        <Select :model-value="props.filters.status" @update:model-value="handleStatusChange">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="statusOption in props.statusOptions"
              :key="statusOption.value"
              :value="statusOption.value"
            >
              {{ statusOption.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="lg:col-span-3">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("providerRuntimePage.filter.sort") }}
        </span>
        <Select :model-value="props.filters.sort" @update:model-value="handleSortChange">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="sortOption in props.sortOptions"
              :key="sortOption.value"
              :value="sortOption.value"
            >
              {{ sortOption.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      <div class="lg:col-span-2">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("providerRuntimePage.filter.direction") }}
        </span>
        <Button variant="outline" class="w-full justify-between" @click="emit('toggleDirection')">
          <span>
            {{ $t(`providerRuntimePage.filter.${props.filters.direction}`) }}
          </span>
          <ArrowUpDown class="h-4 w-4 text-gray-400" />
        </Button>
      </div>
    </div>

    <div class="mt-4 flex flex-col gap-3 border-t border-gray-100 pt-4 sm:flex-row sm:items-center sm:justify-between">
      <label class="inline-flex items-center gap-2 text-sm text-gray-600">
        <Checkbox
          :model-value="props.filters.only_enabled"
          @update:model-value="handleOnlyEnabledChange"
        />
        <span>{{ $t("providerRuntimePage.activeOnly") }}</span>
      </label>
      <Button variant="outline" class="sm:hidden" @click="emit('applySearch')">
        {{ $t("recordPage.filter.applyButton") }}
      </Button>
      <Button variant="outline" class="hidden sm:inline-flex" @click="emit('applySearch')">
        {{ $t("recordPage.filter.applyButton") }}
      </Button>
    </div>
  </div>
</template>
