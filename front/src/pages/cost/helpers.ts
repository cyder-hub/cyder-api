import {
  formatCostRateFromNanos,
  formatCostRateInputFromNanos,
  parseCostRateInputToNanos,
} from "@/lib/utils";
import type {
  CatalogDraft,
  ChargeKind,
  ComponentDraft,
  CostOption,
  ParsedTierConfig,
  PreviewDraft,
  TierBasis,
  TierRowDraft,
  VersionDraft,
} from "./types";
import type { CostPreviewResponse } from "@/store/types";

export const METER_OPTIONS = [
  {
    value: "llm.input_text_tokens",
    labelKey: "costPage.componentEditor.meters.inputText",
  },
  {
    value: "llm.output_text_tokens",
    labelKey: "costPage.componentEditor.meters.outputText",
  },
  {
    value: "llm.input_image_tokens",
    labelKey: "costPage.componentEditor.meters.inputImage",
  },
  {
    value: "llm.output_image_tokens",
    labelKey: "costPage.componentEditor.meters.outputImage",
  },
  {
    value: "llm.cache_read_tokens",
    labelKey: "costPage.componentEditor.meters.cacheRead",
  },
  {
    value: "llm.cache_write_tokens",
    labelKey: "costPage.componentEditor.meters.cacheWrite",
  },
  {
    value: "llm.reasoning_tokens",
    labelKey: "costPage.componentEditor.meters.reasoning",
  },
  {
    value: "invoke.request_calls",
    labelKey: "costPage.componentEditor.meters.requestCalls",
  },
] as const satisfies readonly CostOption[];

export const CHARGE_KIND_OPTIONS = [
  {
    value: "per_unit",
    labelKey: "costPage.componentEditor.chargeKinds.perUnit",
  },
  {
    value: "flat",
    labelKey: "costPage.componentEditor.chargeKinds.flat",
  },
  {
    value: "tiered_per_unit",
    labelKey: "costPage.componentEditor.chargeKinds.tieredPerUnit",
  },
] as const satisfies readonly CostOption<ChargeKind>[];

export const TIER_BASIS_OPTIONS = [
  {
    value: "meter_quantity",
    labelKey: "costPage.componentEditor.tiers.basisMeterQuantity",
  },
  {
    value: "total_input_tokens",
    labelKey: "costPage.componentEditor.tiers.basisTotalInputTokens",
  },
] as const satisfies readonly CostOption<TierBasis>[];

export const MATCH_ATTRIBUTES_PLACEHOLDER = JSON.stringify({
  spec_key: "1024x1024",
});

export const normalizePreviewResponse = (
  response: CostPreviewResponse,
): CostPreviewResponse => ({
  ...response,
  ledger: {
    ...response.ledger,
    items: response.ledger?.items ?? [],
  },
  result: {
    ...response.result,
    detail_lines: response.result?.detail_lines ?? [],
    unmatched_items: response.result?.unmatched_items ?? [],
    warnings: response.result?.warnings ?? [],
  },
});

export const createEmptyCatalogDraft = (): CatalogDraft => ({
  id: null,
  name: "",
  description: "",
});

export const toDateTimeLocal = (timestampMs: number | null | undefined) => {
  if (!timestampMs) {
    return "";
  }

  const date = new Date(timestampMs);
  if (Number.isNaN(date.getTime())) {
    return "";
  }

  const offset = date.getTimezoneOffset();
  const localDate = new Date(date.getTime() - offset * 60_000);
  return localDate.toISOString().slice(0, 16);
};

export const createEmptyVersionDraft = (): VersionDraft => ({
  version: "",
  currency: "USD",
  source: "",
  effective_from: toDateTimeLocal(Date.now()),
  effective_until: "",
  is_enabled: true,
});

export const createEmptyTierRow = (): TierRowDraft => ({
  up_to: "",
  unit_price: "",
});

export const createEmptyComponentDraft = (): ComponentDraft => ({
  id: null,
  meter_key: "llm.input_text_tokens",
  charge_kind: "per_unit",
  unit_price: "",
  flat_fee: "",
  match_attributes_json: "",
  priority: "100",
  description: "",
  tier_basis: "meter_quantity",
  tiers: [createEmptyTierRow()],
});

