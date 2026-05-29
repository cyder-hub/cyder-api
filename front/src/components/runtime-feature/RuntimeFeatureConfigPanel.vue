<template>
  <section
    :class="
      embedded
        ? 'space-y-3 border-t border-gray-100 pt-4'
        : 'space-y-4 rounded-xl border border-gray-200 bg-white p-4 sm:p-5'
    "
  >
    <SectionHeader v-if="!embedded" :title="title">
      <template #actions>
        <Button
          variant="outline"
          size="sm"
          :disabled="isLoading || !ownerId"
          @click="load"
        >
          <RefreshCw class="mr-1.5 h-3.5 w-3.5" />
          {{ t("common.refresh") }}
        </Button>
      </template>
    </SectionHeader>

    <div
      v-if="!embedded"
      class="rounded-lg border border-gray-200 bg-gray-50/70 px-4 py-3 text-sm text-gray-600"
    >
      {{ t("runtimeFeatureConfigPanel.messages.reasoningIndependent") }}
    </div>

    <div v-if="!ownerId" class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500">
      {{ t("runtimeFeatureConfigPanel.messages.saveOwnerFirst") }}
    </div>

    <div v-else-if="isLoading" class="flex items-center justify-center py-8 text-sm text-gray-500">
      <Loader2 class="mr-2 h-4 w-4 animate-spin text-gray-400" />
      {{ t("runtimeFeatureConfigPanel.messages.loading") }}
    </div>

    <div v-else-if="error" class="rounded-lg border border-red-200 bg-red-50 px-4 py-3 text-sm text-red-700">
      {{ error }}
    </div>

    <div v-else class="space-y-3">
      <div
        v-for="feature in featureRows"
        :key="feature.feature_key"
        class="rounded-lg border border-gray-200 p-3.5"
      >
        <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
          <div class="min-w-0 space-y-1">
            <h3 class="text-sm font-semibold text-gray-900">
              {{ featureLabel(feature.feature_key, feature.label) }}
            </h3>
            <p v-if="!embedded" class="break-all font-mono text-xs text-gray-500">
              {{ feature.feature_key }}
            </p>
            <p v-if="!embedded" class="text-sm text-gray-600">
              {{ featureDescription(feature.feature_key, feature.description) }}
            </p>
          </div>
          <div class="flex flex-wrap items-center gap-2">
            <Badge :variant="feature.response?.effective_enabled ? 'secondary' : 'outline'" class="font-mono text-[11px]">
              {{ formatBoolean(feature.response?.effective_enabled ?? false) }}
            </Badge>
            <Badge variant="outline" class="font-mono text-[11px]">
              {{ formatSource(feature.response?.effective_source) }}
            </Badge>
          </div>
        </div>

        <div class="mt-3 flex flex-col gap-3 rounded-md bg-gray-50/80 px-3 py-2.5 sm:flex-row sm:items-center sm:justify-between">
          <div class="min-w-0 space-y-1">
            <Label class="text-sm font-medium text-gray-800">
              {{ toggleLabel }}
            </Label>
            <p v-if="!embedded" class="text-xs text-gray-500">
              {{ toggleDescription }}
            </p>
          </div>
          <Checkbox
            :model-value="drafts[feature.feature_key] ?? false"
            :disabled="isSavingFeature === feature.feature_key"
            @update:model-value="(value) => setFeatureDraft(feature.feature_key, value)"
          />
        </div>

        <div class="mt-3 flex flex-col gap-3 border-t border-gray-100 pt-3 sm:flex-row sm:items-center sm:justify-between">
          <div class="flex flex-wrap gap-2">
            <Badge variant="outline" class="font-mono text-[11px]">
              {{ t("runtimeFeatureConfigPanel.summary.ownerConfig") }}:
              {{ formatConfigValue(feature.response?.owner_config?.enabled) }}
            </Badge>
            <Badge
              v-if="ownerKind === 'model'"
              variant="outline"
              class="font-mono text-[11px]"
            >
              {{ t("runtimeFeatureConfigPanel.summary.providerDefault") }}:
              {{ formatConfigValue(feature.response?.provider_config?.enabled) }}
            </Badge>
            <Badge variant="outline" class="font-mono text-[11px]">
              {{ t("runtimeFeatureConfigPanel.summary.effective") }}:
              {{ formatBoolean(feature.response?.effective_enabled ?? feature.default_enabled) }}
            </Badge>
          </div>

          <div class="flex flex-col gap-2 sm:flex-row sm:justify-end">
            <Button
              v-if="feature.response?.owner_config"
              variant="ghost"
              class="w-full text-gray-600 sm:w-auto"
              :disabled="isSavingFeature === feature.feature_key"
              @click="clearFeature(feature.feature_key)"
            >
              <Trash2 class="mr-1.5 h-4 w-4" />
              {{ clearLabel }}
            </Button>
            <Button
              variant="default"
              class="w-full sm:w-auto"
              :disabled="!isFeatureDirty(feature.feature_key) || isSavingFeature === feature.feature_key"
              @click="saveFeature(feature.feature_key)"
            >
              <Loader2
                v-if="isSavingFeature === feature.feature_key"
                class="mr-1.5 h-4 w-4 animate-spin"
              />
              <Save v-else class="mr-1.5 h-4 w-4" />
              {{ saveLabel }}
            </Button>
          </div>
        </div>
      </div>
    </div>
  </section>
