<script setup lang="ts">
import { Sparkles } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { formatPriceFromNanos } from "@/lib/utils";
import type { CostCatalogVersion, CostPreviewResponse } from "@/store/types";
import type { PreviewDraft } from "./types";

defineProps<{
  selectedVersionSummary: CostCatalogVersion | null;
  previewDraft: PreviewDraft;
  previewResponse: CostPreviewResponse | null;
  canPreview: boolean;
  isRunningPreview: boolean;
  embedded?: boolean;
  meterLabel: (meterKey: string) => string;
  chargeKindLabel: (chargeKind: string) => string;
  formatRateDisplay: (
    micros: number | null | undefined,
    meterKey: string,
    currency?: string | null,
    suffix?: boolean,
  ) => string;
  formatNumber: (value: number | null | undefined) => string;
}>();

const emit = defineEmits<{
  (e: "apply-sample"): void;
  (e: "run-preview"): void;
}>();
</script>

<template>
  <div
    v-if="selectedVersionSummary"
    :class="embedded ? '' : 'border-t border-gray-100 pt-6'"
  >
    <Card class="rounded-2xl border-gray-200">
      <CardHeader>
        <div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
          <div>
            <CardTitle>{{ $t("costPage.preview.title") }}</CardTitle>
            <CardDescription class="mt-1">
              {{ $t("costPage.preview.description") }}
            </CardDescription>
          </div>
          <div class="flex flex-col gap-2 sm:flex-row">
            <Button variant="outline" @click="emit('apply-sample')">
              <Sparkles class="mr-1.5 h-4 w-4" />
              {{ $t("costPage.preview.applySample") }}
            </Button>
            <Button :disabled="!canPreview || isRunningPreview" @click="emit('run-preview')">
              {{ isRunningPreview ? $t("costPage.preview.running") : $t("costPage.preview.run") }}
            </Button>
          </div>
        </div>
      </CardHeader>
      <CardContent class="space-y-5">
        <div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          <div class="space-y-1.5">
            <Label for="preview-total-input">{{ $t("costPage.preview.fields.totalInputTokens") }}</Label>
            <Input id="preview-total-input" v-model="previewDraft.total_input_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-total-output">{{ $t("costPage.preview.fields.totalOutputTokens") }}</Label>
            <Input id="preview-total-output" v-model="previewDraft.total_output_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-input-text">{{ $t("costPage.preview.fields.inputTextTokens") }}</Label>
            <Input id="preview-input-text" v-model="previewDraft.input_text_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-output-text">{{ $t("costPage.preview.fields.outputTextTokens") }}</Label>
            <Input id="preview-output-text" v-model="previewDraft.output_text_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-input-image">{{ $t("costPage.preview.fields.inputImageTokens") }}</Label>
            <Input id="preview-input-image" v-model="previewDraft.input_image_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-output-image">{{ $t("costPage.preview.fields.outputImageTokens") }}</Label>
            <Input id="preview-output-image" v-model="previewDraft.output_image_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-cache-read">{{ $t("costPage.preview.fields.cacheReadTokens") }}</Label>
            <Input id="preview-cache-read" v-model="previewDraft.cache_read_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-cache-write">{{ $t("costPage.preview.fields.cacheWriteTokens") }}</Label>
            <Input id="preview-cache-write" v-model="previewDraft.cache_write_tokens" inputmode="numeric" />
          </div>
          <div class="space-y-1.5">
            <Label for="preview-reasoning">{{ $t("costPage.preview.fields.reasoningTokens") }}</Label>
            <Input id="preview-reasoning" v-model="previewDraft.reasoning_tokens" inputmode="numeric" />
          </div>
        </div>

        <div
          v-if="previewResponse"
          class="space-y-5 rounded-2xl border border-gray-200 bg-gray-50/60 p-4"
        >
          <div class="grid grid-cols-1 gap-3 md:grid-cols-3">
            <div class="rounded-xl border border-white bg-white px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.preview.result.totalCost") }}</div>
              <div class="mt-1 text-lg font-semibold text-gray-900">
                {{
                  formatPriceFromNanos(
                    previewResponse.result.total_cost_nanos,
                    previewResponse.result.currency,
                  )
                }}
              </div>
            </div>
            <div class="rounded-xl border border-white bg-white px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.preview.result.currency") }}</div>
              <div class="mt-1 font-mono text-lg font-semibold text-gray-900">
                {{ previewResponse.result.currency }}
              </div>
            </div>
            <div class="rounded-xl border border-white bg-white px-4 py-3">
              <div class="text-xs text-gray-500">{{ $t("costPage.preview.result.detailLines") }}</div>
              <div class="mt-1 text-lg font-semibold text-gray-900">
                {{ previewResponse.result.detail_lines.length }}
              </div>
            </div>
          </div>

          <div class="space-y-2">
            <h3 class="text-sm font-semibold text-gray-900">
              {{ $t("costPage.preview.ledgerTitle") }}
            </h3>
            <div class="grid grid-cols-1 gap-2 md:grid-cols-2">
              <div
                v-for="(item, index) in previewResponse.ledger.items"
                :key="`${item.meter_key}-${index}`"
                class="rounded-xl border border-white bg-white px-4 py-3"
              >
                <div class="flex flex-wrap items-center gap-2">
                  <span class="text-sm font-medium text-gray-900">
                    {{ meterLabel(item.meter_key) }}
                  </span>
                  <Badge variant="outline" class="font-mono text-[11px]">
                    {{ item.meter_key }}
                  </Badge>
                </div>
                <div class="mt-2 font-mono text-sm text-gray-700">
                  {{ formatNumber(item.quantity) }} {{ item.unit }}
                </div>
                <pre
                  v-if="item.attributes && Object.keys(item.attributes).length > 0"
                  class="mt-2 overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100"
                >{{ JSON.stringify(item.attributes, null, 2) }}</pre>
              </div>
            </div>
          </div>

          <div class="space-y-2">
            <h3 class="text-sm font-semibold text-gray-900">
              {{ $t("costPage.preview.detailLinesTitle") }}
            </h3>
            <div class="space-y-2">
              <div
                v-for="(line, index) in previewResponse.result.detail_lines"
                :key="`${line.component_id}-${index}`"
                class="rounded-xl border border-white bg-white px-4 py-3"
              >
                <div class="flex flex-col gap-2 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <div class="flex flex-wrap items-center gap-2">
                      <span class="text-sm font-medium text-gray-900">
                        {{ meterLabel(line.meter_key) }}
                      </span>
                      <Badge variant="outline" class="font-mono text-[11px]">
                        {{ line.meter_key }}
                      </Badge>
                      <Badge variant="secondary" class="text-[11px]">
                        {{ chargeKindLabel(line.charge_kind) }}
                      </Badge>
                    </div>
                    <p class="mt-2 text-sm text-gray-500">
                      {{ line.description || $t("costPage.versionDetail.noDescription") }}
                    </p>
                  </div>
                  <div class="text-right">
                    <div class="text-sm font-semibold text-gray-900">
                      {{
                        formatPriceFromNanos(
                          line.amount_nanos,
                          previewResponse.result.currency,
                        )
                      }}
                    </div>
                    <div class="mt-1 text-xs text-gray-500">
                      {{ formatNumber(line.quantity) }} {{ line.unit }}
                      <span v-if="line.unit_price_nanos !== null">
                        ·
                        {{
                          formatRateDisplay(
                            line.unit_price_nanos,
                            line.meter_key,
                            previewResponse.result.currency,
                          )
                        }}
                      </span>
                    </div>
                  </div>
                </div>
                <pre
                  v-if="line.attributes && Object.keys(line.attributes).length > 0"
                  class="mt-2 overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100"
                >{{ JSON.stringify(line.attributes, null, 2) }}</pre>
              </div>
            </div>
          </div>

          <div
            v-if="previewResponse.result.unmatched_items.length > 0 || previewResponse.result.warnings.length > 0"
            class="grid grid-cols-1 gap-3 md:grid-cols-2"
          >
            <div
              v-if="previewResponse.result.unmatched_items.length > 0"
              class="rounded-xl border border-amber-200 bg-amber-50 px-4 py-3"
            >
              <div class="text-sm font-semibold text-amber-900">
                {{ $t("costPage.preview.unmatchedTitle") }}
              </div>
              <ul class="mt-2 space-y-1 text-sm text-amber-800">
                <li
                  v-for="(item, index) in previewResponse.result.unmatched_items"
                  :key="`${item}-${index}`"
                >
                  {{ item }}
                </li>
              </ul>
            </div>
            <div
              v-if="previewResponse.result.warnings.length > 0"
              class="rounded-xl border border-amber-200 bg-amber-50 px-4 py-3"
            >
              <div class="text-sm font-semibold text-amber-900">
                {{ $t("costPage.preview.warningsTitle") }}
              </div>
              <ul class="mt-2 space-y-1 text-sm text-amber-800">
                <li
                  v-for="(warning, index) in previewResponse.result.warnings"
                  :key="`${warning}-${index}`"
                >
                  {{ warning }}
                </li>
              </ul>
            </div>
          </div>
        </div>
      </CardContent>
    </Card>
  </div>
</template>
