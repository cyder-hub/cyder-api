<template>
  <div class="rounded-lg border border-gray-200 p-3.5 sm:p-4">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div class="min-w-0">
        <div class="flex flex-wrap items-center gap-1.5">
          <Badge variant="outline" class="font-mono text-[10px]">{{ suffix ?? t("requestPatchVariant.states.base") }}</Badge>
          <Badge v-if="sourceVariant" :variant="sourceVariant.variant.enabled ? 'secondary' : 'outline'" class="font-mono text-[10px]">
            {{ sourceVariant.variant.enabled ? t("requestPatchVariant.states.sourceVariant") : t("requestPatchVariant.states.disabled") }}
          </Badge>
          <Badge v-if="modelVariant" :variant="modelVariant.variant.enabled ? 'default' : 'outline'" class="font-mono text-[10px]">
            {{ modelVariant.variant.enabled ? t("requestPatchVariant.states.modelOverride") : t("requestPatchVariant.states.disabled") }}
          </Badge>
          <Badge v-if="!sourceBound" variant="outline" class="font-mono text-[10px]">{{ t("requestPatchVariant.states.unboundDormant") }}</Badge>
        </div>
        <p class="mt-1 text-xs leading-5 text-gray-500">
          {{ suffix === null ? t("requestPatchVariant.model.baseDescription") : t("requestPatchVariant.model.suffixDescription") }}
        </p>
      </div>
      <div class="flex flex-wrap items-center gap-2 sm:justify-end">
        <div v-if="suffix !== null && !modelVariant" class="flex items-center gap-2 rounded-md bg-gray-50 px-2.5 py-2">
          <Checkbox
            :model-value="true"
            :disabled="!sourceBound"
            @update:model-value="(value) => onToggleInherited(state.source.id, suffix, value === true)"
          />
          <span class="text-xs font-medium text-gray-600">{{ t("requestPatchVariant.actions.allowModel") }}</span>
        </div>
        <Button v-if="disabledTombstone" variant="outline" size="sm" @click="onRestore(state.source.id, modelVariant!)">
          {{ t("requestPatchVariant.actions.restore") }}
        </Button>
        <Button v-else variant="ghost" size="sm" :disabled="!sourceBound" @click="onOpenEditor(state.source.id, suffix, modelVariant)">
          {{ modelVariant ? t("common.edit") : t("requestPatchVariant.actions.addModelRule") }}
        </Button>
        <Button
          v-if="modelVariant && !disabledTombstone"
          variant="ghost"
          size="sm"
          class="text-gray-400 hover:text-red-600"
          @click="onDelete(state.source.id, modelVariant)"
        >
          {{ t("common.delete") }}
        </Button>
      </div>
    </div>

    <div class="mt-4 grid gap-3 sm:grid-cols-2">
      <div class="rounded-md border border-gray-100 bg-gray-50/60 px-3 py-3">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.explain.layers") }}</p>
        <div class="mt-2 flex flex-wrap gap-1.5">
          <Badge v-for="layer in evaluation.layers" :key="`${layer.origin}-${layer.variant_id}`" :variant="statusVariant(layer.status)" class="font-mono text-[10px]" :title="layer.reason ?? undefined">
            {{ originLabel(layer.origin) }} · {{ statusLabel(layer.status) }}
          </Badge>
        </div>
      </div>
      <div class="rounded-md border border-gray-100 bg-gray-50/60 px-3 py-3">
        <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">{{ t("requestPatchVariant.explain.effective") }}</p>
        <p class="mt-1 font-mono text-sm text-gray-900">{{ evaluation.effective_rules.length }}</p>
        <p v-if="evaluation.failure_reason" class="mt-1 text-xs leading-5 text-red-700">{{ evaluation.failure_reason }}</p>
      </div>
    </div>

    <p v-if="metadataOnly" class="mt-3 rounded-md border border-blue-100 bg-blue-50 px-3 py-2 text-xs leading-5 text-blue-800">
      {{ t("requestPatchVariant.states.metadataOverrideDescription", { count: evaluation.effective_rules.length }) }}
    </p>
    <p v-if="disabledTombstone" class="mt-3 rounded-md border border-gray-200 bg-gray-50 px-3 py-2 text-xs leading-5 text-gray-600">
      {{ t("requestPatchVariant.states.tombstoneDescription") }}
    </p>

    <div v-if="sourceRules.length > 0" class="mt-4 rounded-md border border-gray-100">
      <div class="flex items-center justify-between border-b border-gray-100 px-3 py-2">
        <span class="text-xs font-medium text-gray-700">{{ t("requestPatchVariant.model.sourceRulesReadOnly") }}</span>
        <Popover>
          <PopoverTrigger as-child>
            <Button variant="ghost" size="sm">{{ t("requestPatchVariant.actions.details") }}</Button>
          </PopoverTrigger>
          <PopoverContent class="w-80 border-gray-200 bg-white p-3">
            <div class="space-y-3">
              <div v-for="rule in sourceRules" :key="rule.id" class="border-b border-gray-100 pb-2 last:border-0 last:pb-0">
                <p class="font-mono text-xs text-gray-900">{{ rule.placement }} · {{ rule.target }}</p>
                <p class="mt-1 break-all font-mono text-xs text-gray-500">{{ formatRequestPatchValueForDisplay(rule.value_json) }}</p>
              </div>
            </div>
          </PopoverContent>
        </Popover>
      </div>
    </div>

    <div v-if="evaluation.explain.length > 0" class="mt-4 flex flex-wrap gap-1.5">
      <Badge v-for="entry in evaluation.explain" :key="entry.rule.id" :variant="statusVariant(entry.status)" class="font-mono text-[10px]" :title="entry.message ?? undefined">
        {{ originLabel(entry.origin) }} #{{ entry.rule.id }} · {{ statusLabel(entry.status) }}
      </Badge>
    </div>
  </div>
