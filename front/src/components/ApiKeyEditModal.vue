<script setup lang="ts">
import { computed, ref, watch } from "vue";
import { useI18n } from "vue-i18n";
import { Plus, Trash2 } from "lucide-vue-next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Api } from "@/services/request";
import { normalizeError } from "@/lib/error";
import { toastController } from "@/lib/toastController";
import { formatPriceInputFromNanos, majorUnitToNanos } from "@/lib/utils";
import type {
  ApiKeyAclRule,
  ApiKeyAclRulePayload,
  ApiKeyAction,
  ApiKeyAclRuleScope,
  ApiKeyCreatePayload,
  ApiKeyDetail,
  ApiKeyModelOverrideItem,
  ApiKeyModelOverridePayload,
  ApiKeyReveal,
  ApiKeyUpdatePayload,
  ModelRouteListItem,
  ModelSummaryItem,
  ProviderSummaryItem,
} from "@/store/types";

interface EditableRule {
  id?: number;
  effect: ApiKeyAction;
  scope: ApiKeyAclRuleScope;
  priority: string;
  provider_id: number | null;
  model_id: number | null;
  is_enabled: boolean;
  description: string;
}

interface EditableOverride {
  local_id: number;
  source_name: string;
  target_route_id: number | null;
  description: string;
  is_enabled: boolean;
}

interface EditingApiKeyData {
  id: number | null;
  name: string;
  description: string;
  default_action: ApiKeyAction;
  is_enabled: boolean;
  expires_at: string;
  rate_limit_rpm: string;
  max_concurrent_requests: string;
  quota_daily_requests: string;
  quota_daily_tokens: string;
  quota_monthly_tokens: string;
  budget_daily_nanos: string;
  budget_daily_currency: string;
  budget_monthly_nanos: string;
  budget_monthly_currency: string;
  model_overrides: EditableOverride[];
  acl_rules: EditableRule[];
}

interface SaveSuccessPayload {
  detail: ApiKeyDetail;
  reveal?: ApiKeyReveal;
}

interface ApiKeyEditModalProps {
  isOpen: boolean;
  initialData: ApiKeyDetail | null;
  modelRoutes: ModelRouteListItem[];
  providers: ProviderSummaryItem[];
  models: ModelSummaryItem[];
}

const props = defineProps<ApiKeyEditModalProps>();
const emit = defineEmits<{
  (event: "update:isOpen", value: boolean): void;
  (event: "saveSuccess", payload: SaveSuccessPayload): void;
}>();

const { t } = useI18n();

const isSubmitting = ref(false);
const COMMON_BUDGET_CURRENCIES = ["CNY", "USD"] as const;
let nextOverrideDraftId = 1;

const getEmptyRule = (): EditableRule => ({
  effect: "ALLOW",
  scope: "PROVIDER",
  priority: "0",
  provider_id: null,
  model_id: null,
  is_enabled: true,
  description: "",
});

const getEmptyOverride = (): EditableOverride => ({
  local_id: nextOverrideDraftId++,
  source_name: "",
  target_route_id: null,
  description: "",
  is_enabled: true,
});

const getEmptyEditingData = (): EditingApiKeyData => ({
  id: null,
  name: "",
  description: "",
  default_action: "ALLOW",
  is_enabled: true,
  expires_at: "",
  rate_limit_rpm: "",
  max_concurrent_requests: "10",
  quota_daily_requests: "",
  quota_daily_tokens: "",
  quota_monthly_tokens: "",
  budget_daily_nanos: "",
  budget_daily_currency: "",
  budget_monthly_nanos: "",
  budget_monthly_currency: "",
  model_overrides: [],
  acl_rules: [],
});

const editingData = ref<EditingApiKeyData>(getEmptyEditingData());

const actionOptions = computed(() =>
  (["ALLOW", "DENY"] as ApiKeyAction[]).map((value) => ({
    value,
    label: t(`apiKeyEditModal.action.${value}`),
  })),
);

