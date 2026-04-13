<script setup lang="ts">
import { ref, computed, onMounted, watch } from "vue";
import { useRoute, useRouter } from "vue-router";
import { useI18n } from "vue-i18n";
import { Api } from "@/services/request";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { useProviderStore } from "@/store/providerStore";
import type {
  CustomFieldType,
  CustomFieldItem,
  CostCatalogVersion,
  CustomFieldDefinition,
  ModelDetailResponse,
} from "@/store/types";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
import MobileCrudCard from "@/components/MobileCrudCard.vue";
import { formatTimestamp } from "@/lib/utils";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Copy, Loader2, Plus, Settings2, Trash2 } from "lucide-vue-next";
import CostCatalogDialog from "./cost/CostCatalogDialog.vue";
import CostComponentDialog from "./cost/CostComponentDialog.vue";
import CostEditorDialog from "./cost/CostEditorDialog.vue";
import CostTemplateDialog from "./cost/CostTemplateDialog.vue";
import CostVersionDialog from "./cost/CostVersionDialog.vue";
import { useCostPage } from "./cost/useCostPage";

interface EditingModelData {
  id: number;
  provider_id: number;
  cost_catalog_id: number | null;
  model_name: string;
  real_model_name: string;
  is_enabled: boolean;
  custom_fields: CustomFieldItem[];
}

const { t } = useI18n();
const route = useRoute();
const router = useRouter();
const providerStore = useProviderStore();

const modelId = parseInt(route.params.id as string);
const isLoading = ref(true);
const modelDetail = ref<ModelDetailResponse | null>(null);
const allCustomFields = ref<CustomFieldItem[]>([]);
const costManager = useCostPage();

const editingData = ref<EditingModelData | null>(null);
const selectedCustomFieldId = ref<string | null>(null);
const shouldBindCreatedCatalog = ref(false);

const toEditableCustomField = (
  field: Pick<
    CustomFieldDefinition,
    | "id"
    | "name"
    | "field_name"
    | "string_value"
    | "integer_value"
    | "number_value"
    | "boolean_value"
    | "description"
    | "field_type"
  >,
): CustomFieldItem => ({
  id: field.id,
  name: field.name,
  field_name: field.field_name,
  field_value:
    (field.string_value ??
      field.integer_value?.toString() ??
      field.number_value?.toString() ??
      field.boolean_value?.toString()) ||
    "",
  description: field.description,
  field_type: (field.field_type?.toLowerCase() as CustomFieldType) || "unset",
});

const fetchData = async () => {
  if (isNaN(modelId)) {
    toastController.error(
      t("modelEditPage.alert.loadDataFailed", { modelId: route.params.id }),
    );
    isLoading.value = false;
    return;
  }

  try {
    isLoading.value = true;
    const [detail, customFieldsRes] = await Promise.all([
      Api.getModelDetail(modelId),
      Api.getCustomFieldList(),
      costManager.refreshCostData(),
    ]);

    modelDetail.value = detail;

    if (customFieldsRes && customFieldsRes.list) {
      allCustomFields.value = customFieldsRes.list.map(toEditableCustomField);
    }

    if (detail) {
      editingData.value = {
        id: detail.model.id,
        provider_id: detail.model.provider_id,
        cost_catalog_id: detail.model.cost_catalog_id ?? null,
        model_name: detail.model.model_name,
        real_model_name: detail.model.real_model_name ?? "",
        is_enabled: detail.model.is_enabled,
        custom_fields: (detail.custom_fields || []).map(toEditableCustomField),
      };
    }
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.alert.loadDataFailed", { modelId: modelId }),
      normalizedError.message,
    );
  } finally {
    isLoading.value = false;
  }
};

const selectedCatalog = computed(() =>
  costManager.costStore.catalogs.find(
    (item) => item.catalog.id === editingData.value?.cost_catalog_id,
  ) ?? null,
);

const selectedCatalogVersions = computed<CostCatalogVersion[]>(() =>
  selectedCatalog.value?.versions ?? [],
);