</template>

<script setup lang="ts">
import { computed, onMounted, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Loader2, RefreshCw, Save, Trash2 } from "lucide-vue-next";
import SectionHeader from "@/components/SectionHeader.vue";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Label } from "@/components/ui/label";
import * as runtimeFeatureService from "@/services/runtimeFeatureConfig";
import { toastController } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import type {
  RuntimeFeatureCatalog,
  RuntimeFeatureCatalogItem,
  RuntimeFeatureConfigFeature,
  RuntimeFeatureConfigResponse,
  RuntimeFeatureEffectiveSource,
} from "@/services/types";
import {
  createRuntimeFeatureDrafts,
  findRuntimeFeatureResponse,
  runtimeFeatureDraftSnapshot,
  type RuntimeFeatureDrafts,
} from "./runtimeFeatureConfigState";

type RuntimeFeatureOwnerKind = "provider" | "model";

interface FeatureRow extends RuntimeFeatureCatalogItem {
  response: RuntimeFeatureConfigFeature | null;
}

const props = withDefaults(
  defineProps<{
    ownerKind: RuntimeFeatureOwnerKind;
    ownerId: number | null;
    title?: string;
    embedded?: boolean;
  }>(),
  {
    title: undefined,
    embedded: false,
  },
);

const emit = defineEmits<{
  saved: [];
}>();

const { t } = useI18n();

const catalog = ref<RuntimeFeatureCatalog | null>(null);
const config = ref<RuntimeFeatureConfigResponse | null>(null);
const drafts = ref<RuntimeFeatureDrafts>({});
const isLoading = ref(false);
const isSavingFeature = ref<string | null>(null);
const error = ref<string | null>(null);

const title = computed(
  () =>
    props.title ||
    (props.ownerKind === "provider"
      ? t("runtimeFeatureConfigPanel.titles.provider")
      : t("runtimeFeatureConfigPanel.titles.model")),
);

const toggleLabel = computed(() =>
  props.ownerKind === "provider"
    ? t("runtimeFeatureConfigPanel.fields.providerDefault")
    : t("runtimeFeatureConfigPanel.fields.modelOverride"),
);

const toggleDescription = computed(() =>
  props.ownerKind === "provider"
    ? t("runtimeFeatureConfigPanel.messages.providerDefaultHelp")
    : t("runtimeFeatureConfigPanel.messages.modelOverrideHelp"),
);

const saveLabel = computed(() =>
  props.ownerKind === "provider"
    ? t("runtimeFeatureConfigPanel.actions.saveProvider")
    : t("runtimeFeatureConfigPanel.actions.saveModel"),
);

const clearLabel = computed(() =>
  props.ownerKind === "provider"
    ? t("runtimeFeatureConfigPanel.actions.clearProvider")
    : t("runtimeFeatureConfigPanel.actions.clearModel"),
);

const featureRows = computed<FeatureRow[]>(() =>
  (catalog.value?.features ?? []).map((feature) => ({
    ...feature,
    response: findRuntimeFeatureResponse(config.value, feature.feature_key),
  })),
);

function featureLabel(featureKey: string, fallback: string): string {
  if (featureKey === "openai_reasoning_content_repair") {
    return t("runtimeFeatureConfigPanel.features.openaiReasoningContentRepair.label");
  }
  return fallback;
}

function featureDescription(featureKey: string, fallback: string): string {
  if (featureKey === "openai_reasoning_content_repair") {
    return t(
      "runtimeFeatureConfigPanel.features.openaiReasoningContentRepair.description",
    );
  }
  return fallback;
}

function formatBoolean(value: boolean): string {
  return value
    ? t("runtimeFeatureConfigPanel.values.enabled")
    : t("runtimeFeatureConfigPanel.values.disabled");
}

function formatConfigValue(value: boolean | null | undefined): string {
  if (value === null || value === undefined) {
    return t("runtimeFeatureConfigPanel.values.missing");
  }
  return formatBoolean(value);
}

