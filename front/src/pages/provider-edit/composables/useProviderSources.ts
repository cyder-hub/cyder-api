import { computed, reactive, ref, type Ref } from "vue";
import { useI18n } from "vue-i18n";

import * as providerService from "@/services/providers";
import { confirm, toastController } from "@/services/uiFeedback";
import type { SourceImpactAction, SourceImpactReport, UpstreamSource } from "@/services/types";
import type { EditingProviderData, EditingProviderSource } from "../types";
import { summarizeSourceImpact } from "./sourceImpactViewModel";

export const providerProfileTypes = [
  "OPENAI",
  "GEMINI",
  "GEMINI_OPENAI",
  "VERTEX",
  "VERTEX_OPENAI",
  "ANTHROPIC",
  "RESPONSES",
  "OLLAMA",
] as const;

export interface ProviderSourceDraft {
  profile_type: string;
  endpoint: string;
  use_proxy: boolean;
  is_enabled: boolean;
  is_default: boolean;
}

const emptyDraft = (): ProviderSourceDraft => ({
  profile_type: "OPENAI",
  endpoint: "",
  use_proxy: false,
  is_enabled: true,
  is_default: false,
});

const mapSource = (source: UpstreamSource): EditingProviderSource => ({
  id: source.id,
  provider_id: source.provider_id,
  profile_type: source.profile_type,
  endpoint: source.endpoint,
  use_proxy: source.use_proxy,
  is_enabled: source.is_enabled,
  is_default: source.is_default,
  deleted_at: source.deleted_at,
  created_at: source.created_at,
  updated_at: source.updated_at,
});