</template>

<script setup lang="ts">
import { computed } from "vue";
import { useI18n } from "vue-i18n";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { formatRequestPatchValueForDisplay } from "@/utils/requestPatch";
import type {
  RequestPatchEvaluation,
  RequestPatchExplainStatus,
  RequestPatchVariantAggregate,
  RequestPatchVariantOrigin,
} from "@/services/types";
import type { ModelRequestPatchSourceState } from "../composables/useModelRequestPatch";

const props = defineProps<{
  state: ModelRequestPatchSourceState;
  suffix: string | null;
  evaluation: RequestPatchEvaluation;
  sourceVariant: RequestPatchVariantAggregate | null;
  modelVariant: RequestPatchVariantAggregate | null;
  sourceBound: boolean;
  onOpenEditor: (sourceId: number, suffix: string | null, variant: RequestPatchVariantAggregate | null) => void;
  onRestore: (sourceId: number, variant: RequestPatchVariantAggregate) => Promise<void>;
  onDelete: (sourceId: number, variant: RequestPatchVariantAggregate) => void;
  onToggleInherited: (sourceId: number, suffix: string | null, allowed: boolean) => Promise<void>;
}>();

const { t } = useI18n();
const sourceRules = computed(() => props.sourceVariant?.rules ?? []);
const disabledTombstone = computed(() => !!props.modelVariant && !props.modelVariant.variant.enabled && props.modelVariant.rules.length === 0);
const metadataOnly = computed(() => !!props.modelVariant && props.modelVariant.variant.enabled && props.modelVariant.rules.length === 0);

const originLabel = (origin: RequestPatchVariantOrigin) => t(`requestPatchVariant.origins.${origin}`);
const statusLabel = (status: RequestPatchExplainStatus) => t(`requestPatchVariant.status.${status}`);
const statusVariant = (status: RequestPatchExplainStatus) => {
  if (status === "Conflicted") return "destructive" as const;
  if (status === "Effective") return "secondary" as const;
  return "outline" as const;
};
const onOpenEditor = props.onOpenEditor;
const onRestore = props.onRestore;
const onDelete = props.onDelete;
const onToggleInherited = props.onToggleInherited;
</script>
