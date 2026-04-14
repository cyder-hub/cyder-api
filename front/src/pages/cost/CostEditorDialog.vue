<script setup lang="ts">
import { Copy, Edit, Plus, RefreshCw, Sparkles } from "lucide-vue-next";
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
  managingVersionId: number | null;
  duplicatingVersionId: number | null;
  duplicatingCatalogId: number | null;
  showArchivedVersions: boolean;
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
  (e: "archive-version", version: CostCatalogVersion): void;
  (e: "unarchive-version", version: CostCatalogVersion): void;
  (e: "delete-version", version: CostCatalogVersion): void;
  (e: "toggle-archived-visibility"): void;
  (e: "duplicate-version", version: CostCatalogVersion): void;
  (e: "create-component"): void;
  (e: "edit-component", component: CostComponent): void;
  (e: "delete-component", component: CostComponent): void;
  (e: "apply-sample"): void;
  (e: "reset-preview"): void;
  (e: "run-preview"): void;
}>();

</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent
      class="flex max-h-[94dvh] w-[calc(100vw-1.5rem)] max-w-[96vw] flex-col overflow-hidden p-0 xl:max-w-7xl"
    >
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
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
          <div class="grid w-full grid-cols-1 gap-2 sm:grid-cols-2 xl:w-auto xl:grid-cols-4">
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
            <Button v-if="selectedCatalog" @click="emit('create-version')">
              <Plus class="mr-1.5 h-4 w-4" />
              {{ $t("costPage.versions.add") }}
            </Button>
          </div>
        </div>
      </DialogHeader>

      <div class="flex min-h-0 flex-1 flex-col px-4 py-4 sm:px-6">
        <div v-if="selectedCatalog" class="flex min-h-0 flex-1 flex-col">
          <CostVersionSection
            embedded
            :selected-catalog="selectedCatalog"
            :selected-catalog-versions="selectedCatalogVersions"
            :selected-version-id="selectedVersionId"
            :selected-version-summary="selectedVersionSummary"
            :components="components"
            :is-loading-version-detail="isLoadingVersionDetail"
            :toggling-version-id="togglingVersionId"
            :managing-version-id="managingVersionId"
            :duplicating-version-id="duplicatingVersionId"
            :show-archived-versions="showArchivedVersions"
            :meter-label="meterLabel"
            :charge-kind-label="chargeKindLabel"
            :tier-basis-label="tierBasisLabel"
            :format-rate-display="formatRateDisplay"
            :try-format-rate-input-display="tryFormatRateInputDisplay"
            :preview-draft="previewDraft"
            :preview-response="previewResponse"
            :can-preview="canPreview"
            :is-running-preview="isRunningPreview"
            :format-number="formatNumber"
            :pretty-json="prettyJson"
            @create-version="emit('create-version')"
            @select-version="(versionId) => emit('select-version', versionId)"
            @toggle-version-enabled="(version) => emit('toggle-version-enabled', version)"
            @archive-version="(version) => emit('archive-version', version)"
            @unarchive-version="(version) => emit('unarchive-version', version)"
            @delete-version="(version) => emit('delete-version', version)"
            @toggle-archived-visibility="emit('toggle-archived-visibility')"
            @duplicate-version="(version) => emit('duplicate-version', version)"
            @create-component="emit('create-component')"
            @edit-component="(component) => emit('edit-component', component)"
            @delete-component="(component) => emit('delete-component', component)"
            @apply-sample="emit('apply-sample')"
            @reset-preview="emit('reset-preview')"
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
