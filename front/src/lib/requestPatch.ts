import type {
  JsonValue,
  RequestPatchOperation,
  RequestPatchPayload,
  RequestPatchPlacement,
  RequestPatchUpdatePayload,
} from "@/store/types";

export interface RequestPatchEditorState {
  placement: RequestPatchPlacement;
  target: string;
  operation: RequestPatchOperation;
  value_json_text: string;
  description: string;
  is_enabled: boolean;
}

export function formatRequestPatchValueForEditor(
  value: JsonValue | string | null | undefined,
): string {
  if (value === undefined) {
    return "";
  }

  return JSON.stringify(value, null, 2);
}

export function formatRequestPatchValueForDisplay(
  value: JsonValue | string | null | undefined,
): string {
  if (value === undefined) {
    return "-";
  }

  return typeof value === "string" ? value : JSON.stringify(value);
}

export function parseRequestPatchValueJson(text: string): JsonValue | null {
  return JSON.parse(text) as JsonValue | null;
}

export function buildRequestPatchPayloadFromEditor(
  form: RequestPatchEditorState,
  confirmDangerousTarget = false,
): RequestPatchPayload | RequestPatchUpdatePayload {
  const target = form.target.trim();
  if (!target) {
    throw new Error("Target is required.");
  }

  const payload: RequestPatchPayload | RequestPatchUpdatePayload = {
    placement: form.placement,
    target,
    operation: form.operation,
    description: form.description.trim() || null,
    is_enabled: form.is_enabled,
    confirm_dangerous_target: confirmDangerousTarget,
  };

  if (form.operation === "SET") {
    const raw = form.value_json_text.trim();
    if (!raw) {
      throw new Error("Value JSON is required for SET rules.");
    }
    payload.value_json = parseRequestPatchValueJson(raw);
  }

  return payload;
}
