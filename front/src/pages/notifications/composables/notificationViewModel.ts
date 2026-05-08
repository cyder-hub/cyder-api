import type {
  NotificationChannel,
  NotificationDelivery,
  NotificationDeliveryListParams,
  NotificationDeliveryStatus,
} from "../../../services/types";
import { formatTimestamp } from "../../../utils/datetime.ts";
import type {
  ChannelDraft,
  CooldownDraftResult,
  DeliveryStatusFilter,
  HeaderDraftResult,
  NotificationSummaryCounts,
} from "../types";

export function emptyNotificationChannelDraft(): ChannelDraft {
  return {
    channel_key: "",
    name: "",
    endpoint_url: "",
    signing_secret: "",
    headers_json: "",
    clear_headers: false,
    cooldown_seconds: "900",
    clear_signing_secret: false,
    is_enabled: true,
  };
}

export function createNotificationChannelDraft(
  channel: NotificationChannel,
): ChannelDraft {
  return {
    channel_key: channel.channel_key,
    name: channel.name,
    endpoint_url: channel.endpoint_url,
    signing_secret: "",
    headers_json: channel.headers_json || "",
    clear_headers: false,
    cooldown_seconds: String(channel.cooldown_seconds),
    clear_signing_secret: false,
    is_enabled: channel.is_enabled,
  };
}

export function normalizeNotificationHeadersDraft(
  rawHeaders: string,
): HeaderDraftResult {
  const raw = rawHeaders.trim();
  if (!raw) {
    return { valid: true, value: null };
  }

  try {
    const parsed = JSON.parse(raw);
    if (!parsed || Array.isArray(parsed) || typeof parsed !== "object") {
      return { valid: false, issue: "headers_invalid" };
    }
    return { valid: true, value: JSON.stringify(parsed) };
  } catch {
    return { valid: false, issue: "headers_invalid" };
  }
}

export function normalizeNotificationCooldownDraft(
  rawCooldownSeconds: string,
): CooldownDraftResult {
  const raw = rawCooldownSeconds.trim();
  const value = raw ? Number(raw) : 900;
  if (!Number.isInteger(value) || value < 0 || value > 86400) {
    return { valid: false, issue: "cooldown_invalid" };
  }
  return { valid: true, value };
}

export function buildNotificationDeliveryParams(
  status: DeliveryStatusFilter,
  channelId: string,
): NotificationDeliveryListParams {
  return {
    status: status === "all" ? undefined : status,
    channel_id: channelId === "all" ? undefined : Number(channelId),
    limit: 50,
  };
}

export function buildNotificationSummaryCounts(
  channels: NotificationChannel[],
  deliveries: NotificationDelivery[],
): NotificationSummaryCounts {
  return {
    channels: channels.length,
    enabled: channels.filter((channel) => channel.is_enabled).length,
    failed: deliveries.filter((delivery) => delivery.status === "failed").length,
    retrying: deliveries.filter(
      (delivery) => delivery.status === "retry_scheduled",
    ).length,
  };
}

export function notificationChannelStatusClass(
  channel: Pick<NotificationChannel, "is_enabled">,
): string {
  return channel.is_enabled
    ? "border-emerald-200 bg-emerald-50 text-emerald-700"
    : "border-gray-200 bg-gray-100 text-gray-600";
}

export function notificationDeliveryBadgeClass(
  status: NotificationDeliveryStatus,
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

export function formatNotificationDateTime(
  value: number | null | undefined,
): string {
  return formatTimestamp(value) || "-";
}
