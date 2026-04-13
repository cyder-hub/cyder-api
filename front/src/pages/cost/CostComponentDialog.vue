<script setup lang="ts">
import { Plus, Trash2 } from "lucide-vue-next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  CHARGE_KIND_OPTIONS,
  MATCH_ATTRIBUTES_PLACEHOLDER,
  METER_OPTIONS,
  TIER_BASIS_OPTIONS,
  isMillionTokenMeter,
} from "./helpers";
import type { ComponentDraft } from "./types";

defineProps<{
  open: boolean;
  draft: ComponentDraft;
  isSaving: boolean;
  selectedCurrency?: string | null;
  meterLabel: (meterKey: string) => string;
  chargeKindLabel: (chargeKind: string) => string;
}>();

const emit = defineEmits<{
  (e: "update:open", value: boolean): void;
  (e: "save"): void;
  (e: "add-tier"): void;
  (e: "remove-tier", index: number): void;
}>();
</script>

<template>
  <Dialog :open="open" @update:open="(value) => emit('update:open', value)">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-4xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6 sm:pb-4">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{
            draft.id === null
              ? $t("costPage.componentEditor.titleAdd")
              : $t("costPage.componentEditor.titleEdit")
          }}
        </DialogTitle>
      </DialogHeader>
      <form class="contents" @submit.prevent="emit('save')">
        <div class="space-y-5 overflow-y-auto px-4 py-4 sm:px-6">
          <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div class="space-y-1.5">
              <Label>{{ $t("costPage.componentEditor.meterKey") }}</Label>
              <Select v-model="draft.meter_key">
                <SelectTrigger class="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in METER_OPTIONS"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ $t(option.labelKey) }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="space-y-1.5">
              <Label>{{ $t("costPage.componentEditor.chargeKind") }}</Label>
              <Select v-model="draft.charge_kind">
                <SelectTrigger class="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="option in CHARGE_KIND_OPTIONS"
                    :key="option.value"
                    :value="option.value"
                  >
                    {{ $t(option.labelKey) }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <div class="space-y-1.5">
              <Label for="component-priority">{{
                $t("costPage.componentEditor.priority")
              }}</Label>
              <Input
                id="component-priority"
                v-model="draft.priority"
                inputmode="numeric"
              />
            </div>
            <div class="space-y-1.5">
              <Label for="component-description">{{
                $t("costPage.componentEditor.description")
              }}</Label>
              <Input id="component-description" v-model="draft.description" />
            </div>
          </div>

          <div
            v-if="draft.charge_kind === 'per_unit'"
            class="rounded-2xl border border-gray-200 bg-gray-50/60 p-4"
          >
            <div class="space-y-1.5">
              <Label for="component-unit-price">{{
                $t("costPage.componentEditor.unitPrice")
              }}</Label>
              <Input
                id="component-unit-price"
                v-model="draft.unit_price"
                inputmode="decimal"
              />
              <p class="text-xs text-gray-500">
                {{
                  $t(
                    isMillionTokenMeter(draft.meter_key)
                      ? "costPage.componentEditor.unitPriceHintPerMillion"
                      : "costPage.componentEditor.unitPriceHintPerUnit",
                    { currency: selectedCurrency || "USD" },
                  )
                }}
              </p>
            </div>
          </div>

          <div
            v-else-if="draft.charge_kind === 'flat'"
            class="rounded-2xl border border-gray-200 bg-gray-50/60 p-4"
          >
            <div class="space-y-1.5">
              <Label for="component-flat-fee">{{
                $t("costPage.componentEditor.flatFee")
              }}</Label>
              <Input
                id="component-flat-fee"
                v-model="draft.flat_fee"
                inputmode="decimal"
              />
              <p class="text-xs text-gray-500">
                {{
                  $t("costPage.componentEditor.flatFeeHint", {
                    currency: selectedCurrency || "USD",
                  })
                }}
              </p>
            </div>
          </div>

          <div
            v-else
            class="rounded-2xl border border-gray-200 bg-gray-50/60 p-4"
          >
            <div class="flex flex-col gap-4">
              <div class="space-y-1.5">
                <Label>{{ $t("costPage.componentEditor.tiers.basis") }}</Label>
                <Select v-model="draft.tier_basis">
                  <SelectTrigger class="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem
                      v-for="option in TIER_BASIS_OPTIONS"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ $t(option.labelKey) }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div class="flex items-center justify-between">
                <div>
                  <div class="text-sm font-medium text-gray-900">
                    {{ $t("costPage.componentEditor.tiers.title") }}
                  </div>
                  <div class="mt-1 text-sm text-gray-500">
                    {{ $t("costPage.componentEditor.tiers.description") }}
                  </div>
                  <div class="mt-1 text-xs text-gray-500">
                    {{
                      $t(
                        isMillionTokenMeter(draft.meter_key)
                          ? "costPage.componentEditor.tiers.unitPriceHintPerMillion"
                          : "costPage.componentEditor.tiers.unitPriceHintPerUnit",
                        { currency: selectedCurrency || "USD" },
                      )
                    }}
                  </div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  @click="emit('add-tier')"
                >
                  <Plus class="mr-1 h-3.5 w-3.5" />
                  {{ $t("costPage.componentEditor.tiers.add") }}
                </Button>
              </div>

              <div class="space-y-3">
                <div
                  v-for="(tier, index) in draft.tiers"
                  :key="index"
                  class="grid grid-cols-1 gap-3 rounded-xl border border-gray-200 bg-white p-4 md:grid-cols-[minmax(0,1fr)_minmax(0,1fr)_auto]"
                >
                  <div class="space-y-1.5">
                    <Label :for="`tier-up-to-${index}`">
                      {{ $t("costPage.componentEditor.tiers.upTo") }}
                    </Label>
                    <Input
                      :id="`tier-up-to-${index}`"
                      v-model="tier.up_to"
                      :placeholder="
                        $t(
                          'costPage.componentEditor.tiers.unboundedPlaceholder',
                        )
                      "
                      inputmode="numeric"
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label :for="`tier-price-${index}`">
                      {{ $t("costPage.componentEditor.tiers.unitPrice") }}
                    </Label>
                    <Input
                      :id="`tier-price-${index}`"
                      v-model="tier.unit_price"
                      inputmode="decimal"
                    />
                  </div>
                  <div class="flex items-end">
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      class="w-full md:w-auto"
                      @click="emit('remove-tier', index)"
                    >
                      <Trash2 class="mr-1 h-3.5 w-3.5" />
                      {{ $t("common.delete") }}
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </div>

          <div class="grid grid-cols-1 gap-4 md:grid-cols-2">
            <div class="space-y-1.5">
              <Label for="component-match-attrs">{{
                $t("costPage.componentEditor.matchAttributes")
              }}</Label>
              <textarea
                id="component-match-attrs"
                v-model="draft.match_attributes_json"
                rows="7"
                class="flex min-h-[168px] w-full rounded-md border border-input bg-transparent px-3 py-2 text-sm font-mono shadow-xs outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
                :placeholder="MATCH_ATTRIBUTES_PLACEHOLDER"
              />
            </div>
            <div class="space-y-1.5">
              <Label>{{ $t("costPage.componentEditor.previewConfig") }}</Label>
              <div class="rounded-xl border border-gray-200 bg-gray-50/60 p-4">
                <div class="flex flex-wrap gap-2">
                  <Badge variant="outline">{{
                    meterLabel(draft.meter_key)
                  }}</Badge>
                  <Badge variant="secondary">{{
                    chargeKindLabel(draft.charge_kind)
                  }}</Badge>
                  <Badge variant="outline">P{{ draft.priority || "-" }}</Badge>
                </div>
                <pre
                  v-if="draft.match_attributes_json.trim()"
                  class="mt-3 overflow-x-auto rounded-lg bg-gray-950 px-3 py-3 text-xs text-gray-100"
                  >{{ draft.match_attributes_json }}</pre
                >
                <p v-else class="mt-3 text-sm text-gray-500">
                  {{ $t("costPage.componentEditor.previewConfigEmpty") }}
                </p>
              </div>
            </div>
          </div>
        </div>
        <DialogFooter
          class="border-t border-gray-100 px-4 py-4 sm:flex-row sm:justify-end sm:px-6"
          :show-close-button="true"
        >
          <Button type="submit" :disabled="isSaving" @click="emit('save')">
            {{ isSaving ? $t("common.saving") : $t("common.save") }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
