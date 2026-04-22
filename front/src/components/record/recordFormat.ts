import { formatCostRateFromNanos, formatPriceFromNanos, formatTimestamp } from "@/lib/utils";
import type { RecordReplayBody, RecordReplayNameValue } from "@/store/types";

export const emptyValue = "/";

export const formatDate = (timestamp: number | null | undefined) =>
  formatTimestamp(timestamp) || emptyValue;

export const formatCompactMetric = (value: number | string | null | undefined) => {
  if (value == null || value === "" || value === emptyValue) {
    return "-";
  }
  return String(value);
};

export const formatCompactMetrics = (
  values: Array<number | string | null | undefined>,
) => values.map(formatCompactMetric).join(" / ");

export const formatDuration = (
  start: number | null | undefined,
  end: number | null | undefined,
) => {
  if (start == null || end == null || end < start) {
    return emptyValue;
  }
  return `${((end - start) / 1000).toFixed(3)} s`;
};

export const formatPrice = (
  nanos: number | null | undefined,
  currency: string | null | undefined,
) => formatPriceFromNanos(nanos ?? null, currency ?? null, emptyValue);

export const formatUnitPrice = (
  meterKey: string,
  unitPriceNanos: number | null,
  currency?: string | null,
  labels: { tokens: string; unit: string } = { tokens: "tokens", unit: "unit" },
) => {
  const mode = meterKey.startsWith("llm.") ? "per_million_units" : "money";
  const base = formatCostRateFromNanos(unitPriceNanos, mode, currency, emptyValue);
  return meterKey.startsWith("llm.") ? `${base} ${labels.tokens}` : `${base}/${labels.unit}`;
};

export const getStatusBadgeVariant = (status: string | null | undefined) => {
  switch (status) {
    case "SUCCESS":
    case "success":
      return "default";
    case "ERROR":
    case "error":
      return "destructive";
    case "PENDING":
    case "pending":
    case "running":
      return "outline";
    case "CANCELLED":
    case "SKIPPED":
    case "cancelled":
    case "rejected":
      return "secondary";
    default:
      return "secondary";
  }
};

export const hasText = (value: string | null | undefined) =>
  Boolean(value && value.trim().length > 0);

export const formatJsonText = (value: unknown) => {
  if (typeof value === "string") {
    try {
      return JSON.stringify(JSON.parse(value), null, 2);
    } catch {
      return value;
    }
  }
  return JSON.stringify(value, null, 2);
};

export const formatNameValueLines = (items: RecordReplayNameValue[]) => {
  if (items.length === 0) return emptyValue;
  return items
    .map((item) => `${item.name}: ${item.value == null ? "" : item.value}`)
    .join("\n");
};

export const summarizeReplayBody = (
  body: RecordReplayBody | null | undefined,
  captureLabel = "capture",
) => {
  if (!body) return emptyValue;
  if (body.capture_state && body.capture_state !== "complete") {
    return `${captureLabel}: ${body.capture_state}`;
  }
  if (body.json != null) {
    return formatJsonText(body.json);
  }
  if (body.text != null) {
    return body.text;
  }
  return body.media_type || emptyValue;
};

export const formatLossLevel = (value: string | null | undefined) =>
  value ? value.replaceAll("_", " ") : emptyValue;
