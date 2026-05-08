export type AlertSeverity = "info" | "warning" | "critical";
export type AlertStatus = "active" | "resolved";
export type AlertScopeType =
  | "global"
  | "provider"
  | "model"
  | "api_key"
  | "provider_api_key"
  | "provider_model"
  | "system";

export interface AlertEvent {
  id: number;
  fingerprint: string;
  rule_key: string;
  severity: AlertSeverity;
  status: AlertStatus;
  scope_type: AlertScopeType;
  scope_id: string;
  title: string;
  summary: string;
  details_json: string;
  metrics_snapshot_json: string | null;
  first_seen_at: number;
  last_seen_at: number;
  resolved_at: number | null;
  acknowledged_at: number | null;
  acknowledged_note: string | null;
  suppressed_until: number | null;
  suppressed_reason: string | null;
  occurrence_count: number;
  reopened_count: number;
  last_notification_at: number | null;
  created_at: number;
  updated_at: number;
}

export interface AlertListParams {
  status?: AlertStatus;
  acknowledged?: boolean;
  suppressed?: boolean;
  severity?: AlertSeverity;
  rule_key?: string;
  scope_type?: AlertScopeType;
  scope_id?: string;
  start_time?: number;
  end_time?: number;
  limit?: number;
  offset?: number;
}

export interface AlertListResponse {
  items: AlertEvent[];
  limit: number;
  offset: number;
  next_offset: number | null;
}

export interface AlertAckPayload {
  note?: string | null;
}

export interface AlertSuppressPayload {
  suppressed_until: number;
  reason?: string | null;
}
