<script setup lang="ts">
import { useI18n } from "vue-i18n";
import { ChevronDown } from "lucide-vue-next";

import { Badge } from "@/components/ui/badge";
import {
  Drawer,
  DrawerContent,
  DrawerDescription,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import type { ApiKeyItem, ApiKeyRuntimeSnapshot } from "@/services/types";
import {
  buildRuntimeRejectionView,
  emptyRuntimeSnapshot,
  lifecycleLabel,
  limitLabel,
  maskedApiKey,
  runtimeRejectionBadgeClass,
  statusBadgeClass,
} from "../composables/useApiKeyDetail";

const props = defineProps<{
  apiKeys: ApiKeyItem[];
  runtimeById: Map<number, ApiKeyRuntimeSnapshot>;
  selectedKeyId: number | null;
  selectedListKey: ApiKeyItem | null;
  mobileOpen: boolean;
}>();

const emit = defineEmits<{
  (event: "select", id: number): void;
  (event: "update:mobileOpen", value: boolean): void;
}>();

const { t } = useI18n();

function runtimeFor(key: ApiKeyItem) {
  return props.runtimeById.get(key.id) ?? emptyRuntimeSnapshot(key.id);
}

function runtimeRejectionFor(key: ApiKeyItem) {
  return buildRuntimeRejectionView(key, runtimeFor(key), t);
}
</script>

<template>
  <div class="space-y-4 xl:h-full xl:min-h-0">
    <button
      type="button"
      class="flex w-full items-start justify-between gap-3 rounded-xl border border-gray-200 bg-white px-4 py-3 text-left xl:hidden"
      @click="emit('update:mobileOpen', true)"
    >
      <div class="min-w-0 flex-1">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ t("apiKeyPage.sections.listTitle") }}
        </p>
        <h2 class="mt-1 truncate text-sm font-semibold text-gray-900">
          {{ selectedListKey?.name || t("apiKeyPage.sections.detailTitle") }}
        </h2>
        <p class="mt-1 line-clamp-1 text-xs text-gray-500">
          {{ selectedListKey?.description || t("common.notAvailable") }}
        </p>
      </div>
      <div class="flex shrink-0 items-center gap-2 pt-0.5">
        <Badge
          v-if="selectedListKey"
          :class="statusBadgeClass(selectedListKey)"
          class="text-[11px]"
        >
          {{ lifecycleLabel(selectedListKey, t) }}
        </Badge>
        <ChevronDown class="h-4 w-4 text-gray-400" />
      </div>
    </button>

    <div class="hidden rounded-xl border border-gray-200 bg-white xl:flex xl:h-full xl:min-h-0 xl:flex-col">
      <div class="border-b border-gray-100 px-4 py-3 sm:px-5">
        <h2 class="text-base font-semibold text-gray-900">
          {{ t("apiKeyPage.sections.listTitle") }}
        </h2>
        <p class="mt-1 text-xs text-gray-500">
          {{ t("apiKeyPage.sections.listDescription") }}
        </p>
      </div>

      <div class="divide-y divide-gray-100 xl:min-h-0 xl:flex-1 xl:overflow-y-auto">
        <button
          v-for="key in apiKeys"
          :key="key.id"
          type="button"
          class="w-full px-4 py-3 text-left transition-colors sm:px-5"
          :class="
            selectedKeyId === key.id
              ? 'bg-gray-50'
              : 'bg-white hover:bg-gray-50/70'
          "
          @click="emit('select', key.id)"
        >
          <div class="flex items-start justify-between gap-3">
            <div class="min-w-0 flex-1">
              <h3 class="truncate text-sm font-semibold text-gray-900">
                {{ key.name }}
              </h3>
              <p class="mt-1 font-mono text-xs text-gray-500">
                {{ maskedApiKey(key) }}
              </p>
            </div>
            <Badge :class="statusBadgeClass(key)" class="shrink-0 text-[11px]">
              {{ lifecycleLabel(key, t) }}
            </Badge>
          </div>

          <dl class="mt-3 grid grid-cols-2 gap-2 text-xs">
            <div>
              <dt class="text-gray-400">
                {{ t("apiKeyPage.table.rateLimitRpm") }}
              </dt>
              <dd class="mt-0.5 font-medium text-gray-700">
                {{ limitLabel(key.rate_limit_rpm, t) }}
              </dd>
            </div>
            <div>
              <dt class="text-gray-400">
                {{ t("apiKeyPage.table.maxConcurrency") }}
              </dt>
              <dd class="mt-0.5 font-medium text-gray-700">
                {{ limitLabel(key.max_concurrent_requests, t) }}
              </dd>
            </div>
            <div>
              <dt class="text-gray-400">
                {{ t("apiKeyPage.table.quotaDailyRequests") }}
              </dt>
              <dd class="mt-0.5 font-medium text-gray-700">
                {{ limitLabel(key.quota_daily_requests, t) }}
              </dd>
            </div>
            <div>
              <dt class="text-gray-400">
                {{ t("apiKeyPage.runtime.currentConcurrency") }}
              </dt>
              <dd class="mt-0.5 font-medium text-gray-700">
                {{ runtimeFor(key).current_concurrency }}
              </dd>
            </div>
          </dl>

          <Badge
            :class="runtimeRejectionBadgeClass(runtimeRejectionFor(key))"
            class="mt-3 text-[11px]"
          >
            {{ runtimeRejectionFor(key).label }}
          </Badge>
        </button>
      </div>
    </div>

    <Drawer :open="mobileOpen" @update:open="emit('update:mobileOpen', $event)">
      <DrawerContent class="gap-0 border border-gray-200 bg-white p-0 xl:hidden">
        <DrawerHeader class="border-b border-gray-100 pb-3">
          <DrawerTitle class="text-base text-gray-900">
            {{ t("apiKeyPage.sections.listTitle") }}
          </DrawerTitle>
          <DrawerDescription class="text-sm text-gray-500">
            {{ selectedListKey?.name || t("apiKeyPage.sections.detailTitle") }}
          </DrawerDescription>
        </DrawerHeader>

        <div class="min-h-0 flex-1 overflow-y-auto">
          <button
            v-for="key in apiKeys"
            :key="key.id"
            type="button"
            class="w-full border-b border-gray-100 px-4 py-3 text-left transition-colors last:border-b-0"
            :class="
              selectedKeyId === key.id
                ? 'bg-gray-50'
                : 'bg-white hover:bg-gray-50/70'
            "
            @click="emit('select', key.id)"
          >
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <h3 class="truncate text-sm font-semibold text-gray-900">
                  {{ key.name }}
                </h3>
                <p class="mt-1 line-clamp-1 text-xs text-gray-500">
                  {{ key.description || maskedApiKey(key) }}
                </p>
              </div>
              <Badge :class="statusBadgeClass(key)" class="shrink-0 text-[11px]">
                {{ lifecycleLabel(key, t) }}
              </Badge>
            </div>
            <div class="mt-3 flex flex-wrap gap-2">
              <Badge variant="outline" class="text-[11px]">
                RPM {{ limitLabel(key.rate_limit_rpm, t) }}
              </Badge>
              <Badge variant="outline" class="text-[11px]">
                {{ t("apiKeyPage.table.maxConcurrency") }}
                {{ limitLabel(key.max_concurrent_requests, t) }}
              </Badge>
              <Badge
                :class="runtimeRejectionBadgeClass(runtimeRejectionFor(key))"
                class="text-[11px]"
              >
                {{ runtimeRejectionFor(key).label }}
              </Badge>
            </div>
          </button>
        </div>
      </DrawerContent>
    </Drawer>
  </div>
</template>
