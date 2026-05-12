<template>
  <div class="rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader
      :title="t('modelEditPage.routeReferences.heading')"
      :help="t('modelEditPage.routeReferences.description')"
      :help-label="t('modelEditPage.routeReferences.heading')"
    >
      <template #meta>
        <p class="mt-1 text-[11px] font-medium uppercase tracking-wide text-gray-500">
          {{ t("modelEditPage.routeReferences.title") }}
        </p>
      </template>
      <template #actions>
      <Button variant="ghost" @click="$emit('open-routes')">
        {{ t("modelEditPage.routeReferences.openRoutes") }}
      </Button>
      </template>
    </SectionHeader>

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

import SectionHeader from "@/components/SectionHeader.vue";
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
