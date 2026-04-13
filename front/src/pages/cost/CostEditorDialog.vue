<script setup lang="ts">
import { Copy, Edit, RefreshCw, Sparkles } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostComponent,
  CostPreviewResponse,
} from "@/store/types";
import CostPreviewSection from "./CostPreviewSection.vue";
import CostVersionSection from "./CostVersionSection.vue";
import type { PreviewDraft } from "./types";

defineProps<{
  open: boolean;
  selectedCatalog: CostCatalogListItem | null;
  selectedCatalogVersions: CostCatalogVersion[];
  selectedVersionId: number | null;
  selectedVersionSummary: CostCatalogVersion | null;
  components: CostComponent[];
  isLoadingVersionDetail: boolean;
  togglingVersionId: number | null;
  duplicatingCatalogId: number | null;
  previewDraft: PreviewDraft;
  previewResponse: CostPreviewResponse | null;
  canPreview: boolean;
  isRunningPreview: boolean;
  meterLabel: (meterKey: string) => string;
  chargeKindLabel: (chargeKind: string) => string;
  tierBasisLabel: (basis: string) => string;
  formatRateDisplay: (
    micros: number | null | undefined,
    meterKey: string,
    currency?: string | null,
    suffix?: boolean,
  ) => string;
  tryFormatRateInputDisplay: (value: string, meterKey: string) => string;
  formatNumber: (value: number | null | undefined) => string;
  prettyJson: (value: string | null | undefined) => string;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "refresh"): void;
  (e: "open-template"): void;
  (e: "edit-catalog", catalog: { id: number; name: string; description: string | null }): void;
  (e: "duplicate-catalog", catalog: CostCatalogListItem): void;
  (e: "create-version"): void;
  (e: "select-version", versionId: number): void;
  (e: "toggle-version-enabled", version: CostCatalogVersion): void;
  (e: "create-component"): void;
  (e: "edit-component", component: CostComponent): void;
  (e: "delete-component", component: CostComponent): void;
  (e: "apply-sample"): void;
  (e: "run-preview"): void;
}>();
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-6xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div class="min-w-0">
            <DialogTitle class="text-lg font-semibold text-gray-900">
              {{ selectedCatalog?.catalog.name || $t("costPage.editor.title") }}
            </DialogTitle>
            <p class="mt-1 text-sm text-gray-500">
              {{
                selectedCatalog?.catalog.description ||
                $t("costPage.editor.description")
              }}
            </p>
          </div>
          <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row sm:flex-wrap">
            <Button variant="outline" @click="emit('open-template')">
              <Sparkles class="mr-1.5 h-4 w-4" />
              {{ $t("costPage.templates.title") }}
            </Button>
            <Button variant="outline" @click="emit('refresh')">
              <RefreshCw class="mr-1.5 h-4 w-4" />
              {{ $t("common.refresh") }}
            </Button>
            <Button
              v-if="selectedCatalog"
              variant="outline"
              @click="emit('edit-catalog', selectedCatalog.catalog)"
            >
              <Edit class="mr-1.5 h-4 w-4" />
              {{ $t("costPage.catalogs.editMeta") }}
            </Button>
            <Button
              v-if="selectedCatalog"
              variant="outline"
              :disabled="duplicatingCatalogId === selectedCatalog.catalog.id"
              @click="emit('duplicate-catalog', selectedCatalog)"
            >
              <Copy class="mr-1.5 h-4 w-4" />
              {{
                duplicatingCatalogId === selectedCatalog.catalog.id
                  ? $t("common.loading")
                  : $t("costPage.catalogs.duplicate")
              }}
            </Button>
          </div>
        </div>
      </DialogHeader>

      <div class="flex-1 overflow-y-auto px-4 py-4 sm:px-6">
        <div v-if="selectedCatalog" class="space-y-6">
          <div class="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div class="rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.catalogs.versionCount") }}</div>
              <div class="mt-1 text-lg font-semibold text-gray-900">
                {{ selectedCatalogVersions.length }}
              </div>
            </div>
            <div class="rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.catalogs.latestVersion") }}</div>
              <div class="mt-1 text-lg font-semibold text-gray-900">
                {{ selectedCatalogVersions[0]?.version || "-" }}
              </div>
            </div>
            <div class="rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.editor.currentVersion") }}</div>
              <div class="mt-1 flex items-center gap-2">
                <span class="text-lg font-semibold text-gray-900">
                  {{ selectedVersionSummary?.version || "-" }}
                </span>
                <Badge
                  v-if="selectedVersionSummary"
                  :variant="selectedVersionSummary.is_enabled ? 'secondary' : 'outline'"
                  class="text-[11px]"
                >
                  {{
                    selectedVersionSummary.is_enabled
                      ? $t("costPage.state.enabled")
                      : $t("costPage.state.disabled")
                  }}
                </Badge>
              </div>
            </div>
          </div>

          <CostVersionSection
            embedded
            :selected-catalog="selectedCatalog"
            :selected-catalog-versions="selectedCatalogVersions"
            :selected-version-id="selectedVersionId"
            :selected-version-summary="selectedVersionSummary"
            :components="components"
            :is-loading-version-detail="isLoadingVersionDetail"
            :toggling-version-id="togglingVersionId"
            :meter-label="meterLabel"
            :charge-kind-label="chargeKindLabel"
            :tier-basis-label="tierBasisLabel"
            :format-rate-display="formatRateDisplay"
            :try-format-rate-input-display="tryFormatRateInputDisplay"
            :pretty-json="prettyJson"
            @create-version="emit('create-version')"
            @select-version="(versionId) => emit('select-version', versionId)"
            @toggle-version-enabled="(version) => emit('toggle-version-enabled', version)"
            @create-component="emit('create-component')"
            @edit-component="(component) => emit('edit-component', component)"
            @delete-component="(component) => emit('delete-component', component)"
          />

          <CostPreviewSection
            embedded
            :selected-version-summary="selectedVersionSummary"
            :preview-draft="previewDraft"
            :preview-response="previewResponse"
            :can-preview="canPreview"
            :is-running-preview="isRunningPreview"
            :meter-label="meterLabel"
            :charge-kind-label="chargeKindLabel"
            :format-rate-display="formatRateDisplay"
            :format-number="formatNumber"
            @apply-sample="emit('apply-sample')"
            @run-preview="emit('run-preview')"
          />
        </div>

        <div
          v-else
          class="flex flex-col items-center justify-center py-20 text-center"
        >
          <div class="text-sm font-medium text-gray-500">
            {{ $t("costPage.editor.emptyTitle") }}
          </div>
          <p class="mt-2 max-w-md text-sm text-gray-400">
            {{ $t("costPage.editor.emptyDescription") }}
          </p>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
