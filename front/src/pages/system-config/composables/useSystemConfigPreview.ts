import { computed, reactive, ref, shallowRef, watch, type Ref } from "vue";
import { useI18n } from "vue-i18n";

import * as systemConfigService from "@/services/systemConfig";
import type {
  JsonValue,
  SystemConfigChangeRequest,
  SystemConfigField,
  SystemConfigPreviewResponse,
  SystemConfigReport,
} from "@/services/types";
import type { DraftBuildResult, EditDraft } from "../types";
import {
  buildSystemConfigDiffDisplay,
  canApplySystemConfigPreview,
} from "./systemConfigState";
import { toErrorMessage } from "./useSystemConfigReport";

interface UseSystemConfigPreviewOptions {
  fields: Ref<SystemConfigField[]>;
  isMultiInstance: Ref<boolean>;
  setReport: (report: SystemConfigReport) => void;
  afterMutation?: () => void;
}

export function useSystemConfigPreview({
  fields,
  isMultiInstance,
  setReport,
  afterMutation,
}: UseSystemConfigPreviewOptions) {
  const { t } = useI18n();

  const isEditOpen = ref(false);
  const selectedField = shallowRef<SystemConfigField | null>(null);
  const editDraft = reactive<EditDraft>({
    raw: "",
    boolValue: false,
    isNull: false,
  });
  const editReason = ref("");
  const editError = ref<string | null>(null);
  const isPreviewing = ref(false);
  const isApplying = ref(false);
  const preview = shallowRef<SystemConfigPreviewResponse | null>(null);
  const previewPayload = shallowRef<SystemConfigChangeRequest | null>(null);
  const isResetOpen = ref(false);
  const resetTargets = shallowRef<SystemConfigField[]>([]);
  const resetReason = ref("");
  const resetError = ref<string | null>(null);
  const isResetting = ref(false);

  const previewDiffRows = computed(() =>
    buildSystemConfigDiffDisplay(preview.value?.diff ?? []),
  );
  const previewWarningRows = computed(
    () => preview.value?.validation.warnings ?? [],
  );
  const runtimeActionLabels = computed(() => {
    const actions = preview.value?.runtime_actions;
    if (!actions) {
      return [];
    }
    const labels: string[] = [];
    if (actions.update_runtime_snapshot) {
      labels.push(t("systemConfigPage.preview.runtimeSnapshot"));
    }
    if (actions.update_log_level) {
      labels.push(t("systemConfigPage.preview.logLevel"));
    }
    if (actions.rebuild_http_client) {
      labels.push(t("systemConfigPage.preview.httpClient"));
    }
    if (actions.hot_reloadable_paths.length) {
      labels.push(
        t("systemConfigPage.preview.hotPaths", {
          count: actions.hot_reloadable_paths.length,
        }),
      );
    }
    return labels;
  });
  const draftValidationError = computed(() => {
    if (!selectedField.value) {
      return null;
    }
    const result = buildDraftValue(selectedField.value, editDraft, t);
    return result.ok ? null : result.message;
  });
  const currentChangePayload = computed<SystemConfigChangeRequest | null>(() =>
    buildValidChangePayload(),
  );
  const canApplyPreview = computed(() =>
    canApplySystemConfigPreview({
      preview: preview.value,
      previewPayload: previewPayload.value,
      currentPayload: currentChangePayload.value,
      reason: editReason.value,
      draftValidationError: draftValidationError.value,
    }),
  );
  const resetTargetPaths = computed<string[]>(() =>
    resetTargets.value.map((field) => field.path),
  );

  function canEditField(field: SystemConfigField): boolean {
    return field.editable && field.hot_reloadable && !isMultiInstance.value;
  }

  function enumOptionsForField(field: SystemConfigField): string[] {
    if (field.path === "log_level") {
      return ["trace", "debug", "info", "warn", "error"];
    }
    if (field.path === "deployment.mode") {
      return ["single_instance", "multi_instance"];
    }
    const firstConstraint = field.constraints.find((constraint) =>
      constraint.includes(" or "),
    );
    if (!firstConstraint) {
      return [];
    }
    return firstConstraint
      .split(/\s+or\s+|,\s*/)
      .map((item) => item.trim())
      .filter((item) => /^[a-z0-9_]+$/i.test(item));
  }

  function openEditDialog(field: SystemConfigField): void {
    if (!canEditField(field)) {
      return;
    }
    selectedField.value = field;
    const value = field.value;
    editDraft.isNull = value === null;
    editDraft.boolValue = typeof value === "boolean" ? value : false;
    editDraft.raw = value === null ? "" : valueToDraftString(value);
    editReason.value = "";
    editError.value = null;
    preview.value = null;
    previewPayload.value = null;
    isEditOpen.value = true;
  }

  function handleEditOpenChange(open: boolean): void {
    isEditOpen.value = open;
    if (!open) {
      selectedField.value = null;
      editError.value = null;
      preview.value = null;
      previewPayload.value = null;
    }
  }

  function openSingleResetDialog(field: SystemConfigField): void {
    if (!canEditField(field)) {
      return;
    }
    resetTargets.value = [field];
    resetReason.value = "";
    resetError.value = null;
    isResetOpen.value = true;
  }

  function openAllOverridesResetDialog(): void {
    if (isMultiInstance.value) {
      return;
    }
    const targets = fields.value.filter(
      (field) => field.editable && field.source.kind === "override_file",
    );
    if (!targets.length) {
      return;
    }
    resetTargets.value = targets;
    resetReason.value = "";
    resetError.value = null;
    isResetOpen.value = true;
  }

  function handleResetOpenChange(open: boolean): void {
    isResetOpen.value = open;
    if (!open) {
      resetTargets.value = [];
      resetError.value = null;
    }
  }

  function clearEditPreview(): void {
    preview.value = null;
    previewPayload.value = null;
  }

  function buildValidChangePayload(): SystemConfigChangeRequest | null {
    const field = selectedField.value;
    if (!field) {
      return null;
    }
    const result = buildDraftValue(field, editDraft, t);
    if (!result.ok) {
      return null;
    }
    return {
      changes: {
        [field.path]: result.value,
      },
    };
  }

  function buildChangePayload(): SystemConfigChangeRequest | null {
    const payload = buildValidChangePayload();
    if (payload) {
      return payload;
    }
    if (selectedField.value) {
      const result = buildDraftValue(selectedField.value, editDraft, t);
      if (!result.ok) {
        editError.value = result.message;
      }
    }
    return null;
  }

  async function previewEdit(): Promise<void> {
    const payload = buildChangePayload();
    if (!payload) {
      return;
    }
    isPreviewing.value = true;
    editError.value = null;
    preview.value = null;
    previewPayload.value = null;
    try {
      const response = await systemConfigService.previewSystemConfig(payload);
      preview.value = response;
      previewPayload.value = payload;
    } catch (err: unknown) {
      editError.value = toErrorMessage(err);
    } finally {
      isPreviewing.value = false;
    }
  }

  async function applyEdit(): Promise<void> {
    const payload = buildChangePayload();
    if (!payload || !selectedField.value || !canApplyPreview.value) {
      return;
    }
    isApplying.value = true;
    editError.value = null;
    try {
      setReport(
        await systemConfigService.applySystemConfig({
          ...payload,
          reason: editReason.value.trim(),
        }),
      );
      afterMutation?.();
      handleEditOpenChange(false);
    } catch (err: unknown) {
      editError.value = toErrorMessage(err);
    } finally {
      isApplying.value = false;
    }
  }

  async function resetSelectedFields(): Promise<void> {
    const reason = resetReason.value.trim();
    if (!resetTargetPaths.value.length || !reason) {
      return;
    }
    isResetting.value = true;
    resetError.value = null;
    try {
      setReport(
        await systemConfigService.resetSystemConfig({
          paths: resetTargetPaths.value,
          reason,
        }),
      );
      afterMutation?.();
      handleResetOpenChange(false);
    } catch (err: unknown) {
      resetError.value = toErrorMessage(err);
    } finally {
      isResetting.value = false;
    }
  }

  watch(
    () => [
      selectedField.value?.path ?? "",
      editDraft.raw,
      editDraft.boolValue,
      editDraft.isNull,
    ],
    () => {
      if (preview.value || previewPayload.value) {
        clearEditPreview();
      }
    },
  );

  return {
    isEditOpen,
    selectedField,
    editDraft,
    editReason,
    editError,
    isPreviewing,
    isApplying,
    preview,
    previewPayload,
    isResetOpen,
    resetTargets,
    resetReason,
    resetError,
    isResetting,
    previewDiffRows,
    previewWarningRows,
    runtimeActionLabels,
    draftValidationError,
    currentChangePayload,
    canApplyPreview,
    resetTargetPaths,
    canEditField,
    enumOptionsForField,
    openEditDialog,
    handleEditOpenChange,
    openSingleResetDialog,
    openAllOverridesResetDialog,
    handleResetOpenChange,
    previewEdit,
    applyEdit,
    resetSelectedFields,
  };
}

