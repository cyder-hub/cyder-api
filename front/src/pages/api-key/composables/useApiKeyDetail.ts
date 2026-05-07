import { computed, ref, type ComputedRef } from "vue";

import * as apiKeyService from "@/services/apiKeys";
import { confirm, toastController } from "@/services/uiFeedback";
import { copyText } from "@/utils/clipboard";
import { formatTimestamp } from "@/utils/datetime";
import { normalizeError } from "@/utils/error";
import { formatPriceInputFromNanos } from "@/utils/money";
import type {
  ApiKeyAclRule,
  ApiKeyAction,
  ApiKeyAclRuleScope,
  ApiKeyDetail,
  ApiKeyItem,
  ApiKeyModelOverrideItem,
  ApiKeyReveal,
  ApiKeyRuntimeBilledAmount,
  ApiKeyRuntimeSnapshot,
} from "@/services/types";
import { getApiKeyLifecycle } from "./apiKeyViewModel";
import type {
  ApiKeyGovernanceItem,
  ApiKeyRuntimeById,
  ApiKeyRuntimeRejectionReason,
  ApiKeyRuntimeRejectionTone,
  ApiKeyRuntimeRejectionView,
} from "../types";

type TranslateFn = (key: string, named?: Record<string, unknown>) => string;

export function emptyRuntimeSnapshot(apiKeyId: number): ApiKeyRuntimeSnapshot {
  return {
    api_key_id: apiKeyId,
    current_concurrency: 0,
    current_minute_bucket: null,
    current_minute_request_count: 0,
    day_bucket: null,
    daily_request_count: 0,
    daily_token_count: 0,
    month_bucket: null,
    monthly_token_count: 0,
    daily_billed_amounts: [],
    monthly_billed_amounts: [],
  };
}

export function maskedApiKey(key: Pick<ApiKeyItem, "key_prefix" | "key_last4">) {
  return `${key.key_prefix}...${key.key_last4}`;
}

export function actionLabel(action: ApiKeyAction, t: TranslateFn) {
  return t(`apiKeyEditModal.action.${action}`);
}

export function scopeLabel(scope: ApiKeyAclRuleScope, t: TranslateFn) {
  return t(`apiKeyEditModal.scope.${scope}`);
}

export function lifecycleLabel(
  key: Pick<ApiKeyItem, "is_enabled" | "expires_at">,
  t: TranslateFn,
) {
  return t(`apiKeyPage.status.${getApiKeyLifecycle(key)}`);
}

export function statusBadgeClass(key: Pick<ApiKeyItem, "is_enabled" | "expires_at">) {
  const lifecycle = getApiKeyLifecycle(key);
  if (lifecycle === "disabled" || lifecycle === "expired") {
    return "border border-gray-200 bg-gray-100 text-gray-500";
  }
  if (lifecycle === "expiringSoon") {
    return "border border-gray-200 bg-white text-gray-700";
  }
  return "border border-gray-900 bg-gray-900 text-white";
}

export function formatExpiry(expiresAt: number | null, t: TranslateFn) {
  return expiresAt ? formatTimestamp(expiresAt) : t("apiKeyPage.neverExpires");
}

export function limitLabel(value: number | null, t: TranslateFn) {
  return value == null ? t("apiKeyPage.unlimited") : String(value);
}

export function formatApiKeyBudgetAmount(
  nanos: number | null | undefined,
  currency: string | null | undefined,
  t: TranslateFn,
) {
  if (nanos === null || nanos === undefined) {
    return t("common.notAvailable");
  }

  const amount = formatPriceInputFromNanos(nanos, currency);
  const normalizedCurrency = currency?.toUpperCase();
  if (normalizedCurrency === "CNY") {
    return `${amount} ${t("apiKeyPage.currencyUnit.cny")}`;
  }
  if (normalizedCurrency === "USD") {
    return `${amount} ${t("apiKeyPage.currencyUnit.usd")}`;
  }

  return normalizedCurrency ? `${amount} ${normalizedCurrency}` : amount;
}

export function billedAmountLabel(items: ApiKeyRuntimeBilledAmount[], t: TranslateFn) {
  if (!items.length) {
    return t("common.notAvailable");
  }
  return items
    .map((item) => formatApiKeyBudgetAmount(item.amount_nanos, item.currency, t))
    .join(" / ");
}

function billedAmountMap(items: ApiKeyRuntimeBilledAmount[]) {
  const map = new Map<string, number>();
  for (const item of items) {
    map.set(item.currency.toUpperCase(), item.amount_nanos);
  }
  return map;
}

