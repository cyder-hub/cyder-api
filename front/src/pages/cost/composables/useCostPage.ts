import { computed, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { confirm } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import { toastController } from "@/services/uiFeedback";
import { formatPriceInputFromNanos } from "@/utils/money";
import * as costService from "@/services/cost";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostComponent,
  CostCatalogVersionDetail,
  CostTemplateSummary,
} from "@/services/types/cost";
import { useCostCatalogs, resolvePreferredCostVersionId } from "./useCostCatalogs";
import { useCostPreview } from "./useCostPreview";
import {
  CHARGE_KIND_OPTIONS,
  METER_OPTIONS,
  TIER_BASIS_OPTIONS,
  createEmptyCatalogDraft,
  createEmptyComponentDraft,
  createEmptyTierRow,
  createEmptyVersionDraft,
  formatNumber,
  formatRateDisplay,
  formatRateInput,
  parseDateTimeLocal,
  parseOptionalJsonObject,
  parseRequiredNonNegativeInteger,
  parseRequiredPrice,
  parseRequiredRate,
  parseTierConfig,
  prettyJson,
} from "../helpers";
import type { CatalogDraft, ComponentDraft, VersionDraft } from "../types";

export const useCostPage = () => {
  const { t } = useI18n();
  const costCatalogs = useCostCatalogs({
    getCostCatalogList: costService.getCostCatalogList,
    getCostCatalogVersion: costService.getCostCatalogVersion,
  });

  const selectedCatalog = costCatalogs.selectedCatalog;
  const selectedVersion = costCatalogs.selectedVersion;
  const showArchivedVersions = ref(false);
  const selectedCatalogVersions = computed(() =>
    costCatalogs.selectedCatalogVersions.value.filter(
      (version) => showArchivedVersions.value || !version.is_archived,
    ),
  );
  const components = computed(() => costCatalogs.versionDetail.value?.components ?? []);
  const selectedVersionSummary = computed(
    () => costCatalogs.versionDetail.value?.version ?? selectedVersion.value,
  );

  const isCatalogDialogOpen = ref(false);
  const isVersionDialogOpen = ref(false);
  const isComponentDialogOpen = ref(false);
  const isTemplateDialogOpen = ref(false);
  const isEditorDialogOpen = ref(false);
  const isSavingCatalog = ref(false);
  const isSavingVersion = ref(false);
  const isSavingComponent = ref(false);
  const isLoadingTemplates = ref(false);
  const importingTemplateKey = ref<string | null>(null);
  const togglingVersionId = ref<number | null>(null);
  const managingVersionId = ref<number | null>(null);
  const duplicatingVersionId = ref<number | null>(null);
  const duplicatingCatalogId = ref<number | null>(null);
  const shouldOpenEditorAfterCatalogSave = ref(false);

  const catalogDraft = reactive<CatalogDraft>(createEmptyCatalogDraft());
  const versionDraft = reactive<VersionDraft>(createEmptyVersionDraft());
  const componentDraft = reactive<ComponentDraft>(createEmptyComponentDraft());
  const templates = ref<CostTemplateSummary[]>([]);
  const {
    previewDraft,
    previewResponse,
    isRunningPreview,
    canPreview,
    applyPreviewSample,
    resetPreview,
    runPreview,
  } = useCostPreview({
    api: {
      previewCost: costService.previewCost,
    },
    selectedVersion,
    selectedVersionId: costCatalogs.selectedVersionId,
    warn: toastController.warn,
    error: toastController.error,
    t,
    normalizeError: (error: unknown) =>
      normalizeError(error, t("common.unknownError")).message,
  });

  watch(
    () => componentDraft.charge_kind,
    (chargeKind) => {
      if (chargeKind !== "per_unit") {
        componentDraft.unit_price = "";
      }
      if (chargeKind !== "flat") {
        componentDraft.flat_fee = "";
      }
      if (chargeKind !== "tiered_per_unit") {
        componentDraft.tier_basis = "meter_quantity";
        componentDraft.tiers = [createEmptyTierRow()];
      }
    },
  );

  watch(showArchivedVersions, (showArchived) => {
    if (showArchived) {
      return;
    }

    const currentVersion = selectedVersionSummary.value;
    if (currentVersion?.is_archived) {
      costCatalogs.setSelectedVersionId(
        resolvePreferredVersionId(currentVersion.catalog_id, null),
      );
    }
  });

  watch(
    [selectedCatalog, selectedVersionSummary, showArchivedVersions],
    ([catalog, currentVersion, showArchived]) => {
      if (showArchived || !catalog || !currentVersion?.is_archived) {
        return;
      }

      const replacementVersionId = resolvePreferredVersionId(catalog.catalog.id, null);
      if (replacementVersionId !== null && replacementVersionId !== currentVersion.id) {
        costCatalogs.setSelectedVersionId(replacementVersionId);
      }
    },
  );

  const resolvePreferredVersionId = (
    catalogId: number,
    preferredVersionId: number | null,
  ) => {
    const catalog = costCatalogs.catalogs.value.find(
      (item) => item.catalog.id === catalogId,
    );
    const versions = catalog?.versions ?? [];
    return resolvePreferredCostVersionId(
      versions,
      preferredVersionId,
      showArchivedVersions.value,
    );
  };

  const refreshCostData = async () => {
    try {
      await costCatalogs.fetchCatalogs();
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(t("costPage.alert.loadFailed"), normalizedError.message);
    }
  };

  const refreshTemplates = async () => {
    isLoadingTemplates.value = true;
    try {
      templates.value = await costService.getCostTemplateList();
    } finally {
      isLoadingTemplates.value = false;
    }
  };

  const openTemplateDialog = async () => {
    isTemplateDialogOpen.value = true;
    if (templates.value.length === 0) {
      await refreshTemplates();
    }
  };

  const openCatalogWorkspace = (catalogId: number) => {
    costCatalogs.setSelectedCatalogId(catalogId);
    isEditorDialogOpen.value = true;
  };

  const closeCatalogWorkspace = () => {
    isEditorDialogOpen.value = false;
  };

  const handleSelectCatalog = (catalogId: number) => {
    costCatalogs.setSelectedCatalogId(catalogId);
  };

  const handleSelectVersion = (versionId: number) => {
    costCatalogs.setSelectedVersionId(versionId);
  };

  const importTemplate = async (template: CostTemplateSummary) => {
    importingTemplateKey.value = template.key;
    try {
      const response = await costService.importCostTemplate({
        template_key: template.key,
      });
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(response.imported.catalog.id);
      costCatalogs.setSelectedVersionId(response.imported.version.id);
      toastController.success(
        t("costPage.alert.templateImportSuccess", {
          name: response.imported.catalog.name,
        }),
      );
      isTemplateDialogOpen.value = false;
      isEditorDialogOpen.value = true;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.templateImportFailed"),
        normalizedError.message,
      );
    } finally {
      importingTemplateKey.value = null;
    }
  };

  const openCreateCatalogDialog = (openEditorAfterSave = false) => {
    Object.assign(catalogDraft, createEmptyCatalogDraft());
    shouldOpenEditorAfterCatalogSave.value = openEditorAfterSave;
    isCatalogDialogOpen.value = true;
  };

  const openEditCatalogDialog = (catalog: {
    id: number;
    name: string;
    description: string | null;
  }) => {
    Object.assign(catalogDraft, {
      id: catalog.id,
      name: catalog.name,
      description: catalog.description ?? "",
    });
    isCatalogDialogOpen.value = true;
  };

  const saveCatalog = async () => {
    if (!catalogDraft.name.trim()) {
      toastController.warn(t("costPage.alert.catalogNameRequired"));
      return;
    }

    isSavingCatalog.value = true;
    try {
      if (catalogDraft.id === null) {
        const created = await costService.createCostCatalog({
          name: catalogDraft.name.trim(),
          description: catalogDraft.description.trim() || undefined,
        });
        await costCatalogs.fetchCatalogs();
        costCatalogs.setSelectedCatalogId(created.id);
        toastController.success(t("costPage.alert.catalogCreateSuccess"));
        if (shouldOpenEditorAfterCatalogSave.value) {
          isEditorDialogOpen.value = true;
        }
      } else {
        const updated = await costService.updateCostCatalog(catalogDraft.id, {
          name: catalogDraft.name.trim(),
          description: catalogDraft.description.trim() || undefined,
        });
        await costCatalogs.fetchCatalogs();
        costCatalogs.setSelectedCatalogId(updated.id);
        toastController.success(t("costPage.alert.catalogUpdateSuccess"));
      }
      isCatalogDialogOpen.value = false;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.catalogSaveFailed"),
        normalizedError.message,
      );
    } finally {
      isSavingCatalog.value = false;
      shouldOpenEditorAfterCatalogSave.value = false;
    }
  };

  const resolveDuplicateVersionDetail = async (
    sourceCatalog: CostCatalogListItem,
  ): Promise<CostCatalogVersionDetail | null> => {
    const preferredVersion =
      selectedVersionSummary.value &&
      selectedVersionSummary.value.catalog_id === sourceCatalog.catalog.id
        ? selectedVersionSummary.value
        : sourceCatalog.versions[0] ?? null;

    if (!preferredVersion) {
      return null;
    }

    if (costCatalogs.versionDetail.value?.version.id === preferredVersion.id) {
      return costCatalogs.versionDetail.value;
    }

    return costService.getCostCatalogVersion(preferredVersion.id);
  };

  const duplicateCatalog = async (sourceCatalog: CostCatalogListItem) => {
    duplicatingCatalogId.value = sourceCatalog.catalog.id;
    try {
      const sourceVersionDetail = await resolveDuplicateVersionDetail(sourceCatalog);
      const duplicatedCatalog = await costService.createCostCatalog({
        name: t("costPage.catalogs.copyName", {
          name: sourceCatalog.catalog.name,
        }),
        description: sourceCatalog.catalog.description ?? undefined,
      });

      let duplicatedVersionId: number | null = null;
      if (sourceVersionDetail) {
        const duplicatedVersion = await costService.createCostCatalogVersion(
          duplicatedCatalog.id,
          {
            version: sourceVersionDetail.version.version,
            currency: sourceVersionDetail.version.currency,
            source: sourceVersionDetail.version.source,
            effective_from: sourceVersionDetail.version.effective_from,
            effective_until: sourceVersionDetail.version.effective_until,
            is_enabled: sourceVersionDetail.version.is_enabled,
          },
        );
        duplicatedVersionId = duplicatedVersion.id;

        for (const component of sourceVersionDetail.components) {
          await costService.createCostComponent({
            catalog_version_id: duplicatedVersion.id,
            meter_key: component.meter_key,
            charge_kind: component.charge_kind,
            unit_price_nanos: component.unit_price_nanos,
            flat_fee_nanos: component.flat_fee_nanos,
            tier_config_json: component.tier_config_json,
            match_attributes_json: component.match_attributes_json,
            priority: component.priority,
            description: component.description,
          });
        }
      }

      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(duplicatedCatalog.id);
      costCatalogs.setSelectedVersionId(duplicatedVersionId);
      isEditorDialogOpen.value = true;
      toastController.success(t("costPage.alert.catalogDuplicateSuccess"));
      return duplicatedCatalog.id;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.catalogDuplicateFailed"),
        normalizedError.message,
      );
      return null;
    } finally {
      duplicatingCatalogId.value = null;
    }
  };

  const handleDeleteCatalog = async (catalogId: number, name: string) => {
    const confirmed = await confirm({
      title: t("costPage.confirmDeleteCatalog", { name }),
      description: t("costPage.confirmDeleteCatalogDescription"),
    });
    if (!confirmed) {
      return;
    }

    try {
      await costService.deleteCostCatalog(catalogId);
      await costCatalogs.fetchCatalogs();
      toastController.success(t("costPage.alert.catalogDeleteSuccess"));
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.catalogDeleteFailed"),
        normalizedError.message,
      );
    }
  };

  const openCreateVersionDialog = () => {
    const nextDraft = createEmptyVersionDraft();
    if (selectedVersionSummary.value) {
      nextDraft.currency = selectedVersionSummary.value.currency;
      nextDraft.source = selectedVersionSummary.value.source ?? "";
    }
    Object.assign(versionDraft, nextDraft);
    isVersionDialogOpen.value = true;
  };

  const duplicateVersion = async (sourceVersion: {
    id: number;
    catalog_id: number;
    version: string;
  }) => {
    duplicatingVersionId.value = sourceVersion.id;
    try {
      const duplicated = await costService.duplicateCostCatalogVersion(sourceVersion.id);
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(duplicated.catalog_id);
      costCatalogs.setSelectedVersionId(duplicated.id);
      toastController.success(
        t("costPage.alert.versionDuplicateSuccess", {
          version: sourceVersion.version,
        }),
      );
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.versionDuplicateFailed"),
        normalizedError.message,
      );
    } finally {
      duplicatingVersionId.value = null;
    }
  };

  const saveVersion = async () => {
    if (!selectedCatalog.value) {
      toastController.warn(t("costPage.alert.selectCatalogFirst"));
      return;
    }
    if (!versionDraft.version.trim()) {
      toastController.warn(t("costPage.alert.versionNameRequired"));
      return;
    }
    if (!versionDraft.currency.trim()) {
      toastController.warn(t("costPage.alert.versionCurrencyRequired"));
      return;
    }

    let effectiveFrom: number | null = null;
    let effectiveUntil: number | null = null;

    try {
      effectiveFrom = parseDateTimeLocal(versionDraft.effective_from);
      effectiveUntil = parseDateTimeLocal(versionDraft.effective_until);
    } catch {
      toastController.warn(t("costPage.alert.invalidDateTime"));
      return;
    }

    if (effectiveFrom === null) {
      toastController.warn(t("costPage.alert.versionEffectiveFromRequired"));
      return;
    }

    if (effectiveUntil !== null && effectiveUntil <= effectiveFrom) {
      toastController.warn(t("costPage.alert.invalidEffectiveRange"));
      return;
    }

    isSavingVersion.value = true;
    try {
      const created = await costService.createCostCatalogVersion(
        selectedCatalog.value.catalog.id,
        {
          version: versionDraft.version.trim(),
          currency: versionDraft.currency.trim().toUpperCase(),
          source: versionDraft.source.trim() || null,
          effective_from: effectiveFrom,
          effective_until: effectiveUntil,
          is_enabled: versionDraft.is_enabled,
        },
      );
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(selectedCatalog.value.catalog.id);
      costCatalogs.setSelectedVersionId(created.id);
      toastController.success(t("costPage.alert.versionCreateSuccess"));
      isVersionDialogOpen.value = false;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.versionSaveFailed"),
        normalizedError.message,
      );
    } finally {
      isSavingVersion.value = false;
    }
  };

  const handleToggleVersionEnabled = async (
    version: Pick<CostCatalogVersion, "id" | "catalog_id" | "version" | "is_enabled">,
  ) => {
    const enabling = !version.is_enabled;
    const confirmed = await confirm({
      title: enabling
        ? t("costPage.confirmEnableVersion", { version: version.version })
        : t("costPage.confirmDisableVersion", { version: version.version }),
      description: enabling
        ? t("costPage.confirmEnableVersionDescription")
        : t("costPage.confirmDisableVersionDescription"),
    });
    if (!confirmed) {
      return;
    }

    togglingVersionId.value = version.id;
    try {
      const updated = enabling
        ? await costService.enableCostCatalogVersion(version.id)
        : await costService.disableCostCatalogVersion(version.id);
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(updated.catalog_id);
      costCatalogs.setSelectedVersionId(updated.id);
      toastController.success(
        updated.is_enabled
          ? t("costPage.alert.versionEnableSuccess")
          : t("costPage.alert.versionDisableSuccess"),
      );
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.versionToggleFailed"),
        normalizedError.message,
      );
    } finally {
      togglingVersionId.value = null;
    }
  };

  const handleArchiveVersion = async (
    version: Pick<CostCatalogVersion, "id" | "catalog_id" | "version">,
  ) => {
    const confirmed = await confirm({
      title: t("costPage.confirmArchiveVersion", { version: version.version }),
      description: t("costPage.confirmArchiveVersionDescription"),
    });
    if (!confirmed) {
      return;
    }

    managingVersionId.value = version.id;
    try {
      await costService.archiveCostCatalogVersion(version.id);
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(version.catalog_id);
      costCatalogs.setSelectedVersionId(resolvePreferredVersionId(version.catalog_id, null));
      toastController.success(t("costPage.alert.versionArchiveSuccess"));
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.versionArchiveFailed"),
        normalizedError.message,
      );
    } finally {
      managingVersionId.value = null;
    }
  };

  const handleUnarchiveVersion = async (
    version: Pick<CostCatalogVersion, "id" | "catalog_id" | "version">,
  ) => {
    const confirmed = await confirm({
      title: t("costPage.confirmUnarchiveVersion", { version: version.version }),
      description: t("costPage.confirmUnarchiveVersionDescription"),
    });
    if (!confirmed) {
      return;
    }

    managingVersionId.value = version.id;
    try {
      const updated = await costService.unarchiveCostCatalogVersion(version.id);
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(updated.catalog_id);
      costCatalogs.setSelectedVersionId(updated.id);
      toastController.success(t("costPage.alert.versionUnarchiveSuccess"));
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.versionUnarchiveFailed"),
        normalizedError.message,
      );
    } finally {
      managingVersionId.value = null;
    }
  };

  const handleDeleteVersion = async (
    version: Pick<CostCatalogVersion, "id" | "catalog_id" | "version">,
  ) => {
    const confirmed = await confirm({
      title: t("costPage.confirmDeleteVersion", { version: version.version }),
      description: t("costPage.confirmDeleteVersionDescription"),
    });
    if (!confirmed) {
      return;
    }

    managingVersionId.value = version.id;
    try {
      await costService.deleteCostCatalogVersion(version.id);
      await costCatalogs.fetchCatalogs();
      costCatalogs.setSelectedCatalogId(version.catalog_id);
      costCatalogs.setSelectedVersionId(resolvePreferredVersionId(version.catalog_id, null));
      toastController.success(t("costPage.alert.versionDeleteSuccess"));
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.versionDeleteFailed"),
        normalizedError.message,
      );
    } finally {
      managingVersionId.value = null;
    }
  };

  const toggleArchivedVersions = () => {
    showArchivedVersions.value = !showArchivedVersions.value;
  };

  const openCreateComponentDialog = () => {
    const nextDraft = createEmptyComponentDraft();
    const maxPriority =
      components.value.reduce(
        (currentMax, item) => Math.max(currentMax, item.priority),
        0,
      ) + 10;
    nextDraft.priority = String(maxPriority || 100);
    Object.assign(componentDraft, nextDraft);
    isComponentDialogOpen.value = true;
  };

  const openEditComponentDialog = (component: CostComponent) => {
    const parsedTierConfig = parseTierConfig(
      component.tier_config_json,
      component.meter_key,
      selectedVersionSummary.value?.currency,
    );
    Object.assign(componentDraft, {
      id: component.id,
      meter_key: component.meter_key,
      charge_kind: component.charge_kind,
      unit_price: formatRateInput(
        component.unit_price_nanos,
        component.meter_key,
        selectedVersionSummary.value?.currency,
      ),
      flat_fee: formatPriceInputFromNanos(
        component.flat_fee_nanos,
        selectedVersionSummary.value?.currency,
      ),
      match_attributes_json: prettyJson(component.match_attributes_json),
      priority: String(component.priority),
      description: component.description ?? "",
      tier_basis: parsedTierConfig?.basis ?? "meter_quantity",
      tiers: parsedTierConfig?.tiers.length
        ? parsedTierConfig.tiers
        : [createEmptyTierRow()],
    });
    isComponentDialogOpen.value = true;
  };

  const addTier = () => {
    componentDraft.tiers.push(createEmptyTierRow());
  };

  const removeTier = (index: number) => {
    if (componentDraft.tiers.length === 1) {
      componentDraft.tiers[0] = createEmptyTierRow();
      return;
    }
    componentDraft.tiers.splice(index, 1);
  };

  const buildTierConfigJson = () => {
    const tiers = componentDraft.tiers.map((tier, index) => {
      const unit_price_nanos = parseRequiredRate(
        tier.unit_price,
        `tier_${index}_unit_price`,
        componentDraft.meter_key,
        selectedVersionSummary.value?.currency,
      );
      const hasUpTo = tier.up_to.trim().length > 0;
      const up_to = hasUpTo
        ? parseRequiredNonNegativeInteger(tier.up_to, `tier_${index}_up_to`)
        : null;

      return {
        up_to,
        unit_price_nanos,
      };
    });

    const unlimitedIndexes = tiers
      .map((tier, index) => (tier.up_to === null ? index : -1))
      .filter((index) => index >= 0);

    if (unlimitedIndexes.length > 1) {
      throw new Error("tier:multiple_unbounded");
    }
    if (unlimitedIndexes.length === 1 && unlimitedIndexes[0] !== tiers.length - 1) {
      throw new Error("tier:unbounded_not_last");
    }

    for (let index = 1; index < tiers.length; index += 1) {
      const previous = tiers[index - 1].up_to;
      const current = tiers[index].up_to;
      if (previous !== null && current !== null && current <= previous) {
        throw new Error("tier:not_increasing");
      }
    }

    return JSON.stringify({
      basis: componentDraft.tier_basis,
      tiers,
    });
  };

  const saveComponent = async () => {
    if (!selectedVersion.value) {
      toastController.warn(t("costPage.alert.selectVersionFirst"));
      return;
    }

    let priority: number | null = null;
    let matchAttributesJson: string | null = null;
    let unitPriceNanos: number | null = null;
    let flatFeeNanos: number | null = null;
    let tierConfigJson: string | null = null;

    try {
      priority = parseRequiredNonNegativeInteger(componentDraft.priority, "priority");
      matchAttributesJson = parseOptionalJsonObject(
        componentDraft.match_attributes_json,
      );

      if (componentDraft.charge_kind === "per_unit") {
        unitPriceNanos = parseRequiredRate(
          componentDraft.unit_price,
          "unit_price",
          componentDraft.meter_key,
          selectedVersionSummary.value?.currency,
        );
      } else if (componentDraft.charge_kind === "flat") {
        flatFeeNanos = parseRequiredPrice(
          componentDraft.flat_fee,
          "flat_fee",
          selectedVersionSummary.value?.currency,
        );
      } else {
        tierConfigJson = buildTierConfigJson();
      }
    } catch (error: unknown) {
      const message = error instanceof Error ? error.message : "";
      const key = message.includes("match_attributes")
        ? "costPage.alert.invalidMatchAttributes"
        : message.startsWith("tier:")
          ? `costPage.alert.${message.replace(":", ".")}`
          : "costPage.alert.invalidComponentNumber";
      toastController.warn(t(key));
      return;
    }

    isSavingComponent.value = true;
    try {
      if (componentDraft.id === null) {
        await costService.createCostComponent({
          catalog_version_id: selectedVersion.value.id,
          meter_key: componentDraft.meter_key,
          charge_kind: componentDraft.charge_kind,
          unit_price_nanos: unitPriceNanos,
          flat_fee_nanos: flatFeeNanos,
          tier_config_json: tierConfigJson,
          match_attributes_json: matchAttributesJson,
          priority,
          description: componentDraft.description.trim() || null,
        });
        toastController.success(t("costPage.alert.componentCreateSuccess"));
      } else {
        await costService.updateCostComponent(componentDraft.id, {
          meter_key: componentDraft.meter_key,
          charge_kind: componentDraft.charge_kind,
          unit_price_nanos: unitPriceNanos,
          flat_fee_nanos: flatFeeNanos,
          tier_config_json: tierConfigJson,
          match_attributes_json: matchAttributesJson,
          priority,
          description: componentDraft.description.trim() || null,
        });
        toastController.success(t("costPage.alert.componentUpdateSuccess"));
      }
      await costCatalogs.refreshCurrentVersionDetail();
      isComponentDialogOpen.value = false;
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.componentSaveFailed"),
        normalizedError.message,
      );
    } finally {
      isSavingComponent.value = false;
    }
  };

  const handleDeleteComponent = async (component: CostComponent) => {
    const confirmed = await confirm({
      title: t("costPage.confirmDeleteComponent", {
        meter: component.meter_key,
      }),
      description: t("costPage.confirmDeleteComponentDescription"),
    });
    if (!confirmed) {
      return;
    }

    try {
      await costService.deleteCostComponent(component.id);
      await costCatalogs.refreshCurrentVersionDetail();
      toastController.success(t("costPage.alert.componentDeleteSuccess"));
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.componentDeleteFailed"),
        normalizedError.message,
      );
    }
  };

  const meterLabel = (meterKey: string) =>
    t(
      METER_OPTIONS.find((option) => option.value === meterKey)?.labelKey ??
        "costPage.componentEditor.meters.custom",
    );

  const chargeKindLabel = (chargeKind: string) =>
    t(
      CHARGE_KIND_OPTIONS.find((option) => option.value === chargeKind)?.labelKey ??
        "costPage.componentEditor.chargeKinds.custom",
    );

  const tierBasisLabel = (basis: string) =>
    t(
      TIER_BASIS_OPTIONS.find((option) => option.value === basis)?.labelKey ??
        "costPage.componentEditor.tiers.basisMeterQuantity",
    );

  const tryFormatRateInputDisplay = (value: string, meterKey: string) => {
    try {
      const nanos = parseRequiredRate(
        value,
        "display_rate",
        meterKey,
        selectedVersionSummary.value?.currency,
      );
      return formatRateDisplay(
        nanos,
        meterKey,
        selectedVersionSummary.value?.currency,
        false,
      );
    } catch {
      return value.trim() || "-";
    }
  };

  onMounted(() => {
    void refreshCostData();
  });

  return {
    catalogs: costCatalogs.catalogs,
    selectedCatalogId: costCatalogs.selectedCatalogId,
    selectedVersionId: costCatalogs.selectedVersionId,
    isLoadingCatalogs: costCatalogs.isLoadingCatalogs,
    isLoadingVersionDetail: costCatalogs.isLoadingVersionDetail,
    selectedCatalog,
    selectedVersion,
    selectedCatalogVersions,
    selectedVersionSummary,
    components,
    canPreview,
    isCatalogDialogOpen,
    isVersionDialogOpen,
    isComponentDialogOpen,
    isTemplateDialogOpen,
    isEditorDialogOpen,
    isSavingCatalog,
    isSavingVersion,
    isSavingComponent,
    isRunningPreview,
    isLoadingTemplates,
    importingTemplateKey,
    togglingVersionId,
    managingVersionId,
    duplicatingVersionId,
    duplicatingCatalogId,
    showArchivedVersions,
    catalogDraft,
    versionDraft,
    componentDraft,
    previewDraft,
    previewResponse,
    templates,
    refreshCostData,
    refreshTemplates,
    openTemplateDialog,
    openCatalogWorkspace,
    closeCatalogWorkspace,
    handleSelectCatalog,
    handleSelectVersion,
    importTemplate,
    duplicateCatalog,
    openCreateCatalogDialog,
    openEditCatalogDialog,
    saveCatalog,
    handleDeleteCatalog,
    openCreateVersionDialog,
    duplicateVersion,
    saveVersion,
    handleToggleVersionEnabled,
    handleArchiveVersion,
    handleUnarchiveVersion,
    handleDeleteVersion,
    toggleArchivedVersions,
    openCreateComponentDialog,
    openEditComponentDialog,
    addTier,
    removeTier,
    saveComponent,
    handleDeleteComponent,
    applyPreviewSample,
    resetPreview,
    runPreview,
    meterLabel,
    chargeKindLabel,
    tierBasisLabel,
    tryFormatRateInputDisplay,
    formatRateDisplay,
    formatNumber,
    parseTierConfig,
    prettyJson,
  };
};
