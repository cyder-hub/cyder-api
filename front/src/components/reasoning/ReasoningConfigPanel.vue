<template>
  <section class="space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5">
    <SectionHeader :title="title">
      <template #actions>
      <div class="flex flex-wrap gap-2">
        <Badge v-if="currentTargetApiType" variant="outline" class="font-mono text-[11px]">
          {{ currentTargetApiType }}
        </Badge>
        <Badge v-if="config" variant="secondary" class="font-mono text-[11px]">
          {{ formatSource(config.effective_source) }}
        </Badge>
        <Button variant="outline" size="sm" :disabled="isLoading || !ownerId" @click="load">
          <RefreshCw class="mr-1.5 h-3.5 w-3.5" />
          {{ t("common.refresh") }}
        </Button>
      </div>
      </template>
    </SectionHeader>

    <div v-if="!ownerId" class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500">
      {{ t("reasoningConfigPanel.messages.saveOwnerFirst") }}
    </div>

    <div v-else-if="isLoading" class="flex items-center justify-center py-12 text-sm text-gray-500">
      <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
      {{ t("reasoningConfigPanel.messages.loading") }}
    </div>

    <div v-else-if="error" class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ error }}
    </div>

    <template v-else>
      <div class="grid grid-cols-1 gap-3 text-sm sm:grid-cols-3">
        <div
          v-for="item in summaryItems"
          :key="item.label"
          class="space-y-1 border-b border-gray-100 pb-3 last:border-b-0 sm:border-b-0 sm:pb-0"
        >
          <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
            {{ item.label }}
          </p>
          <p class="break-all font-mono text-xs text-gray-800">
            {{ item.value }}
          </p>
        </div>
      </div>

      <div v-if="ownerKind === 'model'" class="space-y-3 border-t border-gray-100 pt-4">
        <div class="flex flex-col gap-2 sm:flex-row">
          <Button
            v-for="option in modelModeOptions"
            :key="option.value"
            :variant="modeDraft === option.value ? 'default' : 'outline'"
            class="justify-start sm:justify-center"
            @click="setMode(option.value)"
          >
            {{ option.label }}
          </Button>
        </div>
        <p v-if="modelSupportsReasoning === false" class="text-xs text-gray-500">
          {{ t("reasoningConfigPanel.messages.modelReasoningUnsupported") }}
        </p>
      </div>

      <slot name="runtime-feature" />

      <div v-if="showCustomEditor" class="space-y-4 border-t border-gray-100 pt-4">
        <div class="grid grid-cols-1 gap-4 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
          <div class="space-y-1.5">
            <Label class="text-gray-700">{{ t("reasoningConfigPanel.fields.patchFamily") }}</Label>
            <Select v-model="familyKeyDraft">
              <SelectTrigger class="w-full">
                <SelectValue :placeholder="t('reasoningConfigPanel.messages.selectPatchFamily')" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem
                  v-for="family in catalog?.families ?? []"
                  :key="family.family_key"
                  :value="family.family_key"
                >
                  <div class="flex min-w-0 flex-col gap-0.5 py-0.5">
                    <span class="truncate font-mono text-xs text-gray-900">
                      {{ family.family_key }}
                    </span>
                    <span
                      class="truncate text-[11px]"
                      :class="familyCompatibilityClass(family)"
                    >
                      {{ familyCompatibilityLabel(family) }}
                    </span>
                  </div>
                </SelectItem>
              </SelectContent>
            </Select>
            <p
              v-if="selectedFamilyCompatibilityMessage"
              class="text-xs"
              :class="selectedFamilyCompatible ? 'text-gray-500' : 'text-red-600'"
            >
              {{ selectedFamilyCompatibilityMessage }}
            </p>
            <p v-if="previewError" class="text-xs text-red-600">
              {{ previewError }}
            </p>
            <p v-else-if="isPreviewLoading" class="text-xs text-gray-500">
              {{ t("reasoningConfigPanel.messages.previewRefreshing") }}
            </p>
          </div>
          <Badge v-if="isDirty" variant="outline" class="h-fit w-fit font-mono text-[11px]">
            {{ t("reasoningConfigPanel.states.unsaved") }}
          </Badge>
        </div>

        <div class="border-y border-gray-100">
          <div class="hidden md:block">
            <div class="app-scroll-x">
              <Table>
                <TableHeader>
                  <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                    <TableHead class="min-w-36 text-xs font-medium uppercase tracking-wider text-gray-500">
                      {{ t("reasoningConfigPanel.fields.preset") }}
                    </TableHead>
                    <TableHead class="min-w-24 text-xs font-medium uppercase tracking-wider text-gray-500">
                      {{ t("reasoningConfigPanel.fields.suffix") }}
                    </TableHead>
                    <TableHead class="min-w-32 text-xs font-medium uppercase tracking-wider text-gray-500">
                      {{ t("reasoningConfigPanel.fields.runtime") }}
                    </TableHead>
                    <TableHead class="w-24 text-center text-xs font-medium uppercase tracking-wider text-gray-500">
                      {{ t("reasoningConfigPanel.fields.enabled") }}
                    </TableHead>
                    <TableHead class="w-24 text-center text-xs font-medium uppercase tracking-wider text-gray-500">
                      {{ t("reasoningConfigPanel.fields.models") }}
                    </TableHead>
                    <TableHead class="min-w-56 text-xs font-medium uppercase tracking-wider text-gray-500">
                      {{ t("reasoningConfigPanel.fields.patch") }}
                    </TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <TableRow v-for="row in matrixRows" :key="row.preset_key">
                    <TableCell class="align-top">
                      <div class="space-y-1">
                        <p class="font-mono text-sm text-gray-900">
                          {{ row.preset_key }}
                        </p>
                        <Badge :variant="row.requires_reasoning ? 'secondary' : 'outline'" class="font-mono text-[11px]">
                          {{ t("reasoningConfigPanel.fields.reasoning") }}: {{ row.requires_reasoning ? t("common.yes") : t("common.no") }}
                        </Badge>
                      </div>
                    </TableCell>
                    <TableCell class="align-top font-mono text-sm text-gray-700">
                      -{{ row.suffix }}
                    </TableCell>
                    <TableCell class="align-top">
                      <div class="space-y-1">
                        <Badge :variant="row.status_variant" class="font-mono text-[11px]">
                          {{ row.status_label }}
                        </Badge>
                      </div>
                    </TableCell>
                    <TableCell class="align-top text-center">
                      <Checkbox
                        :model-value="row.enabled"
                        :disabled="!row.family_supported"
                        @update:model-value="(value) => setPresetEnabled(row.preset_key, value)"
                      />
                    </TableCell>
                    <TableCell class="align-top text-center">
                      <Checkbox
                        :model-value="row.expose_in_models"
                        :disabled="!row.family_supported || !row.enabled"
                        @update:model-value="(value) => setPresetExpose(row.preset_key, value)"
                      />
                    </TableCell>
                    <TableCell class="align-top">
                      <div v-if="row.generated_patches.length" class="space-y-2">
                        <div
                          v-for="(patch, index) in row.generated_patches"
                          :key="`${row.preset_key}-${index}`"
                          class="rounded-md bg-gray-50 px-3 py-2"
                        >
                          <p class="break-all font-mono text-xs text-gray-900">
                            {{ formatPatchPreview(patch) }}
                          </p>
                        </div>
                      </div>
                      <p v-else class="text-xs text-gray-500">
                        {{ row.patch_placeholder }}
                      </p>
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </div>
          </div>

          <div class="divide-y divide-gray-100 md:hidden">
            <div v-for="row in matrixRows" :key="row.preset_key" class="space-y-3 p-3.5">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <p class="font-mono text-sm text-gray-900">
                    {{ row.preset_key }} -{{ row.suffix }}
                  </p>
                </div>
                <Badge :variant="row.status_variant" class="shrink-0 font-mono text-[11px]">
                  {{ row.status_label }}
                </Badge>
              </div>
              <div class="grid grid-cols-1 gap-2 min-[360px]:grid-cols-2">
                <div class="flex items-center justify-between rounded-md bg-gray-50/80 px-3 py-2.5">
                  <Label class="text-sm text-gray-700">{{ t("reasoningConfigPanel.fields.enabled") }}</Label>
                  <Checkbox
                    :model-value="row.enabled"
                    :disabled="!row.family_supported"
                    @update:model-value="(value) => setPresetEnabled(row.preset_key, value)"
                  />
                </div>
                <div class="flex items-center justify-between rounded-md bg-gray-50/80 px-3 py-2.5">
                  <Label class="text-sm text-gray-700">{{ t("reasoningConfigPanel.fields.models") }}</Label>
                  <Checkbox
                    :model-value="row.expose_in_models"
                    :disabled="!row.family_supported || !row.enabled"
                    @update:model-value="(value) => setPresetExpose(row.preset_key, value)"
                  />
                </div>
              </div>
              <div class="rounded-lg bg-gray-50 px-3 py-2">
                <div v-if="row.generated_patches.length" class="space-y-2">
                  <div v-for="(patch, index) in row.generated_patches" :key="index" class="space-y-1">
                    <p class="break-all font-mono text-xs text-gray-900">
                      {{ formatPatchPreview(patch) }}
                    </p>
                  </div>
                </div>
                <p v-else class="text-xs text-gray-500">
                  {{ row.patch_placeholder }}
                </p>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div v-else-if="modeDraft === 'disabled'" class="rounded-md bg-gray-50/80 px-4 py-4 text-sm text-gray-600">
        {{ t("reasoningConfigPanel.messages.disabledMode") }}
      </div>

      <div class="flex flex-col gap-2 border-t border-gray-100 pt-4 sm:flex-row sm:justify-end">
        <Button
          v-if="canClear"
          variant="ghost"
          class="w-full text-gray-600 sm:w-auto"
          :disabled="isSaving"
          @click="clearConfig"
        >
          <Trash2 class="mr-1.5 h-4 w-4" />
          {{ clearLabel }}
        </Button>
        <Button variant="default" class="w-full sm:w-auto" :disabled="isSaving || !ownerId" @click="saveConfig">
          <Loader2 v-if="isSaving" class="mr-1.5 h-4 w-4 animate-spin" />
          <Save v-else class="mr-1.5 h-4 w-4" />
          {{ t("reasoningConfigPanel.actions.save") }}
        </Button>
      </div>
    </template>
  </section>
