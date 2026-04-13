<script setup lang="ts">
import { Sparkles } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { CardDescription } from "@/components/ui/card";
import { Dialog, DialogContent, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { formatTimestamp } from "@/lib/utils";
import type { CostTemplateSummary } from "@/store/types";

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
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-5xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div>
            <DialogTitle class="text-lg font-semibold text-gray-900">
              {{ $t("costPage.templates.title") }}
            </DialogTitle>
            <CardDescription class="mt-1">
              {{ $t("costPage.templates.description") }}
            </CardDescription>
          </div>
          <Button variant="outline" @click="emit('refresh')">
            {{ $t("common.refresh") }}
          </Button>
        </div>
      </DialogHeader>
      <div class="overflow-y-auto px-4 py-4 sm:px-6">
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
        <div v-else class="grid grid-cols-1 gap-3 xl:grid-cols-2">
          <div
            v-for="template in templates"
            :key="template.key"
            class="rounded-xl border border-gray-200 bg-white p-4"
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
                class="sm:min-w-28"
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

            <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2">
              <div class="rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5">
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.catalogName") }}</div>
                <div class="mt-1 text-sm font-medium text-gray-900">
                  {{ template.catalog_name }}
                </div>
              </div>
              <div class="rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5">
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.version") }}</div>
                <div class="mt-1 font-mono text-sm text-gray-900">
                  {{ template.version }}
                </div>
              </div>
              <div class="rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5">
                <div class="text-xs text-gray-500">{{ $t("costPage.templates.effectiveFrom") }}</div>
                <div class="mt-1 text-sm text-gray-900">
                  {{ formatTimestamp(template.effective_from) || "-" }}
                </div>
              </div>
              <div class="rounded-lg border border-gray-100 bg-gray-50/60 px-3 py-2.5">
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
              class="mt-4 rounded-lg border border-amber-200 bg-amber-50 px-3 py-2 text-xs text-amber-800"
            >
              {{ template.rounding_note }}
            </div>
          </div>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