const scopeOptions = computed(() =>
  (["PROVIDER", "MODEL"] as ApiKeyAclRuleScope[]).map((value) => ({
    value,
    label: t(`apiKeyEditModal.scope.${value}`),
  })),
);

const providerOptions = computed(() =>
  props.providers.map((item) => ({
    value: item.id,
    label: `${item.name} (${item.provider_key})`,
    models: props.models
      .filter((modelItem) => modelItem.provider_id === item.id)
      .map((modelItem) => ({
        value: modelItem.id,
        label: modelItem.model_name,
        providerId: item.id,
      })),
  })),
);

const modelToProviderMap = computed(() => {
  const map = new Map<number, number>();
  for (const provider of providerOptions.value) {
    for (const model of provider.models) {
      map.set(model.value, provider.value);
    }
  }
  return map;
});

const allModelOptions = computed(() =>
  providerOptions.value.flatMap((provider) =>
    provider.models.map((model) => ({
      value: model.value,
      label: `${provider.label} / ${model.label}`,
      providerId: provider.value,
    })),
  ),
);

const budgetCurrencyOptions = computed(() => {
  const values = new Set<string>(COMMON_BUDGET_CURRENCIES);
  const dailyCurrency = textOrNull(editingData.value.budget_daily_currency)?.toUpperCase();
  const monthlyCurrency = textOrNull(editingData.value.budget_monthly_currency)?.toUpperCase();

  if (dailyCurrency) {
    values.add(dailyCurrency);
  }
  if (monthlyCurrency) {
    values.add(monthlyCurrency);
  }

  return Array.from(values).map((value) => ({
    value,
    label:
      value === "CNY" || value === "USD"
        ? t(`apiKeyEditModal.currency.${value}`)
        : value,
  }));
});

const routeOptions = computed(() =>
  props.modelRoutes.map((item) => ({
    value: item.route.id,
    label: item.route.route_name,
  })),
);

function toInputNumber(value: number | null | undefined): string {
  return value === null || value === undefined ? "" : String(value);
}

function toBudgetInput(value: number | null | undefined, currency?: string | null): string {
  return formatPriceInputFromNanos(value, currency);
}

function toLocalDatetimeInput(value: number | null | undefined): string {
  if (!value) {
    return "";
  }
  const date = new Date(value);
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function fromLocalDatetimeInput(value: string | number | null | undefined): number | null {
  if (value === null || value === undefined) {
    return null;
  }

  const normalizedValue = String(value).trim();
  if (!normalizedValue) {
    return null;
  }

  const timestamp = new Date(normalizedValue).getTime();
  return Number.isFinite(timestamp) ? timestamp : null;
}

function numberOrNull(value: string | number | null | undefined): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new Error(t("apiKeyEditModal.alert.invalidNumber"));
    }
    return value;
  }

  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) {
    throw new Error(t("apiKeyEditModal.alert.invalidNumber"));
  }
  return parsed;
}

function textOrNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

function updateBudgetCurrency(target: "daily" | "monthly", value: string) {
  const normalizedValue = value === "none" ? "" : value;
  if (target === "daily") {
    editingData.value.budget_daily_currency = normalizedValue;
    return;
  }

  editingData.value.budget_monthly_currency = normalizedValue;
}

function buildBudgetPayload(
  amountInput: string | number | null | undefined,
  currencyInput: string,
  label: string,
) {
  const normalizedAmount = amountInput == null ? "" : String(amountInput).trim();
  if (!normalizedAmount) {
    return {
      nanos: null,
      currency: null,
    };
  }

  const currency = textOrNull(currencyInput)?.toUpperCase() ?? null;
  if (!currency) {
    throw new Error(t("apiKeyEditModal.alert.budgetCurrencyRequired", { label }));
  }

  try {
    const nanos = majorUnitToNanos(normalizedAmount, currency);
    if (nanos === null) {
      return {
        nanos: null,
        currency: null,
      };
    }

    return {
      nanos,
      currency,
    };
  } catch {
    throw new Error(t("apiKeyEditModal.alert.invalidBudgetAmount", { label }));
  }
}