</template>

<script setup lang="ts">
import { computed, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Loader2, RefreshCw, Save, Trash2 } from "lucide-vue-next";
import SectionHeader from "@/components/SectionHeader.vue";
import { normalizeError } from "@/utils/error";
import { toastController } from "@/services/uiFeedback";
import type {
  ModelReasoningConfigWriteMode,
  ModelReasoningConfigPayload,
  ProviderReasoningConfigPreviewPayload,
  ReasoningConfigCatalog,
  ReasoningConfigDetail,
  ReasoningFamilyMetadata,
  ReasoningConfigResponse,
  ReasoningPatchFamilyKey,
  ReasoningPresetKey,
} from "@/services/types";
import type {
  ReasoningConfigActions,
  ReasoningDraftPreviewPayload,
  ReasoningOwnerKind,
} from "./types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

interface PresetDraft {
  is_enabled: boolean;
  expose_in_models: boolean;
}

interface PreviewPatch {
  placement: string;
  target: string;
  operation: string;
  value_json: unknown;
  description: string | null;
}

interface PreviewPreset {
  preset_key: string;
  suffix: string;
  requires_reasoning: boolean;
  allowed_operation_kinds: string[];
  family_supported: boolean;
  enabled: boolean;
  expose_in_models: boolean;
  runtime_supported: boolean;
  unsupported_reason: string | null;
  generated_patches: PreviewPatch[];
}

