import { computed, ref, watch, type ComputedRef, type Ref } from "vue";

import * as apiKeyService from "@/services/apiKeys";
import {
  isManagerReauthCancelled,
  runWithSecretGovernanceReauth,
} from "@/services/managerReauth";
import { toastController } from "@/services/uiFeedback";
import { normalizeError } from "@/utils/error";
import { formatPriceInputFromNanos } from "@/utils/money";
import type {
  ApiKeyAclRule,
  ApiKeyAclRulePayload,
  ApiKeyAction,
  ApiKeyAclRuleScope,
  ApiKeyCreatePayload,
  ApiKeyDetail,
  ApiKeyItem,
  ApiKeyReveal,
  ApiKeyUpdatePayload,
  ModelSummaryItem,
  ProviderSummaryItem,
} from "@/services/types";
import type { ApiKeyEditSuccessPayload } from "../types";
import { buildBudgetPayload } from "./apiKeyViewModel";

type TranslateFn = (key: string, named?: Record<string, unknown>) => string;

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

export interface EditingApiKeyData {
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
  acl_rules: EditableRule[];
}

interface UseApiKeyEditDialogOptions {
  isOpen: Readonly<Ref<boolean>>;
  initialData: Readonly<Ref<ApiKeyDetail | null>>;
  providers: Readonly<Ref<ProviderSummaryItem[]>>;
  models: Readonly<Ref<ModelSummaryItem[]>>;
  t: TranslateFn;
  close: () => void;
  emitSaveSuccess: (payload: ApiKeyEditSuccessPayload) => void;
}

interface UseApiKeyGovernanceOptions {
  t: TranslateFn;
  apiKeys: ComputedRef<ApiKeyItem[]>;
  selectedKeyId: Ref<number | null>;
  selectedDetail: Ref<ApiKeyDetail | null>;
  setIssuedSecret: (reveal: ApiKeyReveal | null) => void;
  clearRevealedSecret: () => void;
  refreshList: (preferredSelectedId: number | null) => Promise<number | null>;
  refreshDetail: (id: number | null) => Promise<void>;
}

const COMMON_BUDGET_CURRENCIES = ["CNY", "USD"] as const;

function getEmptyRule(): EditableRule {
  return {
    effect: "ALLOW",
    scope: "PROVIDER",
    priority: "0",
    provider_id: null,
    model_id: null,
    is_enabled: true,
    description: "",
  };
}

export function getEmptyEditingData(): EditingApiKeyData {
  return {
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
    acl_rules: [],
  };
}

function toInputNumber(value: number | null | undefined): string {
  return value === null || value === undefined ? "" : String(value);
}

function toBudgetInput(value: number | null | undefined, currency?: string | null): string {
  return formatPriceInputFromNanos(value, currency);
}

export function toLocalDatetimeInput(value: number | null | undefined): string {
  if (!value) {
    return "";
  }
  const date = new Date(value);
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

export function fromLocalDatetimeInput(value: string | number | null | undefined): number | null {
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

export function numberOrNull(
  value: string | number | null | undefined,
  invalidNumberMessage = "Invalid number.",
): number | null {
  if (value === null || value === undefined || value === "") {
    return null;
  }

  if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      throw new Error(invalidNumberMessage);
    }
    return value;
  }

  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  const parsed = Number(trimmed);
  if (!Number.isFinite(parsed)) {
    throw new Error(invalidNumberMessage);
  }
  return parsed;
}

function textOrNull(value: string): string | null {
  const trimmed = value.trim();
  return trimmed ? trimmed : null;
}