const availableCustomFields = computed(() => {
  if (!editingData.value || !editingData.value.custom_fields) return [];
  const linkedIds = new Set(editingData.value.custom_fields.map((f) => f.id));
  return allCustomFields.value
    .filter((f) => f.id && !linkedIds.has(f.id))
    .map((f) => ({ ...f, displayName: f.name || f.field_name }));
});

const handleSaveModel = async () => {
  if (!editingData.value) return;

  if (!editingData.value.model_name.trim()) {
    toastController.warn(t("modelEditPage.alert.nameRequired"));
    return;
  }

  const payload = {
    model_name: editingData.value.model_name,
    real_model_name: editingData.value.real_model_name || null,
    is_enabled: editingData.value.is_enabled,
    cost_catalog_id: editingData.value.cost_catalog_id,
  };

  try {
    await Api.updateModel(editingData.value.id, payload);
    toastController.success(t("modelEditPage.alert.updateSuccess"));
    void providerStore.fetchProviders().catch((error) => {
      console.error("Failed to refresh providers after saving model:", error);
    });
    fetchData();
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.alert.saveFailed", {
        error: normalizedError.message,
      }),
    );
  }
};

const handleLinkCustomField = async () => {
  const fieldId = selectedCustomFieldId.value;
  const modelIdVal = editingData.value?.id;

  if (!fieldId) {
    toastController.warn(t("modelEditPage.alert.selectFieldToLink"));
    return;
  }
  if (!modelIdVal) {
    toastController.warn(t("modelEditPage.alert.modelNotLoaded"));
    return;
  }

  try {
    await Api.linkCustomField({
      custom_field_definition_id: parseInt(fieldId),
      model_id: modelIdVal,
      is_enabled: true,
    });

    selectedCustomFieldId.value = null;
    toastController.success(t("modelEditPage.alert.linkSuccess"));
    fetchData();
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.alert.linkFailed", {
        error: normalizedError.message,
      }),
    );
  }
};

const handleUnlinkCustomField = async (fieldId: number) => {
  const modelIdVal = editingData.value?.id;
  if (!modelIdVal) {
    toastController.warn(t("modelEditPage.alert.modelIdNotFound"));
    return;
  }

  try {
    await Api.unlinkCustomField({
      custom_field_definition_id: fieldId,
      model_id: modelIdVal,
    });

    toastController.success(t("modelEditPage.alert.unlinkSuccess"));
    fetchData();
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.alert.unlinkFailed", {
        error: normalizedError.message,
      }),
    );
  }
};

const handleNavigateBack = () => {
  router.push("/provider");
};

const handleOpenSelectedCostCatalog = () => {
  if (!editingData.value?.cost_catalog_id) {
    toastController.warn(t("costPage.alert.selectCatalogFirst"));
    return;
  }
  costManager.openCatalogWorkspace(editingData.value.cost_catalog_id);
};

const handleCreateCostCatalog = () => {
  shouldBindCreatedCatalog.value = true;
  costManager.openCreateCatalogDialog(true);
};

const handleDuplicateSelectedCostCatalog = async () => {
  if (!selectedCatalog.value || !editingData.value) {
    toastController.warn(t("costPage.alert.selectCatalogFirst"));
    return;
  }

  const duplicatedCatalogId = await costManager.duplicateCatalog(selectedCatalog.value);
  if (duplicatedCatalogId !== null) {
    editingData.value.cost_catalog_id = duplicatedCatalogId;
  }
};

const handleCostCatalogDialogOpenChange = (open: boolean) => {
  costManager.isCatalogDialogOpen.value = open;
  if (!open) {
    shouldBindCreatedCatalog.value = false;
  }
};

watch(
  () => costManager.costStore.selectedCatalogId,
  (catalogId) => {
    if (shouldBindCreatedCatalog.value && catalogId !== null && editingData.value) {
      editingData.value.cost_catalog_id = catalogId;
      shouldBindCreatedCatalog.value = false;
    }
  },
);

