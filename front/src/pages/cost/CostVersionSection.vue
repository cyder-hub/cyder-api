<script setup lang="ts">
import { Edit, Eye, Plus, Trash2 } from "lucide-vue-next";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { formatPriceFromNanos, formatTimestamp } from "@/lib/utils";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostComponent,
} from "@/store/types";
import { parseTierConfig } from "./helpers";

defineProps<{
  selectedCatalog: CostCatalogListItem | null;
  selectedCatalogVersions: CostCatalogVersion[];
  selectedVersionId: number | null;
  selectedVersionSummary: CostCatalogVersion | null;
  components: CostComponent[];
  isLoadingVersionDetail: boolean;
  togglingVersionId: number | null;
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
  prettyJson: (value: string | null | undefined) => string;
}>();

const emit = defineEmits<{
  (e: "create-version"): void;
  (e: "select-version", versionId: number): void;
  (e: "toggle-version-enabled", version: CostCatalogVersion): void;
  (e: "create-component"): void;
  (e: "edit-component", component: CostComponent): void;
  (e: "delete-component", component: CostComponent): void;
}>();
</script>

<template>
  <div
    v-if="selectedCatalog"
    class="space-y-6"
    :class="embedded ? '' : 'border-t border-gray-100 pt-6'"
  >
    <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
      <div>
        <h2 class="text-lg font-medium text-gray-900">
          {{ $t("costPage.versions.title") }}
        </h2>
        <p class="mt-1 text-sm text-gray-500">
          {{ selectedCatalog.catalog.name }}
        </p>
      </div>
      <Button @click="emit('create-version')">
        <Plus class="mr-1.5 h-4 w-4" />
        {{ $t("costPage.versions.add") }}
      </Button>
    </div>

    <div
      v-if="selectedCatalogVersions.length === 0"
      class="rounded-xl border border-dashed border-gray-200 bg-gray-50/60 px-6 py-12 text-center text-sm text-gray-500"
    >
      {{ $t("costPage.versions.empty") }}
    </div>
    <div
      v-else
      class="grid grid-cols-1 gap-3 xl:grid-cols-[minmax(0,0.95fr)_minmax(0,1.05fr)]"
    >
      <div class="space-y-3">
        <MobileCrudCard
          v-for="version in selectedCatalogVersions"
          :key="version.id"
          :title="version.version"
          :description="`${version.currency} · ${version.source || $t('costPage.versionDetail.manualSource')}`"
          :selected="selectedVersionId === version.id"
        >
          <template #header>
            <Badge :variant="version.is_enabled ? 'secondary' : 'outline'" class="text-[11px]">
              {{
                version.is_enabled
                  ? $t("costPage.state.enabled")
                  : $t("costPage.state.disabled")
              }}
            </Badge>
          </template>

          <div class="grid grid-cols-2 gap-2 text-xs text-gray-500">
            <div class="rounded-lg border border-gray-100 px-3 py-2.5">
              <div>{{ $t("costPage.versions.publishedAt") }}</div>
              <div class="mt-1 text-gray-900">
                {{ formatTimestamp(version.created_at) || "-" }}
              </div>
            </div>
            <div class="rounded-lg border border-gray-100 px-3 py-2.5">
              <div>{{ $t("costPage.versions.effectiveFrom") }}</div>
              <div class="mt-1 text-gray-900">
                {{ formatTimestamp(version.effective_from) || "-" }}
              </div>
            </div>
          </div>

          <template #actions>
            <div class="flex flex-col gap-2">
              <Button
                :variant="selectedVersionId === version.id ? 'default' : 'outline'"
                size="sm"
                class="w-full"
                @click="emit('select-version', version.id)"
              >
                <Eye class="mr-1 h-3.5 w-3.5" />
                {{
                  selectedVersionId === version.id
                    ? $t("common.selected")
                    : $t("costPage.actions.viewDetail")
                }}
              </Button>
              <Button
                variant="outline"
                size="sm"
                class="w-full"
                :disabled="togglingVersionId === version.id"
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
            </div>
          </template>
        </MobileCrudCard>
      </div>

      <Card class="rounded-2xl border-gray-200">
        <CardHeader>
          <div class="flex items-start justify-between gap-3">
            <div>
              <CardTitle>
                {{
                  selectedVersionSummary?.version ||
                  $t("costPage.versionDetail.emptyTitle")
                }}
              </CardTitle>
              <CardDescription class="mt-1">
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
            >
              {{
                selectedVersionSummary.is_enabled
                  ? $t("costPage.state.enabled")
                  : $t("costPage.state.disabled")
              }}
            </Badge>
          </div>
        </CardHeader>
        <CardContent class="space-y-4">
          <div v-if="selectedVersionSummary" class="grid grid-cols-1 gap-3 sm:grid-cols-3">
            <div class="rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.versions.publishedAt") }}</div>
              <div class="mt-1 text-sm font-medium text-gray-900">
                {{ formatTimestamp(selectedVersionSummary.created_at) || "-" }}
              </div>
            </div>
            <div class="rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.versions.effectiveFrom") }}</div>
              <div class="mt-1 text-sm font-medium text-gray-900">
                {{ formatTimestamp(selectedVersionSummary.effective_from) || "-" }}
              </div>
            </div>
            <div class="rounded-xl border border-gray-200 bg-gray-50/60 px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.versions.effectiveUntil") }}</div>
              <div class="mt-1 text-sm font-medium text-gray-900">
                {{ formatTimestamp(selectedVersionSummary.effective_until) || "-" }}
              </div>
            </div>
          </div>

          <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div>
              <h3 class="text-sm font-semibold text-gray-900">
                {{ $t("costPage.versionDetail.componentsTitle") }}
              </h3>
              <p class="mt-1 text-sm text-gray-500">
                {{ $t("costPage.versionDetail.componentsDescription") }}
              </p>
            </div>
            <div v-if="selectedVersionSummary" class="flex flex-col gap-2 sm:flex-row">
              <Button
                variant="outline"
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
              <Button @click="emit('create-component')">
                <Plus class="mr-1.5 h-4 w-4" />
                {{ $t("costPage.versionDetail.addComponent") }}
              </Button>
            </div>
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
          <div v-else class="space-y-3">
            <div
              v-for="component in components"
              :key="component.id"
              class="rounded-2xl border border-gray-200 bg-gray-50/60 p-4"
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
                  <Button variant="ghost" size="sm" @click="emit('edit-component', component)">
                    <Edit class="mr-1 h-3.5 w-3.5" />
                    {{ $t("common.edit") }}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="text-gray-500 hover:text-red-600"
                    @click="emit('delete-component', component)"
                  >
                    <Trash2 class="mr-1 h-3.5 w-3.5" />
                    {{ $t("common.delete") }}
                  </Button>
                </div>
              </div>

              <div class="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-4">
                <div class="rounded-xl border border-white bg-white px-4 py-3">
                  <div class="text-xs text-gray-500">
                    {{ $t("costPage.versionDetail.chargeKind") }}
                  </div>
                  <div class="mt-1 text-sm font-medium text-gray-900">
                    {{ chargeKindLabel(component.charge_kind) }}
                  </div>
                </div>
                <div class="rounded-xl border border-white bg-white px-4 py-3">
                  <div class="text-xs text-gray-500">
                    {{ $t("costPage.versionDetail.unitPrice") }}
                  </div>
                  <div class="mt-1 font-mono text-sm text-gray-900">
                    {{
                      formatRateDisplay(
                        component.unit_price_nanos,
                        component.meter_key,
                        selectedVersionSummary?.currency,
                      )
                    }}
                  </div>
                </div>
                <div class="rounded-xl border border-white bg-white px-4 py-3">
                  <div class="text-xs text-gray-500">
                    {{ $t("costPage.versionDetail.flatFee") }}
                  </div>
                  <div class="mt-1 font-mono text-sm text-gray-900">
                    {{
                      formatPriceFromNanos(
                        component.flat_fee_nanos,
                        selectedVersionSummary?.currency,
                      )
                    }}
                  </div>
                </div>
                <div class="rounded-xl border border-white bg-white px-4 py-3">
                  <div class="text-xs text-gray-500">
                    {{ $t("costPage.versionDetail.matchAttributes") }}
                  </div>
                  <div class="mt-1 text-sm text-gray-900">
                    {{
                      component.match_attributes_json
                        ? $t("costPage.versionDetail.hasMatchAttributes")
                        : "-"
                    }}
                  </div>
                </div>
              </div>

              <div
                v-if="component.charge_kind === 'tiered_per_unit' && component.tier_config_json"
                class="mt-4 rounded-xl border border-white bg-white px-4 py-3"
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
                <div class="mt-3 grid grid-cols-1 gap-2 md:grid-cols-2">
                  <div
                    v-for="(tier, index) in parseTierConfig(
                      component.tier_config_json,
                      component.meter_key,
                      selectedVersionSummary?.currency,
                    )?.tiers || []"
                    :key="`${component.id}-${index}`"
                    class="rounded-lg border border-gray-100 bg-gray-50 px-3 py-2.5 text-sm text-gray-700"
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
                class="mt-4 rounded-xl border border-white bg-white px-4 py-3"
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
  </div>
</template>
