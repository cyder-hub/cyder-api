import { computed, onMounted, reactive, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { confirm } from "@/lib/confirmController";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { formatPriceInputFromNanos } from "@/lib/utils";
import { Api } from "@/services/request";
import { useCostStore } from "@/store/costStore";
import type {
  CostCatalogListItem,
  CostCatalogVersion,
  CostComponent,
  CostCatalogVersionDetail,
  CostTemplateSummary,
  UsageNormalization,
} from "@/store/types";
import {
  CHARGE_KIND_OPTIONS,
  METER_OPTIONS,
  TIER_BASIS_OPTIONS,
  createEmptyCatalogDraft,
  createEmptyComponentDraft,
  createEmptyTierRow,
  createEmptyVersionDraft,
  createPreviewSample,
  formatNumber,
  formatRateDisplay,
  formatRateInput,
  normalizePreviewResponse,
  parseDateTimeLocal,
  parseOptionalJsonObject,
  parseRequiredNonNegativeInteger,
  parseRequiredPrice,
  parseRequiredRate,
  parseTierConfig,
  prettyJson,
} from "./helpers";
import type {
  CatalogDraft,
  ComponentDraft,
  PreviewDraft,
  VersionDraft,
} from "./types";

export const useCostPage = () => {
  const { t } = useI18n();
  const costStore = useCostStore();

  const selectedCatalog = computed(() => costStore.selectedCatalog);
  const selectedVersion = computed(() => costStore.selectedVersion);
  const showArchivedVersions = ref(false);
  const selectedCatalogVersions = computed(() =>
    costStore.selectedCatalogVersions.filter(
      (version) => showArchivedVersions.value || !version.is_archived,
    ),
  );
  const components = computed(() => costStore.versionDetail?.components ?? []);
  const selectedVersionSummary = computed(
    () => costStore.versionDetail?.version ?? selectedVersion.value,
  );

  const isCatalogDialogOpen = ref(false);
  const isVersionDialogOpen = ref(false);
  const isComponentDialogOpen = ref(false);
  const isTemplateDialogOpen = ref(false);
  const isEditorDialogOpen = ref(false);
  const isSavingCatalog = ref(false);
  const isSavingVersion = ref(false);
  const isSavingComponent = ref(false);
  const isRunningPreview = ref(false);
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
  const previewDraft = reactive<PreviewDraft>(createPreviewSample());

  const previewResponse = ref<ReturnType<typeof normalizePreviewResponse> | null>(null);
  const templates = ref<CostTemplateSummary[]>([]);

  const canPreview = computed(() => selectedVersion.value !== null);

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

  watch(
    () => costStore.selectedVersionId,
    () => {
      previewResponse.value = null;
    },
  );

  watch(showArchivedVersions, (showArchived) => {
    if (showArchived) {
      return;
    }

    const currentVersion = selectedVersionSummary.value;
    if (currentVersion?.is_archived) {
      costStore.setSelectedVersionId(
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
        costStore.setSelectedVersionId(replacementVersionId);
      }
    },
  );

  const resolvePreferredVersionId = (
    catalogId: number,
    preferredVersionId: number | null,
  ) => {
    const catalog = costStore.catalogs.find((item) => item.catalog.id === catalogId);
    const versions = catalog?.versions ?? [];
    const visibleVersions = showArchivedVersions.value
      ? versions
      : versions.filter((version) => !version.is_archived);

    if (
      preferredVersionId !== null &&
      visibleVersions.some((version) => version.id === preferredVersionId)
    ) {
      return preferredVersionId;
    }

    return visibleVersions[0]?.id ?? null;
  };

  const refreshCostData = async () => {
    try {
      await costStore.fetchCatalogs();
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(t("costPage.alert.loadFailed"), normalizedError.message);
    }
  };

  const refreshTemplates = async () => {
    isLoadingTemplates.value = true;
    try {
      templates.value = await Api.getCostTemplateList();
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
    costStore.setSelectedCatalogId(catalogId);
    isEditorDialogOpen.value = true;
  };

  const closeCatalogWorkspace = () => {
    isEditorDialogOpen.value = false;
  };

  const handleSelectCatalog = (catalogId: number) => {
    costStore.setSelectedCatalogId(catalogId);
  };

  const handleSelectVersion = (versionId: number) => {
    costStore.setSelectedVersionId(versionId);
  };

  const importTemplate = async (template: CostTemplateSummary) => {
    importingTemplateKey.value = template.key;
    try {
      const response = await Api.importCostTemplate({
        template_key: template.key,
      });
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(response.imported.catalog.id);
      costStore.setSelectedVersionId(response.imported.version.id);
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
        const created = await Api.createCostCatalog({
          name: catalogDraft.name.trim(),
          description: catalogDraft.description.trim() || undefined,
        });
        await costStore.fetchCatalogs();
        costStore.setSelectedCatalogId(created.id);
        toastController.success(t("costPage.alert.catalogCreateSuccess"));
        if (shouldOpenEditorAfterCatalogSave.value) {
          isEditorDialogOpen.value = true;
        }
      } else {
        const updated = await Api.updateCostCatalog(catalogDraft.id, {
          name: catalogDraft.name.trim(),
          description: catalogDraft.description.trim() || undefined,
        });
        await costStore.fetchCatalogs();
        costStore.setSelectedCatalogId(updated.id);
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

    if (costStore.versionDetail?.version.id === preferredVersion.id) {
      return costStore.versionDetail;
    }

    return Api.getCostCatalogVersion(preferredVersion.id);
  };

  const duplicateCatalog = async (sourceCatalog: CostCatalogListItem) => {
    duplicatingCatalogId.value = sourceCatalog.catalog.id;
    try {
      const sourceVersionDetail = await resolveDuplicateVersionDetail(sourceCatalog);
      const duplicatedCatalog = await Api.createCostCatalog({
        name: t("costPage.catalogs.copyName", {
          name: sourceCatalog.catalog.name,
        }),
        description: sourceCatalog.catalog.description ?? undefined,
      });

      let duplicatedVersionId: number | null = null;
      if (sourceVersionDetail) {
        const duplicatedVersion = await Api.createCostCatalogVersion(
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
          await Api.createCostComponent({
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

      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(duplicatedCatalog.id);
      costStore.setSelectedVersionId(duplicatedVersionId);
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
      await Api.deleteCostCatalog(catalogId);
      await costStore.fetchCatalogs();
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
      const duplicated = await Api.duplicateCostCatalogVersion(sourceVersion.id);
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(duplicated.catalog_id);
      costStore.setSelectedVersionId(duplicated.id);
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
      const created = await Api.createCostCatalogVersion(
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
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(selectedCatalog.value.catalog.id);
      costStore.setSelectedVersionId(created.id);
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
        ? await Api.enableCostCatalogVersion(version.id)
        : await Api.disableCostCatalogVersion(version.id);
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(updated.catalog_id);
      costStore.setSelectedVersionId(updated.id);
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
      await Api.archiveCostCatalogVersion(version.id);
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(version.catalog_id);
      costStore.setSelectedVersionId(resolvePreferredVersionId(version.catalog_id, null));
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
      const updated = await Api.unarchiveCostCatalogVersion(version.id);
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(updated.catalog_id);
      costStore.setSelectedVersionId(updated.id);
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
      await Api.deleteCostCatalogVersion(version.id);
      await costStore.fetchCatalogs();
      costStore.setSelectedCatalogId(version.catalog_id);
      costStore.setSelectedVersionId(resolvePreferredVersionId(version.catalog_id, null));
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
        await Api.createCostComponent({
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
        await Api.updateCostComponent(componentDraft.id, {
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
      await costStore.refreshCurrentVersionDetail();
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
      await Api.deleteCostComponent(component.id);
      await costStore.refreshCurrentVersionDetail();
      toastController.success(t("costPage.alert.componentDeleteSuccess"));
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.componentDeleteFailed"),
        normalizedError.message,
      );
    }
  };

  const applyPreviewSample = () => {
    Object.assign(previewDraft, createPreviewSample());
  };

  const resetPreview = () => {
    Object.assign(previewDraft, createPreviewSample());
    previewResponse.value = null;
  };

  const buildPreviewNormalization = (): UsageNormalization => ({
    total_input_tokens: parseRequiredNonNegativeInteger(
      previewDraft.total_input_tokens,
      "total_input_tokens",
    ),
    total_output_tokens: parseRequiredNonNegativeInteger(
      previewDraft.total_output_tokens,
      "total_output_tokens",
    ),
    input_text_tokens: parseRequiredNonNegativeInteger(
      previewDraft.input_text_tokens,
      "input_text_tokens",
    ),
    output_text_tokens: parseRequiredNonNegativeInteger(
      previewDraft.output_text_tokens,
      "output_text_tokens",
    ),
    input_image_tokens: parseRequiredNonNegativeInteger(
      previewDraft.input_image_tokens,
      "input_image_tokens",
    ),
    output_image_tokens: parseRequiredNonNegativeInteger(
      previewDraft.output_image_tokens,
      "output_image_tokens",
    ),
    cache_read_tokens: parseRequiredNonNegativeInteger(
      previewDraft.cache_read_tokens,
      "cache_read_tokens",
    ),
    cache_write_tokens: parseRequiredNonNegativeInteger(
      previewDraft.cache_write_tokens,
      "cache_write_tokens",
    ),
    reasoning_tokens: parseRequiredNonNegativeInteger(
      previewDraft.reasoning_tokens,
      "reasoning_tokens",
    ),
    warnings: [],
  });

  const runPreview = async () => {
    if (!selectedVersion.value) {
      toastController.warn(t("costPage.alert.selectVersionFirst"));
      return;
    }

    let normalization: UsageNormalization;
    try {
      normalization = buildPreviewNormalization();
    } catch {
      toastController.warn(t("costPage.alert.invalidPreviewNumber"));
      return;
    }

    isRunningPreview.value = true;
    try {
      const response = await Api.previewCost({
        catalog_version_id: selectedVersion.value.id,
        normalization,
      });
      previewResponse.value = normalizePreviewResponse(response);
    } catch (error: unknown) {
      const normalizedError = normalizeError(error, t("common.unknownError"));
      toastController.error(
        t("costPage.alert.previewFailed"),
        normalizedError.message,
      );
    } finally {
      isRunningPreview.value = false;
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
    costStore,
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
