import { computed, ref } from "vue";
import type { ComposerTranslation } from "vue-i18n";
import { toastController } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import * as portableConfigService from "@/services/portableConfig";
import type {
  ConflictStrategy,
  PortableApplyResult,
  PortableDangerousPatchConfirmation,
  PortableModuleId,
  PortableModuleSelection,
  PortablePreviewResponse,
} from "@/services/types";
import {
  buildPortablePreviewModuleRows,
  canApplyPortableImport,
  createDefaultPortableImportSelections,
  flattenPortableBlockingIssues,
  getPortableApplyDisabledReasonCode,
  hasPortableBlockingState,
  mergeDangerousPatchConfirmations,
  summarizePortableApplyResult,
  togglePortablePreviewModuleSelection,
  updateDangerousPatchConfirmation,
} from "./portableConfigState";

interface UsePortableImportOptions {
  t: ComposerTranslation;
}

export function usePortableImport({ t }: UsePortableImportOptions) {
  const fileName = ref("");
  const content = ref("");
  const password = ref("");
  const preview = ref<PortablePreviewResponse | null>(null);
  const selectedModules = ref(createEmptySelections());
  const conflictStrategy = ref<ConflictStrategy>("fail_on_conflict");
  const reason = ref("");
  const dangerousPatchConfirmations = ref(createEmptyConfirmations());
  const applyResult = ref<PortableApplyResult | null>(null);
  const isReadingFile = ref(false);
  const isPreviewing = ref(false);
  const isApplying = ref(false);
  const error = ref<string | null>(null);

  const previewModuleRows = computed(() =>
    buildPortablePreviewModuleRows(preview.value, selectedModules.value),
  );
  const blockingIssues = computed(() =>
    flattenPortableBlockingIssues(preview.value),
  );
  const hasBlockingState = computed(() => hasPortableBlockingState(preview.value));
  const canPreview = computed(
    () => content.value.trim().length > 0 && !isPreviewing.value,
  );
  const canApply = computed(() =>
    canApplyPortableImport({
      preview: preview.value,
      selectedModules: selectedModules.value,
      conflictStrategy: conflictStrategy.value,
      reason: reason.value,
      dangerousPatchConfirmations: dangerousPatchConfirmations.value,
    }),
  );
  const applyDisabledReason = computed(() => {
    const code = getPortableApplyDisabledReasonCode({
      preview: preview.value,
      selectedModules: selectedModules.value,
      conflictStrategy: conflictStrategy.value,
      reason: reason.value,
      dangerousPatchConfirmations: dangerousPatchConfirmations.value,
    });
    return code ? t(`portableConfigPage.import.applyDisabledReason.${code}`) : "";
  });
  const applySummaryText = computed(() =>
    summarizePortableApplyResult(applyResult.value),
  );

  async function readFile(file: File | null) {
    resetImportState();
    if (!file) {
      return;
    }

    isReadingFile.value = true;
    fileName.value = file.name;
    try {
      content.value = await file.text();
    } catch (unknownError: unknown) {
      const normalized = normalizeError(
        unknownError,
        t("common.unknownError"),
      );
      error.value = normalized.message;
      toastController.error(t("portableConfigPage.import.readFailed"), normalized.message);
    } finally {
      isReadingFile.value = false;
    }
  }

  async function runPreview() {
    if (!canPreview.value) {
      return;
    }

    isPreviewing.value = true;
    error.value = null;
    preview.value = null;
    applyResult.value = null;
    selectedModules.value = createEmptySelections();
    dangerousPatchConfirmations.value = createEmptyConfirmations();
    try {
      const response = await portableConfigService.previewPortableImport({
        content: content.value,
        password: password.value.trim() || null,
      });
      preview.value = response;
      selectedModules.value = createDefaultPortableImportSelections(response);
      dangerousPatchConfirmations.value = mergeDangerousPatchConfirmations(
        response,
        [],
      );
      if (response.blocking_issues.length > 0) {
        toastController.warn(t("portableConfigPage.import.previewBlocked"));
      } else {
        toastController.success(t("portableConfigPage.import.previewReady"));
      }
    } catch (unknownError: unknown) {
      const normalized = normalizeError(
        unknownError,
        t("common.unknownError"),
      );
      error.value = normalized.message;
      toastController.error(t("portableConfigPage.import.previewFailed"), normalized.message);
    } finally {
      isPreviewing.value = false;
    }
  }

  function toggleModule(moduleId: PortableModuleId, checked: boolean) {
    if (!preview.value) {
      return;
    }
    selectedModules.value = togglePortablePreviewModuleSelection(
      preview.value,
      selectedModules.value,
      moduleId,
      checked,
    );
  }

  function setDangerousPatchConfirmation(
    path: string,
    target: string,
    confirmed: boolean,
  ) {
    dangerousPatchConfirmations.value = updateDangerousPatchConfirmation(
      dangerousPatchConfirmations.value,
      path,
      target,
      confirmed,
    );
  }

  async function runApply() {
    if (!preview.value || !canApply.value) {
      toastController.warn(t("portableConfigPage.import.applyNotReady"));
      return;
    }

    isApplying.value = true;
    error.value = null;
    applyResult.value = null;
    try {
      const response = await portableConfigService.applyPortableImport({
        content: content.value,
        password: password.value.trim() || null,
        bundle_digest: preview.value.bundle_digest,
        selected_modules: selectedModules.value,
        conflict_strategy: conflictStrategy.value,
        reason: reason.value.trim(),
        dangerous_patch_confirmations: dangerousPatchConfirmations.value,
      });
      applyResult.value = response;
      toastController.success(t("portableConfigPage.import.applySuccess"));
    } catch (unknownError: unknown) {
      const normalized = normalizeError(
        unknownError,
        t("common.unknownError"),
      );
      error.value = normalized.message;
      toastController.error(t("portableConfigPage.import.applyFailed"), normalized.message);
    } finally {
      isApplying.value = false;
    }
  }

  function resetImportState() {
    fileName.value = "";
    content.value = "";
    preview.value = null;
    selectedModules.value = createEmptySelections();
    dangerousPatchConfirmations.value = createEmptyConfirmations();
    applyResult.value = null;
    error.value = null;
  }

  return {
    fileName,
    content,
    password,
    preview,
    selectedModules,
    conflictStrategy,
    reason,
    dangerousPatchConfirmations,
    applyResult,
    isReadingFile,
    isPreviewing,
    isApplying,
    error,
    previewModuleRows,
    blockingIssues,
    hasBlockingState,
    canPreview,
    canApply,
    applyDisabledReason,
    applySummaryText,
    readFile,
    runPreview,
    toggleModule,
    setDangerousPatchConfirmation,
    runApply,
  };
}

function createEmptySelections(): PortableModuleSelection[] {
  return [];
}

function createEmptyConfirmations(): PortableDangerousPatchConfirmation[] {
  return [];
}