interface PreviewResponse {
  config: ReasoningConfigResponse;
  target_api_type: string;
  presets: PreviewPreset[];
}

interface MatrixRow {
  preset_key: ReasoningPresetKey;
  suffix: string;
  requires_reasoning: boolean;
  family_supported: boolean;
  enabled: boolean;
  expose_in_models: boolean;
  patch_placeholder: string;
  generated_patches: PreviewPatch[];
  status_label: string;
  status_variant: "secondary" | "outline";
}

const props = withDefaults(
  defineProps<{
    ownerKind: ReasoningOwnerKind;
    ownerId: number | null;
    actions: ReasoningConfigActions;
    title?: string;
    modelSupportsReasoning?: boolean;
    providerType?: string | null;
  }>(),
  {
    title: undefined,
    modelSupportsReasoning: undefined,
    providerType: undefined,
  },
);

const emit = defineEmits<{
  saved: [];
}>();

const { t } = useI18n();

const catalog = ref<ReasoningConfigCatalog | null>(null);
const config = ref<ReasoningConfigResponse | null>(null);
const savedPreview = ref<PreviewResponse | null>(null);
const draftPreview = ref<PreviewResponse | null>(null);
const modeDraft = ref<ModelReasoningConfigWriteMode>("inherit");
const familyKeyDraft = ref<ReasoningPatchFamilyKey>("");
const presetDrafts = ref<Record<string, PresetDraft>>({});
const lastSavedSnapshot = ref("");
const isLoading = ref(false);
const isSaving = ref(false);
const isPreviewLoading = ref(false);
const error = ref<string | null>(null);
const previewError = ref<string | null>(null);
let draftPreviewTimer: ReturnType<typeof setTimeout> | null = null;
let draftPreviewRequestId = 0;

