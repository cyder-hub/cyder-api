<script setup lang="ts">
import { toRef } from "vue";
import { useI18n } from "vue-i18n";
import { Plus, Trash2 } from "lucide-vue-next";

import SectionHeader from "@/components/SectionHeader.vue";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type {
  ApiKeyAction,
  ApiKeyDetail,
  ModelRouteListItem,
  ModelSummaryItem,
  ProviderSummaryItem,
} from "@/services/types";
import { useApiKeyEditDialog } from "../composables/useApiKeyGovernance";
import type { ApiKeyEditSuccessPayload } from "../types";

interface ApiKeyEditDialogProps {
  isOpen: boolean;
  initialData: ApiKeyDetail | null;
  modelRoutes: ModelRouteListItem[];
  providers: ProviderSummaryItem[];
  models: ModelSummaryItem[];
}

const props = defineProps<ApiKeyEditDialogProps>();
const emit = defineEmits<{
  (event: "update:isOpen", value: boolean): void;
  (event: "saveSuccess", payload: ApiKeyEditSuccessPayload): void;
}>();

const { t } = useI18n();

function handleOpenChange(open: boolean) {
  emit("update:isOpen", open);
}

const {
  isSubmitting,
  editingData,
  actionOptions,
  scopeOptions,
  providerOptions,
  budgetCurrencyOptions,
  routeOptions,
  updateBudgetCurrency,
  clearQuotaLimits,
  clearBudgetLimits,
  addRule,
  removeRule,
  addOverride,
  removeOverride,
  updateRuleScope,
  updateRuleProvider,
  updateOverrideTargetRoute,
  modelOptionsForRule,
  handleCommit,
} = useApiKeyEditDialog({
  isOpen: toRef(props, "isOpen"),
  initialData: toRef(props, "initialData"),
  modelRoutes: toRef(props, "modelRoutes"),
  providers: toRef(props, "providers"),
  models: toRef(props, "models"),
  t,
  close: () => emit("update:isOpen", false),
  emitSaveSuccess: (payload) => emit("saveSuccess", payload),
});
</script>

