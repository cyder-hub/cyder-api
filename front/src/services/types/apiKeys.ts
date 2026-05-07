// ========== API Key Types ==========
export type ApiKeyAction = "ALLOW" | "DENY";
export type ApiKeyAclRuleScope = "PROVIDER" | "MODEL";

export interface ApiKeyAclRule {
  id: number;
  effect: ApiKeyAction;
  priority: number;
  scope: ApiKeyAclRuleScope;
  provider_id: number | null;
  model_id: number | null;
  is_enabled: boolean;
  description: string | null;
}

export interface ApiKeyItem {
  id: number;
  key_prefix: string;
  key_last4: string;
  name: string;
  description: string | null;
  default_action: ApiKeyAction;
  is_enabled: boolean;
  expires_at: number | null;
  rate_limit_rpm: number | null;
  max_concurrent_requests: number | null;
  quota_daily_requests: number | null;
  quota_daily_tokens: number | null;
  quota_monthly_tokens: number | null;
  budget_daily_nanos: number | null;
  budget_daily_currency: string | null;
  budget_monthly_nanos: number | null;
  budget_monthly_currency: string | null;
  created_at: number;
  updated_at: number;
  created_at_formatted?: string;
  updated_at_formatted?: string;
}

export interface ApiKeyDetail extends ApiKeyItem {
  acl_rules: ApiKeyAclRule[];
  model_overrides: ApiKeyModelOverrideItem[];
}

export interface ApiKeyReveal {
  id: number;
  name: string;
  key_prefix: string;
  key_last4: string;
  api_key: string;
  updated_at: number;
}

export interface ApiKeyCreateResponse {
  detail: ApiKeyDetail;
  reveal: ApiKeyReveal;
}

export interface ApiKeyRuntimeBilledAmount {
  currency: string;
  amount_nanos: number;
}

export interface ApiKeyRuntimeSnapshot {
  api_key_id: number;
  current_concurrency: number;
  current_minute_bucket: number | null;
  current_minute_request_count: number;
  day_bucket: number | null;
  daily_request_count: number;
  daily_token_count: number;
  month_bucket: number | null;
  monthly_token_count: number;
  daily_billed_amounts: ApiKeyRuntimeBilledAmount[];
  monthly_billed_amounts: ApiKeyRuntimeBilledAmount[];
}

export interface ApiKeyAclRulePayload {
  id?: number;
  effect: ApiKeyAction;
  priority: number;
  scope: ApiKeyAclRuleScope;
  provider_id: number | null;
  model_id: number | null;
  is_enabled?: boolean;
  description?: string | null;
}

export interface ApiKeyModelOverrideItem {
  id: number;
  source_name: string;
  target_route_id: number;
  target_route_name: string | null;
  description: string | null;
  is_enabled: boolean;
}

export interface ApiKeyModelOverridePayload {
  source_name: string;
  target_route_id: number;
  description?: string | null;
  is_enabled?: boolean;
}

export interface ApiKeyCreatePayload {
  name: string;
  description?: string | null;
  default_action?: ApiKeyAction;
  is_enabled?: boolean;
  expires_at?: number | null;
  rate_limit_rpm?: number | null;
  max_concurrent_requests?: number | null;
  quota_daily_requests?: number | null;
  quota_daily_tokens?: number | null;
  quota_monthly_tokens?: number | null;
  budget_daily_nanos?: number | null;
  budget_daily_currency?: string | null;
  budget_monthly_nanos?: number | null;
  budget_monthly_currency?: string | null;
  acl_rules?: ApiKeyAclRulePayload[];
  model_overrides?: ApiKeyModelOverridePayload[];
}

export interface ApiKeyUpdatePayload {
  name?: string;
  description?: string | null;
  default_action?: ApiKeyAction;
  is_enabled?: boolean;
  expires_at?: number | null;
  rate_limit_rpm?: number | null;
  max_concurrent_requests?: number | null;
  quota_daily_requests?: number | null;
  quota_daily_tokens?: number | null;
  quota_monthly_tokens?: number | null;
  budget_daily_nanos?: number | null;
  budget_daily_currency?: string | null;
  budget_monthly_nanos?: number | null;
  budget_monthly_currency?: string | null;
  acl_rules?: ApiKeyAclRulePayload[];
  model_overrides?: ApiKeyModelOverridePayload[];
}
