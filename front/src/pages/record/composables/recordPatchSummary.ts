import type { RequestPatchSource } from "../../../services/types";

const emptyValue = "/";

export type RuntimePatchRuleSummary = {
  source?: RequestPatchSource | null;
  overridden_sources?: RequestPatchSource[] | null;
};

export type RuntimePatchConflictSummary = {
  lower_priority_source?: RequestPatchSource | null;
  higher_priority_source?: RequestPatchSource | null;
};

export type RuntimePatchSummary = {
  effective_rules?: RuntimePatchRuleSummary[];
  conflicts?: RuntimePatchConflictSummary[];
};

export type PatchSourceTranslator = (
  key: string,
  params?: Record<string, unknown>,
) => string;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  value != null && typeof value === "object" && !Array.isArray(value);

const asArray = <T,>(value: T[] | null | undefined) =>
  Array.isArray(value) ? value : [];

export const parsePatchSummary = (
  raw: unknown,
): RuntimePatchSummary | null => {
  if (typeof raw === "string") {
    const text = raw.trim();
    if (!text) return null;
    try {
      const parsed = JSON.parse(text);
      return isRecord(parsed) ? (parsed as RuntimePatchSummary) : null;
    } catch {
      return null;
    }
  }

  return isRecord(raw) ? (raw as RuntimePatchSummary) : null;
};

const sourceRuleId = (source: Record<string, unknown>) =>
  typeof source.rule_id === "number" ? source.rule_id : emptyValue;

const sourceNumberField = (source: Record<string, unknown>, key: string) =>
  typeof source[key] === "number" ? source[key] : null;

const sourceStringField = (source: Record<string, unknown>, key: string) => {
  const value = source[key];
  return typeof value === "string" && value.trim() ? value : emptyValue;
};

export const reasoningSourceDetail = (source: Record<string, unknown>) => {
  const configId = sourceNumberField(source, "config_id");
  const configPresetId = sourceNumberField(source, "config_preset_id");
  if (configId != null) {
    return `config ${sourceStringField(source, "config_scope")}/${configId} preset row ${
      configPresetId ?? emptyValue
    }`;
  }

  const legacyProfileId = sourceNumberField(source, "profile_id");
  const legacyPresetId = sourceNumberField(source, "profile_preset_id");
  if (legacyProfileId != null) {
    return `legacy reasoning preset profile ${legacyProfileId} preset row ${
      legacyPresetId ?? emptyValue
    }`;
  }

  return emptyValue;
};

export const patchSourceLabel = (
  source: unknown,
  translate: PatchSourceTranslator,
) => {
  if (!isRecord(source)) {
    return translate("recordPage.detailDialog.attempts.patchSources.unknown", {
      kind: emptyValue,
    });
  }

  const kind = sourceStringField(source, "kind");
  switch (kind) {
    case "provider_rule":
      return translate("recordPage.detailDialog.attempts.patchSources.providerRule", {
        id: sourceRuleId(source),
      });
    case "model_rule":
      return translate("recordPage.detailDialog.attempts.patchSources.modelRule", {
        id: sourceRuleId(source),
      });
    case "reasoning_preset":
      return translate("recordPage.detailDialog.attempts.patchSources.reasoningPreset", {
        preset: sourceStringField(source, "preset"),
        suffix: sourceStringField(source, "suffix"),
        family: sourceStringField(source, "family"),
        source: reasoningSourceDetail(source),
      });
    default:
      return translate("recordPage.detailDialog.attempts.patchSources.unknown", {
        kind,
      });
  }
};

export const patchSourceItemsFromSummary = (
  summary: RuntimePatchSummary | null,
  translate: PatchSourceTranslator,
) => {
  if (!summary) return [];

  const labels: string[] = [];
  const seen = new Set<string>();
  const pushSource = (source: unknown) => {
    if (source == null) return;
    const label = patchSourceLabel(source, translate);
    if (!seen.has(label)) {
      seen.add(label);
      labels.push(label);
    }
  };

  asArray(summary.effective_rules).forEach((rule) => {
    pushSource(rule.source);
    asArray(rule.overridden_sources).forEach(pushSource);
  });
  asArray(summary.conflicts).forEach((conflict) => {
    pushSource(conflict.lower_priority_source);
    pushSource(conflict.higher_priority_source);
  });

  return labels;
};

export const patchSourceItemsFromRaw = (
  raw: unknown,
  translate: PatchSourceTranslator,
) => patchSourceItemsFromSummary(parsePatchSummary(raw), translate);