function isBudgetExceeded(
  limitNanos: number | null,
  currency: string | null,
  usage: ApiKeyRuntimeBilledAmount[],
) {
  if (limitNanos == null || !currency) {
    return false;
  }
  const current = billedAmountMap(usage).get(currency.toUpperCase()) ?? 0;
  return current >= limitNanos;
}

function runtimeRejectionTone(
  reason: ApiKeyRuntimeRejectionReason,
): ApiKeyRuntimeRejectionTone {
  if (reason === "none") {
    return "muted";
  }
  if (reason === "disabled" || reason === "expired") {
    return "danger";
  }
  return "warning";
}

export function buildRuntimeRejectionView(
  detail: ApiKeyDetail | ApiKeyItem,
  runtime: ApiKeyRuntimeSnapshot,
  t: TranslateFn,
): ApiKeyRuntimeRejectionView {
  let reason: ApiKeyRuntimeRejectionReason = "none";

  if (!detail.is_enabled) {
    reason = "disabled";
  } else if (detail.expires_at && detail.expires_at <= Date.now()) {
    reason = "expired";
  } else if (
    detail.max_concurrent_requests != null &&
    runtime.current_concurrency >= detail.max_concurrent_requests
  ) {
    reason = "concurrency";
  } else if (
    detail.rate_limit_rpm != null &&
    runtime.current_minute_request_count >= detail.rate_limit_rpm
  ) {
    reason = "rpm";
  } else if (
    detail.quota_daily_requests != null &&
    runtime.daily_request_count >= detail.quota_daily_requests
  ) {
    reason = "dailyRequests";
  } else if (
    detail.quota_daily_tokens != null &&
    runtime.daily_token_count >= detail.quota_daily_tokens
  ) {
    reason = "dailyTokens";
  } else if (
    detail.quota_monthly_tokens != null &&
    runtime.monthly_token_count >= detail.quota_monthly_tokens
  ) {
    reason = "monthlyTokens";
  } else if (
    isBudgetExceeded(
      detail.budget_daily_nanos,
      detail.budget_daily_currency,
      runtime.daily_billed_amounts,
    )
  ) {
    reason = "dailyBudget";
  } else if (
    isBudgetExceeded(
      detail.budget_monthly_nanos,
      detail.budget_monthly_currency,
      runtime.monthly_billed_amounts,
    )
  ) {
    reason = "monthlyBudget";
  }

  return {
    reason,
    label: t(`apiKeyPage.runtimeRejection.${reason}`),
    tone: runtimeRejectionTone(reason),
  };
}

export function runtimeRejectionBadgeClass(view: ApiKeyRuntimeRejectionView) {
  if (view.tone === "danger") {
    return "border border-red-200 bg-red-50 text-red-700";
  }
  if (view.tone === "warning") {
    return "border border-gray-300 bg-gray-50 text-gray-700";
  }
  return "border border-gray-200 bg-white text-gray-500";
}

export function buildApiKeyGovernanceItems(
  detail: ApiKeyDetail,
  t: TranslateFn,
): ApiKeyGovernanceItem[] {
  return [
    {
      key: "expires_at",
      label: t("apiKeyPage.table.expiresAt"),
      value: formatExpiry(detail.expires_at, t),
    },
    {
      key: "rate_limit_rpm",
      label: t("apiKeyPage.table.rateLimitRpm"),
      value: limitLabel(detail.rate_limit_rpm, t),
    },
    {
      key: "max_concurrent_requests",
      label: t("apiKeyPage.table.maxConcurrency"),
      value: limitLabel(detail.max_concurrent_requests, t),
    },
    {
      key: "quota_daily_requests",
      label: t("apiKeyPage.table.quotaDailyRequests"),
      value: limitLabel(detail.quota_daily_requests, t),
    },
    {
      key: "quota_daily_tokens",
      label: t("apiKeyPage.table.quotaDailyTokens"),
      value: limitLabel(detail.quota_daily_tokens, t),
    },
    {
      key: "quota_monthly_tokens",
      label: t("apiKeyPage.table.quotaMonthlyTokens"),
      value: limitLabel(detail.quota_monthly_tokens, t),
    },
    {
      key: "budget_daily",
      label: t("apiKeyPage.table.budgetDaily"),
      value:
        detail.budget_daily_nanos == null
          ? t("apiKeyPage.unlimited")
          : formatApiKeyBudgetAmount(
              detail.budget_daily_nanos,
              detail.budget_daily_currency,
              t,
            ),
    },
    {
      key: "budget_monthly",
      label: t("apiKeyPage.table.budgetMonthly"),
      value:
        detail.budget_monthly_nanos == null
          ? t("apiKeyPage.unlimited")
          : formatApiKeyBudgetAmount(
              detail.budget_monthly_nanos,
              detail.budget_monthly_currency,
              t,
            ),
    },
  ];
}

