<script setup lang="ts">
import { Search } from "lucide-vue-next";

import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { AlertFiltersState, AlertSelectOption } from "../types";

defineProps<{
  statusOptions: AlertSelectOption[];
  severityOptions: AlertSelectOption[];
  scopeOptions: AlertSelectOption[];
  booleanOptions: AlertSelectOption[];
}>();

defineEmits<{
  refresh: [];
}>();

const filters = defineModel<AlertFiltersState>("filters", { required: true });
</script>

<template>
  <div class="rounded-lg border border-gray-200 bg-white p-4">
    <div class="grid grid-cols-1 gap-3 lg:grid-cols-12">
      <div class="lg:col-span-3">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("alertsPage.filter.status") }}
        </span>
        <Select v-model="filters.status" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="option in statusOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="lg:col-span-3">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("alertsPage.filter.severity") }}
        </span>
        <Select v-model="filters.severity" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="option in severityOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="lg:col-span-3">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("alertsPage.filter.scope") }}
        </span>
        <Select v-model="filters.scope_type" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="option in scopeOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div class="lg:col-span-3">
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("alertsPage.filter.search") }}
        </span>
        <div class="relative">
          <Search class="pointer-events-none absolute left-3 top-1/2 h-4 w-4 -translate-y-1/2 text-gray-400" />
          <Input
            v-model="filters.query"
            class="w-full pl-9"
            :placeholder="$t('alertsPage.filter.searchPlaceholder')"
          />
        </div>
      </div>
    </div>
    <div class="mt-3 grid grid-cols-1 gap-3 sm:grid-cols-2">
      <div>
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("alertsPage.filter.acknowledged") }}
        </span>
        <Select v-model="filters.acknowledged" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="option in booleanOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
      <div>
        <span class="mb-1.5 block text-xs font-medium uppercase tracking-wide text-gray-500">
          {{ $t("alertsPage.filter.suppressed") }}
        </span>
        <Select v-model="filters.suppressed" @update:model-value="$emit('refresh')">
          <SelectTrigger class="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent :body-lock="false">
            <SelectItem
              v-for="option in booleanOptions"
              :key="option.value"
              :value="option.value"
            >
              {{ option.label }}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  </div>
</template>