export const createPreviewSample = (): PreviewDraft => ({
  total_input_tokens: "1200",
  total_output_tokens: "640",
  input_text_tokens: "1200",
  output_text_tokens: "640",
  input_image_tokens: "0",
  output_image_tokens: "0",
  cache_read_tokens: "0",
  cache_write_tokens: "0",
  reasoning_tokens: "0",
});

export const parseDateTimeLocal = (value: string) => {
  if (!value.trim()) {
    return null;
  }

  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) {
    throw new Error("invalid datetime");
  }
  return timestamp;
};

export const parseOptionalJsonObject = (value: string) => {
  const trimmed = value.trim();
  if (!trimmed) {
    return null;
  }

  const parsed = JSON.parse(trimmed);
  if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error("invalid json object");
  }
  return JSON.stringify(parsed);
};

export const parseNonNegativeInteger = (
  value: string,
  field: string,
  required = true,
) => {
  const trimmed = value.trim();
  if (!trimmed) {
    if (required) {
      throw new Error(`${field}:required`);
    }
    return null;
  }

  if (!/^\d+$/.test(trimmed)) {
    throw new Error(`${field}:integer`);
  }

  return Number.parseInt(trimmed, 10);
};

export const parseRequiredNonNegativeInteger = (value: string, field: string) => {
  const parsed = parseNonNegativeInteger(value, field, true);
  if (parsed === null) {
    throw new Error(`${field}:required`);
  }
  return parsed;
};

export const parseRequiredPrice = (
  value: string,
  field: string,
  currency?: string | null,
) => {
  const parsed = parseCostRateInputToNanos(value, "money", currency);
  if (parsed === null) {
    throw new Error(`${field}:required`);
  }
  return parsed;
};

export const isMillionTokenMeter = (meterKey: string) => meterKey.startsWith("llm.");

export const getRateInputMode = (meterKey: string) =>
  isMillionTokenMeter(meterKey) ? "per_million_units" : "money";

export const parseRequiredRate = (
  value: string,
  field: string,
  meterKey: string,
  currency?: string | null,
) => {
  const parsed = parseCostRateInputToNanos(
    value,
    getRateInputMode(meterKey),
    currency,
  );
  if (parsed === null) {
    throw new Error(`${field}:required`);
  }
  return parsed;
};

export const formatRateInput = (
  nanos: number | null | undefined,
  meterKey: string,
  currency?: string | null,
) => formatCostRateInputFromNanos(nanos, getRateInputMode(meterKey), currency);

export const formatRateDisplay = (
  nanos: number | null | undefined,
  meterKey: string,
  currency?: string | null,
  suffix = true,
) => {
  const base = formatCostRateFromNanos(nanos, getRateInputMode(meterKey), currency);
  if (!suffix || base === "-") {
    return base;
  }
  return isMillionTokenMeter(meterKey) ? `${base} tokens` : `${base}/unit`;
};

export const formatNumber = (value: number | null | undefined) => {
  if (value === null || value === undefined) {
    return "-";
  }
  return new Intl.NumberFormat().format(value);
};

export const prettyJson = (value: string | null | undefined) => {
  if (!value) {
    return "";
  }

  try {
    return JSON.stringify(JSON.parse(value), null, 2);
  } catch {
    return value;
  }
};

export const parseTierConfig = (
  value: string | null | undefined,
  meterKey = "llm.input_text_tokens",
  currency?: string | null,
): ParsedTierConfig | null => {
  if (!value) {
    return null;
  }

  try {
    const parsed = JSON.parse(value) as {
      basis?: TierBasis;
      tiers?: Array<{ up_to?: number | null; unit_price_nanos?: number }>;
    };

    if (!parsed || !Array.isArray(parsed.tiers)) {
      return null;
    }

    return {
      basis:
        parsed.basis === "total_input_tokens"
          ? "total_input_tokens"
          : "meter_quantity",
      tiers: parsed.tiers.map((tier) => ({
        up_to:
          tier.up_to === null || tier.up_to === undefined ? "" : String(tier.up_to),
        unit_price: formatRateInput(tier.unit_price_nanos, meterKey, currency),
      })),
    };
  } catch {
    return null;
  }
};