export function aclRuleTarget(
  rule: ApiKeyAclRule,
  providerNameById: Map<number, string>,
  modelNameById: Map<number, string>,
  t: TranslateFn,
) {
  if (rule.scope === "PROVIDER") {
    return providerNameById.get(rule.provider_id ?? -1) ?? t("common.notAvailable");
  }
  return modelNameById.get(rule.model_id ?? -1) ?? t("common.notAvailable");
}

export function modelOverrideTargetLabel(
  item: ApiKeyModelOverrideItem,
  routeNameById: Map<number, string>,
  t: TranslateFn,
) {
  return (
    item.target_route_name ??
    routeNameById.get(item.target_route_id) ??
    t("common.notAvailable")
  );
}

export function useApiKeyDetail(
  t: TranslateFn,
  apiKeys: ComputedRef<ApiKeyItem[]>,
  runtimeById: ComputedRef<ApiKeyRuntimeById>,
) {
  const detailLoading = ref(false);
  const selectedKeyId = ref<number | null>(null);
  const selectedDetail = ref<ApiKeyDetail | null>(null);
  const selectedRuntime = ref<ApiKeyRuntimeSnapshot | null>(null);
  const showMobileKeyPicker = ref(false);
  const secretReveal = ref<ApiKeyReveal | null>(null);

  const selectedListKey = computed(
    () => apiKeys.value.find((key) => key.id === selectedKeyId.value) ?? null,
  );

  const selectedRuntimeView = computed(() => {
    if (selectedRuntime.value) {
      return selectedRuntime.value;
    }
    if (selectedKeyId.value != null) {
      return (
        runtimeById.value.get(selectedKeyId.value) ??
        emptyRuntimeSnapshot(selectedKeyId.value)
      );
    }
    return emptyRuntimeSnapshot(0);
  });

  async function loadSelectedKey(id: number | null) {
    selectedKeyId.value = id;
    if (id == null) {
      selectedDetail.value = null;
      selectedRuntime.value = null;
      secretReveal.value = null;
      return;
    }

    detailLoading.value = true;
    try {
      const [detail, runtime] = await Promise.all([
        apiKeyService.getApiKeyDetail(id),
        apiKeyService.getApiKeyRuntime(id),
      ]);
      selectedDetail.value = detail;
      selectedRuntime.value = runtime;
      if (secretReveal.value && secretReveal.value.id !== id) {
        secretReveal.value = null;
      }
    } catch (err: unknown) {
      toastController.error(
        t("apiKeyPage.loadDetailFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    } finally {
      detailLoading.value = false;
    }
  }

  function handleSelectKey(id: number) {
    showMobileKeyPicker.value = false;
    if (selectedKeyId.value === id && selectedDetail.value) {
      return;
    }
    void loadSelectedKey(id);
  }

  function setSelectedDetail(detail: ApiKeyDetail | null) {
    selectedDetail.value = detail;
    selectedKeyId.value = detail?.id ?? null;
  }

  function setSecretReveal(reveal: ApiKeyReveal | null) {
    secretReveal.value = reveal;
  }

  async function handleRevealKey(id: number) {
    const target = apiKeys.value.find((item) => item.id === id);
    if (
      !(await confirm({
        title: t("apiKeyPage.confirmReveal", { name: target?.name ?? String(id) }),
        description: t("apiKeyPage.confirmRevealDescription"),
        confirmText: t("apiKeyPage.actions.reveal"),
      }))
    ) {
      return;
    }

    try {
      secretReveal.value = await apiKeyService.revealApiKey(id);
      if (selectedKeyId.value !== id) {
        await loadSelectedKey(id);
      }
    } catch (err: unknown) {
      toastController.error(
        t("apiKeyPage.revealFailed", {
          error: normalizeError(err, t("common.unknownError")).message,
        }),
      );
    }
  }

  async function copySecret(secret: string) {
    const copied = await copyText(secret);
    if (!copied) {
      toastController.error(t("apiKeyPage.copyFailed"));
      return;
    }
    toastController.success(t("apiKeyPage.secret.copied"));
  }

  return {
    detailLoading,
    selectedKeyId,
    selectedDetail,
    selectedRuntime,
    selectedRuntimeView,
    selectedListKey,
    showMobileKeyPicker,
    secretReveal,
    loadSelectedKey,
    handleSelectKey,
    handleRevealKey,
    copySecret,
    setSelectedDetail,
    setSecretReveal,
  };
}
