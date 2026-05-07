import type {
  AlertScopeType,
  AlertSeverity,
  AlertStatus,
} from "@/services/types";

export type AlertFilterValue<T extends string> = T | "all";

export interface AlertFiltersState {
  status: AlertFilterValue<AlertStatus>;
  severity: AlertFilterValue<AlertSeverity>;
  scope_type: AlertFilterValue<AlertScopeType>;
  acknowledged: AlertFilterValue<"yes" | "no">;
  suppressed: AlertFilterValue<"yes" | "no">;
  query: string;
}

export interface AlertSelectOption<T extends string = string> {
  value: T;
  label: string;
}

export interface AlertSummaryCard {
  key: "active" | "critical" | "suppressed" | "acknowledged";
  label: string;
  value: number;
}

export type AlertActionKey = "ack" | "suppress" | "unsuppress" | "resolve";

export interface AlertSummaryCounts {
  active: number;
  critical: number;
  suppressed: number;
  acknowledged: number;
}