export function useProviderSources(editingData: Ref<EditingProviderData>) {
  const { t } = useI18n();
  const isDrawerOpen = ref(false);
  const editingSourceId = ref<number | null>(null);
  const isSaving = ref(false);
  const busySourceId = ref<number | null>(null);
  const draft = reactive<ProviderSourceDraft>(emptyDraft());

  const isEditing = computed(() => editingSourceId.value !== null);
  const editingSource = computed(() =>
    editingSourceId.value === null
      ? null
      : editingData.value.upstream_sources.find(
          (source) => source.id === editingSourceId.value,
        ) ?? null,
  );

  const sourceTarget = (source: Pick<UpstreamSource, "id" | "profile_type">) =>
    `${source.profile_type} · #${source.id}`;

  const confirmSourceAction = async (
    source: EditingProviderSource,
    action: SourceImpactAction,
    titleKey: string,
    fallbackDescriptionKey: string,
  ) => {
    const target = sourceTarget(source);
    try {
      const report: SourceImpactReport = await providerService.previewProviderSourceImpact(
        editingData.value.id as number,
        source.id,
        action,
      );
      const summary = summarizeSourceImpact(report);
      const protocolDetails = summary.protocolLines
        .map(
          (protocol) =>
            `${protocol.downstream_protocol}: ${protocol.selection_changed_count}/${protocol.would_become_unselectable_count}`,
        )
        .join(" · ");
      return confirm({
        title: t(titleKey, { target }),
        description: t("providerEditPage.sources.impactDescription", {
          changed: summary.selectionChangedCount,
          unselectable: summary.wouldBecomeUnselectableCount,
          inheritAll: report.inherit_all_model_count,
          explicit: report.explicit_binding_model_count,
          defaults: report.explicit_default_model_count,
          protocols: protocolDetails,
        }),
      });
    } catch (error) {
      toastController.warn(
        t("providerEditPage.sources.impactUnavailable", {
          error: (error as Error).message || t("common.unknownError"),
        }),
      );
      return confirm({
        title: t(titleKey, { target }),
        description: t(fallbackDescriptionKey, { target }),
      });
    }
  };

  const syncSources = (sources: UpstreamSource[]) => {
    editingData.value.upstream_sources = sources.map(mapSource);
  };

  const refreshSources = async () => {
    if (!editingData.value.id) return;
    const detail = await providerService.getProviderDetail(editingData.value.id);
    editingData.value.is_enabled = detail.provider.is_enabled;
    editingData.value.provider_api_key_mode = detail.provider.provider_api_key_mode;
    syncSources(detail.provider.upstream_sources);

    const sourceSummaries = new Map(
      detail.models.map((model) => [model.model.id, model.source_config]),
    );
    for (const model of editingData.value.models) {
      if (model.id === null) continue;
      const sourceConfig = sourceSummaries.get(model.id);
      if (sourceConfig) {
        model.source_config = sourceConfig;
      }
    }
  };

  const recoverAfterMutationFailure = () => refreshSources().catch(() => undefined);

  const closeDrawer = () => {
    isDrawerOpen.value = false;
    editingSourceId.value = null;
    Object.assign(draft, emptyDraft());
  };

  const openCreate = () => {
    editingSourceId.value = null;
    Object.assign(draft, emptyDraft());
    isDrawerOpen.value = true;
  };

  const openEdit = (source: EditingProviderSource) => {
    editingSourceId.value = source.id;
    Object.assign(draft, {
      profile_type: source.profile_type,
      endpoint: source.endpoint,
      use_proxy: source.use_proxy,
      is_enabled: source.is_enabled,
      is_default: source.is_default,
    });
    isDrawerOpen.value = true;
  };

  const save = async () => {
    const providerId = editingData.value.id;
    if (!providerId || !draft.endpoint.trim()) {
      toastController.warn(t("providerEditPage.sources.endpointRequired"));
      return;
    }

    const currentSource = editingSource.value;
    const nextIsEnabled = draft.is_default ? true : draft.is_enabled;
    if (currentSource) {
      if (currentSource.is_enabled && !nextIsEnabled) {
        const confirmed = await confirmSourceAction(
          currentSource,
          "DISABLE",
          "providerEditPage.sources.confirmDisable",
          "providerEditPage.sources.confirmDisableFallback",
        );
        if (!confirmed) return;
      }
      if (currentSource.is_default !== draft.is_default) {
        const action: SourceImpactAction = draft.is_default
          ? "SET_DEFAULT"
          : "UNSET_DEFAULT";
        const confirmed = await confirmSourceAction(
          currentSource,
          action,
          draft.is_default
            ? "providerEditPage.sources.confirmSetDefault"
            : "providerEditPage.sources.confirmUnsetDefault",
          draft.is_default
            ? "providerEditPage.sources.confirmSetDefaultFallback"
            : "providerEditPage.sources.confirmUnsetDefaultFallback",
        );
        if (!confirmed) return;
      }
    }

    isSaving.value = true;
    try {
      if (editingSourceId.value === null) {
        const createdSource = await providerService.createProviderSource(providerId, {
          profile_type: draft.profile_type,
          endpoint: draft.endpoint.trim(),
          use_proxy: draft.use_proxy,
          is_enabled: nextIsEnabled,
          // A newly-created Source has no stable ID until the create commits.
          // Keep it non-default so the default mutation can be previewed against
          // the committed Source before it can demote the current default.
          is_default: false,
        });
        await refreshSources();

        if (draft.is_default) {
          const confirmed = await confirmSourceAction(
            createdSource,
            "SET_DEFAULT",
            "providerEditPage.sources.confirmSetDefault",
            "providerEditPage.sources.confirmSetDefaultFallback",
          );
          if (confirmed) {
            await providerService.updateProviderSource(providerId, createdSource.id, {
              is_enabled: true,
              is_default: true,
            });
            await refreshSources();
          }
        }
      } else {
        await providerService.updateProviderSource(providerId, editingSourceId.value, {
          endpoint: draft.endpoint.trim(),
          use_proxy: draft.use_proxy,
          is_enabled: nextIsEnabled,
          is_default: draft.is_default,
        });
        await refreshSources();
      }
      toastController.success(
        t(
          editingSourceId.value === null
            ? "providerEditPage.sources.createSuccess"
            : "providerEditPage.sources.updateSuccess",
        ),
      );
      closeDrawer();
    } catch (error) {
      await recoverAfterMutationFailure();
      toastController.error(
        t("providerEditPage.sources.saveFailed", {
          error: (error as Error).message || t("common.unknownError"),
        }),
      );
    } finally {
      isSaving.value = false;
    }
  };

  const updateSourceState = async (
    source: EditingProviderSource,
    payload: { is_enabled?: boolean; is_default?: boolean },
  ) => {
    const providerId = editingData.value.id;
    if (!providerId || busySourceId.value !== null) return;
    if (payload.is_enabled === false) {
      const confirmed = await confirmSourceAction(
        source,
        "DISABLE",
        "providerEditPage.sources.confirmDisable",
        "providerEditPage.sources.confirmDisableFallback",
      );
      if (!confirmed) return;
    }
    busySourceId.value = source.id;
    try {
      await providerService.updateProviderSource(providerId, source.id, payload);
      await refreshSources();
    } catch (error) {
      await recoverAfterMutationFailure();
      toastController.error(
        t("providerEditPage.sources.saveFailed", {
          error: (error as Error).message || t("common.unknownError"),
        }),
      );
    } finally {
      busySourceId.value = null;
    }
  };

  const toggleEnabled = (source: EditingProviderSource) =>
    updateSourceState(source, { is_enabled: !source.is_enabled });

  const toggleDefault = async (source: EditingProviderSource) => {
    const action: SourceImpactAction = source.is_default ? "UNSET_DEFAULT" : "SET_DEFAULT";
    const confirmed = await confirmSourceAction(
      source,
      action,
      source.is_default
        ? "providerEditPage.sources.confirmUnsetDefault"
        : "providerEditPage.sources.confirmSetDefault",
      source.is_default
        ? "providerEditPage.sources.confirmUnsetDefaultFallback"
        : "providerEditPage.sources.confirmSetDefaultFallback",
    );
    if (!confirmed) return;
    await updateSourceState(source, {
      is_default: !source.is_default,
      ...(source.is_default || source.is_enabled ? {} : { is_enabled: true }),
    });
  };

  const deleteSource = async (source: EditingProviderSource) => {
    const providerId = editingData.value.id;
    if (!providerId || busySourceId.value !== null) return;
    const confirmed = await confirmSourceAction(
      source,
      "DELETE",
      "providerEditPage.sources.confirmDelete",
      "providerEditPage.sources.confirmDeleteFallback",
    );
    if (!confirmed) return;

    busySourceId.value = source.id;
    try {
      await providerService.deleteProviderSource(providerId, source.id);
      await refreshSources();
      toastController.success(t("providerEditPage.sources.deleteSuccess"));
    } catch (error) {
      await recoverAfterMutationFailure();
      toastController.error(
        t("providerEditPage.sources.deleteFailed", {
          error: (error as Error).message || t("common.unknownError"),
        }),
      );
    } finally {
      busySourceId.value = null;
    }
  };

  return {
    draft,
    editingSource,
    editingSourceId,
    isDrawerOpen,
    isEditing,
    isSaving,
    busySourceId,
    closeDrawer,
    deleteSource,
    openCreate,
    openEdit,
    save,
    sourceTarget,
    toggleDefault,
    toggleEnabled,
  };
}
