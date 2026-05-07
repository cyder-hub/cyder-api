import type {
  AlertEvent,
  AlertListParams,
  NotificationDelivery,
} from "../../../services/types";
import { formatTimestamp } from "../../../utils/datetime.ts";
import type { AlertFiltersState, AlertSummaryCounts } from "../types";

export function createDefaultAlertFilters(): AlertFiltersState {
  return {
    status: "active",
    severity: "all",
    scope_type: "all",
    acknowledged: "all",
    suppressed: "all",
    query: "",
  };
}

export function buildAlertListParams(
  filters: AlertFiltersState,
  pagination: { limit?: number; offset?: number } = {},
): AlertListParams {
  const params: AlertListParams = {
    limit: pagination.limit ?? 50,
    offset: pagination.offset ?? 0,
  };

  if (filters.status !== "all") params.status = filters.status;
  if (filters.severity !== "all") params.severity = filters.severity;
  if (filters.scope_type !== "all") params.scope_type = filters.scope_type;
  if (filters.acknowledged !== "all") {
    params.acknowledged = filters.acknowledged === "yes";
  }
  if (filters.suppressed !== "all") {
    params.suppressed = filters.suppressed === "yes";
  }

  return params;
}

export function isAlertSuppressed(
  alert: Pick<AlertEvent, "suppressed_until">,
  now = Date.now(),
): boolean {
  return !!alert.suppressed_until && alert.suppressed_until > now;
}

export function filterAlertsByQuery(
  alerts: AlertEvent[],
  rawQuery: string,
): AlertEvent[] {
  const query = rawQuery.trim().toLowerCase();
  if (!query) return alerts;

  return alerts.filter((alert) =>
    [
      alert.fingerprint,
      alert.rule_key,
      alert.title,
      alert.summary,
      alert.scope_id,
    ].some((value) => value.toLowerCase().includes(query)),
  );
}

export function buildAlertSummaryCounts(
  alerts: AlertEvent[],
  now = Date.now(),
): AlertSummaryCounts {
  return {
    active: alerts.filter((alert) => alert.status === "active").length,
    critical: alerts.filter((alert) => alert.severity === "critical").length,
    suppressed: alerts.filter((alert) => isAlertSuppressed(alert, now)).length,
    acknowledged: alerts.filter((alert) => !!alert.acknowledged_at).length,
  };
}

export function statusBadgeClass(status: AlertEvent["status"]): string {
  return status === "active"
    ? "border-gray-900 bg-gray-900 text-white"
    : "border-gray-200 bg-gray-100 text-gray-600";
}

export function severityBadgeClass(severity: AlertEvent["severity"]): string {
  switch (severity) {
    case "critical":
      return "border-red-200 bg-red-50 text-red-700";
    case "warning":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "info":
      return "border-gray-200 bg-gray-100 text-gray-600";
  }
}

export function alertDeliveryBadgeClass(
  status: NotificationDelivery["status"],
): string {
  switch (status) {
    case "succeeded":
      return "border-emerald-200 bg-emerald-50 text-emerald-700";
    case "failed":
      return "border-red-200 bg-red-50 text-red-700";
    case "retry_scheduled":
      return "border-amber-200 bg-amber-50 text-amber-700";
    case "in_progress":
      return "border-sky-200 bg-sky-50 text-sky-700";
    case "skipped":
      return "border-gray-200 bg-gray-50 text-gray-600";
    case "pending":
      return "border-gray-200 bg-gray-100 text-gray-600";
  }
}

export function formatAlertDateTime(
  value: number | null | undefined,
): string {
  return formatTimestamp(value) || "-";
}

export function toAlertDateTimeLocal(timestampMs: number): string {
  const date = new Date(timestampMs);
  const offset = date.getTimezoneOffset() * 60_000;
  return new Date(timestampMs - offset).toISOString().slice(0, 16);
}

export function parseAlertDateTimeLocal(value: string): number | null {
  if (!value) return null;
  const timestamp = new Date(value).getTime();
  return Number.isNaN(timestamp) ? null : timestamp;
}