const title = computed(
  () =>
    props.title ||
    (props.ownerKind === "provider"
      ? t("reasoningConfigPanel.titles.provider")
      : t("reasoningConfigPanel.titles.model")),
);

const modelModeOptions = computed<
  { value: ModelReasoningConfigWriteMode; label: string }[]
>(() => [
  { value: "inherit", label: t("reasoningConfigPanel.modes.inherit") },
  { value: "disabled", label: t("reasoningConfigPanel.modes.disabled") },
  { value: "custom", label: t("reasoningConfigPanel.modes.custom") },
]);

const selectedFamily = computed(() =>
  catalog.value?.families.find(
    (family) => family.family_key === familyKeyDraft.value,
  ),
);

const showCustomEditor = computed(
  () => props.ownerKind === "provider" || modeDraft.value === "custom",
);

const currentSnapshot = computed(() => buildSnapshot());

const isDirty = computed(
  () => !!lastSavedSnapshot.value && currentSnapshot.value !== lastSavedSnapshot.value,
);

const activePreview = computed(() => {
  if (isDirty.value) return draftPreview.value;
  return draftPreview.value ?? savedPreview.value;
});

const currentTargetApiType = computed(
  () =>
    targetApiTypeFromProviderType(props.providerType) ||
    activePreview.value?.target_api_type ||
    null,
);

const selectedFamilyCompatible = computed(() => {
  if (!selectedFamily.value || !currentTargetApiType.value) return true;
  return selectedFamily.value.target_api_types.includes(currentTargetApiType.value);
});

const selectedFamilyCompatibilityMessage = computed(() => {
  if (!selectedFamily.value) return "";
  const targets = formatTargetApiTypes(selectedFamily.value.target_api_types);
  const current = currentTargetApiType.value;
  if (!current) return t("reasoningConfigPanel.messages.familyTargets", { targets });
  if (selectedFamilyCompatible.value) return "";
  return t("reasoningConfigPanel.messages.familyIncompatible", {
    targets,
    current,
  });
});

const canClear = computed(
  () => !!props.ownerId && (!!config.value?.owner_config || isDirty.value),
);

const clearLabel = computed(() =>
  props.ownerKind === "provider"
    ? t("reasoningConfigPanel.actions.clearProvider")
    : t("reasoningConfigPanel.actions.clearModel"),
);

const summaryItems = computed(() => {
  const current = config.value;
  if (!current) return [];
  const items = [
    {
      label: t("reasoningConfigPanel.summary.ownerConfig"),
      value: formatConfig(current.owner_config, current.status),
    },
  ];

  if (props.ownerKind === "model") {
    items.push({
      label: t("reasoningConfigPanel.summary.providerDefault"),
      value: formatConfig(current.provider_config, "missing"),
    });
  }

  items.push({
    label: t("reasoningConfigPanel.summary.effectiveConfig"),
    value: formatConfig(current.effective_config, current.effective_source),
  });

  return items;
});

const previewByPreset = computed(() => {
  const entries = new Map<string, PreviewPreset>();
  for (const item of activePreview.value?.presets ?? []) {
    entries.set(item.preset_key, item);
  }
  return entries;
});

