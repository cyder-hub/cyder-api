<script setup lang="ts">
import { computed, ref } from "vue";
import { useI18n } from "vue-i18n";
import { CircleHelp, Info, Loader2, Star, WandSparkles } from "lucide-vue-next";

import type {
  ModelSourceConfigSummary,
  ModelSourceExplain,
} from "@/services/types";
import type { UpstreamSource } from "@/services/types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  applySourceSelectionMode,
  clearSourceDefault,
  setSourceDefault,
  sourceConfigWarningKey,
  toggleSourceBinding,
  visibleSourceOptions,
  type SourceConfigDraft,
} from "./sourceConfigViewModel";

const props = withDefaults(defineProps<{
  sources: UpstreamSource[];
  summary?: ModelSourceConfigSummary | null;
  explain?: ModelSourceExplain | null;
  dirty?: boolean;
  saving?: boolean;
  error?: string | null;
  readonly?: boolean;
  showSave?: boolean;
  showExplain?: boolean;
}>(), {
  dirty: false,
  saving: false,
  error: null,
  readonly: false,
  showSave: true,
  showExplain: true,
});

const draft = defineModel<SourceConfigDraft>({ required: true });

const emit = defineEmits<{
  save: [];
  explain: [];
}>();

const { t } = useI18n();
const visibleSources = computed(() => visibleSourceOptions(props.sources));
const selectedSourceIds = computed(
  () => new Set(draft.value.bindings.map((binding) => binding.source_id)),
);
const defaultSourceId = computed(
  () => draft.value.bindings.find((binding) => binding.is_default)?.source_id ?? null,
);

const pendingMode = ref<SourceConfigDraft["source_selection_mode"] | null>(null);
const isModeConfirmOpen = ref(false);

const requestModeChange = (value: unknown) => {
  if (
    props.readonly ||
    typeof value !== "string" ||
    value === draft.value.source_selection_mode
  ) {
    return;
  }
  pendingMode.value = value as SourceConfigDraft["source_selection_mode"];
  isModeConfirmOpen.value = true;
};

const confirmModeChange = () => {
  if (pendingMode.value) {
    draft.value = applySourceSelectionMode(
      draft.value,
      pendingMode.value,
      props.sources,
    );
  }
  pendingMode.value = null;
  isModeConfirmOpen.value = false;
};

const toggleBinding = (sourceId: number, value: boolean | "indeterminate") => {
  if (props.readonly) return;
  draft.value = toggleSourceBinding(draft.value, sourceId, value === true);
};

const chooseDefault = (sourceId: number) => {
  if (props.readonly) return;
  draft.value =
    defaultSourceId.value === sourceId
      ? clearSourceDefault(draft.value)
      : setSourceDefault(draft.value, sourceId);
};

const explainStatusKey = (status: string) =>
  status === "selected"
    ? "modelSourceConfig.explain.selected"
    : "modelSourceConfig.explain.unselectable";
</script>