function clearQuotaLimits() {
  editingData.value.quota_daily_requests = "";
  editingData.value.quota_daily_tokens = "";
  editingData.value.quota_monthly_tokens = "";
}

function clearBudgetLimits() {
  editingData.value.budget_daily_nanos = "";
  editingData.value.budget_daily_currency = "";
  editingData.value.budget_monthly_nanos = "";
  editingData.value.budget_monthly_currency = "";
}

function normalizeEditableRule(rule: ApiKeyAclRule): EditableRule {
  return {
    id: rule.id,
    effect: rule.effect,
    scope: rule.scope,
    priority: String(rule.priority),
    provider_id:
      rule.provider_id ??
      (rule.model_id ? modelToProviderMap.value.get(rule.model_id) ?? null : null),
    model_id: rule.model_id,
    is_enabled: rule.is_enabled,
    description: rule.description ?? "",
  };
}

function normalizeEditableOverride(item: ApiKeyModelOverrideItem): EditableOverride {
  return {
    local_id: nextOverrideDraftId++,
    source_name: item.source_name,
    target_route_id: item.target_route_id,
    description: item.description ?? "",
    is_enabled: item.is_enabled,
  };
}

function resetEditingData() {
  if (!props.initialData) {
    editingData.value = getEmptyEditingData();
    return;
  }

  editingData.value = {
    id: props.initialData.id,
    name: props.initialData.name,
    description: props.initialData.description ?? "",
    default_action: props.initialData.default_action,
    is_enabled: props.initialData.is_enabled,
    expires_at: toLocalDatetimeInput(props.initialData.expires_at),
    rate_limit_rpm: toInputNumber(props.initialData.rate_limit_rpm),
    max_concurrent_requests: toInputNumber(props.initialData.max_concurrent_requests),
    quota_daily_requests: toInputNumber(props.initialData.quota_daily_requests),
    quota_daily_tokens: toInputNumber(props.initialData.quota_daily_tokens),
    quota_monthly_tokens: toInputNumber(props.initialData.quota_monthly_tokens),
    budget_daily_nanos: toBudgetInput(
      props.initialData.budget_daily_nanos,
      props.initialData.budget_daily_currency,
    ),
    budget_daily_currency: props.initialData.budget_daily_currency ?? "",
    budget_monthly_nanos: toBudgetInput(
      props.initialData.budget_monthly_nanos,
      props.initialData.budget_monthly_currency,
    ),
    budget_monthly_currency: props.initialData.budget_monthly_currency ?? "",
    model_overrides: props.initialData.model_overrides.map(normalizeEditableOverride),
    acl_rules: props.initialData.acl_rules.map(normalizeEditableRule),
  };
}

function handleOpenChange(open: boolean) {
  emit("update:isOpen", open);
}

function addRule() {
  editingData.value.acl_rules.push(getEmptyRule());
}

function removeRule(index: number) {
  editingData.value.acl_rules.splice(index, 1);
}

function addOverride() {
  editingData.value.model_overrides.push(getEmptyOverride());
}

function removeOverride(index: number) {
  editingData.value.model_overrides.splice(index, 1);
}

function updateRuleScope(index: number, scope: string) {
  const rule = editingData.value.acl_rules[index];
  rule.scope = scope as ApiKeyAclRuleScope;
  if (rule.scope === "PROVIDER") {
    rule.model_id = null;
  }
}

function updateRuleProvider(index: number, providerId: string) {
  const rule = editingData.value.acl_rules[index];
  rule.provider_id = providerId === "none" ? null : Number(providerId);

  if (rule.scope === "MODEL" && rule.model_id) {
    const isStillVisible = modelOptionsForRule(rule).some(
      (option) => option.value === rule.model_id,
    );
    if (!isStillVisible) {
      rule.model_id = null;
    }
  }
}

function modelOptionsForRule(rule: EditableRule) {
  if (rule.provider_id == null) {
    return allModelOptions.value;
  }
  const provider = providerOptions.value.find((item) => item.value === rule.provider_id);
  return provider?.models ?? [];
}