onMounted(() => {
  fetchData();
});
</script>

<template>
  <div class="app-page">
    <div class="app-page-shell app-page-shell--narrow">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
        <div class="min-w-0">
          <h1 class="text-lg font-semibold text-gray-900 tracking-tight sm:text-xl">
            {{ t("modelEditPage.title") }}
          </h1>
        </div>
        <div class="flex w-full flex-col gap-2 sm:w-auto">
          <Button variant="outline" @click="handleNavigateBack">
            {{ t("common.cancel") }}
          </Button>
        </div>
      </div>

      <div v-if="isLoading" class="flex items-center justify-center py-16">
        <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
        <span class="text-sm text-gray-500">{{
          t("modelEditPage.loading")
        }}</span>
      </div>

      <div
        v-else-if="!modelDetail"
        class="flex flex-col items-center justify-center py-20"
      >
        <div
          class="bg-destructive/10 text-destructive border border-destructive/20 rounded-lg p-6 text-center"
        >
          <p class="text-sm font-medium">
            {{ t("modelEditPage.alert.loadDataFailed", { modelId: modelId }) }}
          </p>
          <Button class="mt-4" @click="fetchData">{{ t("common.retry") }}</Button>
        </div>
      </div>

      <div v-else-if="editingData" class="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>{{ t("common.basicInfo") }}</CardTitle>
        </CardHeader>
        <CardContent class="space-y-4">
          <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div class="grid gap-1.5">
              <Label for="model_name" class="text-gray-700">
                {{ t("modelEditPage.labelModelName") }}
                <span class="text-red-500 ml-0.5">*</span>
              </Label>
              <Input id="model_name" v-model="editingData.model_name" />
            </div>

            <div class="grid gap-1.5">
              <Label for="real_model_name" class="text-gray-700">{{
                t("modelEditPage.labelRealModelName")
              }}</Label>
              <Input id="real_model_name" v-model="editingData.real_model_name" />
            </div>
          </div>

          <div
            class="flex items-center justify-between p-3.5 border border-gray-200 rounded-lg"
          >
            <Label for="is_enabled" class="cursor-pointer text-gray-700">
              {{ t("modelEditPage.labelEnabled") }}
            </Label>
            <Checkbox
              id="is_enabled"
              v-model:checked="editingData.is_enabled"
            />
          </div>
        </CardContent>
      </Card>

      <!-- Cost Catalog Section -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t("modelEditPage.priceSection.title") }}</CardTitle>
        </CardHeader>
        <CardContent class="space-y-6">
          <div class="grid gap-1.5">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div class="grid gap-1.5">
                <Label class="text-gray-700">{{
                  t("modelEditPage.priceSection.labelCatalog")
                }}</Label>
              </div>
              <div class="flex flex-col gap-2 sm:w-auto sm:flex-row">
                <Button variant="outline" @click="handleCreateCostCatalog">
                  <Plus class="mr-1.5 h-4 w-4" />
                  {{ t("costPage.catalogs.add") }}
                </Button>
                <Button
                  variant="outline"
                  :disabled="!selectedCatalog"
                  @click="handleDuplicateSelectedCostCatalog"
                >
                  <Copy class="mr-1.5 h-4 w-4" />
                  {{ t("costPage.catalogs.duplicate") }}
                </Button>
                <Button
                  variant="outline"
                  :disabled="!editingData.cost_catalog_id"
                  @click="handleOpenSelectedCostCatalog"
                >
                  <Settings2 class="mr-1.5 h-4 w-4" />
                  {{ t("costPage.catalogs.openEditor") }}
                </Button>
              </div>
            </div>
            <Select
              :model-value="editingData.cost_catalog_id?.toString() || 'none'"
              @update:model-value="
                (val: any) =>
                  (editingData!.cost_catalog_id =
                    val === 'none' ? null : parseInt(val as string))
              "
            >
              <SelectTrigger class="w-full">
                <SelectValue
                  :placeholder="
                    t('modelEditPage.priceSection.placeholderCatalog')
                  "
                />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">{{
                  t("modelEditPage.priceSection.noCatalog")
                }}</SelectItem>
                <SelectItem
                  v-for="catalog in costManager.costStore.catalogs"
                  :key="catalog.catalog.id"
                  :value="catalog.catalog.id.toString()"
                >
                  {{ catalog.catalog.name }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div
            v-if="!editingData.cost_catalog_id"
            class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
          >
            {{ t("modelEditPage.priceSection.unboundHint") }}
          </div>

          <div v-if="editingData.cost_catalog_id" class="space-y-4">
            <div v-if="selectedCatalogVersions.length > 0" class="space-y-3">
              <h4 class="text-sm font-semibold text-gray-700">
                {{ t("modelEditPage.priceSection.versionTitle") }}
              </h4>
              <div class="space-y-3 md:hidden">
                <MobileCrudCard
                  v-for="version in selectedCatalogVersions"
                  :key="version.id"
                  :title="version.version"
                  :description="version.source || version.currency"
                >
                  <div class="grid grid-cols-1 gap-3 text-sm min-[360px]:grid-cols-2">
                    <div class="space-y-1">
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.priceSection.enabled") }}
                      </p>
                      <div>
                        <Badge variant="secondary" class="font-mono text-xs">
                          {{ version.is_enabled ? t("common.yes") : t("common.no") }}
                        </Badge>
                      </div>
                    </div>
                    <div class="space-y-1">
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("costPage.versions.currency") }}
                      </p>
                      <p class="text-sm text-gray-900">{{ version.currency }}</p>
                    </div>
                    <div class="space-y-1">
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.priceSection.effectiveFrom") }}
                      </p>
                      <p class="text-sm text-gray-900">
                        {{ formatTimestamp(version.effective_from) }}
                      </p>
                    </div>
                    <div class="space-y-1">
                      <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                        {{ t("modelEditPage.priceSection.effectiveUntil") }}
                      </p>
                      <p class="break-all font-mono text-sm text-gray-700">
                        {{ version.effective_until ? formatTimestamp(version.effective_until) : "-" }}
                      </p>
                    </div>
                  </div>
                </MobileCrudCard>
              </div>

              <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
                <Table>
                  <TableHeader>
                    <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("modelEditPage.priceSection.version") }}</TableHead
                      >
                      <TableHead
                        class="w-[80px] text-center text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("modelEditPage.priceSection.enabled") }}</TableHead
                      >
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("costPage.versions.currency") }}</TableHead
                      >
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("modelEditPage.priceSection.effectiveFrom") }}</TableHead
                      >
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("modelEditPage.priceSection.effectiveUntil") }}</TableHead
                      >
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow v-for="version in selectedCatalogVersions" :key="version.id">
                      <TableCell class="text-sm text-gray-900">{{
                        version.version
                      }}</TableCell>
                      <TableCell class="text-center">
                        <Badge variant="secondary" class="font-mono text-xs">
                          {{
                            version.is_enabled ? t("common.yes") : t("common.no")
                          }}
                        </Badge>
                      </TableCell>
                      <TableCell class="text-sm">{{
                        version.currency
                      }}</TableCell>
                      <TableCell class="text-sm">{{
                        formatTimestamp(version.effective_from)
                      }}</TableCell>
                      <TableCell class="text-xs text-gray-500">
                        {{
                          version.effective_until
                            ? formatTimestamp(version.effective_until)
                            : "-"
                        }}
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
            </div>
            <div
              v-else
              class="rounded-lg border border-dashed border-gray-200 bg-gray-50/60 px-4 py-6 text-sm text-gray-500"
            >
              {{ t("costPage.versions.empty") }}
            </div>
          </div>
        </CardContent>
      </Card>

      <!-- Custom Fields Section -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t("modelEditPage.sectionCustomFields") }}</CardTitle>
        </CardHeader>
        <CardContent class="space-y-4">
          <div
            v-if="
              editingData.custom_fields && editingData.custom_fields.length > 0
            "
            class="space-y-3"
          >
            <div class="space-y-3 md:hidden">
              <MobileCrudCard
                v-for="field in editingData.custom_fields"
                :key="field.id"
                :title="field.field_name"
                :description="field.description || '-'"
              >
                <div class="grid grid-cols-1 gap-3">
                  <div class="space-y-1">
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.tableHeaderFieldValue") }}
                    </p>
                    <p class="break-all font-mono text-sm text-gray-700">
                      {{ field.field_value }}
                    </p>
                  </div>
                  <div class="space-y-1">
                    <p class="text-[11px] font-medium uppercase tracking-wide text-gray-500">
                      {{ t("modelEditPage.tableHeaderFieldType") }}
                    </p>
                    <div>
                      <Badge variant="secondary" class="font-mono text-xs">
                        {{ field.field_type }}
                      </Badge>
                    </div>
                  </div>
                </div>

                <template #actions>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="w-full text-red-600 hover:bg-red-50 hover:text-red-700"
                    @click="handleUnlinkCustomField(field.id!)"
                  >
                    <Trash2 class="mr-1.5 h-4 w-4" />
                    {{ t("common.delete") }}
                  </Button>
                </template>
              </MobileCrudCard>
            </div>

            <div class="hidden overflow-hidden rounded-lg border border-gray-200 md:block">
              <Table>
                <TableHeader>
                  <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                    <TableHead
                      class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                      >{{ t("modelEditPage.tableHeaderFieldName") }}</TableHead
                    >
                    <TableHead
                      class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                      >{{ t("modelEditPage.tableHeaderFieldValue") }}</TableHead
                    >
                    <TableHead
                      class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                      >{{ t("modelEditPage.tableHeaderDescription") }}</TableHead
                    >
                    <TableHead
                      class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                      >{{ t("modelEditPage.tableHeaderFieldType") }}</TableHead
                    >
                    <TableHead class="w-[80px] text-right"></TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  <TableRow
                    v-for="field in editingData.custom_fields"
                    :key="field.id"
                  >
                    <TableCell class="text-sm font-medium text-gray-900">{{
                      field.field_name
                    }}</TableCell>
                    <TableCell
                      class="break-all text-sm font-mono text-gray-600"
                      >{{ field.field_value }}</TableCell
                    >
                    <TableCell class="text-sm text-gray-500">{{
                      field.description || "-"
                    }}</TableCell>
                    <TableCell>
                      <Badge variant="secondary" class="font-mono text-xs">{{
                        field.field_type
                      }}</Badge>
                    </TableCell>
                    <TableCell class="text-right">
                      <Button
                        variant="ghost"
                        size="sm"
                        class="text-gray-400 hover:text-red-600"
                        @click="handleUnlinkCustomField(field.id!)"
                      >
                        <Trash2 class="h-3.5 w-3.5" />
                      </Button>
                    </TableCell>
                  </TableRow>
                </TableBody>
              </Table>
            </div>
          </div>

          <div class="flex flex-col gap-3 border-t border-gray-100 pt-4 sm:flex-row sm:items-center">
            <div class="w-full sm:max-w-sm">
              <Select v-model="selectedCustomFieldId">
                <SelectTrigger class="w-full">
                  <SelectValue
                    :placeholder="
                      t('modelEditPage.placeholderSelectCustomField')
                    "
                  />
                </SelectTrigger>
                <SelectContent>
                  <SelectItem
                    v-for="field in availableCustomFields"
                    :key="field.id"
                    :value="field.id!.toString()"
                  >
                    {{ field.displayName }}
                  </SelectItem>
                </SelectContent>
              </Select>
            </div>
            <Button
              variant="default"
              class="w-full sm:w-auto"
              @click="handleLinkCustomField"
              :disabled="!selectedCustomFieldId"
            >
              {{ t("modelEditPage.buttonAddCustomField") }}
            </Button>
          </div>
        </CardContent>
      </Card>

        <div class="flex flex-col gap-2 border-t border-gray-100 pt-4 mt-2 sm:flex-row sm:justify-end">
          <Button
            variant="ghost"
            class="w-full text-gray-600 sm:w-auto"
            @click="handleNavigateBack"
            >{{ t("common.cancel") }}</Button
          >
          <Button variant="default" class="w-full sm:w-auto" @click="handleSaveModel">{{
            t("common.save")
          }}</Button>
        </div>
      </div>
    </div>
  </div>

  <CostEditorDialog
    :open="costManager.isEditorDialogOpen.value"
    :selected-catalog="costManager.selectedCatalog.value"
    :selected-catalog-versions="costManager.selectedCatalogVersions.value"
    :selected-version-id="costManager.costStore.selectedVersionId"
    :selected-version-summary="costManager.selectedVersionSummary.value"
    :components="costManager.components.value"
    :is-loading-version-detail="costManager.costStore.isLoadingVersionDetail"
    :toggling-version-id="costManager.togglingVersionId.value"
    :duplicating-catalog-id="costManager.duplicatingCatalogId.value"
    :preview-draft="costManager.previewDraft"
    :preview-response="costManager.previewResponse.value"
    :can-preview="costManager.canPreview.value"
    :is-running-preview="costManager.isRunningPreview.value"
    :meter-label="costManager.meterLabel"
    :charge-kind-label="costManager.chargeKindLabel"
    :tier-basis-label="costManager.tierBasisLabel"
    :format-rate-display="costManager.formatRateDisplay"
    :try-format-rate-input-display="costManager.tryFormatRateInputDisplay"
    :format-number="costManager.formatNumber"
    :pretty-json="costManager.prettyJson"
    @update:open="(open) => (costManager.isEditorDialogOpen.value = open)"
    @refresh="costManager.refreshCostData"
    @open-template="costManager.openTemplateDialog"
    @edit-catalog="costManager.openEditCatalogDialog"
    @duplicate-catalog="costManager.duplicateCatalog"
    @create-version="costManager.openCreateVersionDialog"
    @select-version="costManager.handleSelectVersion"
    @toggle-version-enabled="costManager.handleToggleVersionEnabled"
    @create-component="costManager.openCreateComponentDialog"
    @edit-component="costManager.openEditComponentDialog"
    @delete-component="costManager.handleDeleteComponent"
    @apply-sample="costManager.applyPreviewSample"
    @run-preview="costManager.runPreview"
  />

  <CostTemplateDialog
    :open="costManager.isTemplateDialogOpen.value"
    :templates="costManager.templates.value"
    :is-loading-templates="costManager.isLoadingTemplates.value"
    :importing-template-key="costManager.importingTemplateKey.value"
    @update:open="(open) => (costManager.isTemplateDialogOpen.value = open)"
    @refresh="costManager.refreshTemplates"
    @import-template="costManager.importTemplate"
  />

  <CostCatalogDialog
    :open="costManager.isCatalogDialogOpen.value"
    :draft="costManager.catalogDraft"
    :is-saving="costManager.isSavingCatalog.value"
    @update:open="handleCostCatalogDialogOpenChange"
    @save="costManager.saveCatalog"
  />

  <CostVersionDialog
    :open="costManager.isVersionDialogOpen.value"
    :draft="costManager.versionDraft"
    :is-saving="costManager.isSavingVersion.value"
    @update:open="(open) => (costManager.isVersionDialogOpen.value = open)"
    @save="costManager.saveVersion"
  />

  <CostComponentDialog
    :open="costManager.isComponentDialogOpen.value"
    :draft="costManager.componentDraft"
    :is-saving="costManager.isSavingComponent.value"
    :selected-currency="costManager.selectedVersionSummary.value?.currency"
    :meter-label="costManager.meterLabel"
    :charge-kind-label="costManager.chargeKindLabel"
    @update:open="(open) => (costManager.isComponentDialogOpen.value = open)"
    @save="costManager.saveComponent"
    @add-tier="costManager.addTier"
    @remove-tier="costManager.removeTier"
  />
</template>