<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <div class="min-w-0">
        <div class="flex items-center gap-2">
          <h3 class="text-base font-semibold text-gray-900">
            {{ t("modelSourceConfig.title") }}
          </h3>
          <Popover>
            <PopoverTrigger as-child>
              <Button
                variant="ghost"
                size="icon"
                class="h-7 w-7 text-gray-400 hover:text-gray-700"
                :aria-label="t('modelSourceConfig.helpLabel')"
              >
                <CircleHelp class="h-4 w-4" />
              </Button>
            </PopoverTrigger>
            <PopoverContent class="max-w-sm text-sm leading-5 text-gray-600">
              {{ t("modelSourceConfig.description") }}
            </PopoverContent>
          </Popover>
        </div>
        <p class="mt-1 text-sm text-gray-500">
          {{
            draft.source_selection_mode === "INHERIT_ALL"
              ? t("modelSourceConfig.modeHint.inheritAll")
              : t("modelSourceConfig.modeHint.explicit")
          }}
        </p>
      </div>
      <div class="flex w-full flex-col gap-2 sm:w-auto sm:flex-row">
        <Button
          v-if="showExplain"
          variant="outline"
          class="w-full sm:w-auto"
          :disabled="readonly"
          @click="emit('explain')"
        >
          <WandSparkles class="mr-1.5 h-4 w-4" />
          {{ t("modelSourceConfig.actions.explain") }}
        </Button>
        <Button
          v-if="showSave"
          variant="default"
          class="w-full sm:w-auto"
          :disabled="readonly || !dirty || saving"
          @click="emit('save')"
        >
          <Loader2 v-if="saving" class="mr-1.5 h-4 w-4 animate-spin" />
          {{ t("common.save") }}
        </Button>
      </div>
    </div>

    <div
      v-if="error"
      class="rounded-lg border border-red-200 bg-red-50 px-3.5 py-3 text-sm text-red-700"
    >
      {{ error }}
    </div>

    <div class="grid gap-1.5">
      <label class="text-sm font-medium text-gray-700" for="model-source-mode">
        {{ t("modelSourceConfig.modeLabel") }}
      </label>
      <Select
        id="model-source-mode"
        :model-value="draft.source_selection_mode"
        :disabled="readonly"
        @update:model-value="requestModeChange"
      >
        <SelectTrigger class="w-full sm:max-w-md">
          <SelectValue :placeholder="t('modelSourceConfig.modePlaceholder')" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="INHERIT_ALL">
            {{ t("modelSourceConfig.modes.inheritAll.label") }}
          </SelectItem>
          <SelectItem value="EXPLICIT">
            {{ t("modelSourceConfig.modes.explicit.label") }}
          </SelectItem>
        </SelectContent>
      </Select>
    </div>

    <div
      v-if="draft.source_selection_mode === 'INHERIT_ALL'"
      class="flex gap-3 rounded-lg border border-gray-200 bg-gray-50/70 px-3.5 py-3 text-sm text-gray-600"
    >
      <Info class="mt-0.5 h-4 w-4 shrink-0 text-gray-400" />
      <p>{{ t("modelSourceConfig.inheritAllFutureSources") }}</p>
    </div>

    <div v-else class="space-y-3">
      <div class="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h4 class="text-sm font-semibold text-gray-900">
            {{ t("modelSourceConfig.explicitSourcesTitle") }}
          </h4>
          <p class="mt-1 text-xs text-gray-500">
            {{ t("modelSourceConfig.explicitSourcesDescription") }}
          </p>
        </div>
        <Badge variant="outline" class="w-fit font-mono text-xs">
          {{ t("modelSourceConfig.sourceCount", { count: draft.bindings.length }) }}
        </Badge>
      </div>

      <div
        v-if="visibleSources.length === 0"
        class="rounded-lg border border-dashed border-gray-200 px-4 py-8 text-center text-sm text-gray-500"
      >
        {{ t("modelSourceConfig.noSources") }}
      </div>

      <div v-else class="divide-y divide-gray-100 rounded-lg border border-gray-200">
        <div
          v-for="source in visibleSources"
          :key="source.id"
          class="flex flex-col gap-3 px-3.5 py-3 sm:flex-row sm:items-center sm:justify-between"
        >
          <div class="flex min-w-0 items-center gap-3">
            <Checkbox
              :model-value="selectedSourceIds.has(source.id)"
              :disabled="readonly"
              :aria-label="t('modelSourceConfig.selectSource', { id: source.id })"
              @update:model-value="(value) => toggleBinding(source.id, value)"
            />
            <div class="min-w-0">
              <div class="flex flex-wrap items-center gap-2">
                <span class="font-mono text-sm text-gray-900">#{{ source.id }}</span>
                <Badge variant="outline" class="font-mono text-[11px]">
                  {{ source.profile_type }}
                </Badge>
                <Badge
                  :variant="source.is_enabled ? 'secondary' : 'outline'"
                  class="font-mono text-[11px]"
                >
                  {{
                    source.is_enabled
                      ? t("modelSourceConfig.enabled")
                      : t("modelSourceConfig.disabled")
                  }}
                </Badge>
              </div>
              <p class="mt-1 text-xs text-gray-500">
                {{
                  selectedSourceIds.has(source.id)
                    ? t("modelSourceConfig.selected")
                    : t("modelSourceConfig.notSelected")
                }}
              </p>
            </div>
          </div>
          <Button
            variant="ghost"
            size="sm"
            class="w-full justify-center text-gray-600 sm:w-auto"
            :disabled="readonly || !selectedSourceIds.has(source.id)"
            @click="chooseDefault(source.id)"
          >
            <Star
              class="mr-1.5 h-3.5 w-3.5"
              :class="defaultSourceId === source.id ? 'fill-gray-900 text-gray-900' : ''"
            />
            {{
              defaultSourceId === source.id
                ? t("modelSourceConfig.clearDefault")
                : t("modelSourceConfig.makeDefault")
            }}
          </Button>
        </div>
      </div>
    </div>

    <div v-if="summary?.warnings?.length" class="flex flex-wrap gap-2">
      <Popover v-for="warning in summary.warnings" :key="warning">
        <PopoverTrigger as-child>
          <Button variant="outline" size="sm" class="font-mono text-xs text-gray-600">
            <Info class="mr-1.5 h-3.5 w-3.5" />
            {{ t(sourceConfigWarningKey(warning)) }}
          </Button>
        </PopoverTrigger>
        <PopoverContent class="max-w-sm text-sm leading-5 text-gray-600">
          {{ t(`${sourceConfigWarningKey(warning)}Description`) }}
        </PopoverContent>
      </Popover>
    </div>

    <div v-if="explain" class="space-y-3 border-t border-gray-100 pt-4">
      <div class="flex items-center justify-between gap-3">
        <h4 class="text-sm font-semibold text-gray-900">
          {{ t("modelSourceConfig.explain.title") }}
        </h4>
        <span class="font-mono text-xs text-gray-500">
          {{ explain.source_selection_mode }}
        </span>
      </div>
      <div class="divide-y divide-gray-100 rounded-lg border border-gray-200">
        <div
          v-for="protocol in explain.protocols"
          :key="protocol.downstream_protocol"
          class="flex flex-col gap-2 px-3.5 py-3 sm:flex-row sm:items-start sm:justify-between"
        >
          <div>
            <p class="font-mono text-xs text-gray-900">{{ protocol.downstream_protocol }}</p>
            <p class="mt-1 text-xs text-gray-500">
              {{ t(explainStatusKey(protocol.selection_status)) }}
            </p>
          </div>
          <div class="text-left sm:text-right">
            <p v-if="protocol.source_id" class="font-mono text-xs text-gray-700">
              #{{ protocol.source_id }} · {{ protocol.profile_type }}
            </p>
            <p class="mt-1 text-xs text-gray-500">
              {{ t("modelSourceConfig.explain.transformRequired") }}:
              <span class="font-mono text-gray-700">
                {{
                  protocol.transform_required === null
                    ? "-"
                    : protocol.transform_required
                      ? t("modelSourceConfig.explain.yes")
                      : t("modelSourceConfig.explain.no")
                }}
              </span>
            </p>
            <p class="mt-1 text-xs text-gray-500">
              {{ t("modelSourceConfig.explain.executionStatus") }}:
              <span class="font-mono text-gray-700">
                {{ protocol.generation_execution_status }}
              </span>
            </p>
            <p class="mt-1 max-w-sm text-xs leading-5 text-gray-500 sm:max-w-md">
              {{ t("modelSourceConfig.explain.executionReason") }}:
              {{ protocol.generation_execution_reason }}
            </p>
            <p v-if="protocol.selection_reason" class="mt-1 font-mono text-[11px] text-gray-500">
              {{ protocol.selection_reason }}
            </p>
            <p v-else-if="protocol.failure_reason" class="mt-1 font-mono text-[11px] text-gray-500">
              {{ protocol.failure_reason }}
            </p>
          </div>
        </div>
      </div>
    </div>
  </section>

  <Dialog v-model:open="isModeConfirmOpen">
    <DialogContent class="border border-gray-200 bg-white sm:max-w-lg">
      <DialogHeader>
        <DialogTitle>{{ t("modelSourceConfig.modeConfirm.title") }}</DialogTitle>
        <DialogDescription>
          {{
            pendingMode === "EXPLICIT"
              ? t("modelSourceConfig.modeConfirm.toExplicit")
              : t("modelSourceConfig.modeConfirm.toInheritAll")
          }}
        </DialogDescription>
      </DialogHeader>
      <DialogFooter class="flex-col gap-2 sm:flex-row sm:justify-end">
        <Button variant="ghost" class="w-full text-gray-600 sm:w-auto" @click="isModeConfirmOpen = false">
          {{ t("common.cancel") }}
        </Button>
        <Button variant="default" class="w-full sm:w-auto" @click="confirmModeChange">
          {{ t("modelSourceConfig.modeConfirm.confirm") }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
