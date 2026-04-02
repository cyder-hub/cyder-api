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
  BillingPlan,
  PriceRule,
  CustomFieldDefinition,
  ModelDetailResponse,
} from "@/store/types";

import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Card, CardHeader, CardTitle, CardContent } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Input } from "@/components/ui/input";
import { Checkbox } from "@/components/ui/checkbox";
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
import { Loader2, Trash2 } from "lucide-vue-next";

interface EditingModelData {
  id: number;
  provider_id: number;
  billing_plan_id: number | null;
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
const billingPlans = ref<BillingPlan[]>([]);
const priceRules = ref<PriceRule[]>([]);
const isLoadingPriceRules = ref(false);

const editingData = ref<EditingModelData | null>(null);
const selectedCustomFieldId = ref<string | null>(null);

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
    const [detail, customFieldsRes, plans] = await Promise.all([
      Api.getModelDetail(modelId),
      Api.getCustomFieldList(),
      Api.getBillingPlanList(),
    ]);

    modelDetail.value = detail;
    billingPlans.value = plans;

    if (customFieldsRes && customFieldsRes.list) {
      allCustomFields.value = customFieldsRes.list.map(toEditableCustomField);
    }

    if (detail) {
      editingData.value = {
        id: detail.model.id,
        provider_id: detail.model.provider_id,
        billing_plan_id: detail.model.billing_plan_id ?? null,
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

const fetchPriceRules = async (planId: number | null) => {
  if (!planId) {
    priceRules.value = [];
    return;
  }
  try {
    isLoadingPriceRules.value = true;
    priceRules.value = await Api.getPriceRuleListByPlan(planId);
  } catch (error: unknown) {
    const normalizedError = normalizeError(error, t("common.unknownError"));
    toastController.error(
      t("modelEditPage.priceSection.loadingRules"),
      normalizedError.message,
    );
    priceRules.value = [];
  } finally {
    isLoadingPriceRules.value = false;
  }
};

watch(
  () => editingData.value?.billing_plan_id,
  (newPlanId) => {
    if (newPlanId !== undefined) {
      fetchPriceRules(newPlanId);
    }
  },
  { immediate: true },
);

const selectedPlan = computed(() =>
  billingPlans.value.find((p) => p.id === editingData.value?.billing_plan_id),
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
    billing_plan_id: editingData.value.billing_plan_id,
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

onMounted(() => {
  fetchData();
});
</script>

<template>
  <div class="p-6 space-y-6 max-w-4xl mx-auto">
    <!-- 页面头部 -->
    <div class="flex justify-between items-start">
      <div>
        <h1 class="text-lg font-semibold text-gray-900 tracking-tight">
          {{ t("modelEditPage.title") }}
        </h1>
      </div>
      <Button variant="outline" @click="handleNavigateBack">
        {{ t("common.cancel") }}
      </Button>
    </div>

    <!-- 加载状态 -->
    <div v-if="isLoading" class="flex items-center justify-center py-16">
      <Loader2 class="h-5 w-5 animate-spin text-gray-400 mr-2" />
      <span class="text-sm text-gray-500">{{
        t("modelEditPage.loading")
      }}</span>
    </div>

    <!-- 错误状态 -->
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

    <!-- 表单区域 -->
    <div v-else-if="editingData" class="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle>{{ t("common.basicInfo") }}</CardTitle>
        </CardHeader>
        <CardContent class="space-y-4">
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

      <!-- Price Management Section -->
      <Card>
        <CardHeader>
          <CardTitle>{{ t("modelEditPage.priceSection.title") }}</CardTitle>
        </CardHeader>
        <CardContent class="space-y-6">
          <div class="grid gap-1.5">
            <Label class="text-gray-700">{{
              t("modelEditPage.priceSection.labelBillingPlan")
            }}</Label>
            <Select
              :model-value="editingData.billing_plan_id?.toString() || 'none'"
              @update:model-value="
                (val: any) =>
                  (editingData!.billing_plan_id =
                    val === 'none' ? null : parseInt(val as string))
              "
            >
              <SelectTrigger class="w-full">
                <SelectValue
                  :placeholder="
                    t('modelEditPage.priceSection.placeholderBillingPlan')
                  "
                />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="none">{{
                  t("modelEditPage.priceSection.noPlan")
                }}</SelectItem>
                <SelectItem
                  v-for="plan in billingPlans"
                  :key="plan.id"
                  :value="plan.id.toString()"
                >
                  {{ plan.name }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>

          <div v-if="editingData.billing_plan_id" class="space-y-4">
            <div
              v-if="isLoadingPriceRules"
              class="flex items-center justify-center py-8"
            >
              <Loader2 class="h-5 w-5 animate-spin mr-2 text-gray-500" />
              <p class="text-sm text-gray-500">
                {{ t("modelEditPage.priceSection.loadingRules") }}
              </p>
            </div>
            <div v-else-if="priceRules.length > 0" class="space-y-2">
              <h4 class="text-sm font-semibold text-gray-700">
                {{ t("modelEditPage.priceSection.rulesTitle") }}
              </h4>
              <div class="border border-gray-200 rounded-lg overflow-hidden">
                <Table>
                  <TableHeader>
                    <TableRow class="bg-gray-50/80 hover:bg-gray-50/80">
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("pricePage.rules.table.description") }}</TableHead
                      >
                      <TableHead
                        class="w-[80px] text-center text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("pricePage.rules.table.enabled") }}</TableHead
                      >
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("pricePage.rules.table.usageType") }}</TableHead
                      >
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("pricePage.rules.table.mediaType") }}</TableHead
                      >
                      <TableHead
                        class="text-right text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{ t("pricePage.rules.table.price") }}</TableHead
                      >
                      <TableHead
                        class="text-xs font-medium text-gray-500 uppercase tracking-wider"
                        >{{
                          t("pricePage.rules.table.effectiveFrom")
                        }}</TableHead
                      >
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    <TableRow v-for="rule in priceRules" :key="rule.id">
                      <TableCell class="text-sm text-gray-900">{{
                        rule.description || "-"
                      }}</TableCell>
                      <TableCell class="text-center">
                        <Badge variant="secondary" class="font-mono text-xs">
                          {{
                            rule.is_enabled ? t("common.yes") : t("common.no")
                          }}
                        </Badge>
                      </TableCell>
                      <TableCell class="text-sm">{{
                        rule.usage_type
                      }}</TableCell>
                      <TableCell class="text-sm">{{
                        rule.media_type || "-"
                      }}</TableCell>
                      <TableCell
                        class="text-right text-sm font-mono text-gray-600"
                      >
                        {{ rule.price_in_micro_units / 1000 }}
                        {{ selectedPlan?.currency }}
                      </TableCell>
                      <TableCell class="text-xs text-gray-500">
                        {{ formatTimestamp(rule.effective_from) }}
                      </TableCell>
                    </TableRow>
                  </TableBody>
                </Table>
              </div>
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
            class="border border-gray-200 rounded-lg overflow-hidden"
          >
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

          <div class="flex items-center gap-3 pt-4 border-t border-gray-100">
            <div class="flex-1 max-w-sm">
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
              @click="handleLinkCustomField"
              :disabled="!selectedCustomFieldId"
            >
              {{ t("modelEditPage.buttonAddCustomField") }}
            </Button>
          </div>
        </CardContent>
      </Card>

      <div class="flex justify-end gap-3 pt-4 border-t border-gray-100 mt-2">
        <Button
          variant="ghost"
          class="text-gray-600"
          @click="handleNavigateBack"
          >{{ t("common.cancel") }}</Button
        >
        <Button variant="default" @click="handleSaveModel">{{
          t("common.save")
        }}</Button>
      </div>
    </div>
  </div>
</template>
