<template>
  <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div class="min-w-0">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ t("modelEditPage.routeReferences.title") }}
        </p>
        <h2 class="mt-1 text-base font-semibold text-gray-900">
          {{ t("modelEditPage.routeReferences.heading") }}
        </h2>
        <p class="mt-2 text-sm text-gray-500">
          {{ t("modelEditPage.routeReferences.description") }}
        </p>
      </div>
      <Button variant="ghost" @click="$emit('open-routes')">
        {{ t("modelEditPage.routeReferences.openRoutes") }}
      </Button>
    </div>

    <div
      v-if="!routeReferences.length"
      class="mt-4 rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
    >
      {{ t("modelEditPage.routeReferences.noRoutes") }}
    </div>

    <div v-else class="mt-4 flex flex-wrap gap-2">
      <div
        v-for="routeReference in routeReferences"
        :key="routeReference.id"
        class="min-w-[12rem] rounded-lg border border-gray-200 px-3 py-3"
      >
        <div class="flex items-center justify-between gap-2">
          <span class="font-mono text-sm text-gray-900">
            {{ routeReference.route_name }}
          </span>
          <Badge
            :variant="routeReference.is_enabled ? 'secondary' : 'outline'"
            class="font-mono text-[11px]"
          >
            {{
              routeReference.is_enabled
                ? t("modelEditPage.routeReferences.enabled")
                : t("modelEditPage.routeReferences.disabled")
            }}
          </Badge>
        </div>
        <p v-if="routeReference.description" class="mt-2 text-xs text-gray-500">
          {{ routeReference.description }}
        </p>
        <p class="mt-2 text-[11px] uppercase tracking-wide text-gray-500">
          /models:
          {{
            routeReference.expose_in_models
              ? t("common.yes")
              : t("common.no")
          }}
        </p>
      </div>
    </div>
  </div>
</template>

<script setup lang="ts">
import { useI18n } from "vue-i18n";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import type { ModelRouteReferenceItem } from "@/services/types";

defineProps<{
  routeReferences: ModelRouteReferenceItem[];
}>();

defineEmits<{
  "open-routes": [];
}>();

const { t } = useI18n();
</script>
