<script setup lang="ts">
import { Sparkles, RefreshCw } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Drawer,
  DrawerContent,
  DrawerHeader,
  DrawerTitle,
} from "@/components/ui/drawer";
import HelpHint from "@/components/HelpHint.vue";
import { formatTimestamp } from "@/utils/datetime";
import type { CostTemplateSummary } from "@/services/types/cost";

defineProps<{
  open: boolean;
  templates: CostTemplateSummary[];
  isLoadingTemplates: boolean;
  importingTemplateKey: string | null;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "refresh"): void;
  (e: "import-template", template: CostTemplateSummary): void;
}>();
</script>

<template>
  <Drawer :open="open" direction="right" @update:open="(val) => emit('update:open', val)">
    <DrawerContent class="flex h-full w-full flex-col rounded-none rounded-l-2xl border-none bg-background sm:max-w-[600px] lg:max-w-[800px] xl:max-w-[1000px] right-0 left-auto mt-0 top-0">
      <DrawerHeader class="border-b border-gray-100 px-6 py-4 text-left">
        <div class="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <DrawerTitle class="text-lg font-semibold text-gray-900">
              {{ $t("costPage.templates.title") }}
            </DrawerTitle>
            <HelpHint
              :label="$t('costPage.templates.title')"
              :content="$t('costPage.templates.description')"
            />
          </div>
          <Button variant="outline" size="sm" @click="emit('refresh')">
            <RefreshCw class="mr-1.5 h-4 w-4" />
            {{ $t("common.refresh") }}
          </Button>
        </div>
      </DrawerHeader>
      <div class="flex-1 overflow-y-auto px-6 py-6">
        <div
          v-if="isLoadingTemplates"
          class="rounded-xl border border-dashed border-gray-200 bg-gray-50/60 px-6 py-12 text-center text-sm text-gray-500"
        >
          {{ $t("costPage.templates.loading") }}
        </div>
        <div
          v-else-if="templates.length === 0"
          class="rounded-xl border border-dashed border-gray-200 bg-gray-50/60 px-6 py-12 text-center text-sm text-gray-500"
        >
          {{ $t("costPage.templates.empty") }}
        </div>
        <div v-else class="flex flex-col gap-6">
          <div
            v-for="(template, index) in templates"
            :key="template.key"
            class="pb-6"
            :class="{ 'border-b border-gray-100': index !== templates.length - 1 }"
          >
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div class="min-w-0">
                <div class="flex flex-wrap items-center gap-2">
                  <h3 class="text-sm font-semibold text-gray-900">
                    {{ template.title }}
                  </h3>
                  <Badge
                    v-for="tag in template.tags"
                    :key="`${template.key}-${tag}`"
                    variant="outline"
                    class="font-mono text-[11px]"
                  >
                    {{ tag }}
                  </Badge>
                </div>
                <p class="mt-2 text-sm text-gray-500">
                  {{ template.description }}
                </p>
              </div>
              <Button
                class="w-full sm:w-auto"
                :disabled="importingTemplateKey === template.key"
                @click="emit('import-template', template)"
              >
                <Sparkles class="mr-1.5 h-4 w-4" />
                {{
                  importingTemplateKey === template.key
                    ? $t("costPage.templates.importing")
                    : $t("costPage.templates.import")
                }}
              </Button>
            </div>

            <div class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2 md:grid-cols-4">
              <div>
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.catalogName") }}</div>
                <div class="mt-1 text-sm font-medium text-gray-900">
                  {{ template.catalog_name }}
                </div>
              </div>
              <div>
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.version") }}</div>
                <div class="mt-1 font-mono text-sm text-gray-900">
                  {{ template.version }}
                </div>
              </div>
              <div>
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.effectiveFrom") }}</div>
                <div class="mt-1 text-sm text-gray-900">
                  {{ formatTimestamp(template.effective_from) || "-" }}
                </div>
              </div>
              <div>
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.source") }}</div>
                <div class="mt-1 break-all text-xs text-gray-600">
                  {{ template.source }}
                </div>
              </div>
            </div>

            <div class="mt-4">
              <div class="text-xs font-medium uppercase tracking-wide text-gray-500">
                {{ $t("costPage.templates.supportedMeters") }}
              </div>
              <div class="mt-2 flex flex-wrap gap-2">
                <Badge
                  v-for="meter in template.supported_meters"
                  :key="`${template.key}-${meter}`"
                  variant="secondary"
                  class="font-mono text-[11px]"
                >
                  {{ meter }}
                </Badge>
              </div>
            </div>

            <div
              v-if="template.rounding_note"
              class="mt-4 text-xs text-amber-600"
            >
              * {{ template.rounding_note }}
            </div>
          </div>
        </div>
      </div>
    </DrawerContent>
  </Drawer>
</template>
