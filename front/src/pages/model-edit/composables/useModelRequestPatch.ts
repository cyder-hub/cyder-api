import { computed, ref, watch, type Ref } from "vue";
import { useI18n } from "vue-i18n";

import * as requestPatchService from "@/services/requestPatch";
import { normalizeError } from "@/utils/error";
import { toastController } from "@/services/uiFeedback";
import type {
  InheritedRequestPatchRule,
  RequestPatchConflict,
  RequestPatchExplainEntry,
  RequestPatchExplainStatus,
  RequestPatchPayload,
  RequestPatchRule,
  RequestPatchUpdatePayload,
  ResolvedRequestPatchRule,
} from "@/services/types";
import type {
  RequestPatchBadgeVariant,
  RequestPatchRuleActions,
} from "@/components/request-patch/types";

export function useModelRequestPatch(
  modelId: Readonly<Ref<number | null>>,
  providerLabel: Readonly<Ref<string>>,
) {
  const { t } = useI18n();

  const isLoading = ref(true);
  const isRefreshing = ref(false);
  const loadError = ref<string | null>(null);
  const directRules = ref<RequestPatchRule[]>([]);
  const inheritedRules = ref<InheritedRequestPatchRule[]>([]);
  const effectiveRules = ref<ResolvedRequestPatchRule[]>([]);
  const explainEntries = ref<RequestPatchExplainEntry[]>([]);
  const conflicts = ref<RequestPatchConflict[]>([]);
  const hasConflicts = ref(false);

  const explainByRuleId = computed<Map<number, RequestPatchExplainEntry>>(() => {
    const map = new Map<number, RequestPatchExplainEntry>();
    for (const entry of explainEntries.value) {
      map.set(entry.rule.id, entry as any);
    }
    return map;
  });

  const actions: RequestPatchRuleActions = {
    createRule: (payload: RequestPatchPayload) => {
      return requestPatchService.createModelRequestPatch(modelId.value!, payload);
    },
    updateRule: (ruleId: number, payload: RequestPatchUpdatePayload) => {
      return requestPatchService.updateModelRequestPatch(
        modelId.value!,
        ruleId,
        payload,
      );
    },
    deleteRule: (ruleId: number) => {
      return requestPatchService.deleteModelRequestPatch(modelId.value!, ruleId);
    },
  };

  function clearState() {
    directRules.value = [];
    inheritedRules.value = [];
    effectiveRules.value = [];
    explainEntries.value = [];
    conflicts.value = [];
    hasConflicts.value = false;
  }

  function formatRuleIds(ruleIds: number[]): string {
    return ruleIds.map((id) => `#${id}`).join(", ");
  }

  function getExplainEntry(ruleId: number): RequestPatchExplainEntry | null {
    return explainByRuleId.value.get(ruleId) ?? null;
  }

  function getExplainStatus(
    status: RequestPatchExplainStatus,
  ): { label: string; variant: RequestPatchBadgeVariant } {
    switch (status) {
      case "Effective":
        return {
          label: t("modelEditPage.requestPatch.states.effective"),
          variant: "secondary",
        };
      case "Overridden":
        return {
          label: t("modelEditPage.requestPatch.states.overridden"),
          variant: "outline",
        };
      case "Conflicted":
        return {
          label: t("modelEditPage.requestPatch.states.conflicted"),
          variant: "destructive",
        };
    }
  }

  function getDirectRuleState(rule: RequestPatchRule): {
    label: string;
    variant: RequestPatchBadgeVariant;
  } {
    if (!rule.is_enabled) {
      return {
        label: t("modelEditPage.requestPatch.states.disabled"),
        variant: "outline",
      };
    }

    const explainEntry = getExplainEntry(rule.id);
    if (!explainEntry) {
      return {
        label: t("modelEditPage.requestPatch.states.enabled"),
        variant: "secondary",
      };
    }

    return getExplainStatus(explainEntry.status);
  }

  function getDirectRuleTrace(rule: RequestPatchRule): string {
    if (!rule.is_enabled) {
      return t("modelEditPage.requestPatch.messages.disabledSkipped");
    }

    const explainEntry = getExplainEntry(rule.id);
    if (!explainEntry) {
      return t("modelEditPage.requestPatch.messages.directEffective");
    }

    return explainEntry.message || t("modelEditPage.requestPatch.messages.directEffective");
  }

  function getInheritedRuleState(item: InheritedRequestPatchRule): {
    label: string;
    variant: RequestPatchBadgeVariant;
  } {
    if (item.conflict_with_rule_ids.length > 0) {
      return {
        label: t("modelEditPage.requestPatch.states.conflicted"),
        variant: "destructive",
      };
    }

    if (item.overridden_by_rule_id !== null) {
      return {
        label: t("modelEditPage.requestPatch.states.overridden"),
        variant: "outline",
      };
    }

    return {
      label: t("modelEditPage.requestPatch.states.effective"),
      variant: "secondary",
    };
  }

  function getInheritedRuleTrace(item: InheritedRequestPatchRule): string {
    if (item.conflict_with_rule_ids.length > 0) {
      return t("modelEditPage.requestPatch.messages.conflictsWithRules", {
        ids: formatRuleIds(item.conflict_with_rule_ids),
      });
    }

    if (item.overridden_by_rule_id !== null) {
      return t("modelEditPage.requestPatch.messages.overriddenByRule", {
        id: `#${item.overridden_by_rule_id}`,
      });
    }

    return t("modelEditPage.requestPatch.messages.inheritedEffective");
  }

  function getEffectiveRuleTrace(rule: ResolvedRequestPatchRule): string {
    if (rule.overridden_rule_ids.length > 0) {
      return t("modelEditPage.requestPatch.messages.overridesProviderRules", {
        ids: formatRuleIds(rule.overridden_rule_ids),
      });
    }

    return t("modelEditPage.requestPatch.messages.effectiveFromOrigin", {
      origin: t(`modelEditPage.requestPatch.origin.${rule.source_origin}`),
      id: `#${rule.source_rule_id}`,
    });
  }

  async function refreshExplainState(showLoading = false) {
    if (!modelId.value) {
      clearState();
      isLoading.value = false;
      loadError.value = null;
      return;
    }

    if (showLoading) {
      isLoading.value = true;
    } else {
      isRefreshing.value = true;
    }

    try {
      loadError.value = null;
      const [directResponse, explainResponse] = await Promise.all([
        requestPatchService.listModelRequestPatches(modelId.value),
        requestPatchService.getModelRequestPatchExplain(modelId.value),
      ]);
      directRules.value = directResponse;
      inheritedRules.value = explainResponse.inherited_rules;
      effectiveRules.value = explainResponse.effective_rules;
      explainEntries.value = explainResponse.explain;
      conflicts.value = explainResponse.conflicts;
      hasConflicts.value = explainResponse.has_conflicts;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      if (showLoading) {
        loadError.value = normalizedError.message;
        clearState();
      } else {
        toastController.error(
          t("modelEditPage.requestPatch.alert.loadFailed"),
          normalizedError.message,
        );
      }
    } finally {
      isLoading.value = false;
      isRefreshing.value = false;
    }
  }

  function handleRefresh() {
    void refreshExplainState(false);
  }

  watch(
    modelId,
    () => {
      void refreshExplainState(true);
    },
    { immediate: true },
  );

  return {
    isLoading,
    isRefreshing,
    loadError,
    directRules,
    inheritedRules,
    effectiveRules,
    explainEntries,
    conflicts,
    hasConflicts,
    providerLabel,
    actions,
    formatRuleIds,
    getExplainStatus,
    getDirectRuleState,
    getDirectRuleTrace,
    getInheritedRuleState,
    getInheritedRuleTrace,
    getEffectiveRuleTrace,
    refreshExplainState,
    handleRefresh,
  };
}