<template>
  <Dialog :open="props.isOpen" @update:open="handleOpenChange">
    <DialogContent class="flex max-h-[92dvh] flex-col p-0 sm:max-w-4xl">
      <DialogHeader class="border-b border-gray-100 px-4 py-4 sm:px-6">
        <DialogTitle class="text-lg font-semibold text-gray-900">
          {{
            editingData.id
              ? t("apiKeyEditModal.titleEdit")
              : t("apiKeyEditModal.titleAdd")
          }}
        </DialogTitle>
      </DialogHeader>

      <form class="flex min-h-0 flex-1 flex-col" @submit.prevent="handleCommit">
        <div class="flex-1 space-y-6 overflow-y-auto px-4 py-4 sm:px-6">
          <section class="space-y-4">
            <SectionHeader
              :title="t('apiKeyEditModal.sections.identity')"
              :help="t('apiKeyEditModal.sections.identityDescription')"
              :help-label="t('apiKeyEditModal.sections.identity')"
            />

            <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
              <div class="space-y-1.5 sm:col-span-2">
                <Label for="api-key-name" class="text-gray-700">
                  {{ t("apiKeyEditModal.labelName") }}
                  <span class="ml-0.5 text-red-500">*</span>
                </Label>
                <Input id="api-key-name" v-model="editingData.name" class="w-full" />
              </div>

              <div class="space-y-1.5 sm:col-span-2">
                <Label for="api-key-description" class="text-gray-700">
                  {{ t("apiKeyEditModal.labelDescription") }}
                </Label>
                <textarea
                  id="api-key-description"
                  v-model="editingData.description"
                  rows="3"
                  class="flex min-h-24 w-full rounded-md border border-gray-200 bg-white px-3 py-2 text-sm text-gray-900 outline-none transition placeholder:text-gray-400 focus:border-gray-400"
                />
              </div>

              <div class="space-y-1.5">
                <Label class="text-gray-700">
                  {{ t("apiKeyEditModal.labelDefaultAction") }}
                </Label>
                <Select
                  :model-value="editingData.default_action"
                  @update:model-value="
                    (value) => (editingData.default_action = value as ApiKeyAction)
                  "
                >
                  <SelectTrigger class="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent :body-lock="false">
                    <SelectItem
                      v-for="option in actionOptions"
                      :key="option.value"
                      :value="option.value"
                    >
                      {{ option.label }}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>

              <div class="space-y-1.5">
                <Label for="api-key-expires-at" class="text-gray-700">
                  {{ t("apiKeyEditModal.labelExpiresAt") }}
                </Label>
                <Input
                  id="api-key-expires-at"
                  v-model="editingData.expires_at"
                  type="datetime-local"
                  class="w-full"
                />
              </div>
            </div>

            <div class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5">
              <Label class="cursor-pointer text-sm font-medium leading-none">
                {{ t("apiKeyEditModal.labelEnabled") }}
              </Label>
              <Checkbox v-model="editingData.is_enabled" />
            </div>
          </section>

          <section class="space-y-4 border-t border-gray-100 pt-6">
            <SectionHeader
              :title="t('apiKeyEditModal.sections.governance')"
              :help="t('apiKeyEditModal.sections.governanceDescription')"
              :help-label="t('apiKeyEditModal.sections.governance')"
            />

            <div class="border-t border-gray-100 pt-4">
              <SectionHeader
                :title="t('apiKeyEditModal.groups.throughputTitle')"
                :help="t('apiKeyEditModal.groups.throughputDescription')"
                :help-label="t('apiKeyEditModal.groups.throughputTitle')"
                title-class="text-sm"
              />

              <div class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div class="space-y-1.5">
                  <Label for="rate-limit-rpm" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelRateLimitRpm") }}
                  </Label>
                  <Input
                    id="rate-limit-rpm"
                    v-model="editingData.rate_limit_rpm"
                    type="number"
                  />
                </div>

                <div class="space-y-1.5">
                  <Label for="max-concurrency" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelMaxConcurrency") }}
                  </Label>
                  <Input
                    id="max-concurrency"
                    v-model="editingData.max_concurrent_requests"
                    type="number"
                  />
                </div>
              </div>
            </div>

            <div class="border-t border-gray-100 pt-4">
              <SectionHeader
                :title="t('apiKeyEditModal.groups.quotaTitle')"
                :help="t('apiKeyEditModal.groups.quotaDescription')"
                :help-label="t('apiKeyEditModal.groups.quotaTitle')"
                title-class="text-sm"
              >
                <template #actions>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  class="text-gray-500"
                  @click="clearQuotaLimits"
                >
                  {{ t("apiKeyEditModal.clearQuota") }}
                </Button>
                </template>
              </SectionHeader>

              <div class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div class="space-y-1.5">
                  <Label for="quota-daily-requests" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelQuotaDailyRequests") }}
                  </Label>
                  <Input
                    id="quota-daily-requests"
                    v-model="editingData.quota_daily_requests"
                    type="number"
                  />
                </div>

                <div class="space-y-1.5">
                  <Label for="quota-daily-tokens" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelQuotaDailyTokens") }}
                  </Label>
                  <Input
                    id="quota-daily-tokens"
                    v-model="editingData.quota_daily_tokens"
                    type="number"
                  />
                </div>

                <div class="space-y-1.5 sm:col-span-2">
                  <Label for="quota-monthly-tokens" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelQuotaMonthlyTokens") }}
                  </Label>
                  <Input
                    id="quota-monthly-tokens"
                    v-model="editingData.quota_monthly_tokens"
                    type="number"
                  />
                </div>
              </div>
            </div>

            <div class="border-t border-gray-100 pt-4">
              <SectionHeader
                :title="t('apiKeyEditModal.groups.budgetTitle')"
                :help="t('apiKeyEditModal.groups.budgetDescription')"
                :help-label="t('apiKeyEditModal.groups.budgetTitle')"
                title-class="text-sm"
              >
                <template #actions>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  class="text-gray-500"
                  @click="clearBudgetLimits"
                >
                  {{ t("apiKeyEditModal.clearBudget") }}
                </Button>
                </template>
              </SectionHeader>

              <div class="mt-4 rounded-lg border border-gray-200 bg-gray-50 px-3 py-2 text-sm text-gray-600">
                {{ t("apiKeyEditModal.budgetMajorUnitHint") }}
              </div>

              <div class="mt-4 space-y-4">
                <div class="grid grid-cols-1 gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_minmax(0,0.9fr)] sm:items-end">
                  <Label for="budget-daily-amount" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelBudgetDaily") }}
                  </Label>
                  <div class="space-y-1.5">
                    <Label for="budget-daily-amount" class="text-gray-700">
                      {{ t("apiKeyEditModal.labelBudgetAmount") }}
                    </Label>
                    <Input
                      id="budget-daily-amount"
                      v-model="editingData.budget_daily_nanos"
                      :placeholder="t('apiKeyEditModal.placeholderBudgetAmount')"
                      inputmode="decimal"
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelBudgetCurrency") }}
                    </Label>
                    <Select
                      :model-value="editingData.budget_daily_currency || 'none'"
                      @update:model-value="
                        (value) => updateBudgetCurrency('daily', String(value ?? 'none'))
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue :placeholder="t('apiKeyEditModal.placeholderCurrency')" />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem value="none">
                          {{ t("common.notAvailable") }}
                        </SelectItem>
                        <SelectItem
                          v-for="option in budgetCurrencyOptions"
                          :key="`daily-${option.value}`"
                          :value="option.value"
                        >
                          {{ option.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>

                <div class="grid grid-cols-1 gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_minmax(0,0.9fr)] sm:items-end">
                  <Label for="budget-monthly-amount" class="text-gray-700">
                    {{ t("apiKeyEditModal.labelBudgetMonthly") }}
                  </Label>
                  <div class="space-y-1.5">
                    <Label for="budget-monthly-amount" class="text-gray-700">
                      {{ t("apiKeyEditModal.labelBudgetAmount") }}
                    </Label>
                    <Input
                      id="budget-monthly-amount"
                      v-model="editingData.budget_monthly_nanos"
                      :placeholder="t('apiKeyEditModal.placeholderBudgetAmount')"
                      inputmode="decimal"
                    />
                  </div>
                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelBudgetCurrency") }}
                    </Label>
                    <Select
                      :model-value="editingData.budget_monthly_currency || 'none'"
                      @update:model-value="
                        (value) => updateBudgetCurrency('monthly', String(value ?? 'none'))
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue :placeholder="t('apiKeyEditModal.placeholderCurrency')" />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem value="none">
                          {{ t("common.notAvailable") }}
                        </SelectItem>
                        <SelectItem
                          v-for="option in budgetCurrencyOptions"
                          :key="`monthly-${option.value}`"
                          :value="option.value"
                        >
                          {{ option.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>
                </div>
              </div>
            </div>
          </section>

          <section class="space-y-4 border-t border-gray-100 pt-6">
            <SectionHeader
              :title="t('apiKeyEditModal.sections.overrides')"
              :help="t('apiKeyEditModal.sections.overridesDescription')"
              :help-label="t('apiKeyEditModal.sections.overrides')"
            >
              <template #actions>
              <Button
                type="button"
                variant="outline"
                class="w-full sm:w-auto"
                :disabled="!routeOptions.length"
                @click="addOverride"
              >
                <Plus class="mr-1.5 h-4 w-4" />
                {{ t("apiKeyEditModal.addOverride") }}
              </Button>
              </template>
            </SectionHeader>

            <div
              v-if="!routeOptions.length"
              class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500"
            >
              {{ t("apiKeyEditModal.noRoutes") }}
            </div>

            <div
              v-else-if="!editingData.model_overrides.length"
              class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500"
            >
              {{ t("apiKeyEditModal.noOverrides") }}
            </div>

            <div v-else class="space-y-4">
              <div
                v-for="(item, index) in editingData.model_overrides"
                :key="item.local_id"
                class="rounded-lg bg-gray-50/70 p-4"
              >
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h4 class="text-sm font-semibold text-gray-900">
                      {{ t("apiKeyEditModal.overrideTitle", { index: index + 1 }) }}
                    </h4>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    class="text-gray-400 hover:text-red-600"
                    @click="removeOverride(index)"
                  >
                    <Trash2 class="mr-1 h-3.5 w-3.5" />
                    {{ t("common.delete") }}
                  </Button>
                </div>

                <div class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelOverrideSourceName") }}
                    </Label>
                    <Input
                      v-model="item.source_name"
                      :placeholder="t('apiKeyEditModal.placeholderOverrideSourceName')"
                    />
                  </div>

                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelOverrideTargetRoute") }}
                    </Label>
                    <Select
                      :model-value="
                        item.target_route_id == null ? 'none' : String(item.target_route_id)
                      "
                      @update:model-value="
                        (value) => updateOverrideTargetRoute(index, String(value ?? 'none'))
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue
                          :placeholder="t('apiKeyEditModal.placeholderOverrideTargetRoute')"
                        />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem value="none">
                          {{ t("apiKeyEditModal.placeholderOverrideTargetRoute") }}
                        </SelectItem>
                        <SelectItem
                          v-for="route in routeOptions"
                          :key="route.value"
                          :value="String(route.value)"
                        >
                          {{ route.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div class="space-y-1.5 sm:col-span-2">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelOverrideDescription") }}
                    </Label>
                    <Input v-model="item.description" />
                  </div>
                </div>

                <div class="mt-4 flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5">
                  <Label class="cursor-pointer text-sm font-medium leading-none">
                    {{ t("apiKeyEditModal.labelOverrideEnabled") }}
                  </Label>
                  <Checkbox v-model="item.is_enabled" />
                </div>
              </div>
            </div>
          </section>

          <section class="space-y-4 border-t border-gray-100 pt-6">
            <SectionHeader
              :title="t('apiKeyEditModal.sections.acl')"
              :help="t('apiKeyEditModal.sections.aclDescription')"
              :help-label="t('apiKeyEditModal.sections.acl')"
            >
              <template #actions>
              <Button
                type="button"
                variant="outline"
                class="w-full sm:w-auto"
                @click="addRule"
              >
                <Plus class="mr-1.5 h-4 w-4" />
                {{ t("apiKeyEditModal.addRule") }}
              </Button>
              </template>
            </SectionHeader>

            <div
              v-if="!editingData.acl_rules.length"
              class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500"
            >
              {{ t("apiKeyEditModal.noRules") }}
            </div>

            <div v-else class="space-y-4">
              <div
                v-for="(rule, index) in editingData.acl_rules"
                :key="rule.id ?? `new-${index}`"
                class="rounded-lg bg-gray-50/70 p-4"
              >
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h4 class="text-sm font-semibold text-gray-900">
                      {{ t("apiKeyEditModal.ruleTitle", { index: index + 1 }) }}
                    </h4>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    class="text-gray-400 hover:text-red-600"
                    @click="removeRule(index)"
                  >
                    <Trash2 class="mr-1 h-3.5 w-3.5" />
                    {{ t("common.delete") }}
                  </Button>
                </div>

                <div class="mt-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelRuleEffect") }}
                    </Label>
                    <Select
                      :model-value="rule.effect"
                      @update:model-value="
                        (value) => (rule.effect = value as ApiKeyAction)
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem
                          v-for="option in actionOptions"
                          :key="option.value"
                          :value="option.value"
                        >
                          {{ option.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelRuleScope") }}
                    </Label>
                    <Select
                      :model-value="rule.scope"
                      @update:model-value="
                        (value) => updateRuleScope(index, String(value ?? 'PROVIDER'))
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem
                          v-for="option in scopeOptions"
                          :key="option.value"
                          :value="option.value"
                        >
                          {{ option.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelRulePriority") }}
                    </Label>
                    <Input v-model="rule.priority" type="number" />
                  </div>

                  <div class="space-y-1.5">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelRuleProvider") }}
                    </Label>
                    <Select
                      :model-value="
                        rule.provider_id == null ? 'none' : String(rule.provider_id)
                      "
                      @update:model-value="
                        (value) => updateRuleProvider(index, String(value ?? 'none'))
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue :placeholder="t('apiKeyEditModal.placeholderProvider')" />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem value="none">
                          {{ t("apiKeyEditModal.allProviders") }}
                        </SelectItem>
                        <SelectItem
                          v-for="provider in providerOptions"
                          :key="provider.value"
                          :value="String(provider.value)"
                        >
                          {{ provider.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div v-if="rule.scope === 'MODEL'" class="space-y-1.5 sm:col-span-2">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelRuleModel") }}
                    </Label>
                    <Select
                      :model-value="rule.model_id == null ? 'none' : String(rule.model_id)"
                      @update:model-value="
                        (value) => (rule.model_id = value === 'none' ? null : Number(value))
                      "
                    >
                      <SelectTrigger class="w-full">
                        <SelectValue :placeholder="t('apiKeyEditModal.placeholderModel')" />
                      </SelectTrigger>
                      <SelectContent :body-lock="false">
                        <SelectItem value="none">
                          {{ t("apiKeyEditModal.selectModel") }}
                        </SelectItem>
                        <SelectItem
                          v-for="model in modelOptionsForRule(rule)"
                          :key="model.value"
                          :value="String(model.value)"
                        >
                          {{ model.label }}
                        </SelectItem>
                      </SelectContent>
                    </Select>
                  </div>

                  <div class="space-y-1.5 sm:col-span-2">
                    <Label class="text-gray-700">
                      {{ t("apiKeyEditModal.labelRuleDescription") }}
                    </Label>
                    <Input v-model="rule.description" />
                  </div>
                </div>

                <div class="mt-4 flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5">
                  <Label class="cursor-pointer text-sm font-medium leading-none">
                    {{ t("apiKeyEditModal.labelRuleEnabled") }}
                  </Label>
                  <Checkbox v-model="rule.is_enabled" />
                </div>
              </div>
            </div>
          </section>
        </div>

        <DialogFooter class="border-t border-gray-100 px-4 py-4 sm:px-6">
          <Button
            type="button"
            variant="ghost"
            class="w-full text-gray-600 sm:w-auto"
            :disabled="isSubmitting"
            @click="handleOpenChange(false)"
          >
            {{ t("common.cancel") }}
          </Button>
          <Button type="submit" class="w-full sm:w-auto" :disabled="isSubmitting">
            {{ isSubmitting ? t("common.saving") : t("common.save") }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