export function useApiKeyEditDialog(options: UseApiKeyEditDialogOptions) {
  const isSubmitting = ref(false);
  const editingData = ref<EditingApiKeyData>(getEmptyEditingData());

  const actionOptions = computed(() =>
    (["ALLOW", "DENY"] as ApiKeyAction[]).map((value) => ({
      value,
      label: options.t(`apiKeyEditModal.action.${value}`),
    })),
  );

  const scopeOptions = computed(() =>
    (["PROVIDER", "MODEL"] as ApiKeyAclRuleScope[]).map((value) => ({
      value,
      label: options.t(`apiKeyEditModal.scope.${value}`),
    })),
  );

  const providerOptions = computed(() =>
    options.providers.value.map((item) => ({
      value: item.id,
      label: `${item.name} (${item.provider_key})`,
      models: options.models.value
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
    const dailyCurrency = textOrNull(
      editingData.value.budget_daily_currency,
    )?.toUpperCase();
    const monthlyCurrency = textOrNull(
      editingData.value.budget_monthly_currency,
    )?.toUpperCase();

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
          ? options.t(`apiKeyEditModal.currency.${value}`)
          : value,
    }));
  });

  function updateBudgetCurrency(target: "daily" | "monthly", value: string) {
    const normalizedValue = value === "none" ? "" : value;
    if (target === "daily") {
      editingData.value.budget_daily_currency = normalizedValue;
      return;
    }

    editingData.value.budget_monthly_currency = normalizedValue;
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

  function resetEditingData() {
    if (!options.initialData.value) {
      editingData.value = getEmptyEditingData();
      return;
    }

    editingData.value = {
      id: options.initialData.value.id,
      name: options.initialData.value.name,
      description: options.initialData.value.description ?? "",
      default_action: options.initialData.value.default_action,
      is_enabled: options.initialData.value.is_enabled,
      expires_at: toLocalDatetimeInput(options.initialData.value.expires_at),
      rate_limit_rpm: toInputNumber(options.initialData.value.rate_limit_rpm),
      max_concurrent_requests: toInputNumber(
        options.initialData.value.max_concurrent_requests,
      ),
      quota_daily_requests: toInputNumber(options.initialData.value.quota_daily_requests),
      quota_daily_tokens: toInputNumber(options.initialData.value.quota_daily_tokens),
      quota_monthly_tokens: toInputNumber(options.initialData.value.quota_monthly_tokens),
      budget_daily_nanos: toBudgetInput(
        options.initialData.value.budget_daily_nanos,
        options.initialData.value.budget_daily_currency,
      ),
      budget_daily_currency: options.initialData.value.budget_daily_currency ?? "",
      budget_monthly_nanos: toBudgetInput(
        options.initialData.value.budget_monthly_nanos,
        options.initialData.value.budget_monthly_currency,
      ),
      budget_monthly_currency: options.initialData.value.budget_monthly_currency ?? "",
      acl_rules: options.initialData.value.acl_rules.map(normalizeEditableRule),
    };
  }

  function addRule() {
    editingData.value.acl_rules.push(getEmptyRule());
  }

  function removeRule(index: number) {
    editingData.value.acl_rules.splice(index, 1);
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

  function buildRulePayloads(): ApiKeyAclRulePayload[] {
    return editingData.value.acl_rules.map((rule, index) => {
      const priority = numberOrNull(
        rule.priority,
        options.t("apiKeyEditModal.alert.invalidNumber"),
      );
      if (priority === null) {
        throw new Error(
          options.t("apiKeyEditModal.alert.rulePriorityRequired", {
            index: index + 1,
          }),
        );
      }

      if (rule.scope === "PROVIDER" && rule.provider_id == null) {
        throw new Error(
          options.t("apiKeyEditModal.alert.ruleProviderRequired", {
            index: index + 1,
          }),
        );
      }

      if (rule.scope === "MODEL" && rule.model_id == null) {
        throw new Error(
          options.t("apiKeyEditModal.alert.ruleModelRequired", {
            index: index + 1,
          }),
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

  async function handleCommit() {
    if (!editingData.value.name.trim()) {
      toastController.error(options.t("apiKeyEditModal.alert.nameRequired"));
      return;
    }

    try {
      isSubmitting.value = true;
      const dailyBudget = buildBudgetPayload(
        editingData.value.budget_daily_nanos,
        editingData.value.budget_daily_currency,
        options.t("apiKeyEditModal.labelBudgetDaily"),
        options.t,
      );
      const monthlyBudget = buildBudgetPayload(
        editingData.value.budget_monthly_nanos,
        editingData.value.budget_monthly_currency,
        options.t("apiKeyEditModal.labelBudgetMonthly"),
        options.t,
      );
      const payloadBase = {
        name: editingData.value.name.trim(),
        description: textOrNull(editingData.value.description),
        default_action: editingData.value.default_action,
        is_enabled: editingData.value.is_enabled,
        expires_at: fromLocalDatetimeInput(editingData.value.expires_at),
        rate_limit_rpm: numberOrNull(
          editingData.value.rate_limit_rpm,
          options.t("apiKeyEditModal.alert.invalidNumber"),
        ),
        max_concurrent_requests: numberOrNull(
          editingData.value.max_concurrent_requests,
          options.t("apiKeyEditModal.alert.invalidNumber"),
        ),
        quota_daily_requests: numberOrNull(
          editingData.value.quota_daily_requests,
          options.t("apiKeyEditModal.alert.invalidNumber"),
        ),
        quota_daily_tokens: numberOrNull(
          editingData.value.quota_daily_tokens,
          options.t("apiKeyEditModal.alert.invalidNumber"),
        ),
        quota_monthly_tokens: numberOrNull(
          editingData.value.quota_monthly_tokens,
          options.t("apiKeyEditModal.alert.invalidNumber"),
        ),
        budget_daily_nanos: dailyBudget.nanos,
        budget_daily_currency: dailyBudget.currency,
        budget_monthly_nanos: monthlyBudget.nanos,
        budget_monthly_currency: monthlyBudget.currency,
        acl_rules: buildRulePayloads(),
      };

      if (editingData.value.id) {
        const response = await apiKeyService.updateApiKey(
          editingData.value.id,
          payloadBase as ApiKeyUpdatePayload,
        );
        options.emitSaveSuccess({ detail: response });
      } else {
        const response = await runWithSecretGovernanceReauth(() =>
          apiKeyService.createApiKey(payloadBase as ApiKeyCreatePayload),
        );
        options.emitSaveSuccess(response);
      }

      toastController.success(options.t("apiKeyEditModal.alert.saveSuccess"));
      options.close();
    } catch (error: unknown) {
      if (isManagerReauthCancelled(error)) return;
      const fallback = options.t("apiKeyEditModal.alert.saveFailed", {
        error: normalizeError(error, options.t("common.unknownError")).message,
      });
      toastController.error(fallback);
    } finally {
      isSubmitting.value = false;
    }
  }

  watch(
    () => options.isOpen.value,
    (isOpen) => {
      if (isOpen) {
        resetEditingData();
      }
    },
  );

  return {
    isSubmitting,
    editingData,
    actionOptions,
    scopeOptions,
    providerOptions,
    budgetCurrencyOptions,
    updateBudgetCurrency,
    clearQuotaLimits,
    clearBudgetLimits,
    addRule,
    removeRule,
    updateRuleScope,
    updateRuleProvider,
    modelOptionsForRule,
    handleCommit,
  };
}

export function useApiKeyGovernance(options: UseApiKeyGovernanceOptions) {
  const showEditDialog = ref(false);
  const editingDetail = ref<ApiKeyDetail | null>(null);

  async function handleStartEditing(id?: number) {
    if (!id) {
      editingDetail.value = null;
      showEditDialog.value = true;
      return;
    }

    try {
      editingDetail.value =
        options.selectedDetail.value?.id === id
          ? options.selectedDetail.value
          : await apiKeyService.getApiKeyDetail(id);
      showEditDialog.value = true;
    } catch (err: unknown) {
      toastController.error(
        options.t("apiKeyPage.loadEditFailed", {
          error: normalizeError(err, options.t("common.unknownError")).message,
        }),
      );
    }
  }

  async function handleSaveSuccess(payload: ApiKeyEditSuccessPayload) {
    options.selectedKeyId.value = payload.detail.id;
    options.selectedDetail.value = payload.detail;
    editingDetail.value = null;
    showEditDialog.value = false;
    if (payload.reveal) {
      options.setIssuedSecret(payload.reveal);
    }
    await options.refreshList(payload.detail.id);
  }

  async function handleRotateKey(id: number): Promise<boolean> {
    try {
      options.setIssuedSecret(
        await runWithSecretGovernanceReauth(() =>
          apiKeyService.rotateApiKey(id),
        ),
      );
      await options.refreshList(id);
      await options.refreshDetail(id);
      return true;
    } catch (err: unknown) {
      if (isManagerReauthCancelled(err)) return false;
      const fallback = options.t("apiKeyPage.rotateFailed", {
        error: normalizeError(err, options.t("common.unknownError")).message,
      });
      toastController.error(fallback);
      return false;
    }
  }

  async function handleDeleteKey(id: number): Promise<boolean> {
    try {
      await runWithSecretGovernanceReauth(() =>
        apiKeyService.deleteApiKey(id),
      );
      if (options.selectedKeyId.value === id) {
        options.selectedKeyId.value = null;
        options.selectedDetail.value = null;
        options.clearRevealedSecret();
      }
      const nextSelectedId = await options.refreshList(null);
      await options.refreshDetail(nextSelectedId);
      return true;
    } catch (err: unknown) {
      if (isManagerReauthCancelled(err)) return false;
      const fallback = options.t("apiKeyPage.deleteFailed", {
        error: normalizeError(err, options.t("common.unknownError")).message,
      });
      toastController.error(fallback);
      return false;
    }
  }

  return {
    showEditDialog,
    editingDetail,
    handleStartEditing,
    handleSaveSuccess,
    handleRotateKey,
    handleDeleteKey,
  };
}