function formatSource(source: RuntimeFeatureEffectiveSource | null | undefined): string {
  switch (source) {
    case "default_false":
      return t("runtimeFeatureConfigPanel.sources.defaultFalse");
    case "provider_default":
      return t("runtimeFeatureConfigPanel.sources.providerDefault");
    case "model_override":
      return t("runtimeFeatureConfigPanel.sources.modelOverride");
    default:
      return source || t("runtimeFeatureConfigPanel.sources.unknown");
  }
}

function hydrateDrafts(nextConfig: RuntimeFeatureConfigResponse | null) {
  drafts.value = createRuntimeFeatureDrafts(catalog.value, nextConfig);
}

async function load() {
  if (!props.ownerId) return;

  isLoading.value = true;
  error.value = null;

  try {
    const [nextCatalog, nextConfig] = await Promise.all([
      runtimeFeatureService.getRuntimeFeatureConfigCatalog(),
      props.ownerKind === "provider"
        ? runtimeFeatureService.getProviderRuntimeFeatureConfig(props.ownerId)
        : runtimeFeatureService.getModelRuntimeFeatureConfig(props.ownerId),
    ]);
    catalog.value = nextCatalog;
    config.value = nextConfig;
    hydrateDrafts(nextConfig);
  } catch (caught: unknown) {
    const normalized = normalizeError(
      caught,
      t("runtimeFeatureConfigPanel.alert.loadFailed"),
    );
    error.value = normalized.message;
    toastController.error(t("runtimeFeatureConfigPanel.alert.loadFailed"));
  } finally {
    isLoading.value = false;
  }
}

function setFeatureDraft(featureKey: string, value: boolean | "indeterminate") {
  drafts.value = {
    ...drafts.value,
    [featureKey]: value === true,
  };
}

function isFeatureDirty(featureKey: string): boolean {
  const currentValue = drafts.value[featureKey] ?? false;
  const currentSnapshot = runtimeFeatureDraftSnapshot({
    [featureKey]: currentValue,
  });
  const savedValue = createRuntimeFeatureDrafts(catalog.value, config.value)[
    featureKey
  ] ?? false;
  const savedSnapshot = runtimeFeatureDraftSnapshot({
    [featureKey]: savedValue,
  });
  return currentSnapshot !== savedSnapshot;
}

async function saveFeature(featureKey: string) {
  if (!props.ownerId) return;

  isSavingFeature.value = featureKey;

  try {
    const payload = { enabled: drafts.value[featureKey] ?? false };
    const nextConfig =
      props.ownerKind === "provider"
        ? await runtimeFeatureService.updateProviderRuntimeFeatureConfig(
            props.ownerId,
            featureKey,
            payload,
          )
        : await runtimeFeatureService.updateModelRuntimeFeatureConfig(
            props.ownerId,
            featureKey,
            payload,
          );
    config.value = nextConfig;
    hydrateDrafts(nextConfig);
    emit("saved");
    toastController.success(t("runtimeFeatureConfigPanel.alert.saved"));
  } catch (caught: unknown) {
    const normalized = normalizeError(
      caught,
      t("runtimeFeatureConfigPanel.alert.saveFailed"),
    );
    toastController.error(
      t("runtimeFeatureConfigPanel.alert.saveFailed"),
      normalized.message,
    );
  } finally {
    isSavingFeature.value = null;
  }
}

async function clearFeature(featureKey: string) {
  if (!props.ownerId) return;

  isSavingFeature.value = featureKey;

  try {
    if (props.ownerKind === "provider") {
      await runtimeFeatureService.deleteProviderRuntimeFeatureConfig(
        props.ownerId,
        featureKey,
      );
    } else {
      await runtimeFeatureService.deleteModelRuntimeFeatureConfig(
        props.ownerId,
        featureKey,
      );
    }
    await load();
    emit("saved");
    toastController.success(t("runtimeFeatureConfigPanel.alert.cleared"));
  } catch (caught: unknown) {
    const normalized = normalizeError(
      caught,
      t("runtimeFeatureConfigPanel.alert.clearFailed"),
    );
    toastController.error(
      t("runtimeFeatureConfigPanel.alert.clearFailed"),
      normalized.message,
    );
  } finally {
    isSavingFeature.value = null;
  }
}

watch(
  () => [props.ownerKind, props.ownerId] as const,
  () => {
    catalog.value = null;
    config.value = null;
    drafts.value = {};
    if (props.ownerId) void load();
  },
);

onMounted(() => {
  if (props.ownerId) void load();
});
</script>