function updateOverrideTargetRoute(index: number, routeId: string) {
  editingData.value.model_overrides[index].target_route_id =
    routeId === "none" ? null : Number(routeId);
}

function buildRulePayloads(): ApiKeyAclRulePayload[] {
  return editingData.value.acl_rules.map((rule, index) => {
    const priority = numberOrNull(rule.priority);
    if (priority === null) {
      throw new Error(
        t("apiKeyEditModal.alert.rulePriorityRequired", { index: index + 1 }),
      );
    }

    if (rule.scope === "PROVIDER" && rule.provider_id == null) {
      throw new Error(
        t("apiKeyEditModal.alert.ruleProviderRequired", { index: index + 1 }),
      );
    }

    if (rule.scope === "MODEL" && rule.model_id == null) {
      throw new Error(
        t("apiKeyEditModal.alert.ruleModelRequired", { index: index + 1 }),
      );
    }

    return {
      ...(rule.id ? { id: rule.id } : {}),
      effect: rule.effect,
      scope: rule.scope,
      priority,
      provider_id: rule.scope === "PROVIDER" ? rule.provider_id : rule.provider_id,
      model_id: rule.scope === "MODEL" ? rule.model_id : null,
      is_enabled: rule.is_enabled,
      description: textOrNull(rule.description),
    };
  });
}

function buildModelOverridePayloads(): ApiKeyModelOverridePayload[] {
  const seenNames = new Set<string>();

  return editingData.value.model_overrides.map((item, index) => {
    const sourceName = item.source_name.trim();
    if (!sourceName) {
      throw new Error(
        t("apiKeyEditModal.alert.overrideSourceNameRequired", { index: index + 1 }),
      );
    }

    const duplicateKey = sourceName.toLowerCase();
    if (seenNames.has(duplicateKey)) {
      throw new Error(
        t("apiKeyEditModal.alert.duplicateOverrideSourceName", {
          name: sourceName,
        }),
      );
    }
    seenNames.add(duplicateKey);

    if (item.target_route_id == null) {
      throw new Error(
        t("apiKeyEditModal.alert.overrideTargetRouteRequired", { index: index + 1 }),
      );
    }

    return {
      source_name: sourceName,
      target_route_id: item.target_route_id,
      description: textOrNull(item.description),
      is_enabled: item.is_enabled,
    };
  });
}

async function handleCommit() {
  if (!editingData.value.name.trim()) {
    toastController.error(t("apiKeyEditModal.alert.nameRequired"));
    return;
  }

  try {
    isSubmitting.value = true;
    const dailyBudget = buildBudgetPayload(
      editingData.value.budget_daily_nanos,
      editingData.value.budget_daily_currency,
      t("apiKeyEditModal.labelBudgetDaily"),
    );
    const monthlyBudget = buildBudgetPayload(
      editingData.value.budget_monthly_nanos,
      editingData.value.budget_monthly_currency,
      t("apiKeyEditModal.labelBudgetMonthly"),
    );
    const payloadBase = {
      name: editingData.value.name.trim(),
      description: textOrNull(editingData.value.description),
      default_action: editingData.value.default_action,
      is_enabled: editingData.value.is_enabled,
      expires_at: fromLocalDatetimeInput(editingData.value.expires_at),
      rate_limit_rpm: numberOrNull(editingData.value.rate_limit_rpm),
      max_concurrent_requests: numberOrNull(editingData.value.max_concurrent_requests),
      quota_daily_requests: numberOrNull(editingData.value.quota_daily_requests),
      quota_daily_tokens: numberOrNull(editingData.value.quota_daily_tokens),
      quota_monthly_tokens: numberOrNull(editingData.value.quota_monthly_tokens),
      budget_daily_nanos: dailyBudget.nanos,
      budget_daily_currency: dailyBudget.currency,
      budget_monthly_nanos: monthlyBudget.nanos,
      budget_monthly_currency: monthlyBudget.currency,
      model_overrides: buildModelOverridePayloads(),
      acl_rules: buildRulePayloads(),
    };

    if (editingData.value.id) {
      const response = await Api.updateApiKey(
        editingData.value.id,
        payloadBase as ApiKeyUpdatePayload,
      );
      emit("saveSuccess", { detail: response });
    } else {
      const response = await Api.createApiKey(payloadBase as ApiKeyCreatePayload);
      emit("saveSuccess", response);
    }

    toastController.success(t("apiKeyEditModal.alert.saveSuccess"));
    emit("update:isOpen", false);
  } catch (error: unknown) {
    toastController.error(
      t("apiKeyEditModal.alert.saveFailed", {
        error: normalizeError(error, t("common.unknownError")).message,
      }),
    );
  } finally {
    isSubmitting.value = false;
  }
}

