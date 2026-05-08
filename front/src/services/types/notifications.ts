export type NotificationChannelType = "webhook";

export interface NotificationChannel {
  id: number;
  channel_key: string;
  channel_type: NotificationChannelType;
  name: string;
  endpoint_url: string;
  signing_secret_redacted: string | null;
  headers_json: string | null;
  cooldown_seconds: number;
  is_enabled: boolean;
  last_test_at: number | null;
  last_test_success: boolean | null;
  last_test_error: string | null;
  created_at: number;
  updated_at: number;
}

export interface NotificationChannelCreatePayload {
  channel_key: string;
  name: string;
  endpoint_url: string;
  signing_secret?: string | null;
  headers_json?: string | null;
  cooldown_seconds?: number | null;
  is_enabled?: boolean;
}

export interface NotificationChannelUpdatePayload {
  name?: string;
  endpoint_url?: string;
  signing_secret?: string | null;
  clear_signing_secret?: boolean;
  headers_json?: string | null;
  clear_headers?: boolean;
  cooldown_seconds?: number | null;
  is_enabled?: boolean;
}

export interface NotificationWebhookTestResult {
  success: boolean;
  status: number | null;
  error: string | null;
  response_body_preview: string | null;
}

export type NotificationDeliveryStatus =
  | "pending"
  | "in_progress"
  | "retry_scheduled"
  | "succeeded"
  | "failed"
  | "skipped";

export type NotificationEventType = "alert_fired" | "alert_recovered" | "test";

export interface NotificationDelivery {
  id: number;
  channel_id: number;
  alert_id: number;
  alert_fingerprint: string;
  event_type: NotificationEventType;
  status: NotificationDeliveryStatus;
  payload_json: string;
  attempt_count: number;
  next_attempt_at: number;
  last_attempt_at: number | null;
  delivered_at: number | null;
  last_status_code: number | null;
  last_error: string | null;
  created_at: number;
  updated_at: number;
}

export interface NotificationDeliveryListParams {
  alert_id?: number;
  channel_id?: number;
  status?: NotificationDeliveryStatus;
  limit?: number;
  offset?: number;
}

export interface NotificationDeliveryListResponse {
  items: NotificationDelivery[];
  next_offset: number | null;
}