const matrixRows = computed<MatrixRow[]>(() => {
  return (catalog.value?.presets ?? []).map((metadata) => {
    const presetKey = metadata.preset_key;
    const draft = presetDrafts.value[presetKey] || {
      is_enabled: false,
      expose_in_models: false,
    };
    const presetPreview = previewByPreset.value.get(presetKey);
    const familySupported = familySupportsPreset(presetKey);
    const enabled = familySupported && draft.is_enabled;
    const exposeInModels = familySupported && enabled && draft.expose_in_models;
    const runtimeSupported = !!presetPreview?.runtime_supported;
    const generatedPatches = presetPreview?.generated_patches ?? [];
    const displayReason = buildDisplayReason(
      familySupported,
      enabled,
      runtimeSupported,
      presetPreview?.unsupported_reason ?? null,
      generatedPatches,
    );
    const status = buildStatus(enabled, runtimeSupported, generatedPatches);

    return {
      preset_key: presetKey,
      suffix: metadata.suffix,
      requires_reasoning: metadata.requires_reasoning,
      family_supported: familySupported,
      enabled,
      expose_in_models: exposeInModels,
      generated_patches: generatedPatches,
      status_label: status.label,
      status_variant: status.variant,
      patch_placeholder: buildPatchPlaceholder(
        familySupported,
        generatedPatches,
        displayReason,
      ),
    };
  });
});

function formatSource(source: string | null | undefined): string {
  switch (source) {
    case "provider_default":
      return t("reasoningConfigPanel.sources.providerDefault");
    case "model_custom":
      return t("reasoningConfigPanel.sources.modelCustom");
    case "model_disabled":
      return t("reasoningConfigPanel.sources.modelDisabled");
    case "missing":
      return t("reasoningConfigPanel.sources.missing");
    default:
      return source || t("reasoningConfigPanel.sources.unknown");
  }
}

function formatConfig(
  item: ReasoningConfigDetail | null,
  fallback: string | null | undefined,
): string {
  if (!item) return formatSource(fallback);
  const family = item.family_key || "disabled";
  return `${item.scope_kind}/${item.id} ${item.mode} ${family}`;
}

function formatValueJson(value: unknown): string {
  if (value === null || value === undefined) return "null";
  return JSON.stringify(value);
}

function formatPatchPreview(patch: PreviewPatch): string {
  const operation = patch.operation.toUpperCase();
  const value =
    operation === "REMOVE" ? "" : ` ${formatValueJson(patch.value_json)}`;
  return `${operation} ${patch.target}${value}`;
}

function normalizeEnumValue(value: string | null | undefined): string | null {
  const normalized = value?.trim().toUpperCase();
  return normalized || null;
}

function targetApiTypeFromProviderType(
  providerType: string | null | undefined,
): string | null {
  switch (normalizeEnumValue(providerType)) {
    case "OPENAI":
    case "VERTEX_OPENAI":
      return "OPENAI";
    case "GEMINI":
    case "VERTEX":
      return "GEMINI";
    case "GEMINI_OPENAI":
      return "GEMINI_OPENAI";
    case "ANTHROPIC":
      return "ANTHROPIC";
    case "RESPONSES":
      return "RESPONSES";
    case "OLLAMA":
      return "OLLAMA";
    default:
      return null;
  }
}

function formatTargetApiTypes(targets: string[]): string {
  return targets.length
    ? targets.join(", ")
    : t("reasoningConfigPanel.messages.none");
}

function familyCompatibilityLabel(family: ReasoningFamilyMetadata): string {
  const current = currentTargetApiType.value;
  const targets = formatTargetApiTypes(family.target_api_types);
  if (!current) {
    return t("reasoningConfigPanel.messages.familyTargetsShort", { targets });
  }
  if (family.target_api_types.includes(current)) {
    return t("reasoningConfigPanel.messages.familyCompatibleShort", { current });
  }
  return t("reasoningConfigPanel.messages.familyTargetsShort", { targets });
}

function familyCompatibilityClass(family: ReasoningFamilyMetadata): string {
  const current = currentTargetApiType.value;
  if (!current) return "text-gray-500";
  return family.target_api_types.includes(current) ? "text-gray-500" : "text-red-600";
}

