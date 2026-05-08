import type {
  ApiKeyDetail,
  ApiKeyItem,
  ApiKeyReveal,
  ApiKeyRuntimeSnapshot,
} from "@/services/types";

export type ApiKeyLifecycle = "active" | "disabled" | "expired" | "expiringSoon";

export type ApiKeyRuntimeRejectionReason =
  | "none"
  | "disabled"
  | "expired"
  | "rpm"
  | "concurrency"
  | "dailyRequests"
  | "dailyTokens"
  | "monthlyTokens"
  | "dailyBudget"
  | "monthlyBudget";

export type ApiKeyRuntimeRejectionTone = "muted" | "warning" | "danger";

export interface ApiKeySummaryCard {
  key: string;
  label: string;
  value: number | string;
}

export interface ApiKeyGovernanceItem {
  key: string;
  label: string;
  value: string;
}

export interface ApiKeyRuntimeRejectionView {
  reason: ApiKeyRuntimeRejectionReason;
  label: string;
  tone: ApiKeyRuntimeRejectionTone;
}

export interface ApiKeyEditSuccessPayload {
  detail: ApiKeyDetail;
  reveal?: ApiKeyReveal;
}

export interface ApiKeySecretRevealState extends ApiKeyReveal {}

export type ApiKeyRuntimeById = Map<number, ApiKeyRuntimeSnapshot>;

export type ApiKeyListRow = ApiKeyItem;
