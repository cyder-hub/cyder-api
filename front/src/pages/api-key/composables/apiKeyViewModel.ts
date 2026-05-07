import type { ApiKeyItem, ApiKeyRuntimeSnapshot } from "../../../services/types";
import { majorUnitToNanos } from "../../../utils/money.ts";
import type {
  ApiKeyLifecycle,
  ApiKeyRuntimeRejectionReason,
  ApiKeyRuntimeRejectionTone,
  ApiKeyRuntimeRejectionView,
  ApiKeySummaryCard,
} from "../types";

type TranslateFn = (key: string, named?: Record<string, unknown>) => string;

export function isApiKeyExpiringSoon(
  expiresAt: number | null,
  now = Date.now(),
): boolean {
  if (!expiresAt || expiresAt <= now) {
    return false;
  }
  return expiresAt - now <= 7 * 24 * 60 * 60 * 1000;
}

export function getApiKeyLifecycle(
  key: Pick<ApiKeyItem, "is_enabled" | "expires_at">,
  now = Date.now(),
): ApiKeyLifecycle {
  if (!key.is_enabled) {
    return "disabled";
  }
  if (key.expires_at && key.expires_at <= now) {
    return "expired";
  }
  if (isApiKeyExpiringSoon(key.expires_at, now)) {
    return "expiringSoon";
  }
  return "active";
}

export function hasApiKeyGovernanceLimits(
  key: Pick<
    ApiKeyItem,
    | "rate_limit_rpm"
    | "max_concurrent_requests"
    | "quota_daily_requests"
    | "quota_daily_tokens"
    | "quota_monthly_tokens"
    | "budget_daily_nanos"
    | "budget_monthly_nanos"
  >,
): boolean {
  return [
    key.rate_limit_rpm,
    key.max_concurrent_requests,
    key.quota_daily_requests,
    key.quota_daily_tokens,
    key.quota_monthly_tokens,
    key.budget_daily_nanos,
    key.budget_monthly_nanos,
  ].some((value) => value !== null);
}

export function buildApiKeySummaryCards(
  apiKeys: ApiKeyItem[],
  runtimeSnapshots: ApiKeyRuntimeSnapshot[],
  t: TranslateFn,
  now = Date.now(),
): ApiKeySummaryCard[] {
  const total = apiKeys.length;
  const enabled = apiKeys.filter((key) => key.is_enabled).length;
  const governed = apiKeys.filter(hasApiKeyGovernanceLimits).length;
  const expiringSoon = apiKeys.filter((key) =>
    isApiKeyExpiringSoon(key.expires_at, now),
  ).length;
  const currentConcurrency = runtimeSnapshots.reduce(
    (sum, item) => sum + item.current_concurrency,
    0,
  );

  return [
    { key: "total", label: t("apiKeyPage.summary.total"), value: total },
    { key: "enabled", label: t("apiKeyPage.summary.enabled"), value: enabled },
    { key: "governed", label: t("apiKeyPage.summary.governed"), value: governed },
    {
      key: "concurrency",
      label: t("apiKeyPage.summary.activeConcurrency"),
      value: currentConcurrency,
    },
    {
      key: "expiring",
      label: t("apiKeyPage.summary.expiringSoon"),
      value: expiringSoon,
    },
  ];
}

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

function billedAmountMap(items: Array<{ currency: string; amount_nanos: number }>) {
  const map = new Map<string, number>();
  for (const item of items) {
    map.set(item.currency.toUpperCase(), item.amount_nanos);
  }
  return map;
}

function isBudgetExceeded(
  limitNanos: number | null,
  currency: string | null,
  usage: Array<{ currency: string; amount_nanos: number }>,
) {
  if (limitNanos == null || !currency) {
    return false;
  }
  const current = billedAmountMap(usage).get(currency.toUpperCase()) ?? 0;
  return current >= limitNanos;
}

export function getRuntimeRejectionReason(
  detail: ApiKeyItem,
  runtime: ApiKeyRuntimeSnapshot,
  now = Date.now(),
): ApiKeyRuntimeRejectionReason {
  if (!detail.is_enabled) {
    return "disabled";
  }
  if (detail.expires_at && detail.expires_at <= now) {
    return "expired";
  }
  if (
    detail.max_concurrent_requests != null &&
    runtime.current_concurrency >= detail.max_concurrent_requests
  ) {
    return "concurrency";
  }
  if (
    detail.rate_limit_rpm != null &&
    runtime.current_minute_request_count >= detail.rate_limit_rpm
  ) {
    return "rpm";
  }
  if (
    detail.quota_daily_requests != null &&
    runtime.daily_request_count >= detail.quota_daily_requests
  ) {
    return "dailyRequests";
  }
  if (
    detail.quota_daily_tokens != null &&
    runtime.daily_token_count >= detail.quota_daily_tokens
  ) {
    return "dailyTokens";
  }
  if (
    detail.quota_monthly_tokens != null &&
    runtime.monthly_token_count >= detail.quota_monthly_tokens
  ) {
    return "monthlyTokens";
  }
  if (
    isBudgetExceeded(
      detail.budget_daily_nanos,
      detail.budget_daily_currency,
      runtime.daily_billed_amounts,
    )
  ) {
    return "dailyBudget";
  }
  if (
    isBudgetExceeded(
      detail.budget_monthly_nanos,
      detail.budget_monthly_currency,
      runtime.monthly_billed_amounts,
    )
  ) {
    return "monthlyBudget";
  }
  return "none";
}

export function runtimeRejectionTone(
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
  detail: ApiKeyItem,
  runtime: ApiKeyRuntimeSnapshot,
  t: TranslateFn,
  now = Date.now(),
): ApiKeyRuntimeRejectionView {
  const reason = getRuntimeRejectionReason(detail, runtime, now);
  return {
    reason,
    label: t(`apiKeyPage.runtimeRejection.${reason}`),
    tone: runtimeRejectionTone(reason),
  };
}

export function buildBudgetPayload(
  amountInput: string | number | null | undefined,
  currencyInput: string,
  label: string,
  t: TranslateFn,
) {
  const normalizedAmount = amountInput == null ? "" : String(amountInput).trim();
  if (!normalizedAmount) {
    return {
      nanos: null,
      currency: null,
    };
  }

  const currency = currencyInput.trim().toUpperCase() || null;
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