function familySupportsPreset(presetKey: ReasoningPresetKey): boolean {
  return !!selectedFamily.value?.supported_presets.includes(presetKey);
}

function buildDisplayReason(
  familySupported: boolean,
  enabled: boolean,
  runtimeSupported: boolean,
  unsupportedReason: string | null,
  generatedPatches: PreviewPatch[],
): string {
  if (!familyKeyDraft.value) return t("reasoningConfigPanel.messages.selectPatchFamily");
  if (!familySupported) return t("reasoningConfigPanel.messages.presetUnsupported");
  if (!enabled && generatedPatches.length) {
    return t("reasoningConfigPanel.messages.reviewOnly");
  }
  if (!enabled) return t("reasoningConfigPanel.messages.presetDisabled");
  if (!runtimeSupported) {
    return unsupportedReason || t("reasoningConfigPanel.messages.runtimeUnsupported");
  }
  return t("reasoningConfigPanel.messages.runtimeSupported");
}

function buildStatus(
  enabled: boolean,
  runtimeSupported: boolean,
  generatedPatches: PreviewPatch[],
): { label: string; variant: "secondary" | "outline" } {
  if (!enabled && generatedPatches.length) {
    return { label: t("reasoningConfigPanel.states.review"), variant: "outline" };
  }
  if (runtimeSupported) {
    return { label: t("reasoningConfigPanel.states.supported"), variant: "secondary" };
  }
  return { label: t("reasoningConfigPanel.states.blocked"), variant: "outline" };
}

function buildPatchPlaceholder(
  familySupported: boolean,
  generatedPatches: PreviewPatch[],
  displayReason: string,
): string {
  if (!familyKeyDraft.value) return t("reasoningConfigPanel.messages.selectPatchFamily");
  if (isPreviewLoading.value && familySupported) {
    return t("reasoningConfigPanel.messages.previewRefreshing");
  }
  if (previewError.value && familySupported) return previewError.value;
  if (generatedPatches.length) return "";
  return displayReason;
}

function chooseSeedConfig(response: ReasoningConfigResponse): ReasoningConfigDetail | null {
  if (props.ownerKind === "provider") return response.owner_config;
  if (response.owner_config?.mode === "custom") return response.owner_config;
  if (response.provider_config?.mode === "custom") return response.provider_config;
  if (response.effective_config?.mode === "custom") return response.effective_config;
  return null;
}

function hydrateDraft(response: ReasoningConfigResponse) {
  if (props.ownerKind === "provider") {
    modeDraft.value = "custom";
  } else if (response.status === "disabled") {
    modeDraft.value = "disabled";
  } else if (response.status === "custom") {
    modeDraft.value = "custom";
  } else {
    modeDraft.value = "inherit";
  }

  const seedConfig = chooseSeedConfig(response);
  familyKeyDraft.value = seedConfig?.family_key || "";

  const rowsByPreset = new Map<string, PresetDraft>();
  for (const row of seedConfig?.presets ?? []) {
    rowsByPreset.set(row.preset_key, {
      is_enabled: row.is_enabled,
      expose_in_models: row.expose_in_models,
    });
  }

  const nextDrafts: Record<string, PresetDraft> = {};
  for (const preset of catalog.value?.presets ?? []) {
    nextDrafts[preset.preset_key] = rowsByPreset.get(preset.preset_key) || {
      is_enabled: false,
      expose_in_models: false,
    };
  }
  presetDrafts.value = nextDrafts;
  clearUnsupportedDraftPresets();
  lastSavedSnapshot.value = buildSnapshot();
}

function buildSnapshot(): string {
  const mode = props.ownerKind === "provider" ? "custom" : modeDraft.value;
  if (mode !== "custom") {
    return JSON.stringify({ mode });
  }

  return JSON.stringify({
    mode,
    family_key: familyKeyDraft.value,
    presets: (catalog.value?.presets ?? []).map((preset) => ({
      preset_key: preset.preset_key,
      is_enabled: !!presetDrafts.value[preset.preset_key]?.is_enabled,
      expose_in_models: !!presetDrafts.value[preset.preset_key]?.expose_in_models,
    })),
  });
}

