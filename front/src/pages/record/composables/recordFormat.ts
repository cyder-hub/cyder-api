import { formatTimestamp } from "../../../utils/datetime.ts";
import { formatPriceFromNanos } from "../../../utils/money.ts";

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
