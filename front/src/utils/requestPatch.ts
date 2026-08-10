import type {
  JsonValue,
  RequestPatchOperation,
  RequestPatchRule,
  RequestPatchRuleInput,
  RequestPatchVariantInput,
  RequestPatchPlacement,
} from "@/services/types";

export interface RequestPatchRuleEditorState {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json_text: string;
  description: string;
}

export interface RequestPatchVariantEditorState {
  source_id: number;
  model_id: number | null;
  suffix: string | null;
  enabled: boolean;
  expose_in_models: boolean;
  rules: RequestPatchRuleEditorState[];
}

export function requestPatchTargetIdentity(
  placement: RequestPatchPlacement,
  target: string,
): string {
  const normalizedTarget = placement === "HEADER"
    ? target.trim().toLowerCase()
    : target.trim();
  return `${placement}:${normalizedTarget}`;
}

export function parseRequestPatchValue(
  valueJsonText: string,
  operation: RequestPatchOperation,
): { value_json: JsonValue | null; error: string | null } {
  if (operation === "REMOVE") {
    return { value_json: null, error: null };
  }

  if (!valueJsonText.trim()) {
    return { value_json: null, error: "required" };
  }

  try {
    return { value_json: JSON.parse(valueJsonText) as JsonValue, error: null };
  } catch {
    return { value_json: null, error: "invalid" };
  }
}

export function buildRequestPatchRuleInput(
  form: RequestPatchRuleEditorState,
): { input: RequestPatchRuleInput | null; error: string | null } {
  const target = form.target.trim();
  if (!target) {
    return { input: null, error: "target" };
  }

  const parsed = parseRequestPatchValue(form.value_json_text, form.operation);
  if (parsed.error) {
    return { input: null, error: parsed.error };
  }

  return {
    input: {
      placement: form.placement,
      target,
      operation: form.operation,
      value_json: parsed.value_json,
      description: form.description.trim() || null,
    },
    error: null,
  };
}

export function buildRequestPatchVariantPayload(
  state: RequestPatchVariantEditorState,
): { payload: RequestPatchVariantInput | null; error: string | null } {
  const rules: RequestPatchRuleInput[] = [];
  for (const rule of state.rules) {
    const result = buildRequestPatchRuleInput(rule);
    if (result.error || !result.input) {
      return { payload: null, error: result.error ?? "invalid" };
    }
    rules.push(result.input);
  }

  return {
    payload: {
      source_id: state.source_id,
      model_id: state.model_id,
      suffix: state.suffix,
      enabled: state.enabled,
      expose_in_models: state.expose_in_models,
      rules,
    },
    error: null,
  };
}

export function requestPatchRuleToEditorState(
  rule: RequestPatchRule,
): RequestPatchRuleEditorState {
  return {
    placement: rule.placement,
    target: rule.target,
    operation: rule.operation,
    value_json_text: formatRequestPatchValueForEditor(rule.value_json),
    description: rule.description ?? "",
  };
}

export function formatRequestPatchValueForEditor(
  valueJson: string | null,
): string {
  if (valueJson === null) return "";
  try {
    return JSON.stringify(JSON.parse(valueJson), null, 2);
  } catch {
    return valueJson;
  }
}

export function formatRequestPatchValueForDisplay(
  valueJson: string | null,
): string {
  if (valueJson === null) return "—";
  try {
    return JSON.stringify(JSON.parse(valueJson));
  } catch {
    return valueJson;
  }
}

export function variantEditorStateFromAggregate(
  aggregate: {
    variant: {
      source_id: number;
      model_id: number | null;
      suffix: string | null;
      enabled: boolean;
      expose_in_models: boolean;
    };
    rules: RequestPatchRule[];
  },
): RequestPatchVariantEditorState {
  return {
    source_id: aggregate.variant.source_id,
    model_id: aggregate.variant.model_id,
    suffix: aggregate.variant.suffix,
    enabled: aggregate.variant.enabled,
    expose_in_models: aggregate.variant.expose_in_models,
    rules: aggregate.rules.map(requestPatchRuleToEditorState),
  };
}