function clearUnsupportedDraftPresets() {
  if (!catalog.value || !familyKeyDraft.value) return;
  const supported = new Set(selectedFamily.value?.supported_presets ?? []);
  const next = { ...presetDrafts.value };
  let changed = false;
  for (const preset of catalog.value.presets) {
    if (!supported.has(preset.preset_key)) {
      const current = next[preset.preset_key];
      if (current?.is_enabled || current?.expose_in_models) {
        next[preset.preset_key] = {
          is_enabled: false,
          expose_in_models: false,
        };
        changed = true;
      }
    }
  }
  if (changed) presetDrafts.value = next;
}

function setMode(mode: ModelReasoningConfigWriteMode) {
  modeDraft.value = mode;
  if (mode === "custom" && !familyKeyDraft.value) {
    familyKeyDraft.value = catalog.value?.families[0]?.family_key || "";
  }
}

function setPresetEnabled(
  presetKey: ReasoningPresetKey,
  value: boolean | "indeterminate",
) {
  const checked = value === true;
  const current = presetDrafts.value[presetKey] || {
    is_enabled: false,
    expose_in_models: false,
  };
  presetDrafts.value = {
    ...presetDrafts.value,
    [presetKey]: {
      is_enabled: checked,
      expose_in_models: checked ? current.expose_in_models : false,
    },
  };
}

function setPresetExpose(
  presetKey: ReasoningPresetKey,
  value: boolean | "indeterminate",
) {
  const current = presetDrafts.value[presetKey] || {
    is_enabled: false,
    expose_in_models: false,
  };
  presetDrafts.value = {
    ...presetDrafts.value,
    [presetKey]: {
      is_enabled: current.is_enabled,
      expose_in_models: current.is_enabled && value === true,
    },
  };
}

function buildPresetPayload() {
  return (catalog.value?.presets ?? [])
    .filter((preset) => familySupportsPreset(preset.preset_key))
    .map((preset) => {
      const draft = presetDrafts.value[preset.preset_key] || {
        is_enabled: false,
        expose_in_models: false,
      };
      return {
        preset_key: preset.preset_key,
        is_enabled: draft.is_enabled,
        expose_in_models: draft.is_enabled && draft.expose_in_models,
      };
    });
}

function buildProviderDraftPreviewPayload(): ProviderReasoningConfigPreviewPayload {
  return {
    provider_type: normalizeEnumValue(props.providerType),
    family_key: familyKeyDraft.value || null,
    presets: familyKeyDraft.value ? buildPresetPayload() : [],
  };
}

function buildModelDraftPreviewPayload(): ModelReasoningConfigPayload {
  if (modeDraft.value === "inherit") return { mode: "inherit" };
  if (modeDraft.value === "disabled") return { mode: "disabled" };
  return {
    mode: "custom",
    family_key: familyKeyDraft.value || null,
    presets: familyKeyDraft.value ? buildPresetPayload() : [],
  };
}

function clearDraftPreviewTimer() {
  if (draftPreviewTimer === null) return;
  clearTimeout(draftPreviewTimer);
  draftPreviewTimer = null;
}

function shouldRequestDraftPreview(): boolean {
  if (!isDirty.value || !props.ownerId || !catalog.value || !config.value) return false;
  return !showCustomEditor.value || !!familyKeyDraft.value;
}

function scheduleDraftPreview() {
  clearDraftPreviewTimer();

  if (!isDirty.value) {
    draftPreviewRequestId += 1;
    draftPreview.value = null;
    previewError.value = null;
    isPreviewLoading.value = false;
    return;
  }

  draftPreview.value = null;
  previewError.value = null;
  if (!shouldRequestDraftPreview()) {
    isPreviewLoading.value = false;
    return;
  }

  isPreviewLoading.value = true;
  const requestId = draftPreviewRequestId + 1;
  draftPreviewRequestId = requestId;
  draftPreviewTimer = setTimeout(() => {
    draftPreviewTimer = null;
    void refreshDraftPreview(requestId);
  }, 180);
}

