<script setup lang="ts">
import CrudPageLayout from "@/components/CrudPageLayout.vue";
import CostCatalogDialog from "./cost/CostCatalogDialog.vue";
import CostCatalogSection from "./cost/CostCatalogSection.vue";
import CostComponentDialog from "./cost/CostComponentDialog.vue";
import CostEditorDialog from "./cost/CostEditorDialog.vue";
import CostTemplateDialog from "./cost/CostTemplateDialog.vue";
import CostVersionDialog from "./cost/CostVersionDialog.vue";
import { useCostPage } from "./cost/useCostPage";

const {
  costStore,
  selectedCatalog,
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
  duplicatingCatalogId,
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
  handleSelectVersion,
  importTemplate,
  duplicateCatalog,
  openCreateCatalogDialog,
  openEditCatalogDialog,
  saveCatalog,
  handleDeleteCatalog,
  openCreateVersionDialog,
  saveVersion,
  handleToggleVersionEnabled,
  openCreateComponentDialog,
  openEditComponentDialog,
  addTier,
  removeTier,
  saveComponent,
  handleDeleteComponent,
  applyPreviewSample,
  runPreview,
  meterLabel,
  chargeKindLabel,
  tierBasisLabel,
  tryFormatRateInputDisplay,
  formatRateDisplay,
  formatNumber,
  prettyJson,
} = useCostPage();
</script>

<template>
  <CrudPageLayout
    :title="$t('costPage.title')"
    :description="$t('costPage.description')"
  >
    <div class="space-y-6">
      <CostCatalogSection
        :catalogs="costStore.catalogs"
        :is-loading="costStore.isLoadingCatalogs"
        :selected-catalog-id="costStore.selectedCatalogId"
        :duplicating-catalog-id="duplicatingCatalogId"
        @open-template="openTemplateDialog"
        @refresh="refreshCostData"
        @create-catalog="openCreateCatalogDialog(true)"
        @open-catalog="openCatalogWorkspace"
        @duplicate-catalog="duplicateCatalog"
        @edit-catalog="openEditCatalogDialog"
        @delete-catalog="handleDeleteCatalog"
      />
    </div>
  </CrudPageLayout>

  <CostEditorDialog
    :open="isEditorDialogOpen"
    :selected-catalog="selectedCatalog"
    :selected-catalog-versions="selectedCatalogVersions"
    :selected-version-id="costStore.selectedVersionId"
    :selected-version-summary="selectedVersionSummary"
    :components="components"
    :is-loading-version-detail="costStore.isLoadingVersionDetail"
    :toggling-version-id="togglingVersionId"
    :duplicating-catalog-id="duplicatingCatalogId"
    :preview-draft="previewDraft"
    :preview-response="previewResponse"
    :can-preview="canPreview"
    :is-running-preview="isRunningPreview"
    :meter-label="meterLabel"
    :charge-kind-label="chargeKindLabel"
    :tier-basis-label="tierBasisLabel"
    :format-rate-display="formatRateDisplay"
    :try-format-rate-input-display="tryFormatRateInputDisplay"
    :format-number="formatNumber"
    :pretty-json="prettyJson"
    @update:open="(open) => (isEditorDialogOpen = open)"
    @refresh="refreshCostData"
    @open-template="openTemplateDialog"
    @edit-catalog="openEditCatalogDialog"
    @duplicate-catalog="duplicateCatalog"
    @create-version="openCreateVersionDialog"
    @select-version="handleSelectVersion"
    @toggle-version-enabled="handleToggleVersionEnabled"
    @create-component="openCreateComponentDialog"
    @edit-component="openEditComponentDialog"
    @delete-component="handleDeleteComponent"
    @apply-sample="applyPreviewSample"
    @run-preview="runPreview"
  />

  <CostTemplateDialog
    :open="isTemplateDialogOpen"
    :templates="templates"
    :is-loading-templates="isLoadingTemplates"
    :importing-template-key="importingTemplateKey"
    @update:open="(open) => (isTemplateDialogOpen = open)"
    @refresh="refreshTemplates"
    @import-template="importTemplate"
  />

  <CostCatalogDialog
    :open="isCatalogDialogOpen"
    :draft="catalogDraft"
    :is-saving="isSavingCatalog"
    @update:open="(open) => (isCatalogDialogOpen = open)"
    @save="saveCatalog"
  />

  <CostVersionDialog
    :open="isVersionDialogOpen"
    :draft="versionDraft"
    :is-saving="isSavingVersion"
    @update:open="(open) => (isVersionDialogOpen = open)"
    @save="saveVersion"
  />

  <CostComponentDialog
    :open="isComponentDialogOpen"
    :draft="componentDraft"
    :is-saving="isSavingComponent"
    :selected-currency="selectedVersionSummary?.currency"
    :meter-label="meterLabel"
    :charge-kind-label="chargeKindLabel"
    @update:open="(open) => (isComponentDialogOpen = open)"
    @save="saveComponent"
    @add-tier="addTier"
    @remove-tier="removeTier"
  />
</template>