export function valueToDraftString(value: JsonValue): string {
  if (typeof value === "string") {
    return value;
  }
  if (typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value, null, 2);
}

export function buildDraftValue(
  field: SystemConfigField,
  editDraft: EditDraft,
  t: (key: string) => string,
): DraftBuildResult {
  if (field.value_kind === "bool") {
    return { ok: true, value: editDraft.boolValue };
  }
  if (field.value_kind === "nullable_string") {
    return { ok: true, value: editDraft.isNull ? null : editDraft.raw };
  }
  if (field.value_kind === "string" || field.value_kind === "enum") {
    return { ok: true, value: editDraft.raw };
  }
  if (field.value_kind === "nullable_u64") {
    if (editDraft.isNull) {
      return { ok: true, value: null };
    }
    return parseUnsignedInteger(field.value_kind, editDraft.raw, t);
  }
  if (
    field.value_kind === "u16" ||
    field.value_kind === "u32" ||
    field.value_kind === "u64" ||
    field.value_kind === "usize"
  ) {
    return parseUnsignedInteger(field.value_kind, editDraft.raw, t);
  }
  try {
    return { ok: true, value: JSON.parse(editDraft.raw) as JsonValue };
  } catch {
    return {
      ok: false,
      message: t("systemConfigPage.edit.invalidJson"),
    };
  }
}

function parseUnsignedInteger(
  kind: SystemConfigField["value_kind"],
  rawValue: string,
  t: (key: string) => string,
): DraftBuildResult {
  const trimmed = rawValue.trim();
  if (!/^\d+$/.test(trimmed)) {
    return {
      ok: false,
      message: t("systemConfigPage.edit.invalidNumber"),
    };
  }
  const value = Number(trimmed);
  if (!Number.isSafeInteger(value)) {
    return {
      ok: false,
      message: t("systemConfigPage.edit.invalidNumber"),
    };
  }
  if (kind === "u16" && value > 65535) {
    return {
      ok: false,
      message: t("systemConfigPage.edit.invalidU16"),
    };
  }
  if (kind === "u32" && value > 4294967295) {
    return {
      ok: false,
      message: t("systemConfigPage.edit.invalidU32"),
    };
  }
  return { ok: true, value };
}