async function refreshDraftPreview(requestId: number) {
  if (!props.ownerId || !shouldRequestDraftPreview()) {
    if (requestId === draftPreviewRequestId) {
      isPreviewLoading.value = false;
    }
    return;
  }

  try {
    const payload: ReasoningDraftPreviewPayload =
      props.ownerKind === "provider"
        ? buildProviderDraftPreviewPayload()
        : buildModelDraftPreviewPayload();
    const response = await props.actions.previewDraft(props.ownerId, payload);
    if (requestId !== draftPreviewRequestId) return;
    draftPreview.value = response;
    previewError.value = null;
  } catch (err) {
    if (requestId !== draftPreviewRequestId) return;
    const normalized = normalizeError(
      err,
      t("reasoningConfigPanel.alert.previewFailed"),
    );
    previewError.value = normalized.message;
  } finally {
    if (requestId === draftPreviewRequestId) {
      isPreviewLoading.value = false;
    }
  }
}

async function load() {
  if (!props.ownerId) return;
  isLoading.value = true;
  error.value = null;
  try {
    const [catalogResponse, configResponse, previewResponse] = await Promise.all([
      props.actions.getCatalog(),
      props.actions.getConfig(props.ownerId),
      props.actions.previewSaved(props.ownerId),
    ]);

    catalog.value = catalogResponse;
    config.value = configResponse;
    savedPreview.value = previewResponse;
    draftPreview.value = null;
    previewError.value = null;
    hydrateDraft(configResponse);
  } catch (err) {
    const normalized = normalizeError(
      err,
      t("reasoningConfigPanel.alert.loadFailed"),
    );
    error.value = normalized.message;
  } finally {
    isLoading.value = false;
  }
}

async function saveConfig() {
  if (!props.ownerId || isSaving.value) return;

  if (showCustomEditor.value && !familyKeyDraft.value) {
    toastController.warn(t("reasoningConfigPanel.alert.selectPatchFamily"));
    return;
  }

  isSaving.value = true;
  try {
    if (props.ownerKind === "provider") {
      await props.actions.updateConfig(props.ownerId, {
        family_key: familyKeyDraft.value,
        presets: buildPresetPayload(),
      });
    } else if (modeDraft.value === "inherit") {
      await props.actions.updateConfig(props.ownerId, { mode: "inherit" });
    } else if (modeDraft.value === "disabled") {
      await props.actions.updateConfig(props.ownerId, { mode: "disabled" });
    } else {
      await props.actions.updateConfig(props.ownerId, {
        mode: "custom",
        family_key: familyKeyDraft.value,
        presets: buildPresetPayload(),
      });
    }

    await load();
    emit("saved");
    toastController.success(t("reasoningConfigPanel.alert.saved"));
  } catch (err) {
    const normalized = normalizeError(
      err,
      t("reasoningConfigPanel.alert.saveFailed"),
    );
    toastController.error(normalized.message);
  } finally {
    isSaving.value = false;
  }
}

async function clearConfig() {
  if (!props.ownerId || isSaving.value) return;

  if (!config.value?.owner_config) {
    if (config.value) hydrateDraft(config.value);
    draftPreview.value = null;
    previewError.value = null;
    toastController.success(t("reasoningConfigPanel.alert.draftCleared"));
    return;
  }

  isSaving.value = true;
  try {
    await props.actions.deleteConfig(props.ownerId);
    await load();
    emit("saved");
    toastController.success(t("reasoningConfigPanel.alert.cleared"));
  } catch (err) {
    const normalized = normalizeError(
      err,
      t("reasoningConfigPanel.alert.clearFailed"),
    );
    toastController.error(normalized.message);
  } finally {
    isSaving.value = false;
  }
}

watch(
  () => props.ownerId,
  () => {
    void load();
  },
);

watch(familyKeyDraft, () => {
  clearUnsupportedDraftPresets();
});

watch(currentSnapshot, () => {
  scheduleDraftPreview();
});

watch(
  () => props.providerType,
  () => {
    if (props.ownerKind === "provider" && props.ownerId) {
      void load();
      return;
    }
    scheduleDraftPreview();
  },
);

onMounted(() => {
  void load();
});

onBeforeUnmount(() => {
  clearDraftPreviewTimer();
  draftPreviewRequestId += 1;
});
</script>
