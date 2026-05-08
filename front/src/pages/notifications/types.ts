import type { NotificationDeliveryStatus } from "@/services/types";

export type DeliveryStatusFilter = NotificationDeliveryStatus | "all";

export interface ChannelDraft {
  channel_key: string;
  name: string;
  endpoint_url: string;
  signing_secret: string;
  headers_json: string;
  clear_headers: boolean;
  cooldown_seconds: string;
  clear_signing_secret: boolean;
  is_enabled: boolean;
}

export interface NotificationSummaryCounts {
  channels: number;
  enabled: number;
  failed: number;
  retrying: number;
}

export interface NotificationSummaryCard {
  key: keyof NotificationSummaryCounts;
  label: string;
  value: number;
}

export interface NotificationSelectOption<T extends string = string> {
  value: T;
  label: string;
}

export type HeaderDraftResult =
  | { valid: true; value: string | null }
  | { valid: false; issue: "headers_invalid" };

export type CooldownDraftResult =
  | { valid: true; value: number }
  | { valid: false; issue: "cooldown_invalid" };