watch(
  () => props.isOpen,
  (isOpen) => {
    if (isOpen) {
      resetEditingData();
    }
  },
);
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
            <div>
              <h3 class="text-base font-semibold text-gray-900">
                {{ t("apiKeyEditModal.sections.identity") }}
              </h3>
              <p class="mt-1 text-sm text-gray-500">
                {{ t("apiKeyEditModal.sections.identityDescription") }}
              </p>
            </div>

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

            <div class="space-y-1.5">
              <div
                class="flex items-center justify-between rounded-lg border border-gray-200 p-3.5"
              >
                <Label class="cursor-pointer text-sm font-medium leading-none">
                  {{ t("apiKeyEditModal.labelEnabled") }}
                </Label>
                <Checkbox v-model="editingData.is_enabled" />
              </div>
            </div>
          </section>

          <section class="space-y-4 border-t border-gray-100 pt-6">
            <div>
              <h3 class="text-base font-semibold text-gray-900">
                {{ t("apiKeyEditModal.sections.governance") }}
              </h3>
              <p class="mt-1 text-sm text-gray-500">
                {{ t("apiKeyEditModal.sections.governanceDescription") }}
              </p>
            </div>

            <div class="space-y-4">
              <div class="rounded-lg border border-gray-200 p-4">
                <div class="mb-4">
                  <h4 class="text-sm font-semibold text-gray-900">
                    {{ t("apiKeyEditModal.groups.throughputTitle") }}
                  </h4>
                  <p class="mt-1 text-sm text-gray-500">
                    {{ t("apiKeyEditModal.groups.throughputDescription") }}
                  </p>
                </div>

                <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
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

              <div class="rounded-lg border border-gray-200 p-4">
                <div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <h4 class="text-sm font-semibold text-gray-900">
                      {{ t("apiKeyEditModal.groups.quotaTitle") }}
                    </h4>
                    <p class="mt-1 text-sm text-gray-500">
                      {{ t("apiKeyEditModal.groups.quotaDescription") }}
                    </p>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    class="text-gray-500"
                    @click="clearQuotaLimits"
                  >
                    {{ t("apiKeyEditModal.clearQuota") }}
                  </Button>
                </div>

                <div class="grid grid-cols-1 gap-4 sm:grid-cols-2">
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

              <div class="rounded-lg border border-gray-200 p-4">
                <div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
                  <div>
                    <h4 class="text-sm font-semibold text-gray-900">
                      {{ t("apiKeyEditModal.groups.budgetTitle") }}
                    </h4>
                    <p class="mt-1 text-sm text-gray-500">
                      {{ t("apiKeyEditModal.groups.budgetDescription") }}
                    </p>
                  </div>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    class="text-gray-500"
                    @click="clearBudgetLimits"
                  >
                    {{ t("apiKeyEditModal.clearBudget") }}
                  </Button>
                </div>

                <div class="rounded-lg border border-gray-200 bg-gray-50 px-3 py-2 text-sm text-gray-600">
                  {{ t("apiKeyEditModal.budgetMajorUnitHint") }}
                </div>

                <div class="mt-4 space-y-4">
                  <div class="grid grid-cols-1 gap-4 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)_minmax(0,0.9fr)] sm:items-end">
                    <div>
                      <Label for="budget-daily-amount" class="text-gray-700">
                        {{ t("apiKeyEditModal.labelBudgetDaily") }}
                      </Label>
                    </div>
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
                    <div>
                      <Label for="budget-monthly-amount" class="text-gray-700">
                        {{ t("apiKeyEditModal.labelBudgetMonthly") }}
                      </Label>
                    </div>
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
            </div>
          </section>

          <section class="space-y-4 border-t border-gray-100 pt-6">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div>
                <h3 class="text-base font-semibold text-gray-900">
                  {{ t("apiKeyEditModal.sections.overrides") }}
                </h3>
                <p class="mt-1 text-sm text-gray-500">
                  {{ t("apiKeyEditModal.sections.overridesDescription") }}
                </p>
              </div>
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
            </div>

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
                class="rounded-lg border border-gray-200 bg-gray-50/50 p-4"
              >
                <div class="flex items-center justify-between gap-3">
                  <div>
                    <h4 class="text-sm font-semibold text-gray-900">
                      {{ t("apiKeyEditModal.overrideTitle", { index: index + 1 }) }}
                    </h4>
                    <p class="mt-1 text-xs text-gray-500">
                      {{ t("apiKeyEditModal.overrideDescription") }}
                    </p>
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

                <div
                  class="mt-4 flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5"
                >
                  <Label class="cursor-pointer text-sm font-medium leading-none">
                    {{ t("apiKeyEditModal.labelOverrideEnabled") }}
                  </Label>
                  <Checkbox v-model="item.is_enabled" />
                </div>
              </div>
            </div>
          </section>

          <section class="space-y-4 border-t border-gray-100 pt-6">
            <div class="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
              <div>
                <h3 class="text-base font-semibold text-gray-900">
                {{ t("apiKeyEditModal.sections.acl") }}
              </h3>
              <p class="mt-1 text-sm text-gray-500">
                {{ t("apiKeyEditModal.sections.aclDescription") }}
              </p>
            </div>
            <Button
              type="button"
              variant="outline"
              class="w-full sm:w-auto"
              @click="addRule"
            >
              <Plus class="mr-1.5 h-4 w-4" />
              {{ t("apiKeyEditModal.addRule") }}
            </Button>
          </div>

          <div v-if="!editingData.acl_rules.length" class="rounded-lg border border-dashed border-gray-200 px-4 py-6 text-sm text-gray-500">
            {{ t("apiKeyEditModal.noRules") }}
          </div>

          <div v-else class="space-y-4">
            <div
              v-for="(rule, index) in editingData.acl_rules"
              :key="rule.id ?? `new-${index}`"
              class="rounded-lg border border-gray-200 bg-gray-50/50 p-4"
            >
              <div class="flex items-center justify-between gap-3">
                <div>
                  <h4 class="text-sm font-semibold text-gray-900">
                    {{ t("apiKeyEditModal.ruleTitle", { index: index + 1 }) }}
                  </h4>
                  <p class="mt-1 text-xs text-gray-500">
                    {{ t("apiKeyEditModal.ruleDescription") }}
                  </p>
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
                  <Label class="text-gray-700">{{ t("apiKeyEditModal.labelRuleEffect") }}</Label>
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
                  <Label class="text-gray-700">{{ t("apiKeyEditModal.labelRuleScope") }}</Label>
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
                  <Label class="text-gray-700">{{ t("apiKeyEditModal.labelRulePriority") }}</Label>
                  <Input v-model="rule.priority" type="number" />
                </div>

                <div class="space-y-1.5">
                  <Label class="text-gray-700">{{ t("apiKeyEditModal.labelRuleProvider") }}</Label>
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
                  <Label class="text-gray-700">{{ t("apiKeyEditModal.labelRuleModel") }}</Label>
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
                  <Label class="text-gray-700">{{ t("apiKeyEditModal.labelRuleDescription") }}</Label>
                  <Input v-model="rule.description" />
                </div>
              </div>

              <div
                class="mt-4 flex items-center justify-between rounded-lg border border-gray-200 bg-white p-3.5"
              >
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
