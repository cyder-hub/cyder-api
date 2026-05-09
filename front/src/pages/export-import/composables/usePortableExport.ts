import { computed, onMounted, ref } from "vue";
import type { ComposerTranslation } from "vue-i18n";
import { toastController } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import * as portableConfigService from "@/services/portableConfig";
import type {
  FileProtectionMode,
  PortableExportResponse,
  PortableModuleId,
  PortableModuleRegistryResponse,
  PortableModuleSelection,
  PortableSubrangeId,
} from "@/services/types";
import {
  buildPortableModuleRows,
  createDefaultPortableExportSelections,
  downloadPortableExport,
  enforcePortableExportSelections,
  selectedPortableModuleIds,
  togglePortableModuleSelection,
  togglePortableSubrangeSelection,
} from "./portableConfigState";

interface UsePortableExportOptions {
  t: ComposerTranslation;
}

export function usePortableExport({ t }: UsePortableExportOptions) {
  const registry = ref<PortableModuleRegistryResponse | null>(null);
  const moduleSelections = ref(createEmptySelections());
  const fileProtection = ref<FileProtectionMode>("password_encrypted");
  const password = ref("");
  const autoGeneratePassword = ref(true);
  const isLoadingModules = ref(false);
  const isExporting = ref(false);
  const error = ref<string | null>(null);
  const exportResult = ref<PortableExportResponse | null>(null);
  const downloadedFilename = ref("");

  const selectedModuleIds = computed(() =>
    selectedPortableModuleIds(moduleSelections.value),
  );
  const moduleRows = computed(() =>
    buildPortableModuleRows(registry.value, moduleSelections.value),
  );
  const selectedModulesContainSecrets = computed(() =>
    (registry.value?.modules ?? []).some(
      (module) =>
        selectedModuleIds.value.has(module.module_id) && module.contains_secrets,
    ),
  );
  const canExport = computed(
    () => moduleSelections.value.length > 0 && !isExporting.value,
  );

  async function loadModules() {
    isLoadingModules.value = true;
    error.value = null;
    try {
      const response = await portableConfigService.getPortableModules();
      registry.value = response;
      moduleSelections.value = createDefaultPortableExportSelections(response);
    } catch (unknownError: unknown) {
      const normalized = normalizeError(
        unknownError,
        t("common.unknownError"),
      );
      error.value = normalized.message;
      toastController.error(t("portableConfigPage.export.loadFailed"), normalized.message);
    } finally {
      isLoadingModules.value = false;
    }
  }

  function setFileProtection(mode: FileProtectionMode) {
    fileProtection.value = mode;
    if (mode === "plaintext") {
      autoGeneratePassword.value = false;
    } else if (!password.value.trim()) {
      autoGeneratePassword.value = true;
    }
  }

  function toggleModule(moduleId: PortableModuleId, checked: boolean) {
    if (!registry.value) {
      return;
    }
    moduleSelections.value = togglePortableModuleSelection(
      registry.value,
      moduleSelections.value,
      moduleId,
      checked,
    );
  }

  function toggleSubrange(
    moduleId: PortableModuleId,
    subrangeId: PortableSubrangeId,
    checked: boolean,
  ) {
    if (!registry.value) {
      return;
    }
    moduleSelections.value = togglePortableSubrangeSelection(
      registry.value,
      moduleSelections.value,
      moduleId,
      subrangeId,
      checked,
    );
  }

  async function runExport() {
    if (!registry.value || !canExport.value) {
      return;
    }
    if (
      fileProtection.value === "password_encrypted" &&
      !password.value.trim() &&
      !autoGeneratePassword.value
    ) {
      toastController.warn(t("portableConfigPage.export.passwordRequired"));
      return;
    }

    isExporting.value = true;
    error.value = null;
    exportResult.value = null;
    downloadedFilename.value = "";
    try {
      moduleSelections.value = enforcePortableExportSelections(
        registry.value,
        moduleSelections.value,
      );
      const response = await portableConfigService.exportPortableConfig({
        selected_modules: moduleSelections.value,
        file_protection: fileProtection.value,
        password:
          fileProtection.value === "password_encrypted"
            ? password.value.trim() || null
            : null,
        auto_generate_password:
          fileProtection.value === "password_encrypted" &&
          autoGeneratePassword.value,
      });
      exportResult.value = response;
      downloadedFilename.value = downloadPortableExport(response);
      toastController.success(t("portableConfigPage.export.success"));
    } catch (unknownError: unknown) {
      const normalized = normalizeError(
        unknownError,
        t("common.unknownError"),
      );
      error.value = normalized.message;
      toastController.error(t("portableConfigPage.export.failed"), normalized.message);
    } finally {
      isExporting.value = false;
    }
  }

  onMounted(() => {
    void loadModules();
  });

  return {
    registry,
    moduleSelections,
    fileProtection,
    password,
    autoGeneratePassword,
    isLoadingModules,
    isExporting,
    error,
    exportResult,
    downloadedFilename,
    moduleRows,
    selectedModulesContainSecrets,
    canExport,
    loadModules,
    setFileProtection,
    toggleModule,
    toggleSubrange,
    runExport,
  };
}

function createEmptySelections(): PortableModuleSelection[] {
  return [];
}
