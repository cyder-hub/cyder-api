<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { Archive, Copy, Edit, Eye, RotateCcw, Sparkles, Trash2 } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { formatPriceFromNanos, formatTimestamp } from "@/lib/utils";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostComponent,
  CostPreviewResponse,
} from "@/store/types";
import CostPreviewSection from "./CostPreviewSection.vue";
import { parseTierConfig } from "./helpers";
import type { PreviewDraft } from "./types";

const props = defineProps<{
  selectedCatalog: CostCatalogListItem | null;
  selectedCatalogVersions: CostCatalogVersion[];
  selectedVersionId: number | null;
  selectedVersionSummary: CostCatalogVersion | null;
  components: CostComponent[];
  isLoadingVersionDetail: boolean;
  togglingVersionId: number | null;
  managingVersionId: number | null;
  duplicatingVersionId: number | null;
  showArchivedVersions: boolean;
  embedded?: boolean;
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
  previewDraft: PreviewDraft;
  previewResponse: CostPreviewResponse | null;
  canPreview: boolean;
  isRunningPreview: boolean;
  formatNumber: (value: number | null | undefined) => string;
  prettyJson: (value: string | null | undefined) => string;
}>();

const emit = defineEmits<{
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

const isPreviewOpen = ref(false);

const isVersionEditable = (version: CostCatalogVersion) =>
  version.first_used_at === null && !version.is_archived;

const versionStateLabel = (version: CostCatalogVersion) => {
  if (version.is_archived) {
    return "costPage.state.archived";
  }
  if (version.is_enabled) {
    return "costPage.state.active";
  }
  if (version.first_used_at !== null) {
    return "costPage.state.frozen";
  }
  return "costPage.state.draft";
};

const archivedVersionCount = computed(
  () => props.selectedCatalog?.versions.filter((version) => version.is_archived).length ?? 0,
);

const canArchiveVersion = (version: CostCatalogVersion) =>
  version.first_used_at !== null && !version.is_enabled && !version.is_archived;

const isDraftVersion = (version: CostCatalogVersion) =>
  version.first_used_at === null && !version.is_archived && !version.is_enabled;

const readOnlyStateDescription = (version: CostCatalogVersion) => {
  if (version.is_archived) {
    return "costPage.versionDetail.archivedDescription";
  }
  if (version.first_used_at !== null) {
    return "costPage.versionDetail.frozenDescription";
  }
  if (version.is_enabled) {
    return "costPage.versionDetail.activeDescription";
  }
  return "costPage.versionDetail.draftDescription";
};

watch(isPreviewOpen, (open, previousOpen) => {
  if (!open && previousOpen) {
    emit("reset-preview");
  }
});
</script>

<template>
  <div
    v-if="selectedCatalog"
    class="min-h-0"
    :class="embedded ? 'flex h-full flex-col gap-6' : 'space-y-6 border-t border-gray-100 pt-6'"
  >
    <div
      v-if="selectedCatalogVersions.length === 0"
      class="rounded-xl border border-dashed border-gray-200 bg-gray-50/60 px-6 py-12 text-center text-sm text-gray-500"
    >
      {{
        archivedVersionCount > 0 && !showArchivedVersions
          ? $t("costPage.versions.hiddenArchivedOnly")
          : $t("costPage.versions.empty")
      }}
    </div>
    <div
      v-else
      class="grid min-h-0 flex-1 grid-cols-1 gap-6 2xl:grid-cols-[minmax(20rem,24rem)_minmax(0,1fr)]"
    >
      <section class="flex min-h-0 flex-col overflow-hidden rounded-2xl border border-gray-200 bg-white">
        <div
          v-if="archivedVersionCount > 0"
          class="flex items-center justify-end border-b border-gray-100 px-3 py-3"
        >
          <Button
            variant="outline"
            size="sm"
            @click="emit('toggle-archived-visibility')"
          >
            <Archive class="mr-1.5 h-4 w-4" />
            {{
              showArchivedVersions
                ? $t("costPage.actions.hideArchivedVersions")
                : $t("costPage.actions.showArchivedVersions", {
                    count: archivedVersionCount,
                  })
            }}
          </Button>
        </div>

        <div class="min-h-0 flex-1 space-y-3 overflow-y-auto bg-gray-50/40 p-3">
          <div
            v-for="version in selectedCatalogVersions"
            :key="version.id"
            class="rounded-xl border px-4 py-4 shadow-sm transition-colors sm:px-5"
            :class="
              selectedVersionId === version.id
                ? 'border-gray-900 bg-white ring-1 ring-gray-200'
                : 'border-gray-200 bg-white hover:border-gray-300 hover:bg-gray-50/60'
            "
          >
            <div class="flex flex-col gap-4">
              <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-2">
                    <h3 class="text-sm font-semibold text-gray-900">
                      {{ version.version }}
                    </h3>
                    <Badge
                      :variant="version.is_enabled ? 'secondary' : 'outline'"
                      class="text-[11px]"
                    >
                      {{ $t(versionStateLabel(version)) }}
                    </Badge>
                    <Badge variant="outline" class="font-mono text-[11px]">
                      {{ version.currency }}
                    </Badge>
                  </div>
                  <p class="mt-1 text-sm text-gray-500">
                    {{ version.source || $t("costPage.versionDetail.manualSource") }}
                  </p>
                </div>

                <Button
                  :variant="selectedVersionId === version.id ? 'default' : 'outline'"
                  size="sm"
                  class="w-full sm:w-auto"
                  @click="emit('select-version', version.id)"
                >
                  <Eye class="mr-1 h-3.5 w-3.5" />
                  {{
                    selectedVersionId === version.id
                      ? $t("common.selected")
                      : $t("costPage.actions.viewDetail")
                  }}
                </Button>
              </div>

              <dl class="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versions.publishedAt") }}
                  </dt>
                  <dd class="mt-1 text-gray-900">
                    {{ formatTimestamp(version.created_at) || "-" }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versions.effectiveFrom") }}
                  </dt>
                  <dd class="mt-1 text-gray-900">
                    {{ formatTimestamp(version.effective_from) || "-" }}
                  </dd>
                </div>
              </dl>

              <div class="grid grid-cols-1 gap-2 sm:grid-cols-2">
                <Button
                  v-if="!version.is_archived"
                  variant="outline"
                  size="sm"
                  class="w-full justify-start"
                  :disabled="version.is_archived || togglingVersionId === version.id"
                  @click="emit('toggle-version-enabled', version)"
                >
                  {{
                    togglingVersionId === version.id
                      ? $t("common.loading")
                      : version.is_enabled
                        ? $t("costPage.actions.disableVersion")
                        : $t("costPage.actions.enableVersion")
                  }}
                </Button>
                <Button
                  v-if="!version.is_archived"
                  variant="outline"
                  size="sm"
                  class="w-full justify-start"
                  :disabled="duplicatingVersionId === version.id"
                  @click="emit('duplicate-version', version)"
                >
                  <Copy class="mr-1 h-3.5 w-3.5" />
                  {{
                    duplicatingVersionId === version.id
                      ? $t("common.loading")
                      : $t("costPage.actions.copyVersion")
                  }}
                </Button>
                <Button
                  v-if="canArchiveVersion(version)"
                  variant="outline"
                  size="sm"
                  class="w-full justify-start"
                  :disabled="managingVersionId === version.id"
                  @click="emit('archive-version', version)"
                >
                  <Archive class="mr-1 h-3.5 w-3.5" />
                  {{
                    managingVersionId === version.id
                      ? $t("common.loading")
                      : $t("costPage.actions.archiveVersion")
                  }}
                </Button>
                <Button
                  v-if="version.is_archived"
                  variant="outline"
                  size="sm"
                  class="w-full justify-start"
                  :disabled="managingVersionId === version.id"
                  @click="emit('unarchive-version', version)"
                >
                  <RotateCcw class="mr-1 h-3.5 w-3.5" />
                  {{
                    managingVersionId === version.id
                      ? $t("common.loading")
                      : $t("costPage.actions.unarchiveVersion")
                  }}
                </Button>
                <Button
                  v-if="isDraftVersion(version)"
                  variant="ghost"
                  size="sm"
                  class="w-full justify-start text-gray-500 hover:text-red-600"
                  :disabled="managingVersionId === version.id"
                  @click="emit('delete-version', version)"
                >
                  <Trash2 class="mr-1 h-3.5 w-3.5" />
                  {{
                    managingVersionId === version.id
                      ? $t("common.loading")
                      : $t("common.delete")
                  }}
                </Button>
              </div>
            </div>
          </div>
        </div>
      </section>

      <Card class="flex min-h-0 flex-col overflow-hidden rounded-2xl border-gray-200">
        <CardHeader class="space-y-5 border-b border-gray-100">
          <div class="flex flex-col gap-4 xl:flex-row xl:items-start xl:justify-between">
            <div class="min-w-0 space-y-4">
              <div class="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
                <div class="min-w-0">
                  <CardTitle class="text-lg font-semibold text-gray-900 tracking-tight">
                    {{
                      selectedVersionSummary?.version ||
                      $t("costPage.versionDetail.emptyTitle")
                    }}
                  </CardTitle>
                  <CardDescription class="mt-1 text-sm text-gray-500">
                    {{
                      selectedVersionSummary
                        ? `${selectedVersionSummary.currency} · ${selectedVersionSummary.source || $t("costPage.versionDetail.manualSource")}`
                        : $t("costPage.versionDetail.emptyDescription")
                    }}
                  </CardDescription>
                </div>
                <Badge
                  v-if="selectedVersionSummary"
                  :variant="selectedVersionSummary.is_enabled ? 'secondary' : 'outline'"
                  class="w-fit"
                >
                  {{ $t(versionStateLabel(selectedVersionSummary)) }}
                </Badge>
              </div>

              <dl v-if="selectedVersionSummary" class="grid grid-cols-1 gap-4 sm:grid-cols-3">
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versions.publishedAt") }}
                  </dt>
                  <dd class="mt-1 text-sm font-medium text-gray-900">
                    {{ formatTimestamp(selectedVersionSummary.created_at) || "-" }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versions.effectiveFrom") }}
                  </dt>
                  <dd class="mt-1 text-sm font-medium text-gray-900">
                    {{ formatTimestamp(selectedVersionSummary.effective_from) || "-" }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versions.effectiveUntil") }}
                  </dt>
                  <dd class="mt-1 text-sm font-medium text-gray-900">
                    {{ formatTimestamp(selectedVersionSummary.effective_until) || "-" }}
                  </dd>
                </div>
              </dl>
            </div>
            <div v-if="selectedVersionSummary" class="grid grid-cols-1 gap-2 sm:grid-cols-2 xl:w-[24rem] xl:grid-cols-2">
              <Button variant="outline" class="w-full" @click="isPreviewOpen = true">
                <Sparkles class="mr-1.5 h-4 w-4" />
                {{ $t("costPage.preview.title") }}
              </Button>
              <Button
                v-if="!selectedVersionSummary.is_archived"
                variant="outline"
                class="w-full"
                :disabled="togglingVersionId === selectedVersionSummary.id"
                @click="emit('toggle-version-enabled', selectedVersionSummary)"
              >
                {{
                  togglingVersionId === selectedVersionSummary.id
                    ? $t("common.loading")
                    : selectedVersionSummary.is_enabled
                      ? $t("costPage.actions.disableVersion")
                      : $t("costPage.actions.enableVersion")
                }}
              </Button>
              <Button
                v-if="!selectedVersionSummary.is_archived"
                variant="outline"
                class="w-full"
                :disabled="duplicatingVersionId === selectedVersionSummary.id"
                @click="emit('duplicate-version', selectedVersionSummary)"
              >
                <Copy class="mr-1.5 h-4 w-4" />
                {{
                  duplicatingVersionId === selectedVersionSummary.id
                    ? $t("common.loading")
                    : $t("costPage.actions.copyVersion")
                }}
              </Button>
              <Button
                v-if="canArchiveVersion(selectedVersionSummary)"
                variant="outline"
                class="w-full"
                :disabled="managingVersionId === selectedVersionSummary.id"
                @click="emit('archive-version', selectedVersionSummary)"
              >
                <Archive class="mr-1.5 h-4 w-4" />
                {{
                  managingVersionId === selectedVersionSummary.id
                    ? $t("common.loading")
                    : $t("costPage.actions.archiveVersion")
                }}
              </Button>
              <Button
                v-if="selectedVersionSummary.is_archived"
                variant="outline"
                class="w-full"
                :disabled="managingVersionId === selectedVersionSummary.id"
                @click="emit('unarchive-version', selectedVersionSummary)"
              >
                <RotateCcw class="mr-1.5 h-4 w-4" />
                {{
                  managingVersionId === selectedVersionSummary.id
                    ? $t("common.loading")
                    : $t("costPage.actions.unarchiveVersion")
                }}
              </Button>
              <Button
                v-if="isDraftVersion(selectedVersionSummary)"
                variant="ghost"
                class="w-full text-gray-600 hover:text-red-600"
                :disabled="managingVersionId === selectedVersionSummary.id"
                @click="emit('delete-version', selectedVersionSummary)"
              >
                <Trash2 class="mr-1.5 h-4 w-4" />
                {{
                  managingVersionId === selectedVersionSummary.id
                    ? $t("common.loading")
                    : $t("costPage.actions.deleteDraftVersion")
                }}
              </Button>
              <Button
                v-if="isVersionEditable(selectedVersionSummary)"
                class="w-full"
                @click="emit('create-component')"
              >
                <Plus class="mr-1.5 h-4 w-4" />
                {{ $t("costPage.versionDetail.addComponent") }}
              </Button>
            </div>
          </div>
          <div class="border-t border-gray-100 pt-4">
            <h3 class="text-base font-semibold text-gray-900">
              {{ $t("costPage.versionDetail.componentsTitle") }}
            </h3>
            <p class="mt-1 text-sm text-gray-500">
              {{ $t("costPage.versionDetail.componentsDescription") }}
            </p>
          </div>
        </CardHeader>
        <CardContent class="min-h-0 flex-1 space-y-6 overflow-y-auto pt-6">
          <div
            v-if="selectedVersionSummary"
            class="rounded-xl bg-gray-50 px-4 py-3 text-sm text-gray-500"
          >
            {{ $t(readOnlyStateDescription(selectedVersionSummary)) }}
          </div>

          <div
            v-if="isLoadingVersionDetail"
            class="py-10 text-center text-sm text-gray-500"
          >
            {{ $t("costPage.versionDetail.loading") }}
          </div>
          <div
            v-else-if="components.length === 0"
            class="rounded-xl border border-dashed border-gray-200 bg-gray-50/60 px-6 py-12 text-center text-sm text-gray-500"
          >
            {{ $t("costPage.versionDetail.emptyComponents") }}
          </div>
          <div v-else class="space-y-4">
            <div
              v-for="component in components"
              :key="component.id"
              class="rounded-2xl border border-gray-200 bg-white p-4 sm:p-5"
            >
              <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
                <div class="min-w-0">
                  <div class="flex flex-wrap items-center gap-2">
                    <p class="text-sm font-semibold text-gray-900">
                      {{ meterLabel(component.meter_key) }}
                    </p>
                    <Badge variant="outline" class="font-mono text-[11px]">
                      {{ component.meter_key }}
                    </Badge>
                    <Badge variant="secondary" class="text-[11px]">
                      {{ chargeKindLabel(component.charge_kind) }}
                    </Badge>
                    <Badge variant="outline" class="text-[11px]">
                      P{{ component.priority }}
                    </Badge>
                  </div>
                  <p class="mt-2 text-sm text-gray-500">
                    {{ component.description || $t("costPage.versionDetail.noDescription") }}
                  </p>
                </div>
                <div class="flex gap-2">
                  <Button
                    variant="ghost"
                    size="sm"
                    :disabled="!selectedVersionSummary || !isVersionEditable(selectedVersionSummary)"
                    @click="emit('edit-component', component)"
                  >
                    <Edit class="mr-1 h-3.5 w-3.5" />
                    {{ $t("common.edit") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="text-gray-500 hover:text-red-600"
                    :disabled="!selectedVersionSummary || !isVersionEditable(selectedVersionSummary)"
                    @click="emit('delete-component', component)"
                  >
                    <Trash2 class="mr-1 h-3.5 w-3.5" />
                    {{ $t("common.delete") }}
                  </Button>
                </div>
              </div>

              <dl class="mt-4 grid grid-cols-1 gap-4 border-t border-gray-100 pt-4 sm:grid-cols-2 xl:grid-cols-4">
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versionDetail.chargeKind") }}
                  </dt>
                  <dd class="mt-1 text-sm font-medium text-gray-900">
                    {{ chargeKindLabel(component.charge_kind) }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versionDetail.unitPrice") }}
                  </dt>
                  <dd class="mt-1 font-mono text-sm text-gray-900">
                    {{
                      formatRateDisplay(
                        component.unit_price_nanos,
                        component.meter_key,
                        selectedVersionSummary?.currency,
                      )
                    }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versionDetail.flatFee") }}
                  </dt>
                  <dd class="mt-1 font-mono text-sm text-gray-900">
                    {{
                      formatPriceFromNanos(
                        component.flat_fee_nanos,
                        selectedVersionSummary?.currency,
                      )
                    }}
                  </dd>
                </div>
                <div>
                  <dt class="text-xs font-medium uppercase tracking-wider text-gray-500">
                    {{ $t("costPage.versionDetail.matchAttributes") }}
                  </dt>
                  <dd class="mt-1 text-sm text-gray-900">
                    {{
                      component.match_attributes_json
                        ? $t("costPage.versionDetail.hasMatchAttributes")
                        : "-"
                    }}
                  </dd>
                </div>
              </dl>

              <div
                v-if="component.charge_kind === 'tiered_per_unit' && component.tier_config_json"
                class="mt-4 rounded-xl bg-gray-50 px-4 py-3"
              >
                <div class="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
                  <div class="text-sm font-medium text-gray-900">
                    {{ $t("costPage.componentEditor.tiers.title") }}
                  </div>
                  <Badge variant="outline" class="text-[11px]">
                    {{
                      tierBasisLabel(
                        parseTierConfig(
                          component.tier_config_json,
                          component.meter_key,
                          selectedVersionSummary?.currency,
                        )?.basis ||
                          "meter_quantity",
                      )
                    }}
                  </Badge>
                </div>
                <div class="mt-3 grid grid-cols-1 gap-2 lg:grid-cols-2">
                  <div
                    v-for="(tier, index) in parseTierConfig(
                      component.tier_config_json,
                      component.meter_key,
                      selectedVersionSummary?.currency,
                    )?.tiers || []"
                    :key="`${component.id}-${index}`"
                    class="rounded-lg border border-gray-200 bg-white px-3 py-2.5 text-sm text-gray-700"
                  >
                    <div class="font-medium text-gray-900">
                      {{ $t("costPage.componentEditor.tiers.rowLabel", { index: index + 1 }) }}
                    </div>
                    <div class="mt-1">
                      {{
                        tier.up_to
                          ? $t("costPage.componentEditor.tiers.upToValue", { value: tier.up_to })
                          : $t("costPage.componentEditor.tiers.unbounded")
                      }}
                    </div>
                    <div class="font-mono text-xs text-gray-500">
                      {{ tryFormatRateInputDisplay(tier.unit_price, component.meter_key) }}
                    </div>
                  </div>
                </div>
              </div>

              <div
                v-if="component.match_attributes_json"
                class="mt-4 rounded-xl bg-gray-50 px-4 py-3"
              >
                <div class="text-sm font-medium text-gray-900">
                  {{ $t("costPage.versionDetail.matchAttributes") }}
                </div>
                <pre class="mt-2 overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100">{{ prettyJson(component.match_attributes_json) }}</pre>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>
    </div>

    <Dialog v-model:open="isPreviewOpen">
      <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-5xl">
        <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
          <DialogTitle>{{ $t("costPage.preview.title") }}</DialogTitle>
        </DialogHeader>
        <div class="min-h-0 flex-1 overflow-y-auto">
          <CostPreviewSection
            class="p-4 sm:p-6"
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
      </DialogContent>
    </Dialog>
  </div>
</template>
